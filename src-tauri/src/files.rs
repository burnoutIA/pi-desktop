//! Arbre des fichiers, état git, diffs et surveillance du dossier.

use anyhow::{anyhow, Result};
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
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
    Ok(Command::new("git").args(args).current_dir(root).output()?)
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

pub type Watcher = Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>;

/// Surveille le dossier et émet `fs-changed` (liste de chemins relatifs).
pub fn watch(app: AppHandle, root: &str) -> Result<Watcher> {
    let root_path = Path::new(root).canonicalize()?;
    let base = root_path.clone();
    let mut debouncer = new_debouncer(Duration::from_millis(250), move |res: DebounceEventResult| {
        let Ok(events) = res else { return };
        let mut paths: Vec<String> = events
            .iter()
            .filter_map(|e| e.path.strip_prefix(&base).ok())
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
    })?;
    debouncer.watcher().watch(&root_path, RecursiveMode::Recursive)?;
    Ok(debouncer)
}
