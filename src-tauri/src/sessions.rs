//! Sessions de pi pour le dossier ouvert (fichiers JSONL dans `<agent-dir>/sessions/--<chemin>--/`).

use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub path: String,
    pub id: String,
    pub title: String,
    /// Dernière modification (ms depuis 1970).
    pub updated: u64,
    pub messages: usize,
    /// La session a un vrai nom (sinon le titre est sa première demande).
    pub named: bool,
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn summarize(path: &Path) -> Option<SessionInfo> {
    let file = std::fs::File::open(path).ok()?;
    let updated = file
        .metadata()
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    let (mut id, mut name, mut first_user) = (String::new(), None::<String>, None::<String>);
    let mut messages = 0;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        match v["type"].as_str() {
            Some("session") => id = v["id"].as_str().unwrap_or_default().to_string(),
            Some("session_info") => name = v["name"].as_str().map(str::to_string).filter(|n| !n.is_empty()),
            Some("message") => {
                let role = v["message"]["role"].as_str().unwrap_or_default();
                if role == "user" || role == "assistant" {
                    messages += 1;
                }
                if role == "user" && first_user.is_none() {
                    let t = text_of(&v["message"]["content"]);
                    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !t.is_empty() {
                        first_user = Some(t);
                    }
                }
            }
            _ => {}
        }
    }
    let named = name.is_some();
    let title = name.or(first_user).unwrap_or_else(|| "Nouvelle session".into());
    let title = if title.chars().count() > 70 { title.chars().take(70).collect::<String>() + "…" } else { title };
    Some(SessionInfo { path: path.to_string_lossy().into_owned(), id, title, updated, messages, named })
}

/// Sessions du même dossier que `current` (le fichier de la session active), les plus récentes d'abord.
/// Les sessions vides sont masquées, sauf la session active.
pub fn list(current: &str) -> Vec<SessionInfo> {
    let current = Path::new(current);
    let Some(dir) = current.parent() else { return vec![] };
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<SessionInfo> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|p| summarize(&p))
        .filter(|s| s.messages > 0 || Path::new(&s.path) == current)
        .collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out
}

/// Demande à pi (appel ponctuel, sans outils ni session) un titre court pour une conversation.
pub async fn generate_title(cwd: &str, model: Option<&str>, user: &str, assistant: &str) -> anyhow::Result<String> {
    let clip = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    let prompt = format!(
        "Donne un titre très court (3 à 6 mots, en français, sans guillemets ni ponctuation finale) \
         pour cette conversation. Réponds uniquement par le titre.\n\nUtilisateur : {}\nAssistant : {}",
        clip(user, 600),
        clip(assistant, 600)
    );
    let pi = crate::pi::find_pi().ok_or_else(|| anyhow::anyhow!("pi introuvable"))?;
    let mut cmd = tokio::process::Command::new(pi);
    cmd.env("PATH", crate::pi::extended_path());
    cmd.args(["-p", "--no-session", "--no-tools", "--thinking", "off"]);
    if let Some(m) = model.filter(|m| !m.is_empty()) {
        cmd.args(["--model", m]);
    }
    cmd.arg(prompt).current_dir(cwd).kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(40), cmd.output()).await??;
    anyhow::ensure!(out.status.success(), "pi a échoué : {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let title = line
        .trim_matches(|c: char| c == '"' || c == '«' || c == '»' || c == '*' || c.is_whitespace())
        .trim_end_matches(['.', '!', '?', ':'])
        .trim()
        .to_string();
    anyhow::ensure!(!title.is_empty(), "titre vide");
    Ok(title.chars().take(60).collect())
}

/// Met une session à la corbeille (ou la supprime si la corbeille n'est pas disponible).
pub fn trash(path: &str) -> anyhow::Result<()> {
    let p = Path::new(path);
    anyhow::ensure!(p.extension().is_some_and(|x| x == "jsonl"), "ce n'est pas un fichier de session");
    anyhow::ensure!(p.components().any(|c| c.as_os_str() == "sessions"), "fichier hors du dossier des sessions");
    let trashed = std::process::Command::new("gio").args(["trash", path]).status().is_ok_and(|s| s.success());
    if !trashed {
        std::fs::remove_file(p)?;
    }
    Ok(())
}
