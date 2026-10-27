//! Pilote `pi --mode rpc` : un processus enfant qui parle en JSONL sur stdin/stdout.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex};

/// Consigne ajoutée au prompt système de pi : il est piloté à la voix (dictée transcrite).
pub const VOICE_PROMPT: &str = "Tu es piloté à la voix depuis une application de bureau. \
Les messages de l'utilisateur sont souvent des transcriptions vocales automatiques : ils peuvent contenir \
des erreurs de reconnaissance, interprète-les avec bon sens.";

/// Consigne vocale, avec le nom de l'agent s'il en a un, suivie de la consigne sur les extensions.
fn voice_prompt(name: Option<&str>) -> String {
    let base = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!(
            "Tu t'appelles {n} ; tu n'as pas besoin de le répéter. {VOICE_PROMPT}"
        ),
        None => VOICE_PROMPT.to_string(),
    };
    format!("{base}\n\n{}", extensions_prompt())
}

/// Où pi doit ranger les extensions qu'il crée ou installe : dans ses extensions globales (hors git),
/// jamais dans `.pi/` du projet ouvert, ni dans `pi-extensions/` (réservé à celles livrées avec l'app).
fn extensions_prompt() -> String {
    format!(
        "Quand tu crées ou installes une extension pi, range-la dans tes extensions globales ({}) \
et installe les paquets sans --local, sauf si l'utilisateur demande explicitement une extension \
propre au projet (.pi/ du projet).",
        agent_dir().join("extensions").display()
    )
}

/// Balise qui entoure la consigne vocale ajoutée aux messages envoyés à un dot (l'interface la
/// masque : voir APP_NOTE dans session.svelte.ts).
const DOT_NOTE_TAG: &str = "pi-voice";

/// Consigne vocale pour un dot : ses options de démarrage lui appartiennent, elle part donc avec
/// chaque message envoyé depuis l'app.
fn dot_voice_note(name: Option<&str>) -> String {
    let who = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!(" Dans cette application, on t'appelle {n}."),
        None => String::new(),
    };
    format!(
        "\n\n<{DOT_NOTE_TAG}>Consigne de l'application (ne la commente pas) : ce message vient de \
l'application vocale de bureau, pas de Telegram.{who} {VOICE_PROMPT}</{DOT_NOTE_TAG}>"
    )
}

/// Fichier de consignes personnelles que pi ajoute à son prompt système.
/// Global : `<agent-dir>/APPEND_SYSTEM.md`. Projet : `.pi/APPEND_SYSTEM.md` (prioritaire, non cumulé).
pub fn prompt_path(scope: &str, root: Option<&str>) -> Result<std::path::PathBuf> {
    match scope {
        "project" => {
            let root = root.filter(|r| !r.is_empty()).ok_or_else(|| anyhow!("aucun projet ouvert"))?;
            Ok(std::path::Path::new(root).join(".pi/APPEND_SYSTEM.md"))
        }
        _ => Ok(agent_dir().join("APPEND_SYSTEM.md")),
    }
}

/// Ce qu'un run fait tourner : le pi de l'app sur un dossier, ou un dot joint par `dot @nom rpc`.
pub enum Launch {
    /// `session` : fichier de session à reprendre ; sinon `resume` reprend la dernière (--continue).
    Local { cwd: String, resume: bool, session: Option<String>, extra_prompt: Option<String> },
    /// `dot <dossier> rpc` : même protocole, mais la configuration (session, extensions, prompt)
    /// appartient au dot.
    Dot { cli: std::path::PathBuf, dir: std::path::PathBuf },
}

struct PiProcess {
    child: Arc<Mutex<Child>>,
    stdin: ChildStdin,
}

type Pending = Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<Value>>>>;

