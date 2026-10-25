//! Dots : agents pi autonomes. Un dot est un dossier (`<nom>.dot`, `skills/`, `extensions/`,
//! `state/`) ; ceux de l'app vivent dans `data/dots/`. Ils tournent dans leurs conteneurs, derrière
//! leur coffre : on les allume, éteint, crée et supprime avec le script `dot` du projet dots, en les
//! visant par leur dossier. La conversation passe par `dot <dossier> rpc`, qui parle le protocole de
//! `pi --mode rpc`.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// Dossier des données de l'app (`data/`).
fn data_dir() -> PathBuf {
    let dir = crate::pi::agent_dir();
    dir.parent().map(|p| p.to_path_buf()).unwrap_or(dir)
}

/// Où vivent les dots de l'app : `data/dots/<nom>/`.
pub fn home() -> PathBuf {
    data_dir().join("dots")
}

/// Réglage enregistré : `data/dots.json` ({"cli": "/chemin/vers/dot"}).
fn config_file() -> PathBuf {
    data_dir().join("dots.json")
}

fn saved_cli() -> Option<PathBuf> {
    let text = std::fs::read_to_string(config_file()).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v["cli"].as_str().filter(|s| !s.is_empty()).map(PathBuf::from)
}

/// Le script `dot` : celui des réglages, sinon `DOTS_CLI`, sinon un dépôt `dots` voisin de l'app.
pub fn cli() -> Option<PathBuf> {
    let candidates = [
        saved_cli(),
        std::env::var("DOTS_CLI").ok().filter(|s| !s.is_empty()).map(PathBuf::from),
        crate::servers::project_root().and_then(|r| r.parent().map(|p| p.join("dots/dot"))),
    ];
    candidates.into_iter().flatten().find(|p| p.is_file())
}

/// Chemin choisi dans les réglages (vide : retour à la détection automatique).
pub fn set_cli(path: &str) -> Result<()> {
    let path = path.trim();
    if !path.is_empty() {
        anyhow::ensure!(Path::new(path).is_file(), "« {path} » n'est pas un fichier");
    }
    std::fs::write(config_file(), serde_json::to_string_pretty(&json!({ "cli": path }))?)?;
    Ok(())
}

/// Le skill d'exemple du dépôt dots, à donner au pi local pour qu'il gère des dots.
pub fn skill_dir() -> Option<PathBuf> {
    let dir = cli()?.parent()?.join("integrations/pi-skill-dots");
    dir.join("SKILL.md").is_file().then_some(dir)
}

pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= 31
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Le fichier `.dot` d'un dossier de dot (il en faut un seul).
fn dotfile_of(dir: &Path) -> Option<PathBuf> {
    let files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "dot"))
        .collect();
    (files.len() == 1).then(|| files.into_iter().next().unwrap())
}

/// Un dossier de dot valide, en chemin absolu.
fn dot_dir(dir: &str) -> Result<PathBuf> {
    let d = std::fs::canonicalize(dir).with_context(|| format!("pas de dossier {dir}"))?;
    anyhow::ensure!(dotfile_of(&d).is_some(), "{} n'est pas un dot (il faut un seul fichier .dot)", d.display());
    Ok(d)
}

/// La description de l'en-tête du `.dot` (entre les deux `---`). On ne lit rien au-delà : le reste
/// du fichier contient les secrets.
fn description(file: &Path) -> String {
    use std::io::{BufRead, BufReader};
    let Ok(f) = std::fs::File::open(file) else { return String::new() };
    let mut lines = BufReader::new(f).lines().map_while(Result::ok);
    if lines.next().map(|l| l.trim().to_string()).as_deref() != Some("---") {
        return String::new();
    }
    for line in lines.take(40) {
        if line.trim() == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix("description:") {
            return v.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
        }
    }
    String::new()
}

