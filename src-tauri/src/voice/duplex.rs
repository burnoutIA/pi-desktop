//! Conversation duplex (expérimental) : toutes les 0,6 s, ce que le micro a entendu part vers un
//! LLM local (llama-server + LoRA MicroTurn de codacus), qui répond par quelques mots à dire ou par
//! `<listen>`. Pas de détection de fin de phrase : c'est le modèle qui décide quand parler, quand
//! se taire et quand te laisser la parole. Le micro reste ouvert pendant qu'il parle.
//!
//! Le protocole (prompt système, gabarit brut, règle du seuil à 0,3) doit rester identique à celui
//! de l'entraînement : https://huggingface.co/codacus/Qwen3.6-35B-A3B-MicroTurn-Duplex-LoRA

use super::*;

pub(super) const TICK: Duration = Duration::from_millis(600);
const LISTEN: &str = "<listen>";
const NOVOICE: &str = "<no voice>";
/// On n'écoute que si « < » pèse au moins ça dans le premier token ; sinon le modèle doit parler.
/// Sans cette règle, il s'arrête au milieu de ses phrases (voir la carte du modèle).
const LISTEN_P: f64 = 0.3;
/// Le modèle a été entraîné sur ~8000 tokens par appel (~500 tours) : au-delà, on oublie le début.
const MAX_TURNS: usize = 500;
const KEEP_TURNS: usize = 250;
/// Kyutai n'aime pas les flux de plusieurs dizaines de minutes : on rouvre la session STT.
pub(super) const STT_MAX_SESSION: Duration = Duration::from_secs(600);

fn system_prompt() -> String {
    format!(
        "You are a voice assistant in a live phone call; both sides can talk at the same time. Every \
         0.6 seconds you get what the microphone heard in that moment, as a streaming transcript fragment, \
         or {NOVOICE}. You reply with up to three new words to say next, or {LISTEN} to add nothing."
    )
}