/// Un pi : un processus `pi --mode rpc` pour un run (un projet, ou une branche de travail).
pub struct PiInstance {
    run: String,
    /// Emplacement de profil Chromium (Playwright) réservé à ce run : 0 = data/playwright/profile.
    slot: usize,
    /// pi travaille (entre agent_start et agent_settled) : fermer l'app demandera confirmation.
    busy: Arc<std::sync::atomic::AtomicBool>,
    proc: Mutex<Option<PiProcess>>,
    /// Consigne ajoutée aux messages (dot seulement : il ignore les options de démarrage).
    note: std::sync::Mutex<Option<String>>,
    pending: Pending,
    next_id: AtomicU64,
    generation: Arc<AtomicU64>,
}

/// Dossiers où l'installeur officiel (et npm, snap…) dépose `pi`. Une app lancée depuis le
/// menu n'a pas forcément ~/.local/bin ou ~/.pi/agent/bin dans son PATH : on les ajoute.
fn extra_bin_dirs() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    ["/.pi/agent/bin", "/.local/bin", "/bin", "/.local/node/bin", "/.npm-global/bin"]
        .iter()
        .map(|d| format!("{home}{d}"))
        .chain(["/snap/bin".to_string(), "/usr/local/bin".to_string()])
        .collect()
}

/// PATH courant complété des dossiers où pi peut être installé.
pub fn extended_path() -> String {
    let mut dirs = extra_bin_dirs();
    if let Ok(p) = std::env::var("PATH") {
        dirs.extend(p.split(':').map(str::to_string));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| !d.is_empty() && seen.insert(d.clone()));
    dirs.join(":")
}

/// Le pi propre à l'app : installé dans `runtime/pi` du projet par `scripts/setup-pi.sh`,
/// jamais le pi global. `PI_BIN` permet d'en imposer un autre.
pub fn find_pi() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("PI_BIN") {
        return Some(p.into());
    }
    let bin = crate::servers::project_root()?.join("runtime/pi/node_modules/.bin/pi");
    bin.is_file().then_some(bin)
}

/// Dossier de config de pi propre à l'app (`data/pi-agent` du projet), sauf si
/// `PI_CODING_AGENT_DIR` en impose un autre.
pub fn agent_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("PI_CODING_AGENT_DIR") {
        return d.into();
    }
    match crate::servers::project_root() {
        Some(root) => root.join("data/pi-agent"),
        None => std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".pi/agent"),
    }
}

/// Crée le dossier de config au premier lancement en reprenant la connexion et les modèles du
/// pi global (`~/.pi/agent`), puis le transmet à tous les pi lancés par l'app. Les sessions ne
/// sont pas reprises. À appeler au démarrage, avant tout autre thread.
pub fn init_agent_dir() {
    let dir = agent_dir();
    if !dir.exists() {
        let global = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".pi/agent");
        match std::fs::create_dir_all(&dir) {
            Ok(()) => {
                for f in ["auth.json", "models.json", "settings.json"] {
                    if std::fs::copy(global.join(f), dir.join(f)).is_ok() {
                        applog!("config de pi : {f} repris de {}", global.display());
                    }
                }
            }
            Err(e) => applog!("impossible de créer {} : {e}", dir.display()),
        }
    }
    std::env::set_var("PI_CODING_AGENT_DIR", &dir);
    if let Err(e) = write_mcp_config(&dir) {
        applog!("config MCP de pi : {e:#}");
    }
}

/// Extensions de pi propres à l'app (dossier `pi-extensions/` du projet), passées avec `--extension` :
/// chaque `*.ts` du dossier, et chaque sous-dossier qui contient un `index.ts`.
fn app_extensions() -> Vec<String> {
    let Some(dir) = crate::servers::project_root().map(|r| r.join("pi-extensions")) else { return vec![] };
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| {
            if p.is_dir() {
                Some(p.join("index.ts")).filter(|i| i.is_file())
            } else {
                Some(p).filter(|p| p.extension().is_some_and(|x| x == "ts"))
            }
        })
        .collect();
    files.sort();
    files.into_iter().flat_map(|p| ["--extension".to_string(), p.to_string_lossy().into_owned()]).collect()
}

