//! Arbre des fichiers, état git, diffs et surveillance du dossier.

use anyhow::{anyhow, Result};
use notify_debouncer_mini::notify::{self, event::AccessKind, event::AccessMode, EventKind, RecursiveMode, Watcher as _};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const MAX_FILES: usize = 20_000;
const MAX_READ: u64 = 1_000_000;

pub fn list_files(root: &str) -> Result<Vec<String>> {
    let root_path = Path::new(root);
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(root_path)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| e.file_name() != ".git" && e.file_name() != "node_modules")
        .build();
    for entry in walker.flatten() {
        if entry.file_type().is_some_and(|t| t.is_file()) {
            if let Ok(rel) = entry.path().strip_prefix(root_path) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
            if out.len() >= MAX_FILES {
                break;
            }
        }
    }
    out.sort();
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFile {
    pub path: String,
    pub status: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub files: Vec<GitFile>,
}

fn git(root: &str, args: &[&str]) -> Result<std::process::Output> {
    // Sans verrous facultatifs : `git status` ne réécrit pas .git/index au passage. Sinon la
    // surveillance du dossier y voit un changement, relance git status… et boucle.
    Ok(Command::new("git").args(args).env("GIT_OPTIONAL_LOCKS", "0").current_dir(root).output()?)
}

pub fn git_status(root: &str) -> Result<GitStatus> {
    let inside = git(root, &["rev-parse", "--is-inside-work-tree"])?;
    if !inside.status.success() {
        return Ok(GitStatus { is_repo: false, branch: None, files: vec![] });
    }
    let branch = git(root, &["branch", "--show-current"])
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|b| !b.is_empty());
    // Chemins relatifs au dossier ouvert (qui peut être un sous-dossier du dépôt).
    let out = git(root, &["status", "--porcelain=v1", "-z", "-uall", "--", "."])?;
    let prefix = git(root, &["rev-parse", "--show-prefix"])?;
    let prefix = String::from_utf8_lossy(&prefix.stdout).trim().to_string();

    let raw = String::from_utf8_lossy(&out.stdout);
    let mut parts = raw.split('\0');
    let mut files = Vec::new();
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let xy = &entry[..2];
        let path = &entry[3..];
        if xy.starts_with('R') || xy.starts_with('C') {
            parts.next(); // ancien nom
        }
        let status = match xy {
            "??" => "untracked",
            s if s.contains('D') => "deleted",
            s if s.contains('A') => "added",
            s if s.contains('R') => "renamed",
            s if s.contains('U') => "conflict",
            _ => "modified",
        };
        let rel = path.strip_prefix(prefix.as_str()).unwrap_or(path);
        files.push(GitFile { path: rel.to_string(), status });
    }
    Ok(GitStatus { is_repo: true, branch, files })
}

fn has_head(root: &str) -> bool {
    git(root, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok_and(|o| o.status.success())
}

/// Diff unifié du fichier par rapport au dernier commit (ou fichier complet si nouveau).
pub fn git_diff(root: &str, path: &str) -> Result<String> {
    safe_join(root, path)?;
    let is_repo = git(root, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|o| o.status.success());
    if is_repo && has_head(root) {
        let o = git(root, &["diff", "HEAD", "--no-color", "--no-ext-diff", "--", path])?;
        let d = String::from_utf8_lossy(&o.stdout).to_string();
        if !d.is_empty() {
            return Ok(d);
        }
        let tracked = git(root, &["ls-files", "--error-unmatch", "--", path]).is_ok_and(|o| o.status.success());
        if tracked {
            return Ok(String::new());
        }
    }
    // Fichier non suivi : tout est ajouté. (code 1 = différences trouvées, c'est normal)
    let o = git(root, &["diff", "--no-index", "--no-color", "--", "/dev/null", path])?;
    Ok(String::from_utf8_lossy(&o.stdout).to_string())
}

fn safe_join(root: &str, path: &str) -> Result<PathBuf> {
    let root = Path::new(root).canonicalize()?;
    let full = root.join(path);
    let check = full.canonicalize().unwrap_or_else(|_| full.clone());
    if !check.starts_with(&root) {
        return Err(anyhow!("chemin hors du projet"));
    }
    Ok(full)
}

pub fn read_file(root: &str, path: &str) -> Result<String> {
    let full = safe_join(root, path)?;
    if std::fs::metadata(&full)?.len() > MAX_READ {
        return Err(anyhow!("fichier trop gros pour l'aperçu"));
    }
    let bytes = std::fs::read(full)?;
    if bytes.contains(&0) {
        return Err(anyhow!("fichier binaire"));
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

pub fn git_init(root: &str) -> Result<()> {
    let o = git(root, &["init"])?;
    if !o.status.success() {
        return Err(anyhow!(String::from_utf8_lossy(&o.stderr).to_string()));
    }
    Ok(())
}

/// Surveillance en cours ; s'arrête quand on la lâche.
pub struct Watcher {
    _inner: notify::RecommendedWatcher,
}

/// Surveille le dossier et émet `fs-changed` (liste de chemins relatifs), par lots de 250 ms.
/// Les simples lectures (ouverture d'un fichier, accès) sont ignorées : sinon `git status` ou le
/// serveur de l'aperçu, en lisant des fichiers, déclencheraient un rafraîchissement… en boucle.
pub fn watch(app: AppHandle, root: &str) -> Result<Watcher> {
    let base = Path::new(root).canonicalize()?;
    let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
    let mut inner = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        // Lecture seule : on ignore. La fin d'une écriture (close-write) compte, elle.
        if matches!(ev.kind, EventKind::Access(k) if k != AccessKind::Close(AccessMode::Write)) {
            return;
        }
        for p in ev.paths {
            let _ = tx.send(p);
        }
    })?;
    inner.watch(&base, RecursiveMode::Recursive)?;
    // Regroupe les événements : le premier ouvre un lot de 250 ms. Le fil s'arrête avec la
    // surveillance (plus d'émetteur).
    std::thread::spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut batch = vec![first];
            let end = std::time::Instant::now() + Duration::from_millis(250);
            while let Ok(p) = rx.recv_timeout(end.saturating_duration_since(std::time::Instant::now())) {
                batch.push(p);
            }
            let mut paths: Vec<String> = batch
                .iter()
                .filter_map(|p| p.strip_prefix(&base).ok())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .filter(|p| {
                    // Dans .git, seuls l'index et HEAD nous intéressent (commit, checkout…).
                    !p.starts_with(".git/") || p == ".git/index" || p == ".git/HEAD"
                })
                .filter(|p| !p.contains("node_modules/"))
                .collect();
            paths.sort();
            paths.dedup();
            if !paths.is_empty() {
                let _ = app.emit("fs-changed", paths);
            }
        }
    });
    Ok(Watcher { _inner: inner })
}