/// Lance le script `dot` et renvoie sa sortie standard ; en cas d'échec, ses dernières erreurs.
async fn dot(args: &[&str], timeout: Duration) -> Result<String> {
    let cli = cli().ok_or_else(|| anyhow!("script dot introuvable : indique son chemin dans les réglages (onglet Agent)"))?;
    applog!("dots : {} {}", cli.display(), args.join(" "));
    let mut cmd = tokio::process::Command::new(&cli);
    cmd.args(args)
        .env("PATH", crate::pi::extended_path())
        // Les dots créés par le script (ou par l'agent, avec le skill) atterrissent dans l'app.
        .env("DOTS_HOME", home())
        .current_dir(cli.parent().unwrap_or(Path::new("/")))
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| anyhow!("`dot {}` ne répond pas", args.join(" ")))?
        .with_context(|| format!("impossible de lancer {}", cli.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = error_summary(&stderr);
        applog!("dots : échec ({}) : {msg}\n{}", out.status, stderr.chars().take(4000).collect::<String>());
        return Err(anyhow!(if msg.is_empty() { stdout } else { msg }));
    }
    Ok(stdout)
}

/// Les lignes utiles d'une sortie d'erreur : sans la trace de pile ni les lignes de Node autour
/// (« at … », « node:… », accolades, « Node.js v… »), les trois dernières qui restent.
fn error_summary(stderr: &str) -> String {
    let noise = |l: &str| {
        let t = l.trim();
        t.is_empty()
            || t.starts_with("at ")
            || t.starts_with("node:")
            || t.starts_with("Node.js v")
            || t.starts_with('^')
            || matches!(t, "{" | "}" | "})" | "};")
            || t.starts_with("code:")
            || t.starts_with("errno:")
            || t.starts_with("syscall:")
    };
    let useful: Vec<&str> = stderr.lines().filter(|l| !noise(l)).map(str::trim).collect();
    useful[useful.len().saturating_sub(3)..].join(" ")
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// L'état d'un dot (`dot <dossier> status --json`), complété de sa description et de sa place.
async fn status(dir: &Path) -> Result<Value> {
    let out = dot(&[&path_str(dir), "status", "--json"], Duration::from_secs(30)).await?;
    serde_json::from_str(&out).context("réponse de `dot status --json` illisible")
}

fn enrich(mut d: Value) -> Value {
    let dir = PathBuf::from(d["dir"].as_str().unwrap_or_default());
    let file = PathBuf::from(d["file"].as_str().unwrap_or_default());
    d["description"] = json!(description(&file));
    d["inApp"] = json!(dir.parent() == Some(home().as_path()));
    d
}

/// Les dots : ceux de l'app (`data/dots/`) et ceux que le script connaît ailleurs.
/// [{name, running, vault, interface, dir, file, socket, description, inApp}, …], ceux de l'app d'abord.
pub async fn list() -> Result<Value> {
    let out = dot(&["list", "--json"], Duration::from_secs(30)).await?;
    let mut all: Vec<Value> = serde_json::from_str::<Vec<Value>>(&out).context("réponse de `dot list --json` illisible")?;
    // Un dossier de data/dots/ que le script ne connaît pas encore (copié à la main) : `status`
    // l'enregistre au passage.
    if let Ok(entries) = std::fs::read_dir(home()) {
        for dir in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir() && dotfile_of(p).is_some()) {
            let known = all.iter().any(|d| d["dir"].as_str().is_some_and(|x| Path::new(x) == dir));
            if !known {
                match status(&dir).await {
                    Ok(d) => all.push(d),
                    Err(e) => applog!("dots : {} ignoré : {e:#}", dir.display()),
                }
            }
        }
    }
    let mut all: Vec<Value> = all.into_iter().map(enrich).collect();
    all.sort_by_key(|d| (d["inApp"] != true, d["name"].as_str().unwrap_or_default().to_string()));
    Ok(Value::Array(all))
}

/// Nouveau dot dans `data/dots/<nom>/` : copie vierge d'un autre (`from`, son dossier), ou fichier
/// `.dot` à remplir.
pub async fn create(name: &str, from: Option<&str>) -> Result<String> {
    anyhow::ensure!(valid_name(name), "nom invalide : minuscules, chiffres et tirets (31 caractères au plus)");
    anyhow::ensure!(!home().join(name).exists(), "un dot « {name} » existe déjà dans l'app");
    std::fs::create_dir_all(home())?;
    let home = path_str(&home());
    match from.filter(|f| !f.is_empty()) {
        Some(f) => {
            let src = path_str(&dot_dir(f)?);
            dot(&["create", name, "--dans", &home, "--depuis", &src], Duration::from_secs(120)).await
        }
        None => dot(&["create", name, "--dans", &home], Duration::from_secs(30 * 60)).await,
    }
}

