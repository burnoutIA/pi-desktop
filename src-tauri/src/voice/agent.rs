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
/// Slot du moteur réservé à l'agent (llama-server derrière harn) : il ne prend jamais celui de
/// pi, qui garde tout son contexte en mémoire au lieu de le relire (60 à 150 s mesurées).
const AGENT_SLOT: u32 = 1;
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
        "Tu es {name}, un assistant de développement à qui l'utilisateur parle à voix haute. Tu as deux \
         facettes : cette voix, qui répond tout de suite, et ton agent de code, qui travaille sur le projet \
         (il lit les fichiers, code, lance des commandes). Pour l'utilisateur, vous êtes un seul assistant : \
         parle du travail de l'agent de code à la première personne (« je regarde », « je m'en occupe », \
         « je suis en train de… »), jamais de lui à la troisième personne.\n\n\
         Ce que dit l'utilisateur vient d'une transcription vocale : elle contient des erreurs (mots déformés, \
         noms mal écrits, « Lanspi » pour « lance pi »). Comprends l'intention d'après le son des mots.\n\n\
         Commence TOUJOURS par une balise d'action :\n\
         [TRANSMETTRE] <consigne pour l'agent de code> puis retour à la ligne : dès que la réponse demande de \
         regarder ou de toucher le projet. Faire, coder, corriger, modifier, lancer, tester, chercher, mais aussi \
         expliquer le code, faire un bilan ou un état des lieux du projet, résumer ce qu'il contient, ou ajouter \
         une contrainte au travail en cours. La consigne reprend fidèlement ses mots, corrigés des erreurs de \
         transcription, complétée seulement par ce qui a été dit avant quand c'est nécessaire. Une phrase \
         courte. N'ajoute aucune exigence, fonctionnalité ou détail technique qu'il n'a pas dit. Si tu \
         hésites, transmets.\n\
         [ARRETER] : il veut arrêter, annuler ou interrompre le travail en cours.\n\
         [REPONDRE] : seulement quand tu peux répondre sans regarder le projet : salutation, discussion, ou \
         avancement du travail en cours d'après l'état fourni.\n\n\
         Exemples :\n\
         « fais-moi un petit bilan du projet » -> [TRANSMETTRE] Fais un petit bilan du projet.\n\
         « lance pi, demande-lui ce que fait ce fichier » -> [TRANSMETTRE] Explique ce que fait ce fichier.\n\
         « tu en es où ? » (travail en cours) -> [REPONDRE] Je suis en train de…\n\
         « laisse tomber » -> [ARRETER]\n\n\
         Après la balise, ta réponse orale : une ou deux phrases courtes et naturelles, en français, sans liste, \
         sans code, sans markdown, sans lire de chemin de fichier en entier. Si tu transmets, dis simplement que \
         tu t'en occupes. Ne parle jamais d'un résultat que tu n'as pas encore : l'agent de code donnera sa \
         réponse lui-même quand il aura fini. Pour l'avancement, base-toi uniquement sur l'état fourni avec \
         chaque message : n'invente rien."
    )
}

