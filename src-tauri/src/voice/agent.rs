//! Agent vocal : il répond tout de suite à ce que tu dis, même quand pi travaille. Il tourne sur un
//! serveur compatible OpenAI (harn par défaut, deuxième slot du moteur), sans réflexion, et commence
//! chaque réponse par une ligne d'action :
//!   [REPONDRE]                  : il répond seul (avancement, discussion, question simple)
//!   [TRANSMETTRE] <consigne>    : la consigne part à pi (nouvelle demande, ou ajout en cours de route)
//!   [ARRETER]                   : pi s'arrête
//! puis sa réponse orale, lue mot par mot dès qu'elle arrive. L'interface exécute l'action.
//!
//! La clé d'API n'est jamais copiée : elle est lue dans la configuration de pi
//! (~/.pi/agent/models.json, fournisseur dont `baseUrl` correspond à l'adresse de l'agent).

use super::*;

/// Échanges gardés en mémoire (une question + une réponse = 2).
const HISTORY_MAX: usize = 16;
/// Au-delà, la ligne d'action est jugée absente : tout le texte est lu.
const HEADER_MAX: usize = 400;

#[derive(Default)]
pub(super) struct AgentState {
    history: Vec<(&'static str, String)>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum AgentEvent {
    /// L'action, dès que la première ligne est arrivée (l'interface agit pendant qu'il parle).
    Action { action: String, instruction: String },
    /// Sa réponse orale complète, pour l'afficher.
    Reply { text: String },
    /// Requête impossible : l'interface envoie alors la demande directement à pi.
    Error { message: String, text: String },
}

fn system_prompt(name: &str) -> String {
    format!(
        "Tu es {name}, la voix d'un assistant de développement. L'utilisateur te parle à voix haute pendant \
         qu'un agent de code, pi, travaille sur son projet. Ta réponse est lue à voix haute.\n\n\
         Commence TOUJOURS par une seule ligne d'action, puis ta réponse orale sur la ligne suivante :\n\
         [TRANSMETTRE] <consigne pour pi> — quand l'utilisateur veut que quelque chose soit fait sur le projet \
         (coder, corriger, modifier, lancer, chercher dans le code, ajouter une contrainte au travail en cours). \
         La consigne reprend fidèlement ses mots, complétée seulement par ce qui a été dit avant quand c'est \
         nécessaire pour la comprendre. N'ajoute aucune exigence ni aucun détail qu'il n'a pas demandé.\n\
         [ARRETER] — quand il veut arrêter, annuler ou interrompre le travail de pi.\n\
         [REPONDRE] — pour tout le reste : avancement, question simple, discussion, salutation.\n\n\
         Réponse orale : une ou deux phrases courtes et naturelles, en français, sans liste, sans code, sans \
         markdown, sans lire de chemin de fichier en entier. Si tu transmets, dis-le simplement. Pour l'avancement, \
         base-toi uniquement sur l'état de pi fourni avec chaque message : n'invente rien."
    )
}

/// La clé du fournisseur de pi qui pointe vers `url` (aucune clé : harn sans clé, ou serveur ouvert).
fn api_key_for(url: &str) -> Option<String> {
    let path = std::path::PathBuf::from(std::env::var("HOME").ok()?).join(".pi/agent/models.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let want = url.trim_end_matches('/');
    v["providers"].as_object()?.values().find_map(|p| {
        (p["baseUrl"].as_str()?.trim_end_matches('/') == want).then(|| p["apiKey"].as_str().map(str::to_string)).flatten()
    })
}

/// Lit la ligne d'action ; None tant qu'elle n'est pas complète.
fn parse_header(buf: &str, done: bool) -> Option<(String, String, usize)> {
    let end = match buf.find('\n') {
        Some(i) => i,
        None if done || buf.len() > HEADER_MAX => buf.len(),
        None => return None,
    };
    let line = buf[..end].trim();
    let upper = line.to_uppercase();
    let tag = |t: &str| upper.starts_with(t);
    let (action, instruction) = if tag("[TRANSMETTRE]") {
        ("transmettre", line["[TRANSMETTRE]".len()..].trim().to_string())
    } else if tag("[ARRETER]") || tag("[ARRÊTER]") {
        ("arreter", String::new())
    } else if tag("[REPONDRE]") || tag("[RÉPONDRE]") {
        ("repondre", String::new())
    } else {
        // Pas de ligne d'action : tout est à lire.
        return Some(("repondre".into(), String::new(), 0));
    };
    // Le texte oral commence après la ligne d'action (ou après la balise si tout est sur une ligne).
    let skip = if action == "repondre" || action == "arreter" {
        let tag_len = upper.find(']').map(|i| i + 1).unwrap_or(0);
        let rest = line[tag_len..].trim();
        if rest.is_empty() { (end + 1).min(buf.len()) } else { buf.find(rest).unwrap_or(end) }
    } else {
        (end + 1).min(buf.len())
    };
    Some((action.into(), instruction, skip))
}

impl VoiceEngine {
    /// Réponse de l'agent à ce que tu viens de dire. `pi_state` : l'état de pi vu par l'interface.
    pub fn agent_ask(self: &Arc<Self>, text: String, pi_state: String) {
        self.stop_speaking(); // une nouvelle phrase coupe la précédente
        let generation = self.agent_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = me.agent_run(generation, &text, &pi_state).await {
                crate::applog!("agent vocal : {e}");
                let _ = me.app.emit("voice-agent", AgentEvent::Error { message: e.to_string(), text });
            }
        });
    }

    pub fn agent_reset(&self) {
        self.agent.lock().unwrap().history.clear();
    }

    fn agent_current(&self, generation: u64) -> bool {
        self.agent_gen.load(Ordering::SeqCst) == generation
    }