#[derive(Default)]
pub(super) struct DuplexState {
    /// Mots entendus depuis le dernier tour.
    pending: Vec<String>,
    /// (rôle, contenu) : "user" puis "assistant", un couple par tour. Jamais modifié en place,
    /// pour que llama-server reprenne son cache à chaque requête.
    history: Vec<(&'static str, String)>,
    reset: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TurnEvent {
    heard: String,
    said: String,
    p_listen: f64,
    ms: u64,
    /// L'IA parlait, tu as parlé, elle a choisi de se taire : sa voix est coupée.
    cut: bool,
}

impl VoiceEngine {
    pub(super) fn duplex_on(&self) -> bool {
        self.duplex_on.load(Ordering::SeqCst)
    }

    pub(super) fn duplex_heard(&self, words: Vec<String>) {
        let mut d = self.duplex.lock().unwrap();
        for w in &words {
            let _ = self.app.emit("duplex-heard", w);
        }
        d.pending.extend(words);
    }

    /// Repart d'une conversation vide (au prochain tour).
    pub fn duplex_reset(&self) {
        self.stop_speaking();
        self.duplex.lock().unwrap().reset = true;
    }

    pub(super) async fn duplex_loop(self: Arc<Self>) {
        let client = reqwest::Client::new();
        let mut tick = tokio::time::interval(TICK);
        // Un tour trop long décale les suivants au lieu de les enchaîner en rafale.
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut was_on = false;
        let mut failing = false;
        loop {
            tick.tick().await;
            let on = self.duplex_on() && self.stt_connected();
            if on != was_on {
                was_on = on;
                {
                    let mut d = self.duplex.lock().unwrap();
                    d.pending.clear();
                    d.history.clear();
                }
                if !on {
                    self.stop_speaking();
                }
                crate::applog!("duplex : {}", if on { "actif" } else { "inactif" });
                let _ = self.app.emit("duplex-active", on);
            }
            if !on {
                continue;
            }
            let (heard, prompt) = {
                let mut d = self.duplex.lock().unwrap();
                if std::mem::take(&mut d.reset) {
                    d.history.clear();
                    d.pending.clear();
                }
                let heard = if d.pending.is_empty() { NOVOICE.to_string() } else { d.pending.drain(..).collect::<Vec<_>>().join(" ") };
                if d.history.len() / 2 >= MAX_TURNS && !self.agent_speaking() {
                    let drop = (MAX_TURNS - KEEP_TURNS) * 2;
                    d.history.drain(..drop);
                    crate::applog!("duplex : début de la conversation oublié ({KEEP_TURNS} tours gardés)");
                }
                let mut p = format!("<|im_start|>system\n{}<|im_end|>\n", system_prompt());
                for (role, content) in d.history.iter().map(|(r, c)| (*r, c.as_str())).chain([("user", heard.as_str())]) {
                    p.push_str(&format!("<|im_start|>{role}\n{content}<|im_end|>\n"));
                }
                p.push_str("<|im_start|>assistant\n");
                (heard, p)
            };
            let url = self.config().duplex_url;
            let t0 = Instant::now();
            let (said, p_listen) = match ask(&client, &url, &prompt).await {
                Ok(r) => {
                    if failing {
                        failing = false;
                        crate::applog!("duplex : LLM de nouveau joignable");
                    }
                    r
                }
                Err(e) => {
                    if !failing {
                        failing = true;
                        crate::applog!("duplex : LLM injoignable ({url}) : {e}");
                        let _ = self.app.emit(
                            "voice-error",
                            format!("Conversation duplex : LLM injoignable ({url}). Lance microturn-bench/scripts/serve.sh 40."),
                        );
                    }
                    // Ce tour est perdu : on remet les mots entendus pour le suivant.
                    if heard != NOVOICE {
                        self.duplex.lock().unwrap().pending.insert(0, heard);
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            let ms = t0.elapsed().as_millis() as u64;
            // Une remise à zéro demandée pendant la requête : on jette ce tour.
            if self.duplex.lock().unwrap().reset {
                continue;
            }
            let mut cut = false;
            if said == LISTEN {
                if self.agent_speaking() {
                    if heard != NOVOICE {
                        // Tu parles pendant qu'elle parle, et elle te laisse la parole.
                        crate::applog!("duplex : coupée par « {heard} »");
                        self.stop_speaking();
                        cut = true;
                    } else {
                        // Fin de sa phrase : le TTS peut finir les derniers mots.
                        self.speak_end();
                    }
                }
            } else {
                for w in said.split_whitespace() {
                    let w = speech::clean_word(w);
                    if !w.is_empty() {
                        self.speak_word(w);
                    }
                }
            }
            {
                let mut d = self.duplex.lock().unwrap();
                d.history.push(("user", heard.clone()));
                d.history.push(("assistant", said.clone()));
            }
            if heard != NOVOICE || said != LISTEN || ms > 500 {
                crate::applog!("duplex : « {heard} » -> « {said} » (P(listen) {p_listen:.2}, {ms} ms)");
            }
            let _ = self.app.emit("duplex-turn", TurnEvent { heard, said, p_listen, ms, cut });
        }
    }
}

/// Un micro-tour : ce que le modèle dit (quelques mots ou `<listen>`) et la probabilité de « < ».
async fn ask(client: &reqwest::Client, url: &str, prompt: &str) -> anyhow::Result<(String, f64)> {
    let endpoint = format!("{}/completion", url.trim_end_matches('/'));
    let mut body = json!({
        "prompt": prompt,
        "n_predict": 64,
        "temperature": 0,
        "stop": ["<|im_end|>"],
        "cache_prompt": true,
        "n_probs": 8,
        "id_slot": 0,
        "lora": [{ "id": 0, "scale": 1.0 }],
    });
    let j: Value = client.post(&endpoint).json(&body).timeout(Duration::from_secs(10)).send().await?.error_for_status()?.json().await?;
    let mut out = j["content"].as_str().unwrap_or("").trim().to_string();
    let top = j["completion_probabilities"][0]["top_logprobs"].as_array().cloned().unwrap_or_default();
    let listen: Vec<&Value> = top.iter().filter(|p| p["token"].as_str().is_some_and(|t| t.starts_with('<'))).collect();
    let p_listen: f64 = listen.iter().filter_map(|p| p["logprob"].as_f64()).map(f64::exp).sum::<f64>().max(0.0);
    // « < » choisi alors qu'il ne pèse que 27 % : le reste est réparti entre plein de mots possibles.
    // On l'interdit et le modèle doit continuer sa phrase.
    if out.starts_with('<') && p_listen < LISTEN_P && !listen.is_empty() {
        body["logit_bias"] = Value::Array(listen.iter().map(|p| json!([p["id"], false])).collect());
        body["n_probs"] = json!(0);
        let j: Value = client.post(&endpoint).json(&body).timeout(Duration::from_secs(10)).send().await?.error_for_status()?.json().await?;
        out = j["content"].as_str().unwrap_or("").trim().to_string();
    }
    let said = if out.is_empty() || out.starts_with('<') { LISTEN.to_string() } else { out };
    Ok((said, p_listen))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_matches_training_byte_for_byte() {
        assert_eq!(
            system_prompt(),
            "You are a voice assistant in a live phone call; both sides can talk at the same time. Every 0.6 seconds you get what the microphone heard in that moment, as a streaming transcript fragment, or <no voice>. You reply with up to three new words to say next, or <listen> to add nothing."
        );
    }
}
