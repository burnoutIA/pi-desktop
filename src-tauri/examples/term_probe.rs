//! Diagnostic : lance pi dans un pseudo-terminal, tape /login, affiche l'écran obtenu, puis Échap.
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            if it.peek() == Some(&'[') {
                it.next();
                while let Some(&n) = it.peek() { it.next(); if n.is_ascii_alphabetic() || n == '~' { break; } }
            } else { it.next(); }
        } else if c == '\r' { } else { out.push(c); }
    }
    out
}

fn main() {
    let pair = native_pty_system().openpty(PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 }).unwrap();
    let mut cmd = CommandBuilder::new("pi");
    cmd.env("TERM", "xterm-256color");
    cmd.cwd(std::env::var("PROBE_CWD").unwrap());
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let out = Arc::new(Mutex::new(String::new()));
    let o2 = out.clone();
    std::thread::spawn(move || { let mut b = [0u8; 4096]; while let Ok(n) = reader.read(&mut b) { if n == 0 { break; } o2.lock().unwrap().push_str(&String::from_utf8_lossy(&b[..n])); } });
    std::thread::sleep(Duration::from_secs(4));
    let before = out.lock().unwrap().len();
    writer.write_all(b"/login\r").unwrap(); writer.flush().unwrap();
    std::thread::sleep(Duration::from_secs(3));
    let after = strip(&out.lock().unwrap()[before..]);
    let lines: Vec<&str> = after.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    println!("--- écran après /login ({} lignes) ---", lines.len());
    for l in lines.iter().rev().take(25).rev() { println!("{l}"); }
    writer.write_all(b"\x1b").unwrap(); writer.flush().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    writer.write_all(b"\x1b").unwrap(); writer.flush().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let _ = child.kill();
}