/// Outils installés avec pi par `scripts/setup-pi.sh` : (ketch, serveur MCP de Playwright).
fn tool_paths() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let rt = crate::servers::project_root()?.join("runtime");
    Some((rt.join("ketch/ketch"), rt.join("playwright/node_modules/.bin/playwright-mcp")))
}

/// Les outils de pi sont-ils tous installés ? Sinon l'assistant de démarrage relance l'installation.
pub fn tools_installed() -> bool {
    tool_paths().is_some_and(|(k, p)| k.is_file() && p.is_file())
}

/// Branche les outils de l'app dans `<agent-dir>/mcp.json` : ketch (recherche web) et Playwright
/// (navigateur). Réécrit à chaque démarrage, sans toucher aux autres serveurs du fichier.
fn write_mcp_config(dir: &std::path::Path) -> Result<()> {
    let (ketch, playwright) = tool_paths().ok_or_else(|| anyhow!("dossier du projet introuvable"))?;
    let data = dir.parent().unwrap_or(dir);
    let file = dir.join("mcp.json");
    let mut config: Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    if !config.is_object() {
        config = json!({});
    }
    if !config["mcpServers"].is_object() {
        config["mcpServers"] = json!({});
    }
    let servers = config["mcpServers"].as_object_mut().unwrap();
    servers.remove("ketch");
    servers.remove("playwright");
    if ketch.is_file() {
        // Config et marque-pages de ketch isolés dans data/ketch, comme harn.
        let kdir = data.join("ketch");
        std::fs::create_dir_all(&kdir)?;
        servers.insert(
            "ketch".into(),
            json!({
                "command": ketch,
                "args": ["mcp", "serve"],
                "env": {
                    "KETCH_CONFIG": kdir.join("config.json"),
                    "KETCH_TAGS_PATH": kdir.join("tags.db"),
                },
                "timeout": 120,
                "exposure": "direct",
                "toolExposure": { "crawl": "hidden", "tag": "hidden" },
                "description": "Recherche web, documentation de librairies, code open source et lecture de pages web (ketch, sans clé API).",
            }),
        );
    }
    if playwright.is_file() {
        // Navigateur visible : la personne voit pi naviguer et peut cliquer ou taper dans sa
        // fenêtre. Profil gardé dans data/playwright/profile* (une connexion faite pour pi le reste).
        // Beaucoup d'outils : chargés à la demande (tool_search) pour ne pas alourdir le prompt.
        servers.insert(
            "playwright".into(),
            json!({
                "command": playwright,
                "args": ["--browser", "chromium", "--output-dir", data.join("playwright")],
                // Profil propre à chaque run (variable posée au lancement de pi, voir PiInstance).
                "env": { "PLAYWRIGHT_MCP_USER_DATA_DIR": "${PI_DESKTOP_PW_PROFILE}" },
                "timeout": 120,
                "exposure": "deferred",
                "description": "Navigateur web piloté (Playwright) : ouvrir une page, cliquer, remplir un formulaire, faire une capture, tester une interface. Il s'ouvre dans une fenêtre visible : la personne voit ce que fait pi et peut y cliquer ou taper.",
            }),
        );
    }
    if config.get("autoEnableCodemode").is_none() {
        config["autoEnableCodemode"] = json!(false);
    }
    std::fs::write(&file, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

fn pi_binary() -> String {
    find_pi().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "pi".into())
}

/// Version de pi installée, ou None si pi est introuvable.
pub fn pi_version() -> Option<(String, String)> {
    let path = find_pi()?;
    let out = std::process::Command::new(&path).arg("--version").output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some((path.to_string_lossy().into_owned(), v))
}

/// Date de modification de auth.json (identifiants des fournisseurs), pour voir une connexion aboutir.
pub fn auth_mtime() -> u64 {
    std::fs::metadata(agent_dir().join("auth.json"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}


impl PiInstance {
    fn new(run: String, slot: usize) -> Self {
        Self {
            run,
            slot,
            busy: Arc::default(),
            proc: Mutex::new(None),
            note: std::sync::Mutex::new(None),
            pending: Arc::new(std::sync::Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Lance pi (ou rejoint un dot) et arrête l'instance précédente.
    pub async fn start(&self, app: AppHandle, launch: Launch, agent_name: Option<String>) -> Result<()> {
        self.stop().await;
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let mut command = match &launch {
            Launch::Local { cwd, resume, session, extra_prompt } => {
                *self.note.lock().unwrap() = None;
                self.local_command(cwd, *resume, session.as_deref(), extra_prompt.as_deref(), agent_name.as_deref())
            }
            Launch::Dot { cli, dir } => {
                *self.note.lock().unwrap() = Some(dot_voice_note(agent_name.as_deref()));
                applog!("connexion au dot [{}] : {} {} rpc", self.run, cli.display(), dir.display());
                let mut c = Command::new(cli);
                c.env("PATH", extended_path())
                    .arg(dir)
                    .arg("rpc")
                    .current_dir(cli.parent().unwrap_or(std::path::Path::new("/")));
                c
            }
        };
        let program = match &launch {
            Launch::Local { .. } => pi_binary(),
            Launch::Dot { cli, .. } => cli.to_string_lossy().into_owned(),
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("impossible de lancer `{program}`"))
            .inspect_err(|e| applog!("échec du lancement de pi : {e:#}"))?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("pas de stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("pas de stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("pas de stderr"))?;

        // stdout : enregistrements JSONL. On découpe uniquement sur LF (pas de `lines()`
        // sur les séparateurs Unicode, comme le demande la doc de pi).
        let child = Arc::new(Mutex::new(child));
        let last_err: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let pending = self.pending.clone();
        let generation = self.generation.clone();
        let app_out = app.clone();
        let run_out = self.run.clone();
        let busy_out = self.busy.clone();
        let child_out = child.clone();
        let last_err_out = last_err.clone();
        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::with_capacity(8192);
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                if buf.is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_slice::<Value>(&buf) else {
                    applog!("[pi] ligne non JSON ignorée : {}", String::from_utf8_lossy(&buf).chars().take(200).collect::<String>());
                    continue;
                };
                if value["type"] == "response" {
                    if let Some(id) = value["id"].as_str() {
                        if let Some(tx) = pending.lock().unwrap().remove(id) {
                            let _ = tx.send(value);
                            continue;
                        }
                    }
                }
                match value["type"].as_str() {
                    Some("agent_start") => busy_out.store(true, Ordering::SeqCst),
                    Some("agent_settled") => busy_out.store(false, Ordering::SeqCst),
                    _ => {}
                }
                let _ = app_out.emit("pi-event", json!({ "run": run_out, "event": value }));
            }
            busy_out.store(false, Ordering::SeqCst);
            if generation.load(Ordering::SeqCst) == gen {
                let code = tokio::time::timeout(Duration::from_secs(3), async { child_out.lock().await.wait().await })
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "inconnu".into());
                let tail = last_err_out.lock().unwrap().join("\n");
                applog!("pi [{run_out}] s'est arrêté ({code}). Dernières erreurs :\n{tail}");
                let _ = app_out.emit("pi-exit", json!({ "run": run_out, "code": code, "stderr": tail }));
            }
        });

        let app_err = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = String::from_utf8_lossy(&line).trim_end().to_string();
                        if !text.is_empty() {
                            applog!("[pi stderr] {text}");
                            let mut l = last_err.lock().unwrap();
                            l.push(text.clone());
                            if l.len() > 12 {
                                l.remove(0);
                            }
                            let _ = app_err.emit("pi-stderr", text);
                        }
                    }
                }
            }
        });

        *self.proc.lock().await = Some(PiProcess { child, stdin });
        Ok(())
    }

    /// `pi --mode rpc` de l'app dans `cwd`, avec sa consigne vocale, ses extensions et ses outils.
    fn local_command(&self, cwd: &str, resume: bool, session: Option<&str>, extra_prompt: Option<&str>, agent_name: Option<&str>) -> Command {
        let program = pi_binary();
        applog!("lancement de pi [{}] : {program} --mode rpc (dossier {cwd})", self.run);
        let profile = agent_dir().parent().unwrap_or(std::path::Path::new(".")).join(match self.slot {
            0 => "playwright/profile".to_string(),
            n => format!("playwright/profile-{}", n + 1),
        });
        let mut c = Command::new(&program);
        c.env("PATH", extended_path())
            .args(["--mode", "rpc", "--append-system-prompt", &match extra_prompt {
                Some(x) => format!("{}\n\n{x}", voice_prompt(agent_name)),
                None => voice_prompt(agent_name),
            }])
            .args(match session {
                Some(f) => vec!["--session".to_string(), f.to_string()],
                None if resume => vec!["--continue".to_string()],
                None => vec![],
            })
            // Profil Chromium propre au run : deux Chromium ne peuvent pas partager un profil.
            .env("PI_DESKTOP_PW_PROFILE", &profile)
            // Sorties de ctx-optimizer dans les données de l'app, pas dans `.pi/` du projet ouvert.
            .env("CTX_OPT_DIR", std::env::var("CTX_OPT_DIR").unwrap_or_else(|_| agent_dir().join("ctx-out").to_string_lossy().into_owned()))
            .args(app_extensions())
            .current_dir(cwd);
        // Gérer des dots : le skill du dépôt dots, et le chemin de son script.
        if let Some(cli) = crate::dots::cli() {
            c.env("DOTS_CLI", &cli).env("DOTS_HOME", crate::dots::home());
            if let Some(skill) = crate::dots::skill_dir() {
                c.arg("--skill").arg(skill);
            }
        }
        c
    }

    pub async fn stop(&self) {
        if let Some(p) = self.proc.lock().await.take() {
            self.generation.fetch_add(1, Ordering::SeqCst);
            drop(p.stdin); // fermeture de stdin = arrêt propre demandé
            let mut child = p.child.lock().await;
            if tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_err() {
                let _ = child.kill().await;
            }
        }
    }

    /// Écrit un enregistrement sans attendre de réponse (ex. `extension_ui_response`).
    pub async fn write(&self, record: Value) -> Result<()> {
        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');
        let mut guard = self.proc.lock().await;
        let p = guard.as_mut().ok_or_else(|| anyhow!("pi n'est pas démarré"))?;
        p.stdin.write_all(&line).await?;
        p.stdin.flush().await?;
        Ok(())
    }

    /// Dot : la consigne vocale part avec chaque message (pas avec les commandes « / »).
    fn add_note(&self, cmd: &mut Value) {
        let Some(note) = self.note.lock().unwrap().clone() else { return };
        if !matches!(cmd["type"].as_str(), Some("prompt" | "steer" | "follow_up")) {
            return;
        }
        if let Some(m) = cmd["message"].as_str().filter(|m| !m.trim_start().starts_with('/')) {
            cmd["message"] = Value::String(format!("{m}{note}"));
        }
    }

    /// Envoie une commande RPC et attend sa réponse (corrélée par `id`).
    pub async fn send(&self, mut cmd: Value) -> Result<Value> {
        self.add_note(&mut cmd);
        let id = format!("ui-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
        cmd["id"] = Value::String(id.clone());
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);

        let mut line = serde_json::to_vec(&cmd)?;
        line.push(b'\n');
        {
            let mut guard = self.proc.lock().await;
            let p = guard.as_mut().ok_or_else(|| anyhow!("pi n'est pas démarré"))?;
            applog!("→ pi [{}] : {}", self.run, cmd["type"]);
            p.stdin.write_all(&line).await?;
            p.stdin.flush().await?;
        }

        match tokio::time::timeout(Duration::from_secs(120), rx).await {
            Ok(Ok(v)) => Ok(v),
            _ => {
                self.pending.lock().unwrap().remove(&id);
                Err(anyhow!("pas de réponse de pi pour `{}`", cmd["type"]))
            }
        }
    }
}

