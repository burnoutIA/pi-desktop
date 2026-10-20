#[macro_use]
mod applog;
mod audio;
mod files;
mod pi;
mod servers;
mod sessions;
mod speech;
mod term;
mod voice;

use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use voice::{VoiceConfig, VoiceEngine};

struct AppState {
    pi: pi::PiManager,
    terms: term::Terms,
    voice: Arc<VoiceEngine>,
    watcher: Mutex<Option<files::Watcher>>,
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Ouvre un projet : lance pi dans ce dossier et surveille ses fichiers.
#[tauri::command]
async fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    cwd: String,
    agent_name: Option<String>,
    resume: Option<bool>,
) -> Res<()> {
    if !std::path::Path::new(&cwd).is_dir() {
        return Err(format!("Le dossier « {cwd} » n'existe pas. Ouvre un projet avec Ctrl+O."));
    }
    let voice = state.voice.clone();
    let hook: pi::EventHook = Arc::new(move |ev: &Value| voice.on_pi_event(ev));
    state.pi.start(app.clone(), cwd.clone(), agent_name, resume.unwrap_or(false), hook).await.map_err(err)?;
    let w = files::watch(app, &cwd).map_err(err)?;
    *state.watcher.lock().unwrap() = Some(w);
    Ok(())
}

#[tauri::command]
async fn pi_send(state: State<'_, AppState>, cmd: Value) -> Res<Value> {
    state.pi.send(cmd).await.map_err(err)
}

/// Réponse à un dialogue d'extension de pi (pas de réponse attendue en retour).
#[tauri::command]
async fn pi_write(state: State<'_, AppState>, record: Value) -> Res<()> {
    state.pi.write(record).await.map_err(err)
}

#[tauri::command]
fn voice_listen(state: State<'_, AppState>, on: bool, submit: bool) {
    state.voice.set_listening(on, submit);
}

#[tauri::command]
fn voice_stop_speaking(state: State<'_, AppState>) {
    state.voice.stop_speaking();
}

#[tauri::command]
fn voice_speak(state: State<'_, AppState>, text: String) {
    state.voice.speak_text(&text);
}

/// Agent vocal : ce que tu viens de dire, avec l'état de pi vu par l'interface.
#[tauri::command]
fn voice_agent_ask(state: State<'_, AppState>, text: String, pi_state: String, pi_model: Option<String>) {
    state.voice.agent_ask(text, pi_state, pi_model);
}

#[tauri::command]
fn voice_agent_reset(state: State<'_, AppState>) {
    state.voice.agent_reset();
}

#[tauri::command]
fn voice_get_config(state: State<'_, AppState>) -> VoiceConfig {
    state.voice.config()
}

#[tauri::command]
fn voice_set_config(state: State<'_, AppState>, config: VoiceConfig) {
    state.voice.set_config(config);
}

#[tauri::command]
fn voice_stt_connected(state: State<'_, AppState>) -> bool {
    state.voice.stt_connected()
}

#[tauri::command]
fn list_files(root: String) -> Res<Vec<String>> {
    files::list_files(&root).map_err(err)
}

#[tauri::command]
fn git_status(root: String) -> Res<files::GitStatus> {
    files::git_status(&root).map_err(err)
}

#[tauri::command]
fn git_diff(root: String, path: String) -> Res<String> {
    files::git_diff(&root, &path).map_err(err)
}

#[tauri::command]
fn read_file(root: String, path: String) -> Res<String> {
    files::read_file(&root, &path).map_err(err)
}

#[tauri::command]
fn git_init(root: String) -> Res<()> {
    files::git_init(&root).map_err(err)
}

#[derive(serde::Serialize)]
struct PromptFile {
    path: String,
    content: String,
    exists: bool,
}

/// Lit le prompt système personnel (`scope` = "global" ou "project").
#[tauri::command]
fn read_prompt(scope: String, root: Option<String>) -> Res<PromptFile> {
    let path = pi::prompt_path(&scope, root.as_deref()).map_err(err)?;
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    Ok(PromptFile { path: path.to_string_lossy().into_owned(), exists: path.exists(), content })
}

/// Écrit le prompt système. Un contenu vide supprime le fichier projet (le global reprend la main).
#[tauri::command]
fn write_prompt(scope: String, root: Option<String>, content: String) -> Res<()> {
    let path = pi::prompt_path(&scope, root.as_deref()).map_err(err)?;
    if scope == "project" && content.trim().is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(err)?;
        }
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    std::fs::write(&path, content).map_err(err)?;
    applog!("prompt système enregistré : {}", path.display());
    Ok(())
}

/// Sessions de pi dans le même dossier que la session active.
#[tauri::command]
fn list_sessions(current: String) -> Vec<sessions::SessionInfo> {
    sessions::list(&current)
}

#[tauri::command]
async fn generate_title(cwd: String, model: Option<String>, user: String, assistant: String) -> Res<String> {
    sessions::generate_title(&cwd, model.as_deref(), &user, &assistant).await.map_err(err)
}

#[tauri::command]
fn trash_session(path: String) -> Res<()> {
    sessions::trash(&path).map_err(err)
}

/// Serveurs vocaux, gérés par l'app (une seule instance pour tout le processus).
static SERVERS: std::sync::LazyLock<servers::Servers> = std::sync::LazyLock::new(servers::Servers::new);