/// Le fournisseur de pi qui pointe vers `url` : (son nom, sa clé). Pas de clé : harn sans clé,
/// ou serveur ouvert.
fn provider_for(url: &str) -> Option<(String, Option<String>)> {
    let path = std::path::PathBuf::from(std::env::var("HOME").ok()?).join(".pi/agent/models.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let want = url.trim_end_matches('/');
    v["providers"].as_object()?.iter().find_map(|(name, p)| {
        (p["baseUrl"].as_str()?.trim_end_matches('/') == want).then(|| (name.clone(), p["apiKey"].as_str().map(str::to_string)))
    })
}

/// Lit la balise d'action en tête de réponse : (action, consigne, octet où commence le texte oral).
/// None tant qu'on ne sait pas encore. [REPONDRE] et [ARRETER] sont connus dès leur `]` : la voix
/// démarre aussitôt. [TRANSMETTRE] attend la fin de sa ligne (la consigne pour pi).
fn parse_header(buf: &str, done: bool) -> Option<(String, String, usize)> {
    let lead = buf.len() - buf.trim_start().len();
    let t = &buf[lead..];
    let text_only = || Some(("repondre".to_string(), String::new(), lead));
    if t.is_empty() {
        return done.then(|| ("repondre".to_string(), String::new(), buf.len()));
    }
    if !t.starts_with('[') {
        return text_only(); // pas de balise : tout est à lire
    }
    let Some(close) = t.find(']') else {
        return if done || t.len() > HEADER_MAX { text_only() } else { None };
    };
    let tag: String = t[1..close]
        .trim()
        .to_uppercase()
        .chars()
        .map(|c| match c {
            'É' | 'È' | 'Ê' => 'E',
            c => c,
        })
        .collect();
    let after = lead + close + 1;
    match tag.as_str() {
        "REPONDRE" => Some(("repondre".into(), String::new(), after)),
        "ARRETER" => Some(("arreter".into(), String::new(), after)),
        "TRANSMETTRE" => {
            let rest = &buf[after..];
            match rest.find('\n') {
                Some(i) => Some(("transmettre".into(), rest[..i].trim().to_string(), after + i + 1)),
                None if done || rest.len() > HEADER_MAX => Some(("transmettre".into(), rest.trim().to_string(), buf.len())),
                None => None,
            }
        }
        _ => Some(("repondre".into(), String::new(), after)), // balise inconnue : on lit la suite
    }
}

impl VoiceEngine {
    /// Réponse de l'agent à ce que tu viens de dire. `pi_state` : l'état de pi vu par l'interface.
    /// `pi_model` : le modèle de pi (« fournisseur/id »). S'il passe par le même serveur que l'agent,
    /// l'agent prend le même modèle : harn ne bascule jamais d'un modèle à l'autre au milieu d'une phrase.
    pub fn agent_ask(self: &Arc<Self>, text: String, pi_state: String, pi_model: Option<String>) {
        self.stop_speaking(); // une nouvelle phrase coupe la précédente
        let generation = self.agent_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = me.agent_run(generation, &text, &pi_state, pi_model.as_deref()).await {
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

    async fn agent_run(self: &Arc<Self>, generation: u64, text: &str, pi_state: &str, pi_model: Option<&str>) -> anyhow::Result<()> {
        let cfg = self.config();
        let name = {
            let n = cfg.agent_name.trim();
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
        let provider = provider_for(&cfg.agent_url);
        let mut body = json!({
            "messages": messages,
            "stream": true,
            "max_tokens": 220,
            "temperature": 0.6,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let client = reqwest::Client::new();
        if ["//127.0.0.1", "//localhost", "//[::1]"].iter().any(|h| cfg.agent_url.contains(h)) {
            body["id_slot"] = json!(AGENT_SLOT);
        }
        // Même modèle que pi s'il passe par ce serveur. Sinon pas de champ `model` : harn répond
        // avec le modèle déjà chargé, sans en changer.
        if let (Some((name, _)), Some((p, id))) = (&provider, pi_model.and_then(|m| m.split_once('/'))) {
            if name == p {
                body["model"] = json!(id);
            }
        }
        let mut req = client.post(&url).json(&body).timeout(Duration::from_secs(90));
        if let Some((_, Some(key))) = &provider {
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
        let oral = |txt: &str, done: bool| parse_header(txt, done).map(|(a, i, s)| (a, i, txt[s..].trim().to_string()));
        let o = |a: &str, i: &str, t: &str| Some((a.to_string(), i.to_string(), t.to_string()));
        assert_eq!(oral("[TRANSMETTRE] Ajoute un export CSV\nJe lui transmets.", false), o("transmettre", "Ajoute un export CSV", "Je lui transmets."));
        assert_eq!(oral("[TRANSMETTRE] pas fini", false), None);
        assert_eq!(oral("[REPONDRE]\nIl en est aux tests.", false), o("repondre", "", "Il en est aux tests."));
        // Sur une seule ligne : connu dès la balise, la voix démarre sans attendre la fin.
        assert_eq!(oral("[RÉPONDRE] Salut ! Je", false), o("repondre", "", "Salut ! Je"));
        assert_eq!(oral("[REPON", false), None);
        assert_eq!(oral("[ARRETER]\nJ'arrête.", false), o("arreter", "", "J'arrête."));
        assert_eq!(oral("Bonjour, je t'écoute.", false), o("repondre", "", "Bonjour, je t'écoute."));
    }
}
