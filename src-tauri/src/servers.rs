//! Serveurs vocaux gérés par l'app : au lancement, si la voix ne répond pas, on démarre
//! `scripts/voice-server.sh` ; à la fermeture, on arrête ce qu'on a démarré (GPU libéré).
//! Des serveurs déjà lancés à la main sont utilisés tels quels et jamais arrêtés.

use serde::Serialize;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const PORTS: [(u16, &str); 3] = [(8090, "transcription"), (8089, "synthèse"), (8091, "réveil")];
/// Premier chargement des modèles : ~30 s d'habitude, beaucoup plus la toute première fois.
const BOOT_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Serialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Tout répondait déjà au lancement (serveurs lancés à la main).
    External,
    Starting,
    Ready,
    /// Script introuvable ou serveurs jamais prêts.
    Failed,
}

pub struct Servers {
    state: Mutex<State>,
    /// Groupe de processus démarré par l'app (à arrêter à la fermeture).
    pgid: Mutex<Option<u32>>,
}

fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Dossier du projet pi-desktop (là où se trouve `scripts/voice-server.sh`).
pub fn project_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PIVOICE_ROOT") {
        return Some(PathBuf::from(dir));
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|d| d.join("scripts/voice-server.sh").is_file()).map(PathBuf::from)
}

impl Servers {
    pub fn new() -> Self {
        Self { state: Mutex::new(State::Starting), pgid: Mutex::new(None) }
    }

    pub fn state(&self) -> State {
        *self.state.lock().unwrap()
    }

    fn set(&self, app: &AppHandle, s: State) {
        *self.state.lock().unwrap() = s;
        crate::applog!("serveurs vocaux : {s:?}");
        let _ = app.emit("voice-servers", s);
    }

    /// À appeler au lancement de l'app.
    pub fn ensure(&'static self, app: AppHandle) {
        std::thread::spawn(move || {
            let missing: Vec<&str> = PORTS.iter().filter(|(p, _)| !port_open(*p)).map(|(_, n)| *n).collect();
            if missing.is_empty() {
                return self.set(&app, State::External);
            }
            let Some(root) = project_root() else {
                crate::applog!("serveurs vocaux : scripts/voice-server.sh introuvable (PIVOICE_ROOT ?)");
                return self.set(&app, State::Failed);
            };
            crate::applog!("serveurs vocaux : démarrage ({} manquant)", missing.join(", "));
            self.set(&app, State::Starting);
            let log = crate::applog::path().with_file_name("voice-server.log");
            let out = std::fs::File::create(&log).ok();
            // `setsid` : le script devient chef d'un groupe de processus, qu'on arrête d'un coup.
            let mut cmd = std::process::Command::new("setsid");
            cmd.arg(root.join("scripts/voice-server.sh"))
                .current_dir(&root)
                // Filet de sécurité : le script s'arrête de lui-même si l'app disparaît.
                .env("PIVOICE_PARENT", std::process::id().to_string());
            if let Some(f) = out {
                if let Ok(f2) = f.try_clone() {
                    cmd.stdout(f).stderr(f2);
                }
            }
            match cmd.spawn() {
                Ok(child) => *self.pgid.lock().unwrap() = Some(child.id()),
                Err(e) => {
                    crate::applog!("serveurs vocaux : lancement impossible : {e}");
                    return self.set(&app, State::Failed);
                }
            }
            let start = Instant::now();
            while start.elapsed() < BOOT_TIMEOUT {
                if PORTS.iter().all(|(p, _)| port_open(*p)) {
                    crate::applog!("serveurs vocaux : prêts en {:.0} s", start.elapsed().as_secs_f32());
                    return self.set(&app, State::Ready);
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            self.set(&app, State::Failed);
        });
    }

    /// À la fermeture de l'app : arrête les serveurs qu'elle a démarrés.
    pub fn shutdown(&self) {
        if let Some(pgid) = self.pgid.lock().unwrap().take() {
            crate::applog!("serveurs vocaux : arrêt (groupe {pgid})");
            let _ = std::process::Command::new("kill").args(["-TERM", "--", &format!("-{pgid}")]).status();
        }
    }
}
