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

/// Consigne ajoutée au prompt système de pi pour produire un résumé à lire à voix haute.
pub const VOICE_PROMPT: &str = "Tu es piloté à la voix depuis une application de bureau. \
Les messages de l'utilisateur sont des transcriptions vocales automatiques : ils peuvent contenir \
des erreurs de reconnaissance, interprète-les avec bon sens. \
À la toute fin de ta réponse finale (quand tu as fini de travailler), ajoute un court résumé \
destiné à être lu à voix haute, entre les balises <oral> et </oral>. Règles pour ce résumé : \
en français, une à trois phrases naturelles et parlées, sans markdown, sans code, sans liste, \
sans URL, sans chemin complet (dis juste le nom du fichier). N'utilise ces balises qu'une fois, à la fin.";

/// Consigne vocale, avec le nom de l'agent s'il en a un.
fn voice_prompt(name: Option<&str>) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!(
            "Tu t'appelles {n} ; tu n'as pas besoin de le répéter. {VOICE_PROMPT}"
        ),
        None => VOICE_PROMPT.to_string(),
    }
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

pub type EventHook = Arc<dyn Fn(&Value) + Send + Sync>;

struct PiProcess {
    child: Arc<Mutex<Child>>,
    stdin: ChildStdin,
}

type Pending = Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<Value>>>>;

pub struct PiManager {
    proc: Mutex<Option<PiProcess>>,
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


impl PiManager {
    pub fn new() -> Self {
        Self {
            proc: Mutex::new(None),
            pending: Arc::new(std::sync::Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Lance pi dans `cwd` (et arrête l'instance précédente).
    pub async fn start(
        &self,
        app: AppHandle,
        cwd: String,
        agent_name: Option<String>,
        resume: bool,
        hook: EventHook,
    ) -> Result<()> {
        self.stop().await;
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let program = pi_binary();
        applog!("lancement de pi : {program} --mode rpc (dossier {cwd})");
        let mut child = Command::new(&program)
            .env("PATH", extended_path())
            .args(["--mode", "rpc", "--append-system-prompt", &voice_prompt(agent_name.as_deref())])
            .args(resume.then_some("--continue"))
            .current_dir(&cwd)
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
                hook(&value);
                let _ = app_out.emit("pi-event", &value);
            }
            if generation.load(Ordering::SeqCst) == gen {
                let code = tokio::time::timeout(Duration::from_secs(3), async { child_out.lock().await.wait().await })
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "inconnu".into());
                let tail = last_err_out.lock().unwrap().join("\n");
                applog!("pi s'est arrêté ({code}). Dernières erreurs :\n{tail}");
                let _ = app_out.emit("pi-exit", json!({ "code": code, "stderr": tail }));
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

    /// Envoie une commande RPC et attend sa réponse (corrélée par `id`).
    pub async fn send(&self, mut cmd: Value) -> Result<Value> {
        let id = format!("ui-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
        cmd["id"] = Value::String(id.clone());
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);

        let mut line = serde_json::to_vec(&cmd)?;
        line.push(b'\n');
        {
            let mut guard = self.proc.lock().await;
            let p = guard.as_mut().ok_or_else(|| anyhow!("pi n'est pas démarré"))?;
            applog!("→ pi : {}", cmd["type"]);
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
