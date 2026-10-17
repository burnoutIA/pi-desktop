//! Terminal intégré : un vrai pseudo-terminal (pour l'installeur de pi, `/login`…), affiché
//! dans l'interface avec xterm.js. La sortie est envoyée en base64 (octets bruts).

use anyhow::{anyhow, Result};
use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

struct Term {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
pub struct Terms {
    map: Arc<Mutex<HashMap<u32, Term>>>,
    next: AtomicU32,
}

impl Terms {
    pub fn spawn(&self, app: AppHandle, program: &str, args: &[String], cwd: Option<&str>, cols: u16, rows: u16) -> Result<u32> {
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        let home = std::env::var("HOME").unwrap_or_default();
        cmd.cwd(cwd.map(str::to_string).unwrap_or(home));
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("PATH", crate::pi::extended_path());
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        self.map.lock().unwrap().insert(id, Term { writer, master: pair.master, child });
        crate::applog!("terminal {id} : {program} {}", args.join(" "));

        let map = self.map.clone();
        std::thread::spawn(move || {
            let b64 = base64::engine::general_purpose::STANDARD;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let _ = app.emit("term-data", json!({ "id": id, "data": b64.encode(&buf[..n]) }));
                    }
                }
            }
            let code = map
                .lock()
                .unwrap()
                .remove(&id)
                .and_then(|mut t| t.child.wait().ok())
                .map(|s| s.exit_code() as i64)
                .unwrap_or(-1);
            crate::applog!("terminal {id} : terminé ({code})");
            let _ = app.emit("term-exit", json!({ "id": id, "code": code }));
        });
        Ok(id)
    }

    pub fn write(&self, id: u32, data: &str) -> Result<()> {
        let mut map = self.map.lock().unwrap();
        let t = map.get_mut(&id).ok_or_else(|| anyhow!("terminal fermé"))?;
        t.writer.write_all(data.as_bytes())?;
        t.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> Result<()> {
        let map = self.map.lock().unwrap();
        let t = map.get(&id).ok_or_else(|| anyhow!("terminal fermé"))?;
        t.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        Ok(())
    }

    pub fn kill(&self, id: u32) {
        if let Some(t) = self.map.lock().unwrap().get_mut(&id) {
            let _ = t.child.kill();
        }
    }
}