#[tauri::command]
fn voice_servers_state() -> servers::State {
    SERVERS.state()
}

#[derive(serde::Serialize)]
struct PiStatus {
    installed: bool,
    path: Option<String>,
    version: Option<String>,
}

/// Le pi de l'app (runtime/pi) est-il installé ?
#[tauri::command]
fn pi_status() -> PiStatus {
    match pi::pi_version() {
        Some((path, version)) => PiStatus { installed: true, path: Some(path), version: Some(version) },
        None => PiStatus { installed: false, path: None, version: None },
    }
}

#[tauri::command]
fn auth_mtime() -> u64 {
    pi::auth_mtime()
}

/// Lance un programme dans un terminal intégré ; « pi » désigne le pi de l'app.
#[tauri::command]
fn term_spawn(app: AppHandle, state: State<'_, AppState>, program: String, args: Vec<String>, cwd: Option<String>, cols: u16, rows: u16) -> Res<u32> {
    let program = if program == "pi" {
        pi::find_pi().map(|p| p.to_string_lossy().into_owned()).ok_or("pi introuvable")?
    } else if program.starts_with("scripts/") {
        // Scripts du projet pi-desktop (ex. scripts/setup-pi.sh)
        servers::project_root().ok_or("dossier du projet introuvable")?.join(&program).to_string_lossy().into_owned()
    } else {
        program
    };
    state.terms.spawn(app, &program, &args, cwd.as_deref(), cols, rows).map_err(err)
}

#[tauri::command]
fn term_write(state: State<'_, AppState>, id: u32, data: String) -> Res<()> {
    state.terms.write(id, &data).map_err(err)
}

#[tauri::command]
fn term_resize(state: State<'_, AppState>, id: u32, cols: u16, rows: u16) -> Res<()> {
    state.terms.resize(id, cols, rows).map_err(err)
}

#[tauri::command]
fn term_kill(state: State<'_, AppState>, id: u32) {
    state.terms.kill(id);
}

#[derive(serde::Serialize)]
struct Attachment {
    name: String,
    path: String,
    size: u64,
    /// Type MIME d'une image (sinon None : le fichier sera transmis par son chemin).
    mime: Option<String>,
    /// Image encodée en base64 (envoyée telle quelle au modèle).
    data: Option<String>,
}

/// Prépare une pièce jointe : les images sont lues et encodées, les autres fichiers gardent leur chemin.
#[tauri::command]
fn read_attachment(path: String) -> Res<Attachment> {
    use base64::Engine;
    let p = std::path::Path::new(&path);
    let meta = std::fs::metadata(p).map_err(err)?;
    if !meta.is_file() {
        return Err("ce n'est pas un fichier".into());
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let mime = match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    };
    let data = match mime {
        Some(_) if meta.len() <= 20_000_000 => {
            Some(base64::engine::general_purpose::STANDARD.encode(std::fs::read(p).map_err(err)?))
        }
        Some(_) => return Err("image trop lourde (20 Mo maximum)".into()),
        None => None,
    };
    Ok(Attachment {
        name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path,
        size: meta.len(),
        mime: mime.map(str::to_string),
        data,
    })
}

/// Erreur JavaScript remontée par l'interface.
#[tauri::command]
fn log_frontend(msg: String) {
    applog!("[interface] {msg}");
}

/// Banc d'essai de l'affichage (PIVOICE_BENCH=1) : voix simulée, mesures dans le journal.
#[tauri::command]
fn bench_mode() -> bool {
    std::env::var("PIVOICE_BENCH").is_ok_and(|v| v == "1")
}

#[tauri::command]
fn initial_dir() -> Option<String> {
    std::env::args().nth(1).or_else(|| std::env::var("PI_DESKTOP_CWD").ok())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    applog::init();
    applog!("démarrage de Pi Voice");
    pi::init_agent_dir();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            SERVERS.ensure(app.handle().clone());
            // Arrêt par signal (kill, fermeture de session) : on arrête aussi les serveurs.
            tauri::async_runtime::spawn(async {
                use tokio::signal::unix::{signal, SignalKind};
                if let (Ok(mut term), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt())) {
                    tokio::select! { _ = term.recv() => {}, _ = int.recv() => {} }
                    SERVERS.shutdown();
                    std::process::exit(0);
                }
            });
            let voice = VoiceEngine::new(app.handle().clone());
            voice.start();
            app.manage(AppState { pi: pi::PiManager::new(), terms: term::Terms::default(), voice, watcher: Mutex::new(None) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_project,
            pi_send,
            pi_write,
            voice_listen,
            voice_stop_speaking,
            voice_speak,
            voice_agent_ask,
            voice_agent_reset,
            voice_get_config,
            voice_set_config,
            voice_stt_connected,
            list_files,
            git_status,
            git_diff,
            read_file,
            git_init,
            initial_dir,
            log_frontend,
            read_prompt,
            write_prompt,
            list_sessions,
            trash_session,
            generate_title,
            voice_servers_state,
            pi_status,
            auth_mtime,
            term_spawn,
            term_write,
            term_resize,
            term_kill,
            read_attachment,
            bench_mode,
        ])
        .build(tauri::generate_context!())
        .expect("erreur au lancement de l'application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                SERVERS.shutdown();
            }
        });
}
