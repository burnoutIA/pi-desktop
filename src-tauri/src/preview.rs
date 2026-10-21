//! Serveur local de l'aperçu : sert en HTTP (127.0.0.1 seulement) les dossiers que l'app lui
//! confie, pour qu'une page HTML s'affiche avec ses styles, scripts et liens relatifs.
//! Chaque dossier reçoit un jeton aléatoire : `http://127.0.0.1:<port>/<jeton>/<chemin>`.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct Server {
    port: u16,
    dirs: Mutex<HashMap<String, PathBuf>>,
}

static SERVER: OnceLock<Server> = OnceLock::new();

/// Démarre le serveur (une fois) et renvoie son port.
async fn server() -> Result<&'static Server> {
    if let Some(s) = SERVER.get() {
        return Ok(s);
    }
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    if SERVER.set(Server { port, dirs: Mutex::default() }).is_err() {
        // Un autre appel l'a démarré entre-temps : on garde le sien.
        return Ok(SERVER.get().unwrap());
    }
    applog!("aperçu : serveur local sur le port {port}");
    tauri::async_runtime::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tauri::async_runtime::spawn(async move {
                let _ = handle(stream).await;
            });
        }
    });
    Ok(SERVER.get().unwrap())
}

fn token() -> String {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    format!("{:016x}", h.finish())
}

/// Adresse de l'aperçu d'un fichier. `root` (le projet ouvert) est servi en entier quand le
/// fichier est dedans, pour que ses liens relatifs marchent ; sinon seul son dossier l'est.
pub async fn url_for(root: &str, path: &str) -> Result<String> {
    let file = std::fs::canonicalize(path).map_err(|e| anyhow!("{path} : {e}"))?;
    let root = std::fs::canonicalize(root).ok().filter(|r| !root_is_too_wide(r));
    let base = match root {
        Some(r) if file.starts_with(&r) => r,
        _ if file.is_dir() => file.clone(),
        _ => file.parent().ok_or_else(|| anyhow!("dossier introuvable"))?.to_path_buf(),
    };
    let rel = file.strip_prefix(&base).unwrap_or(Path::new(""));
    let s = server().await?;
    let tok = {
        let mut dirs = s.dirs.lock().unwrap();
        match dirs.iter().find(|(_, d)| **d == base) {
            Some((t, _)) => t.clone(),
            None => {
                let t = token();
                dirs.insert(t.clone(), base);
                t
            }
        }
    };
    let rel: Vec<String> = rel.components().map(|c| encode(&c.as_os_str().to_string_lossy())).collect();
    Ok(format!("http://127.0.0.1:{}/{tok}/{}", s.port, rel.join("/")))
}

/// On ne sert jamais / ni le dossier personnel en entier.
fn root_is_too_wide(r: &Path) -> bool {
    r.parent().is_none() || std::env::var("HOME").is_ok_and(|h| Path::new(&h) == r)
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("pdf") => "application/pdf",
        Some("txt" | "md" | "csv" | "log") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Le fichier demandé, s'il est bien dans un dossier confié au serveur.
fn resolve(target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next()?.trim_start_matches('/');
    let (tok, rest) = path.split_once('/').unwrap_or((path, ""));
    let base = SERVER.get()?.dirs.lock().unwrap().get(tok)?.clone();
    let rel = PathBuf::from(decode(rest));
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let mut file = std::fs::canonicalize(base.join(rel)).ok()?;
    if !file.starts_with(&base) {
        return None; // lien symbolique qui sort du dossier
    }
    if file.is_dir() {
        file = file.join("index.html");
    }
    file.is_file().then_some(file)
}

async fn handle(mut stream: TcpStream) -> Result<()> {
    let mut buf = vec![0u8; 8192];
    let mut len = 0;
    while !buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf[len..]).await?;
        if n == 0 || len + n == buf.len() {
            break;
        }
        len += n;
    }
    let head = String::from_utf8_lossy(&buf[..len]);
    let mut parts = head.lines().next().unwrap_or("").split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let (status, ctype, body) = match (method, resolve(target)) {
        ("GET" | "HEAD", Some(file)) => ("200 OK", mime(&file), tokio::fs::read(&file).await.unwrap_or_default()),
        ("GET" | "HEAD", None) => ("404 Not Found", "text/plain; charset=utf-8", b"Introuvable".to_vec()),
        _ => ("405 Method Not Allowed", "text/plain; charset=utf-8", Vec::new()),
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    if method != "HEAD" {
        stream.write_all(&body).await?;
    }
    stream.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_handles_percent_escapes() {
        assert_eq!(decode("mon%20fichier.html"), "mon fichier.html");
        assert_eq!(decode("%C3%A9t%C3%A9"), "été");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(encode("été 1.png"), "%C3%A9t%C3%A9%201.png");
    }
}

#[cfg(test)]
mod serve_tests {
    #[tokio::test]
    async fn serves_project_files_only() {
        let dir = std::env::temp_dir().join(format!("pv-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("css")).unwrap();
        std::fs::write(dir.join("index.html"), "<link href=css/a.css>").unwrap();
        std::fs::write(dir.join("css/a.css"), "body{}").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let url = super::url_for(&root, &format!("{root}/index.html")).await.unwrap();
        let r = reqwest::get(&url).await.unwrap();
        assert_eq!(r.headers()["content-type"], "text/html; charset=utf-8");
        assert_eq!(r.text().await.unwrap(), "<link href=css/a.css>");
        let css = url.replace("index.html", "css/a.css");
        assert_eq!(reqwest::get(&css).await.unwrap().text().await.unwrap(), "body{}");
        let base = url.trim_end_matches("index.html");
        assert_eq!(reqwest::get(base).await.unwrap().status(), 200); // dossier → index.html
        let escape = format!("{base}..%2F..%2Fetc%2Fpasswd");
        assert_eq!(reqwest::get(&escape).await.unwrap().status(), 404);
        let bad = url.replace(&url.split('/').nth(3).unwrap().to_string(), "0000000000000000");
        assert_eq!(reqwest::get(&bad).await.unwrap().status(), 404);
        std::fs::remove_dir_all(dir).ok();
    }
}