#[cfg(test)]
mod watch_tests {
    #[test]
    fn reads_do_not_count_as_changes() {
        use notify_debouncer_mini::notify::{self, event::AccessKind, event::AccessMode, EventKind, RecursiveMode, Watcher as _};
        let dir = std::env::temp_dir().join(format!("watch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.html");
        std::fs::write(&f, "x").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if !matches!(ev.kind, EventKind::Access(k) if k != AccessKind::Close(AccessMode::Write)) {
                    let _ = tx.send(ev.kind);
                }
            }
        })
        .unwrap();
        w.watch(&dir, RecursiveMode::Recursive).unwrap();
        for _ in 0..5 {
            std::fs::read(&f).unwrap();
        }
        assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "une lecture a été vue comme un changement");
        std::fs::write(&f, "y").unwrap();
        assert!(rx.recv_timeout(std::time::Duration::from_millis(1000)).is_ok(), "l'écriture n'a pas été vue");
        std::fs::remove_dir_all(dir).ok();
    }
}

// ---------- Branches de travail (git worktree) ----------
// Un run lancé en parallèle sur un projet déjà occupé travaille sur sa propre branche, dans un
// dossier à part (data/worktrees/…), pour ne pas se marcher dessus avec l'autre run.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    /// Branche du projet au moment de la création : celle où le travail sera fusionné.
    pub base: String,
}

