//! Moteur vocal : transcription seule (Parakeet, processeur), au protocole des serveurs Kyutai
//! (WebSocket + MessagePack). Détection de fin de tour et niveaux du micro pour l'égaliseur.
//! Push-to-talk : le micro n'est transcrit qu'après Espace. pi répond à l'écrit, sans voix.

use crate::audio::{self, Analyzer, AudioShared, DisplayGain, Resampler};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio_tungstenite::tungstenite::Message;

/// 80 ms à 24 kHz : la taille de trame attendue par le STT.
const STT_CHUNK: usize = 1920;
/// Son gardé avant l'ouverture d'une session (0,5 s) : le premier mot n'est pas coupé.
const PRE_ROLL: usize = 12_000;
/// Après la fin de l'écoute, on continue d'envoyer le micro le temps que le STT rende ses derniers mots.
const STT_TAIL: Duration = Duration::from_millis(1200);
/// Fin de dictée (Espace relâché) : l'app envoie `Flush` et le serveur répond `Flushed` une fois les
/// derniers mots transcrits. Filet si le serveur ne le gère pas : au moins FLUSH_MIN_STEPS pas de
/// 80 ms traités après l'arrêt, sans mot depuis FLUSH_QUIET_STEPS pas.
const FLUSH_MIN_STEPS: u32 = 10;
const FLUSH_QUIET_STEPS: u32 = 6;
/// Tête du VAD sémantique : 0 = 0,5 s, 1 = 1 s, 2 = 2 s, 3 = 3 s de pause prédite.
const PAUSE_HEAD: usize = 2;
const PAUSE_THRESHOLD: f32 = 0.6;
/// Fin de phrase : en plus de la pause prédite, aucun mot rendu depuis N pas de 80 ms (1,6 s).
/// Sans ça, une simple hésitation (« tu me conseilles comme… micro-cravate ») coupait la phrase.
const END_SILENCE_STEPS: u32 = 20;
/// Délai maximal laissé au STT pour rendre les derniers mots après un arrêt manuel.
const MANUAL_FLUSH: Duration = Duration::from_millis(3000);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfig {
    pub stt_url: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self { stt_url: "ws://127.0.0.1:8090".into() }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum SttMsg {
    Word {
        text: String,
        #[serde(default)]
        start_time: f64,
    },
    Step { prs: Vec<f32> },
    /// Réponse à `Flush` : les derniers mots sont partis.
    Flushed,
    Ready,
    Error { message: String },
    #[serde(other)]
    Other,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum SttOut<'a> {
    Audio { pcm: &'a [f32] },
    /// Fin de dictée : transcrire tout de suite ce qui reste.
    Flush,
}

#[derive(Default)]
struct TurnState {
    listening: bool,
    speech_started: bool,
    pause_ema: f32,
    /// Pas STT (80 ms) depuis le dernier mot de l'utilisateur.
    steps_since_word: u32,
    /// Après un arrêt manuel, on accepte encore les derniers mots pendant un court instant.
    flush_until: Option<Instant>,
    /// Fin de dictée en attente des derniers mots du STT, et pas STT reçus depuis l'arrêt.
    flush_pending: bool,
    flush_steps: u32,
}

pub struct VoiceEngine {
    app: AppHandle,
    pub audio: Arc<AudioShared>,
    config: Mutex<VoiceConfig>,
    turn: Mutex<TurnState>,
    stt_connected: AtomicBool,
    /// Fin de dictée à signaler au serveur de transcription (`Flush`).
    stt_flush: AtomicBool,
}

fn with_auth(url: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}auth_id=public_token")
}

impl VoiceEngine {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
            audio: AudioShared::new(),
            config: Mutex::new(VoiceConfig::default()),
            turn: Mutex::new(TurnState::default()),
            stt_connected: AtomicBool::new(false),
            stt_flush: AtomicBool::new(false),
        })
    }

    pub fn config(&self) -> VoiceConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn set_config(&self, c: VoiceConfig) {
        *self.config.lock().unwrap() = c;
    }

    /// Démarre l'audio, la boucle STT et la boucle de niveaux.
    pub fn start(self: &Arc<Self>) {
        let (mic_tx, mic_rx) = mpsc::unbounded_channel();
        match audio::start(self.audio.clone(), vec![mic_tx]) {
            Ok(rate) => {
                let me = self.clone();
                tauri::async_runtime::spawn(async move { me.stt_loop(mic_rx, rate).await });
                // Un micro qui ne renvoie rien (périphérique bloqué, mauvais choix par défaut…)
                // ne produit aucune erreur : on le signale nous-mêmes.
                let me = self.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let n = me.audio.mic_samples.load(Ordering::Relaxed);
                    crate::applog!("audio : {n} échantillons reçus du micro en 3 s");
                    if n == 0 {
                        let _ = me.app.emit(
                            "voice-error",
                            "Le micro ne renvoie aucun son. Débranche/rebranche-le, ou relance PipeWire \
                             (systemctl --user restart pipewire wireplumber).",
                        );
                    }
                });
            }
            Err(e) => {
                crate::applog!("audio indisponible : {e}");
                let _ = self.app.emit("voice-error", format!("Audio indisponible : {e}"));
            }
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.levels_loop().await });
    }

    // ---------- Écoute ----------

    pub fn set_listening(self: &Arc<Self>, on: bool, submit: bool) {
        {
            let mut t = self.turn.lock().unwrap();
            if on {
                t.listening = true;
                t.speech_started = false;
                t.pause_ema = 0.0;
                t.flush_pending = false;
                self.stt_flush.store(false, Ordering::SeqCst);
            } else {
                t.listening = false;
                self.stt_flush.store(submit, Ordering::SeqCst);
                t.flush_until = submit.then(|| Instant::now() + MANUAL_FLUSH);
                t.flush_pending = submit;
                t.flush_steps = 0;
            }
        }
        self.emit_listening();
        if !on && submit {
            // On laisse arriver les derniers mots avant d'envoyer : le serveur répond `Flushed` dès
            // qu'ils sont partis ; ceci n'est qu'un filet s'il ne répond plus.
            let me = self.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(MANUAL_FLUSH).await;
                if std::mem::take(&mut me.turn.lock().unwrap().flush_pending) {
                    crate::applog!("fin de dictée (délai maximal)");
                    let _ = me.app.emit("stt-turn-end", json!({ "reason": "manual" }));
                }
            });
        }
    }

    fn emit_listening(&self) {
        let l = self.turn.lock().unwrap().listening;
        crate::applog!("écoute : {}", if l { "ouverte" } else { "fermée" });
        let _ = self.app.emit("voice-listening", l);
    }

    fn accepting_words(&self, t: &TurnState) -> bool {
        t.listening || t.flush_until.is_some_and(|u| Instant::now() < u)
    }

    fn on_stt_word(&self, text: String, start_time: f64) {
        crate::applog!("STT : mot « {text} » ({start_time:.2} s)");
        {
            let mut t = self.turn.lock().unwrap();
            if !self.accepting_words(&t) {
                return;
            }
            t.speech_started = true;
            t.pause_ema = 0.0;
            t.steps_since_word = 0;
        }
        let _ = self.app.emit("stt-word", text);
    }

    /// Le serveur a transcrit les derniers mots après `Flush` : la dictée peut partir.
    fn on_stt_flushed(&self) {
        if std::mem::take(&mut self.turn.lock().unwrap().flush_pending) {
            crate::applog!("fin de dictée : derniers mots reçus");
            let _ = self.app.emit("stt-turn-end", json!({ "reason": "manual" }));
        }
    }

    fn on_stt_step(&self, prs: &[f32]) {
        let flushed = {
            let mut t = self.turn.lock().unwrap();
            if t.flush_pending {
                t.flush_steps += 1;
                let done = t.flush_steps >= FLUSH_MIN_STEPS && t.steps_since_word >= FLUSH_QUIET_STEPS;
                if done {
                    t.flush_pending = false;
                }
                done
            } else {
                false
            }
        };
        if flushed {
            crate::applog!("fin de dictée : derniers mots reçus");
            let _ = self.app.emit("stt-turn-end", json!({ "reason": "manual" }));
        }
        let Some(&p) = prs.get(PAUSE_HEAD) else { return };
        let ended = {
            let mut t = self.turn.lock().unwrap();
            t.pause_ema = 0.6 * t.pause_ema + 0.4 * p;
            t.steps_since_word = t.steps_since_word.saturating_add(1);
            // Le micro reste ouvert : si l'utilisateur reprend pendant le compte à rebours,
            // l'interface annule l'envoi et la dictée continue. C'est elle qui ferme l'écoute.
            if t.listening
                && t.speech_started
                && t.pause_ema > PAUSE_THRESHOLD
                && t.steps_since_word >= END_SILENCE_STEPS
            {
                t.speech_started = false;
                true
            } else {
                false
            }
        };
        if ended {
            crate::applog!("fin de phrase détectée (pause)");
            let _ = self.app.emit("stt-turn-end", json!({ "reason": "pause" }));
        }
    }

    /// Faut-il une session de transcription ? (prise de parole, ou derniers mots après l'arrêt)
    fn wants_stt(&self) -> bool {
        let t = self.turn.lock().unwrap();
        self.accepting_words(&t)
    }

    /// Vérifie que le serveur de transcription répond (sans garder la connexion).
    async fn probe_stt(&self) -> bool {
        let url = with_auth(&format!("{}/api/asr-streaming", self.config().stt_url));
        let ok = match tokio::time::timeout(Duration::from_secs(3), tokio_tungstenite::connect_async(url.as_str())).await {
            Ok(Ok((mut ws, _))) => {
                let _ = ws.close(None).await;
                true
            }
            _ => false,
        };
        self.set_stt_connected(ok);
        ok
    }

    /// Une session STT par prise de parole. Entre deux prises de parole, on garde juste les
    /// derniers instants du micro pour ne pas couper le début.
    async fn stt_loop(self: Arc<Self>, mut mic_rx: UnboundedReceiver<Vec<f32>>, mic_rate: u32) {
        let mut resampler = Resampler::new(mic_rate);
        let mut pre_roll: std::collections::VecDeque<f32> = std::collections::VecDeque::new();
        let mut out = Vec::new();
        let mut last_probe = Instant::now();
        self.probe_stt().await;
        loop {
            // 1. Attente d'une prise de parole (Espace).
            while !self.wants_stt() {
                match tokio::time::timeout(Duration::from_millis(500), mic_rx.recv()).await {
                    Ok(Some(samples)) => {
                        out.clear();
                        resampler.process(&samples, &mut out);
                        pre_roll.extend(out.iter().copied());
                        while pre_roll.len() > PRE_ROLL {
                            pre_roll.pop_front();
                        }
                    }
                    Ok(None) => return,
                    Err(_) => {}
                }
                if !self.stt_connected() && last_probe.elapsed() > Duration::from_secs(5) {
                    last_probe = Instant::now();
                    self.probe_stt().await;
                }
            }

            // 2. Ouverture de la session.
            let url = with_auth(&format!("{}/api/asr-streaming", self.config().stt_url));
            let ws = match tokio_tungstenite::connect_async(url.as_str()).await {
                Ok((ws, _)) => ws,
                Err(e) => {
                    crate::applog!("STT injoignable : {e}");
                    self.set_stt_connected(false);
                    let _ = self.app.emit("voice-error", "Serveur de transcription injoignable (scripts/voice-server.sh ?)");
                    self.turn.lock().unwrap().listening = false;
                    self.emit_listening();
                    continue;
                }
            };
            self.set_stt_connected(true);
            crate::applog!("STT : session ouverte");
            let (mut sink, mut stream) = ws.split();
            let me = self.clone();
            let reader = tauri::async_runtime::spawn(async move {
                while let Some(Ok(msg)) = stream.next().await {
                    let Message::Binary(bytes) = msg else { continue };
                    match rmp_serde::from_slice::<SttMsg>(&bytes) {
                        Ok(SttMsg::Word { text, start_time }) => me.on_stt_word(text, start_time),
                        Ok(SttMsg::Step { prs }) => me.on_stt_step(&prs),
                        Ok(SttMsg::Flushed) => me.on_stt_flushed(),
                        Ok(SttMsg::Error { message }) => {
                            crate::applog!("STT : erreur {message}");
                            let _ = me.app.emit("voice-error", format!("STT : {message}"));
                        }
                        Ok(SttMsg::Ready | SttMsg::Other) => {}
                        Err(e) => crate::applog!("STT : message illisible : {e}"),
                    }
                }
            });

            // 3. Envoi du micro tant que la prise de parole dure (+ un court délai pour les derniers mots).
            let skip = pre_roll.len().saturating_sub(PRE_ROLL);
            let mut pending: Vec<f32> = pre_roll.drain(..).skip(skip).collect();
            let opened = Instant::now();
            let mut idle_since: Option<Instant> = None;
            loop {
                let Some(samples) = mic_rx.recv().await else { return };
                out.clear();
                resampler.process(&samples, &mut out);
                pending.extend_from_slice(&out);
                let mut failed = false;
                while pending.len() >= STT_CHUNK {
                    let chunk: Vec<f32> = pending.drain(..STT_CHUNK).collect();
                    let Ok(bytes) = rmp_serde::to_vec_named(&SttOut::Audio { pcm: &chunk }) else { continue };
                    if sink.send(Message::Binary(bytes.into())).await.is_err() {
                        failed = true;
                        break;
                    }
                }
                if failed || reader.inner().is_finished() {
                    crate::applog!("STT : connexion perdue");
                    break;
                }
                // Espace relâché : le serveur transcrit tout de suite ce qui reste.
                if self.stt_flush.swap(false, Ordering::SeqCst) {
                    if let Ok(bytes) = rmp_serde::to_vec_named(&SttOut::Flush) {
                        let _ = sink.send(Message::Binary(bytes.into())).await;
                    }
                }
                if self.wants_stt() {
                    idle_since = None;
                } else if idle_since.get_or_insert_with(Instant::now).elapsed() > STT_TAIL {
                    break;
                }
            }
            let _ = sink.close().await;
            reader.abort();
            crate::applog!("STT : session fermée ({:.1} s)", opened.elapsed().as_secs_f32());
        }
    }

    fn set_stt_connected(&self, on: bool) {
        if self.stt_connected.swap(on, Ordering::SeqCst) != on {
            crate::applog!("STT {}", if on { "connecté" } else { "déconnecté" });
            let _ = self.app.emit("voice-status", json!({ "stt": on }));
        }
    }

    pub fn stt_connected(&self) -> bool {
        self.stt_connected.load(Ordering::SeqCst)
    }

    // ---------- Niveaux pour l'égaliseur ----------

    async fn levels_loop(self: Arc<Self>) {
        let mut analyzer = Analyzer::new();
        let mut gain = DisplayGain::new();
        let mut g = 1.0;
        let mut tick = tokio::time::interval(Duration::from_millis(22));
        loop {
            tick.tick().await;
            let (inp, rms, wave) = {
                let ring = self.audio.in_ring.lock().unwrap();
                let (b, r) = analyzer.bands(&ring, g);
                (b, r, Analyzer::wave(&ring, g))
            };
            // Gain du prochain tour, d'après le niveau brut de celui-ci.
            g = gain.update(rms);
            let _ = self.app.emit("audio-levels", json!({ "in": inp, "inRms": rms, "inWave": wave }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn outgoing_messages_are_tagged_maps() {
        let b = rmp_serde::to_vec_named(&SttOut::Audio { pcm: &[0.5, -0.25] }).unwrap();
        assert_eq!(rmp_serde::from_slice::<Value>(&b).unwrap(), json!({ "type": "Audio", "pcm": [0.5, -0.25] }));
        let b = rmp_serde::to_vec_named(&SttOut::Flush).unwrap();
        assert_eq!(rmp_serde::from_slice::<Value>(&b).unwrap(), json!({ "type": "Flush" }));
    }

    #[test]
    fn incoming_messages_parse() {
        let word = rmp_serde::to_vec_named(&json!({ "type": "Word", "text": "bonjour", "start_time": 0.5 })).unwrap();
        assert!(matches!(rmp_serde::from_slice::<SttMsg>(&word).unwrap(), SttMsg::Word { text, start_time } if text == "bonjour" && start_time == 0.5));
        let step = rmp_serde::to_vec_named(&json!({ "type": "Step", "step_idx": 3, "prs": [0.1, 0.2, 0.9, 0.95] })).unwrap();
        assert!(matches!(rmp_serde::from_slice::<SttMsg>(&step).unwrap(), SttMsg::Step { prs } if prs.len() == 4));
        let flushed = rmp_serde::to_vec_named(&json!({ "type": "Flushed" })).unwrap();
        assert!(matches!(rmp_serde::from_slice::<SttMsg>(&flushed).unwrap(), SttMsg::Flushed));
        let end = rmp_serde::to_vec_named(&json!({ "type": "EndWord", "stop_time": 1.0 })).unwrap();
        assert!(matches!(rmp_serde::from_slice::<SttMsg>(&end).unwrap(), SttMsg::Other));
    }
}
