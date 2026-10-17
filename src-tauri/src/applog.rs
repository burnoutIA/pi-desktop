//! Journal de l'app : stderr + ~/.cache/pi-voice/app.log (utile quand l'app est lancée sans terminal).

use std::io::Write;
use std::sync::Mutex;

static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join(".cache/pi-voice/app.log")
}

pub fn init() {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // On repart d'un journal vide à chaque lancement.
    *FILE.lock().unwrap() = std::fs::File::create(&p).ok();
}

pub fn write(line: &str) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let t = (secs % 86_400.0) as u64;
    let stamp = format!("{:02}:{:02}:{:02}Z", t / 3600, (t / 60) % 60, t % 60);
    eprintln!("[{stamp}] {line}");
    if let Some(f) = FILE.lock().unwrap().as_mut() {
        let _ = writeln!(f, "[{stamp}] {line}");
    }
}

#[macro_export]
macro_rules! applog {
    ($($arg:tt)*) => { $crate::applog::write(&format!($($arg)*)) };
}
