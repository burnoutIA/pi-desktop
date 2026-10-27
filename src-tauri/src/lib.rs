#[macro_use]
mod applog;
mod audio;
mod browser;
mod dots;
mod files;
mod pi;
mod preview;
mod servers;
mod sessions;
mod term;
mod voice;

use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
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

/// Lance (ou relance) le pi d'un run dans `cwd` (le projet, ou la branche de travail du run).
/// `session` : fichier de session à reprendre ; `extra_prompt` : consignes propres au run.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn pi_start(
    app: AppHandle,
    state: State<'_, AppState>,
    run: String,
    cwd: String,
    agent_name: Option<String>,
    resume: Option<bool>,
    session: Option<String>,
    extra_prompt: Option<String>,
) -> Res<()> {
    if !std::path::Path::new(&cwd).is_dir() {
        return Err(format!("Le dossier « {cwd} » n'existe pas. Ouvre un projet avec Ctrl+O."));
    }
    let launch = pi::Launch::Local { cwd, resume: resume.unwrap_or(false), session, extra_prompt };
    state.pi.start(app, run, launch, agent_name).await.map_err(err)
}

/// Rejoint un dot (`dot <dossier> rpc`) : même protocole que pi, sa configuration lui appartient.
#[tauri::command]
async fn pi_start_dot(app: AppHandle, state: State<'_, AppState>, run: String, dir: String, agent_name: Option<String>) -> Res<()> {
    let cli = dots::cli().ok_or("script dot introuvable : indique son chemin dans les réglages (onglet Agent)")?;
    let dir = dots::ready(&dir).await.inspect_err(|e| applog!("dots : {dir} pas prêt : {e:#}")).map_err(err)?;
    state.pi.start(app, run.clone(), pi::Launch::Dot { cli, dir: dir.clone() }, agent_name).await.map_err(err)?;
    // Le connecteur s'arrête aussitôt si le dot refuse : on vérifie que la conversation répond.
    let probe = tokio::time::timeout(std::time::Duration::from_secs(20), state.pi.send(&run, serde_json::json!({ "type": "get_state" }))).await;
    if !matches!(probe, Ok(Ok(_))) {
        state.pi.stop(&run).await;
        applog!("dots : {} ne répond pas sur son socket RPC", dir.display());
        return Err("Le dot ne répond pas.".into());
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DotsInfo {
    /// Script `dot` trouvé (réglages, DOTS_CLI ou dépôt voisin).
    cli: Option<String>,
    /// Le pi local a le skill de gestion des dots.
    skill: bool,
    /// Où vivent les dots de l'app.
    home: String,
}

#[tauri::command]
fn dots_info() -> DotsInfo {
    DotsInfo {
        cli: dots::cli().map(|p| p.to_string_lossy().into_owned()),
        skill: dots::skill_dir().is_some(),
        home: dots::home().to_string_lossy().into_owned(),
    }
}

#[tauri::command]
fn dots_set_cli(path: String) -> Res<()> {
    dots::set_cli(&path).map_err(err)
}

#[tauri::command]
async fn dots_list() -> Res<Value> {
    dots::list().await.map_err(err)
}

/// Nouveau dot dans l'app, vierge (`.dot` à remplir) ou copie vierge du dot du dossier `from`.
#[tauri::command]
async fn dots_create(name: String, from: Option<String>) -> Res<String> {
    dots::create(&name, from.as_deref()).await.map_err(err)
}

/// Allumer, éteindre, supprimer, passer en RPC ou ranger dans l'app le dot d'un dossier.
#[tauri::command]
async fn dots_action(action: String, dir: String) -> Res<String> {
    match action.as_str() {
        "start" => dots::start(&dir).await,
        "stop" => dots::stop(&dir).await,
        "remove" => dots::remove(&dir).await,
        "use-rpc" => dots::use_rpc(&dir).map(|_| String::new()),
        "import" => dots::import(&dir).await,
        _ => return Err(format!("action inconnue : {action}")),
    }
    .map_err(err)
}

/// Arrête le pi d'un run.
#[tauri::command]
async fn pi_stop(state: State<'_, AppState>, run: String) -> Res<()> {
    state.pi.stop(&run).await;
    Ok(())
}

/// Surveille les fichiers du dossier affiché (celui du run visible).
#[tauri::command]
fn watch_dir(app: AppHandle, state: State<'_, AppState>, cwd: String) -> Res<()> {
    let w = files::watch(app, &cwd).map_err(err)?;
    *state.watcher.lock().unwrap() = Some(w);
    Ok(())
}

#[tauri::command]
async fn pi_send(state: State<'_, AppState>, run: String, cmd: Value) -> Res<Value> {
    state.pi.send(&run, cmd).await.map_err(err)
}

/// Réponse à un dialogue d'extension de pi (pas de réponse attendue en retour).
#[tauri::command]
async fn pi_write(state: State<'_, AppState>, run: String, record: Value) -> Res<()> {
    state.pi.write(&run, record).await.map_err(err)
}

/// Quitter pour de bon (après confirmation quand des runs travaillent).
#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn worktree_create(root: String, label: String) -> Res<files::Worktree> {
    files::worktree_create(&root, &label).map_err(err)
}

#[tauri::command]
fn worktree_info(root: String, path: String, branch: String, base: String) -> Res<files::WorktreeInfo> {
    files::worktree_info(&root, &path, &branch, &base).map_err(err)
}

#[tauri::command]
fn worktree_merge(root: String, path: String, branch: String, message: String) -> Res<files::MergeOutcome> {
    files::worktree_merge(&root, &path, &branch, &message).map_err(err)
}

#[tauri::command]
fn worktree_remove(root: String, path: String, branch: String, delete_branch: bool) -> Res<()> {
    files::worktree_remove(&root, &path, &branch, delete_branch).map_err(err)
}

#[tauri::command]
fn voice_listen(state: State<'_, AppState>, on: bool, submit: bool) {
    state.voice.set_listening(on, submit);
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

/// Le pi de l'app (runtime/pi) et ses outils (ketch, Playwright) sont-ils installés ?
#[tauri::command]
fn pi_status() -> PiStatus {
    match pi::pi_version() {
        // Un pi imposé par PI_BIN se passe des outils de runtime/.
        Some((path, version)) => PiStatus { installed: pi::tools_installed() || std::env::var("PI_BIN").is_ok(), path: Some(path), version: Some(version) },
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

/// Adresse de l'aperçu d'un fichier local (page HTML, image…), servie par le serveur local.
#[tauri::command]
async fn preview_url(root: String, path: String) -> Res<String> {
    preview::url_for(&root, &path).await.map_err(err)
}

// Async : add_child attend le fil principal, une commande synchrone y tourne déjà (blocage).
#[tauri::command]
async fn browser_show(app: AppHandle, url: String, bounds: browser::Bounds) -> Res<()> {
    browser::show(&app, &url, bounds).map_err(err)
}

#[tauri::command]
async fn browser_bounds(app: AppHandle, bounds: browser::Bounds) -> Res<()> {
    browser::bounds(&app, bounds).map_err(err)
}

#[tauri::command]
async fn browser_hide(app: AppHandle) -> Res<()> {
    browser::hide(&app).map_err(err)
}

#[tauri::command]
async fn browser_close(app: AppHandle) -> Res<()> {
    browser::close(&app).map_err(err)
}

#[tauri::command]
async fn browser_reload(app: AppHandle) -> Res<()> {
    browser::reload(&app).map_err(err)
}

/// Ouvre une adresse web ou un fichier dans le programme par défaut (navigateur…).
#[tauri::command]
fn open_external(target: String) -> Res<()> {
    let ok = target.starts_with("http://") || target.starts_with("https://") || std::path::Path::new(&target).exists();
    if !ok {
        return Err(format!("impossible d'ouvrir {target}"));
    }
    std::process::Command::new("xdg-open").arg(&target).spawn().map(|_| ()).map_err(err)
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
                    dots::stop_all_blocking();
                    std::process::exit(0);
                }
            });
            let voice = VoiceEngine::new(app.handle().clone());
            voice.start();
            app.manage(AppState { pi: pi::PiManager::new(), terms: term::Terms::default(), voice, watcher: Mutex::new(None) });
            Ok(())
        })
        // Fermer la fenêtre pendant que des runs travaillent : l'interface demande confirmation
        // (puis appelle quit_app). Suivi côté Rust : même une interface plantée n'empêche pas de
        // fermer quand rien ne tourne.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let busy = window.state::<AppState>().pi.busy_runs();
                if !busy.is_empty() {
                    api.prevent_close();
                    let _ = window.emit("close-requested", busy);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            pi_start,
            pi_start_dot,
            dots_info,
            dots_set_cli,
            dots_list,
            dots_create,
            dots_action,
            pi_stop,
            watch_dir,
            pi_send,
            pi_write,
            quit_app,
            worktree_create,
            worktree_info,
            worktree_merge,
            worktree_remove,
            voice_listen,
            voice_get_config,
            voice_set_config,
            voice_stt_connected,
            list_files,
            git_status,
            git_diff,
            read_file,
            git_init,
            initial_dir,
            preview_url,
            open_external,
            browser_show,
            browser_bounds,
            browser_hide,
            browser_close,
            browser_reload,
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
                // Fermer l'app éteint aussi les dots (ils reprendront au prochain allumage).
                dots::stop_all_blocking();
            }
        });
}