pub async fn remove(dir: &str) -> Result<String> {
    dot(&["remove", &path_str(&dot_dir(dir)?), "--yes"], Duration::from_secs(120)).await
}

/// Allumer peut construire l'image au premier lancement : on laisse du temps.
pub async fn start(dir: &str) -> Result<String> {
    dot(&[&path_str(&dot_dir(dir)?), "start"], Duration::from_secs(30 * 60)).await
}

pub async fn stop(dir: &str) -> Result<String> {
    dot(&[&path_str(&dot_dir(dir)?), "stop"], Duration::from_secs(90)).await
}

/// À la fermeture de l'app : éteint tous les dots allumés (ils reprendront là où ils en étaient),
/// en parallèle. Bloquant, une seule fois, 45 s au plus.
pub fn stop_all_blocking() {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let Some(cli) = cli() else { return };
    let command = |args: &[&str]| {
        let mut c = std::process::Command::new(&cli);
        c.args(args)
            .env("PATH", crate::pi::extended_path())
            .env("DOTS_HOME", home())
            .current_dir(cli.parent().unwrap_or(Path::new("/")))
            .stdin(Stdio::null());
        c
    };
    let Ok(out) = command(&["list", "--json"]).stderr(Stdio::null()).output() else { return };
    let dots: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let on: Vec<String> = dots
        .iter()
        .filter(|d| d["running"] == true || d["vault"] == true)
        .filter_map(|d| d["dir"].as_str().map(str::to_string))
        .collect();
    if on.is_empty() {
        return;
    }
    applog!("fermeture : extinction de {} dot(s)", on.len());
    let mut children: Vec<_> = on
        .iter()
        .filter_map(|dir| command(&[dir, "stop"]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok())
        .collect();
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    while std::time::Instant::now() < deadline {
        children.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        if children.is_empty() {
            applog!("fermeture : dots éteints");
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    applog!("fermeture : {} dot(s) ne se sont pas éteints à temps", children.len());
}

/// Avant de lancer `dot <dossier> rpc` : le dot tourne, en mode RPC, et son socket est là (il
/// apparaît quelques secondes après l'allumage).
pub async fn ready(dir: &str) -> Result<PathBuf> {
    let d = dot_dir(dir)?;
    // Juste après l'allumage, le moteur peut mettre un instant à le voir en marche.
    let mut st = status(&d).await?;
    for _ in 0..20 {
        if st["running"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        st = status(&d).await?;
    }
    let name = st["name"].as_str().unwrap_or("?").to_string();
    anyhow::ensure!(st["running"] == true, "le dot « {name} » est éteint : allume-le d'abord");
    anyhow::ensure!(
        st["interface"] == "rpc",
        "le dot « {name} » est en mode terminal : passe-le en mode RPC (menu du dot) pour lui parler depuis l'app"
    );
    let socket = PathBuf::from(st["socket"].as_str().unwrap_or_default());
    for _ in 0..120 {
        if socket.exists() {
            return Ok(d);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(anyhow!("le dot « {name} » ne répond pas (pas de socket {})", socket.display()))
}

/// `DOT_INTERFACE: rpc` dans le nœud `env` du `.dot`, sans toucher au reste (commentaires, vault).
fn set_rpc(text: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    if let Some(l) = lines.iter_mut().find(|l| l.trim_start().starts_with("DOT_INTERFACE:")) {
        let indent: String = l.chars().take_while(|c| c.is_whitespace()).collect();
        *l = format!("{indent}DOT_INTERFACE: rpc");
    } else if let Some(i) = lines.iter().position(|l| l.trim_end() == "env:") {
        // Même retrait que la première ligne du nœud.
        let indent = lines
            .get(i + 1)
            .filter(|l| l.starts_with(char::is_whitespace))
            .map(|l| l.chars().take_while(|c| c.is_whitespace()).collect())
            .unwrap_or_else(|| "  ".to_string());
        lines.insert(i + 1, format!("{indent}DOT_INTERFACE: rpc"));
    } else {
        lines.push("env:".into());
        lines.push("  DOT_INTERFACE: rpc".into());
    }
    lines.join("\n") + "\n"
}

/// Passe un dot en mode RPC ; il faut le rallumer ensuite.
pub fn use_rpc(dir: &str) -> Result<()> {
    let d = dot_dir(dir)?;
    let file = dotfile_of(&d).unwrap();
    let text = std::fs::read_to_string(&file).with_context(|| format!("lecture de {}", file.display()))?;
    std::fs::write(&file, set_rpc(&text))?;
    applog!("dots : {} passé en mode RPC", file.display());
    Ok(())
}

/// Range dans l'app (`data/dots/`) un dot qui vit ailleurs. Il doit être éteint : ses conteneurs
/// portent l'empreinte de son dossier.
pub async fn import(dir: &str) -> Result<String> {
    let d = dot_dir(dir)?;
    anyhow::ensure!(d.parent() != Some(home().as_path()), "ce dot est déjà dans l'app");
    let st = status(&d).await?;
    anyhow::ensure!(st["running"] != true && st["vault"] != true, "éteins-le d'abord");
    let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    anyhow::ensure!(valid_name(&name), "nom de dossier invalide : {name}");
    let dest = home().join(&name);
    anyhow::ensure!(!dest.exists(), "un dot « {name} » existe déjà dans l'app");
    std::fs::create_dir_all(home())?;
    std::fs::rename(&d, &dest).with_context(|| format!("déplacement vers {}", dest.display()))?;
    applog!("dots : {} rangé dans {}", d.display(), dest.display());
    status(&dest).await?; // l'enregistre auprès du script à sa nouvelle place
    Ok(path_str(&dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("dot"));
        assert!(valid_name("travail-2"));
        assert!(!valid_name(""));
        assert!(!valid_name("2abc"));
        assert!(!valid_name("Abc"));
        assert!(!valid_name("a b"));
        assert!(!valid_name("a;rm"));
        assert!(!valid_name(&"a".repeat(32)));
    }

    #[test]
    fn rpc_in_dotfile() {
        let with = "---\nname: a\n---\nenv:\n  DOT_MODEL: x\n  DOT_INTERFACE: tui   # avant\nvault:\n  secrets: {}\n";
        assert_eq!(set_rpc(with), with.replace("  DOT_INTERFACE: tui   # avant", "  DOT_INTERFACE: rpc"));
        let without = "---\nname: a\n---\n# réglages\nenv:\n    DOT_MODEL: x\nvault: {}\n";
        assert_eq!(set_rpc(without), "---\nname: a\n---\n# réglages\nenv:\n    DOT_INTERFACE: rpc\n    DOT_MODEL: x\nvault: {}\n");
        assert!(set_rpc("---\nname: a\n---\n").ends_with("env:\n  DOT_INTERFACE: rpc\n"));
    }

    #[test]
    fn description_from_header_only() {
        let dir = std::env::temp_dir().join(format!("dots-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.dot");
        std::fs::write(&f, "---\nname: a\ndescription: \"Mon assistant\"\n---\nvault:\n  description: secret\n").unwrap();
        assert_eq!(description(&f), "Mon assistant");
        std::fs::write(&f, "---\nname: a\n---\ndescription: pas dans l'en-tête\n").unwrap();
        assert_eq!(description(&f), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod live {
    /// Avec un vrai dépôt dots voisin : `cargo test dots_live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn dots_live_list() {
        let list = super::list().await.expect("dot list --json");
        assert!(list.as_array().is_some_and(|a| a.iter().all(|d| d["name"].is_string() && d["dir"].is_string())));
        println!("{list:#}");
    }
}

#[cfg(test)]
mod errors {
    #[test]
    fn summary_skips_node_stack() {
        let e = "file:///opt/dot/dotfile.mjs:42\n    throw new Error(`clé « foo » mal formée`);\n    ^\n\nError: clé « foo » mal formée\n    at parse (file:///opt/dot/dotfile.mjs:42:11)\n    at node:internal/main:1:1\n}\n\nNode.js v24.22.1\nLe fichier /x/a.dot ne peut pas être utilisé tel quel (voir ci-dessus).\n";
        let s = super::error_summary(e);
        assert!(s.contains("Error: clé « foo » mal formée"), "{s}");
        assert!(s.ends_with("(voir ci-dessus)."));
        assert!(!s.contains("Node.js") && !s.contains(" at "));
    }
}