fn git_ok(dir: &str, args: &[&str]) -> Result<String> {
    let o = git(dir, args)?;
    if !o.status.success() {
        let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
        return Err(anyhow!("git {} : {}", args.first().unwrap_or(&""), if msg.is_empty() { "échec".into() } else { msg }));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn slug(label: &str) -> String {
    let mut s: String = label
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-');
    let s: String = s.chars().take(30).collect();
    if s.is_empty() { "travail".into() } else { s.trim_end_matches('-').to_string() }
}

/// Crée une branche `pi/<nom>` et son dossier de travail, à partir du dernier commit du projet.
pub fn worktree_create(root: &str, label: &str) -> Result<Worktree> {
    let base = git_ok(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    anyhow::ensure!(base != "HEAD", "le projet n'est sur aucune branche (HEAD détachée)");
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let id = format!("{:x}", stamp % 0xfffff);
    let name = format!("{}-{id}", slug(label));
    let branch = format!("pi/{name}");
    let root_path = Path::new(root).canonicalize()?;
    let project = root_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "projet".into());
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        root_path.hash(&mut h);
        format!("{:08x}", h.finish() as u32)
    };
    let dir = crate::pi::agent_dir().parent().unwrap_or(Path::new(".")).join("worktrees").join(format!("{project}-{hash}")).join(&name);
    std::fs::create_dir_all(dir.parent().unwrap())?;
    let dir_s = dir.to_string_lossy().into_owned();
    git_ok(root, &["worktree", "add", "-b", &branch, &dir_s, "HEAD"])?;
    Ok(Worktree { path: dir_s, branch, base })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    /// Commits de la branche absents de la branche de base.
    pub ahead: u32,
    /// Modifications pas encore enregistrées dans un commit.
    pub dirty: bool,
    pub exists: bool,
}

pub fn worktree_info(root: &str, path: &str, branch: &str, base: &str) -> Result<WorktreeInfo> {
    if !Path::new(path).is_dir() {
        return Ok(WorktreeInfo { ahead: 0, dirty: false, exists: false });
    }
    let ahead = git_ok(root, &["rev-list", "--count", &format!("{base}..{branch}")])?.parse().unwrap_or(0);
    let dirty = !git_ok(path, &["status", "--porcelain"])?.is_empty();
    Ok(WorktreeInfo { ahead, dirty, exists: true })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeOutcome {
    pub merged: bool,
    /// Fichiers en conflit (la fusion a été annulée : rien n'a bougé dans le projet).
    pub conflicts: Vec<String>,
}

/// Fusionne le travail de la branche dans la branche courante du projet. Les modifications pas
/// encore enregistrées sont d'abord mises dans un commit sur la branche.
pub fn worktree_merge(root: &str, path: &str, branch: &str, message: &str) -> Result<MergeOutcome> {
    if !git_ok(path, &["status", "--porcelain"])?.is_empty() {
        git_ok(path, &["add", "-A"])?;
        git_ok(path, &["commit", "-m", message])?;
    }
    let o = git(root, &["merge", "--no-ff", "--no-edit", "-m", &format!("Fusion de {branch} : {message}"), branch])?;
    if o.status.success() {
        return Ok(MergeOutcome { merged: true, conflicts: vec![] });
    }
    let conflicts: Vec<String> =
        git_ok(root, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default().lines().map(str::to_string).collect();
    if conflicts.is_empty() {
        // Refus avant de commencer (modifications locales qui seraient écrasées…) : on rapporte git.
        let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
        let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
        return Err(anyhow!("fusion impossible : {}", if msg.is_empty() { out } else { msg }));
    }
    let _ = git(root, &["merge", "--abort"]);
    Ok(MergeOutcome { merged: false, conflicts })
}

/// Supprime le dossier de travail, et la branche si demandé.
pub fn worktree_remove(root: &str, path: &str, branch: &str, delete_branch: bool) -> Result<()> {
    if Path::new(path).exists() {
        git_ok(root, &["worktree", "remove", "--force", path])?;
    }
    let _ = git(root, &["worktree", "prune"]);
    if delete_branch {
        git_ok(root, &["branch", "-D", branch])?;
    }
    Ok(())
}

#[cfg(test)]
mod worktree_tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let o = Command::new("git").args(args).current_dir(dir).output().unwrap();
        assert!(o.status.success(), "git {args:?} : {}", String::from_utf8_lossy(&o.stderr));
    }

    #[test]
    fn create_work_merge_remove() {
        let tmp = std::env::temp_dir().join(format!("wt-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let repo = tmp.join("projet");
        std::fs::create_dir_all(&repo).unwrap();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.join("data/pi-agent"));
        sh(&repo, &["init", "-q", "-b", "main"]);
        sh(&repo, &["config", "user.email", "t@t"]);
        sh(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("a.txt"), "un\n").unwrap();
        sh(&repo, &["add", "."]);
        sh(&repo, &["commit", "-qm", "init"]);
        let root = repo.to_string_lossy().into_owned();

        let wt = worktree_create(&root, "Ajoute l'export CSV").unwrap();
        assert_eq!(wt.base, "main");
        assert!(wt.branch.starts_with("pi/ajoute-l-export-csv-"), "{}", wt.branch);
        std::fs::write(Path::new(&wt.path).join("b.txt"), "nouveau\n").unwrap();
        let info = worktree_info(&root, &wt.path, &wt.branch, &wt.base).unwrap();
        assert!(info.dirty && info.exists && info.ahead == 0);

        let out = worktree_merge(&root, &wt.path, &wt.branch, "export CSV").unwrap();
        assert!(out.merged);
        assert!(repo.join("b.txt").exists());

        // Conflit : les deux côtés changent a.txt.
        let wt2 = worktree_create(&root, "conflit").unwrap();
        std::fs::write(Path::new(&wt2.path).join("a.txt"), "branche\n").unwrap();
        std::fs::write(repo.join("a.txt"), "projet\n").unwrap();
        sh(&repo, &["commit", "-qam", "côté projet"]);
        let out = worktree_merge(&root, &wt2.path, &wt2.branch, "conflit").unwrap();
        assert!(!out.merged);
        assert_eq!(out.conflicts, vec!["a.txt".to_string()]);
        assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "projet\n"); // fusion annulée

        worktree_remove(&root, &wt.path, &wt.branch, true).unwrap();
        worktree_remove(&root, &wt2.path, &wt2.branch, true).unwrap();
        assert!(!Path::new(&wt2.path).exists());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