    async fn agent_run(self: &Arc<Self>, generation: u64, text: &str, pi_state: &str) -> anyhow::Result<()> {
        let cfg = self.config();
        let name = {
            let n = cfg.wake_word.trim();
            let mut c = n.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_else(|| "Pi".into())
        };
        let mut messages = vec![json!({ "role": "system", "content": system_prompt(&name) })];
        for (role, content) in self.agent.lock().unwrap().history.iter() {
            messages.push(json!({ "role": role, "content": content }));
        }
        // L'état change à chaque fois : il va dans le dernier message, pour garder le reste en cache.
        messages.push(json!({ "role": "user", "content": format!("État de pi :\n{pi_state}\n\nL'utilisateur dit : {text}") }));
        let url = format!("{}/chat/completions", cfg.agent_url.trim_end_matches('/'));
        // Pas de champ `model` : harn répond avec le modèle déjà chargé, sans en changer.
        let body = json!({
            "messages": messages,
            "stream": true,
            "max_tokens": 220,
            "temperature": 0.6,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let client = reqwest::Client::new();
        let mut req = client.post(&url).json(&body).timeout(Duration::from_secs(60));
        if let Some(key) = api_key_for(&cfg.agent_url) {
            req = req.bearer_auth(key);
        }
        let t0 = Instant::now();
        let mut resp = req.send().await?.error_for_status()?;

        let (mut sse, mut out) = (String::new(), String::new());
        let mut header: Option<(String, String)> = None;
        let mut spoken_upto = 0usize; // octets de `out` déjà envoyés à la voix
        let mut done = false;
        let mut first_word: Option<u64> = None;
        while !done {
            let Some(chunk) = resp.chunk().await? else { break };
            if !self.agent_current(generation) {
                crate::applog!("agent vocal : coupé");
                return Ok(());
            }
            sse.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(i) = sse.find('\n') {
                let line: String = sse.drain(..=i).collect();
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    done = true;
                    break;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(d) = v["choices"][0]["delta"]["content"].as_str() {
                        out.push_str(d);
                    }
                }
            }
            if header.is_none() {
                if let Some((action, instruction, skip)) = parse_header(&out, done) {
                    crate::applog!("agent vocal : action {action} ({} ms){}", t0.elapsed().as_millis(),
                        if instruction.is_empty() { String::new() } else { format!(" « {instruction} »") });
                    let _ = self.app.emit("voice-agent", AgentEvent::Action { action: action.clone(), instruction: instruction.clone() });
                    header = Some((action, instruction));
                    spoken_upto = skip;
                }
            }
            if header.is_some() {
                // Les mots complets partent à la voix ; le dernier attend la suite.
                let pending = &out[spoken_upto..];
                let complete = if done { pending.len() } else { pending.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0) };
                for w in pending[..complete].split_whitespace() {
                    let w = speech::clean_word(w);
                    if !w.is_empty() {
                        first_word.get_or_insert(t0.elapsed().as_millis() as u64);
                        self.speak_word(w);
                    }
                }
                spoken_upto += complete;
            }
        }
        if header.is_none() {
            let (action, instruction, skip) = parse_header(&out, true).unwrap_or(("repondre".into(), String::new(), 0));
            let _ = self.app.emit("voice-agent", AgentEvent::Action { action, instruction });
            for w in out[skip..].split_whitespace() {
                let w = speech::clean_word(w);
                if !w.is_empty() {
                    self.speak_word(w);
                }
            }
        } else if spoken_upto < out.len() {
            for w in out[spoken_upto..].split_whitespace() {
                let w = speech::clean_word(w);
                if !w.is_empty() {
                    self.speak_word(w);
                }
            }
        }
        if !self.agent_current(generation) {
            return Ok(());
        }
        self.speak_end();
        let skip = parse_header(&out, true).map(|(_, _, s)| s).unwrap_or(0);
        let oral = out[skip..].trim().to_string();
        crate::applog!("agent vocal : premier mot {} ms, fin {} ms : « {oral} »", first_word.unwrap_or(0), t0.elapsed().as_millis());
        let _ = self.app.emit("voice-agent", AgentEvent::Reply { text: oral });
        let mut st = self.agent.lock().unwrap();
        st.history.push(("user", text.to_string()));
        st.history.push(("assistant", out.trim().to_string()));
        let excess = st.history.len().saturating_sub(HISTORY_MAX);
        st.history.drain(..excess);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_parsed() {
        let (a, i, s) = parse_header("[TRANSMETTRE] Ajoute un export CSV\nJe lui transmets.", false).unwrap();
        assert_eq!((a.as_str(), i.as_str()), ("transmettre", "Ajoute un export CSV"));
        assert_eq!(&"[TRANSMETTRE] Ajoute un export CSV\nJe lui transmets."[s..], "Je lui transmets.");
        let (a, _, s) = parse_header("[REPONDRE]\nIl en est aux tests.", false).unwrap();
        assert_eq!(a, "repondre");
        assert_eq!(&"[REPONDRE]\nIl en est aux tests."[s..], "Il en est aux tests.");
        let txt = "[RÉPONDRE] Il en est aux tests.";
        let (a, _, s) = parse_header(txt, true).unwrap();
        assert_eq!((a.as_str(), &txt[s..]), ("repondre", "Il en est aux tests."));
        assert_eq!(parse_header("[ARRETER]\nJ'arrête.", false).unwrap().0, "arreter");
        assert!(parse_header("[TRANSMETTRE] pas fini", false).is_none());
        let (a, _, s) = parse_header("Bonjour, je t'écoute.", true).unwrap();
        assert_eq!((a.as_str(), s), ("repondre", 0));
    }
}