/// Tous les pi en cours, un par run. Chacun tourne indépendamment des autres.
pub struct PiManager {
    runs: Mutex<HashMap<String, Arc<PiInstance>>>,
}

impl PiManager {
    pub fn new() -> Self {
        Self { runs: Mutex::new(HashMap::new()) }
    }

    async fn get(&self, run: &str) -> Result<Arc<PiInstance>> {
        self.runs.lock().await.get(run).cloned().ok_or_else(|| anyhow!("pi n'est pas démarré"))
    }

    /// Lance (ou relance) le pi d'un run.
    pub async fn start(&self, app: AppHandle, run: String, launch: Launch, agent_name: Option<String>) -> Result<()> {
        let inst = {
            let mut runs = self.runs.lock().await;
            match runs.get(&run) {
                Some(i) => i.clone(),
                None => {
                    // Premier emplacement de profil Chromium libre.
                    let used: Vec<usize> = runs.values().map(|i| i.slot).collect();
                    let slot = (0..).find(|n| !used.contains(n)).unwrap_or(0);
                    let i = Arc::new(PiInstance::new(run.clone(), slot));
                    runs.insert(run.clone(), i.clone());
                    i
                }
            }
        };
        inst.start(app, launch, agent_name).await
    }

    /// Arrête le pi d'un run et l'oublie.
    pub async fn stop(&self, run: &str) {
        let inst = self.runs.lock().await.remove(run);
        if let Some(i) = inst {
            i.stop().await;
        }
    }

    pub async fn send(&self, run: &str, cmd: Value) -> Result<Value> {
        self.get(run).await?.send(cmd).await
    }

    /// Runs dont le pi travaille en ce moment (appelé à la fermeture, sur le fil principal : pas
    /// d'attente bloquante, la liste n'est verrouillée qu'un instant par ailleurs).
    pub fn busy_runs(&self) -> Vec<String> {
        for _ in 0..200 {
            if let Ok(runs) = self.runs.try_lock() {
                return runs.values().filter(|i| i.busy.load(Ordering::SeqCst)).map(|i| i.run.clone()).collect();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        vec!["?".into()] // état inconnu : mieux vaut demander confirmation
    }

    pub async fn write(&self, run: &str, record: Value) -> Result<()> {
        self.get(run).await?.write(record).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_note_goes_with_messages_only() {
        let inst = PiInstance::new("d:test".into(), 0);
        *inst.note.lock().unwrap() = Some(dot_voice_note(Some("Jarvis")));

        let mut prompt = json!({ "type": "prompt", "message": "bonjour" });
        inst.add_note(&mut prompt);
        let m = prompt["message"].as_str().unwrap();
        assert!(m.starts_with("bonjour\n\n<pi-voice>") && m.ends_with("</pi-voice>"));
        assert!(m.contains("Jarvis") && m.contains("transcriptions vocales"));

        let mut cmd = json!({ "type": "prompt", "message": "/status" });
        inst.add_note(&mut cmd);
        assert_eq!(cmd["message"], "/status");

        let mut other = json!({ "type": "get_messages" });
        inst.add_note(&mut other);
        assert!(other.get("message").is_none());

        // Pi local : aucune consigne ajoutée (elle passe par --append-system-prompt).
        *inst.note.lock().unwrap() = None;
        let mut local = json!({ "type": "prompt", "message": "bonjour" });
        inst.add_note(&mut local);
        assert_eq!(local["message"], "bonjour");
    }
}
