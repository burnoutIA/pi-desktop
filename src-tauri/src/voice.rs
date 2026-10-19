//! Moteur vocal : STT et TTS Kyutai (moshi-server, WebSocket + MessagePack),
//! détection de fin de tour, filtre anti-écho et niveaux pour l'égaliseur.
//! Push-to-talk : le micro n'est transcrit qu'après Espace.
//! Mains libres : un petit détecteur (Vosk, processeur) attend le nom de l'agent ; « Jarvis, ma
//! requête » ouvre la transcription Kyutai (avec les 2 s de son précédentes), le nom est retiré, et
//! une pause juste après le nom déclenche une courte réponse préenregistrée (« Oui ? »).

use crate::audio::{self, Analyzer, AudioShared, Resampler};
use crate::speech::{self, OralExtractor, Speech};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;

mod duplex;

/// 80 ms à 24 kHz : la taille de trame attendue par le STT.
const STT_CHUNK: usize = 1920;
/// Son gardé avant l'ouverture d'une session : 0,5 s en push-to-talk (ne pas couper le premier
/// mot), 2 s au réveil par le nom (le nom et le début de la demande sont déjà prononcés).
const PRE_ROLL_PTT: usize = 12_000;
const PRE_ROLL_WAKE: usize = 48_000;
/// Après la fin de l'écoute, on continue d'envoyer le micro le temps que le STT rende ses derniers mots.
const STT_TAIL: Duration = Duration::from_millis(1200);
/// Tête du VAD sémantique : 0 = 0,5 s, 1 = 1 s, 2 = 2 s, 3 = 3 s de pause prédite.
const PAUSE_HEAD: usize = 2;
const PAUSE_THRESHOLD: f32 = 0.6;
/// Fin de phrase : en plus de la pause prédite, aucun mot rendu depuis N pas de 80 ms (1,6 s).
/// Sans ça, une simple hésitation (« tu me conseilles comme… micro-cravate ») coupait la phrase.
const END_SILENCE_STEPS: u32 = 20;
/// Temps laissé au STT (0,5 s de retard) pour finir les derniers mots après un arrêt manuel.
const MANUAL_FLUSH: Duration = Duration::from_millis(900);
/// Après la fin de la voix de pi, le STT (0,5 s de retard) peut encore rendre son écho.
const ECHO_TAIL: Duration = Duration::from_millis(2500);
/// Mains libres : si ta voix s'arrête juste après le nom, on répond « Oui ? » et on attend.
/// On juge la pause au volume du micro (immédiat), pas aux mots du STT (0,5 s de retard ou plus).
const ACK_DELAY: Duration = Duration::from_millis(800);
/// Ta voix « continue » si le micro dépasse cette fraction de ton volume au moment du nom.
const ACK_VOICE_RATIO: f32 = 0.25;
const ACK_MIN_RMS: f32 = 0.01;
/// Réponses préenregistrées, générées une fois par voix puis gardées sur disque.
const ACK_PHRASES: [&str; 10] = [
    "Oui ?",
    "Hm hm ?",
    "Je t'écoute.",
    "Vas-y.",
    "Dis-moi.",
    "Oui, je t'écoute.",
    "Je suis là.",
    "Oui, dis-moi ?",
    "Mmh ?",
    "Je t'écoute, vas-y.",
];
const WAKE_RATE: u32 = 16_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfig {
    pub stt_url: String,
    pub tts_url: String,
    pub voice: String,
    pub tts_enabled: bool,
    /// Réveil par le nom de l'agent au lieu d'Espace.
    #[serde(default)]
    pub hands_free: bool,
    #[serde(default = "default_wake_url")]
    pub wake_url: String,
    /// Nom de l'agent, à prononcer pour le réveiller.
    #[serde(default = "default_wake_word")]
    pub wake_word: String,
    /// Mains libres : les mots plus faibles que ta voix de plus de N dB sont ignorés.
    #[serde(default = "default_filter_db")]
    pub filter_db: f32,
    /// Mains libres : répondre « Oui ? » quand tu marques une pause après le nom.
    #[serde(default = "default_true")]
    pub ack_on_pause: bool,
    /// Conversation duplex (expérimental) : un LLM local décide toutes les 0,6 s de parler ou
    /// d'écouter, le micro reste ouvert pendant qu'il parle. Rien n'est envoyé à pi.
    #[serde(default)]
    pub duplex: bool,
    #[serde(default = "default_duplex_url")]
    pub duplex_url: String,
}

fn default_duplex_url() -> String {
    "http://127.0.0.1:18093".into()
}

fn default_true() -> bool {
    true
}

fn default_wake_url() -> String {
    "ws://127.0.0.1:8091".into()
}
fn default_wake_word() -> String {
    "pi".into()
}
fn default_filter_db() -> f32 {
    20.0
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            stt_url: "ws://127.0.0.1:8090".into(),
            tts_url: "ws://127.0.0.1:8089".into(),
            voice: "estelle".into(),
            tts_enabled: true,
            hands_free: false,
            wake_url: default_wake_url(),
            wake_word: default_wake_word(),
            filter_db: default_filter_db(),
            ack_on_pause: true,
            duplex: false,
            duplex_url: default_duplex_url(),
        }
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
    Ready,
    Error { message: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum TtsMsg {
    Audio { pcm: Vec<f32> },
    Text { text: String },
    Ready,
    Error { message: String },
    #[serde(other)]
    Other,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum SttOut<'a> {
    Audio { pcm: &'a [f32] },
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum TtsOut {
    Text { text: String },
    Eos,
}

struct TtsSession {
    id: u64,
    tx: UnboundedSender<TtsOut>,
    words: HashSet<String>,
    /// Fin de texte envoyée : le prochain mot ouvre une nouvelle session, et celle-ci finit sa lecture.
    ended: bool,
}

#[derive(Default)]
struct TurnState {
    listening: bool,
    speech_started: bool,
    pause_ema: f32,
    /// Pas STT (80 ms) depuis le dernier mot de l'utilisateur.
    steps_since_word: u32,
    /// Mots prononcés récemment par l'agent (filtre anti-écho).
    echo_words: HashSet<String>,
    echo_until: Option<Instant>,
    /// Niveaux récents du micro (~1,5 s), pour calibrer sur ta voix au moment du nom.
    mic_hist: std::collections::VecDeque<(Instant, f32)>,
    /// Le tour en cours a commencé par le nom (mains libres).
    wake_turn: bool,
    /// Pré-roll à donner à la prochaine session STT.
    pre_roll: usize,
    /// On retire le nom (et ce qui le précède) du début de la transcription.
    strip_name: bool,
    strip_buf: Vec<(String, f64)>,
    /// Ta voix au moment du nom (référence du filtre de volume).
    ref_rms: f32,
    /// Niveau de chaque trame de 80 ms envoyée au STT pendant la session.
    chunk_rms: Vec<f32>,
    words_after_wake: usize,
    /// Réponse préenregistrée en cours de lecture (« Oui ? ») et ses mots (anti-écho).
    ack_until: Option<Instant>,
    ack_words: HashSet<String>,
    /// Après un arrêt manuel, on accepte encore les derniers mots pendant un court instant.
    flush_until: Option<Instant>,
}

#[derive(Default)]
struct RunState {
    extractor: OralExtractor,
    spoke: bool,
    last_text: String,
}

pub struct VoiceEngine {
    app: AppHandle,
    pub audio: Arc<AudioShared>,
    config: Mutex<VoiceConfig>,
    tts: Mutex<Option<TtsSession>>,
    tts_ids: AtomicU64,
    turn: Mutex<TurnState>,
    run: Mutex<RunState>,
    stt_connected: AtomicBool,
    /// Réponses préenregistrées de la voix courante : (voix, [(phrase, pcm 24 kHz)]).
    acks: Mutex<(String, Vec<(String, Vec<f32>)>)>,
    acks_loading: AtomicBool,
    last_ack: AtomicU64,
    /// Les sessions TTS d'identifiant <= à celui-ci sont coupées (voix interrompue).
    tts_cut: AtomicU64,
    duplex_on: AtomicBool,
    duplex: Mutex<duplex::DuplexState>,
}

fn norm(w: &str) -> String {
    w.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Deux mots « pareils » malgré les petites erreurs de transcription (souci/soucis, vous/vou…).
fn similar(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (ca, cb): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let common = ca.iter().zip(&cb).take_while(|(x, y)| x == y).count();
    if common >= 4 && common + 2 >= ca.len().max(cb.len()) {
        return true;
    }
    ca.len().min(cb.len()) >= 4 && levenshtein(&ca, &cb) <= 1
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Mots gardés au plus en attendant le nom, au début d'un réveil.
const NAME_LOOKAHEAD: usize = 6;

/// Clé de prononciation française approximative : « Jarviss » et « jarvis » donnent la même,
/// « J'envisse » ou « leur vise » en restent proches. Sert à reconnaître le nom mal transcrit.
fn phon(s: &str) -> String {
    let s: String = norm(s)
        .chars()
        .map(|c| match c {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'à' | 'â' | 'ä' => 'a',
            'î' | 'ï' | 'y' => 'i',
            'ô' | 'ö' => 'o',
            'û' | 'ù' | 'ü' => 'u',
            'ç' | 'z' => 's',
            'w' => 'v',
            'x' => 's',
            c => c,
        })
        .filter(|c| *c != 'h')
        .collect();
    let s = s.replace("eau", "o").replace("au", "o").replace("ai", "e").replace("ei", "e").replace("ou", "u").replace("ph", "f").replace("qu", "k");
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let c = match c {
            'c' if matches!(chars.get(i + 1), Some('e' | 'i')) => 's',
            'c' => 'k',
            c => c,
        };
        if !out.ends_with(c) {
            out.push(c); // lettres doublées : « ss » -> « s »
        }
    }
    if out.len() > 2 && out.ends_with('e') {
        out.pop(); // « vise » -> « vis »
    }
    out
}

/// Où finit le nom de l'agent parmi les premiers mots transcrits après son réveil ? Le détecteur
/// a déjà entendu le nom : ici on le cherche, même déformé (« J'envisse », « J'ai un » + « vice »,
/// « leur vise »), en comparant la prononciation d'un à quatre mots collés. Renvoie l'indice du
/// premier mot qui suit le nom (ce qui le précède est retiré aussi), ou None.
fn name_end(words: &[(String, f64)], name: &str) -> Option<usize> {
    let key: Vec<char> = phon(name).chars().collect();
    let tolerance = match key.len() {
        0 => return None,
        1..=2 => 0,
        3..=4 => 1,
        n => n / 2,
    };
    let mut best: Option<(usize, usize)> = None;
    for i in 0..words.len().min(3) {
        for k in 1..=4 {
            if i + k > words.len() {
                break;
            }
            let joined: String = words[i..i + k].iter().map(|(w, _)| w.as_str()).collect();
            let cand: Vec<char> = phon(&joined).chars().collect();
            if cand.is_empty() {
                continue;
            }
            let d = levenshtein(&cand, &key);
            if d <= tolerance && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i + k));
            }
        }
    }
    best.map(|(_, end)| end)
}

/// Les mots qui suivent le nom, sans la ponctuation qui le suivait (« Jarvis ? pars » -> « pars »).
fn after_name(words: Vec<(String, f64)>, end: usize) -> Vec<(String, f64)> {
    let mut rest: Vec<(String, f64)> = words.into_iter().skip(end).skip_while(|(w, _)| norm(w).is_empty()).collect();
    if let Some((w, _)) = rest.first_mut() {
        *w = w.trim_start_matches(|c: char| !c.is_alphanumeric()).to_string();
    }
    rest
}

fn is_echo(words: &HashSet<String>, n: &str) -> bool {
    n.is_empty() || words.iter().any(|w| similar(w, n))
}

fn with_auth(url: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}auth_id=public_token")
}

fn url_escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl VoiceEngine {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
            audio: AudioShared::new(),
            config: Mutex::new(VoiceConfig::default()),
            tts: Mutex::new(None),
            tts_ids: AtomicU64::new(0),
            turn: Mutex::new(TurnState::default()),
            run: Mutex::new(RunState::default()),
            stt_connected: AtomicBool::new(false),
            acks: Mutex::new((String::new(), Vec::new())),
            acks_loading: AtomicBool::new(false),
            last_ack: AtomicU64::new(u64::MAX),
            tts_cut: AtomicU64::new(0),
            duplex_on: AtomicBool::new(false),
            duplex: Mutex::new(duplex::DuplexState::default()),
        })
    }

    pub fn config(&self) -> VoiceConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn set_config(&self, c: VoiceConfig) {
        self.duplex_on.store(c.duplex, Ordering::SeqCst);
        *self.config.lock().unwrap() = c;
    }

    /// Démarre l'audio, la boucle STT, le détecteur de réveil et la boucle de niveaux.
    pub fn start(self: &Arc<Self>) {
        let (mic_tx, mic_rx) = mpsc::unbounded_channel();
        let (wake_tx, wake_rx) = mpsc::unbounded_channel();
        match audio::start(self.audio.clone(), vec![mic_tx, wake_tx]) {
            Ok(rate) => {
                let me = self.clone();
                tauri::async_runtime::spawn(async move { me.stt_loop(mic_rx, rate).await });
                let me = self.clone();
                tauri::async_runtime::spawn(async move { me.wake_loop(wake_rx, rate).await });
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
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.duplex_loop().await });
    }

    // ---------- Écoute ----------

    pub fn set_listening(self: &Arc<Self>, on: bool, submit: bool) {
        {
            let mut t = self.turn.lock().unwrap();
            if on {
                t.listening = true;
                t.speech_started = false;
                t.pause_ema = 0.0;
                t.wake_turn = false;
                t.strip_name = false;
                t.pre_roll = PRE_ROLL_PTT;
            } else {
                t.listening = false;
                t.wake_turn = false;
                t.flush_until = submit.then(|| Instant::now() + MANUAL_FLUSH);
            }
        }
        self.emit_listening();
        if !on && submit {
            // On laisse arriver les derniers mots avant d'envoyer.
            let me = self.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(MANUAL_FLUSH).await;
                let _ = me.app.emit("stt-turn-end", json!({ "reason": "manual" }));
            });
        }
    }

    fn emit_listening(&self) {
        let l = self.turn.lock().unwrap().listening;
        crate::applog!("écoute : {}", if l { "ouverte" } else { "fermée" });
        let _ = self.app.emit("voice-listening", l);
    }

    fn accepting_words(&self, t: &TurnState) -> bool {
        t.listening || self.duplex_on() || t.flush_until.is_some_and(|u| Instant::now() < u)
    }

    fn on_stt_word(self: &Arc<Self>, text: String, start_time: f64) {
        crate::applog!("STT : mot « {text} » ({start_time:.2} s)");
        let current_words = self.tts.lock().unwrap().as_ref().map(|s| s.words.clone()).unwrap_or_default();
        let speaking = self.agent_speaking();
        let cfg = self.config();
        let name = norm(&cfg.wake_word);
        let now = Instant::now();
        let mut to_emit: Vec<(String, f64)> = Vec::new();
        {
            let mut t = self.turn.lock().unwrap();
            if !self.accepting_words(&t) {
                return;
            }
            // Pendant (ou juste après) la voix de pi ou un « Oui ? », le micro peut les capter.
            let mut echo_set = t.echo_words.clone();
            echo_set.extend(current_words);
            let echo_active = speaking || t.echo_until.is_some_and(|u| now < u);
            if t.ack_until.is_some_and(|u| now < u + ECHO_TAIL) {
                echo_set.extend(t.ack_words.iter().cloned());
            }
            let n = norm(&text);
            let ack_echo = t.ack_words.contains(&n) && t.ack_until.is_some_and(|u| now < u + ECHO_TAIL);
            if (echo_active && is_echo(&echo_set, &n)) || ack_echo {
                crate::applog!("écho ignoré : {text}");
                return;
            }
            if self.duplex_on() {
                // Conversation duplex : les mots vont au prochain micro-tour, même pendant sa voix.
                drop(t);
                self.duplex_heard(vec![text]);
                return;
            }
            if speaking && !t.wake_turn {
                // Push-to-talk : l'interface coupe la voix avant d'ouvrir l'écoute ;
                // un mot qui arrive ici est un reste de sa voix.
                return;
            }
            if t.wake_turn && t.strip_name {
                // On retire « (dis) Jarvis » du début : le nom a déjà servi à le réveiller. Le
                // détecteur se déclenche parfois au milieu du nom, et Kyutai le transcrit souvent
                // de travers et en morceaux : on garde les premiers mots jusqu'à le retrouver.
                t.strip_buf.extend(text.split_whitespace().map(|w| (w.to_string(), start_time)));
                if let Some(end) = name_end(&t.strip_buf, &name) {
                    let held = std::mem::take(&mut t.strip_buf);
                    let removed: Vec<&str> = held[..end].iter().map(|(w, _)| w.as_str()).collect();
                    crate::applog!("nom retiré du texte : « {} »", removed.join(" "));
                    t.strip_name = false;
                    to_emit = after_name(held, end);
                    if to_emit.is_empty() {
                        return;
                    }
                } else if t.strip_buf.len() >= NAME_LOOKAHEAD {
                    // Pas de nom dans les premiers mots : on rend tout.
                    t.strip_name = false;
                    to_emit = std::mem::take(&mut t.strip_buf);
                } else {
                    return;
                }
            } else {
                to_emit.push((text, start_time));
            }

            if t.wake_turn && t.ref_rms > 0.0 {
                // Filtre de volume : on ne garde que les mots à peu près aussi forts que ta voix.
                let threshold = t.ref_rms * 10f32.powf(-cfg.filter_db / 20.0);
                let chunks = &t.chunk_rms;
                to_emit.retain(|(w, start)| {
                    // L'horodatage Kyutai arrive tard : ~0,5 s après le début du mot, et pour un mot
                    // isolé (après une hésitation) à peu près à sa fin. Mesuré : « micro-cravate »
                    // horodaté 4,08 s pour une voix entre 3,4 et 3,9 s. Fenêtre : start-1,2 s … start+0,4 s.
                    let i = ((start - 1.2) / 0.08).max(0.0) as usize;
                    let level = chunks.iter().skip(i).take(20).copied().fold(0.0f32, f32::max);
                    let keep = level == 0.0 || level >= threshold;
                    if !keep {
                        crate::applog!("mot trop faible ignoré : « {w} » ({level:.4} < {threshold:.4})");
                    }
                    keep
                });
            }
            if to_emit.is_empty() {
                return;
            }
            t.words_after_wake += to_emit.len();
            t.speech_started = true;
            t.pause_ema = 0.0;
            t.steps_since_word = 0;
        }
        for (w, _) in to_emit {
            let _ = self.app.emit("stt-word", w);
        }
    }

    // ---------- Réveil par le nom (mains libres) ----------

    /// Le détecteur a entendu le nom de l'agent.
    fn on_wake(self: &Arc<Self>) {
        let cfg = self.config();
        if !cfg.hands_free {
            return;
        }
        let name = norm(&cfg.wake_word);
        // pi est peut-être en train de dire son propre nom : on ne se réveille pas sur sa voix.
        let saying_name = self.tts.lock().unwrap().as_ref().is_some_and(|s| s.words.contains(&name));
        if saying_name {
            crate::applog!("réveil ignoré : pi prononce son propre nom");
            return;
        }
        let was_speaking = self.agent_speaking();
        {
            let now = Instant::now();
            let mut t = self.turn.lock().unwrap();
            if t.listening && t.wake_turn && !was_speaking {
                return; // déjà à l'écoute
            }
            t.ref_rms = t
                .mic_hist
                .iter()
                .filter(|(at, _)| now.duration_since(*at) < Duration::from_millis(1500))
                .map(|(_, r)| *r)
                .fold(0.0f32, f32::max);
            t.listening = true;
            t.wake_turn = true;
            t.speech_started = false;
            t.pause_ema = 0.0;
            t.strip_name = true;
            t.strip_buf.clear();
            t.words_after_wake = 0;
            t.pre_roll = PRE_ROLL_WAKE;
            crate::applog!("réveil : « {} » (voix de référence {:.4})", cfg.wake_word, t.ref_rms);
        }
        if was_speaking {
            crate::applog!("interruption par le nom");
            self.stop_speaking();
        }
        let _ = self.app.emit("voice-wake", ());
        self.emit_listening();
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.ensure_acks().await });
        // Pause juste après le nom : il répond « Oui ? » et attend la demande.
        if !cfg.ack_on_pause {
            return;
        }
        let me = self.clone();
        let woke_at = Instant::now();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(ACK_DELAY).await;
            let (pause, peak, threshold) = {
                let t = me.turn.lock().unwrap();
                // Volume du micro depuis le réveil (on saute les 150 premières ms : fin du nom).
                let peak = t
                    .mic_hist
                    .iter()
                    .filter(|(at, _)| at.duration_since(woke_at) > Duration::from_millis(150))
                    .map(|(_, r)| *r)
                    .fold(0.0f32, f32::max);
                let threshold = ACK_MIN_RMS.max(t.ref_rms * ACK_VOICE_RATIO);
                (t.listening && t.wake_turn && t.words_after_wake == 0 && peak < threshold, peak, threshold)
            };
            if pause {
                me.play_ack();
            } else {
                crate::applog!("pas de « Oui ? » : tu continues à parler (micro {peak:.4} / seuil {threshold:.4})");
            }
        });
    }

    fn play_ack(&self) {
        let acks = self.acks.lock().unwrap();
        if acks.1.is_empty() {
            return;
        }
        // Au hasard, sans répéter la précédente.
        let n = acks.1.len() as u64;
        let mut i = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0)
            % n;
        if i == self.last_ack.load(Ordering::Relaxed) {
            i = (i + 1) % n;
        }
        self.last_ack.store(i, Ordering::Relaxed);
        let (phrase, pcm) = &acks.1[i as usize];
        crate::applog!("réponse préenregistrée : « {phrase} »");
        {
            let mut t = self.turn.lock().unwrap();
            t.ack_until = Some(Instant::now() + Duration::from_secs_f32(pcm.len() as f32 / audio::MODEL_RATE as f32));
            // « Dis-moi » -> « dis », « moi » : le STT découpe souvent autrement que nous.
            t.ack_words = phrase
                .split(|c: char| !c.is_alphanumeric())
                .map(norm)
                .filter(|w| !w.is_empty())
                .collect();
        }
        self.audio.playback.lock().unwrap().push(pcm);
    }

    /// Génère (une fois par voix) et charge les réponses préenregistrées.
    async fn ensure_acks(self: Arc<Self>) {
        let voice = self.config().voice;
        // Déjà toutes prêtes pour cette voix, ou génération en cours : rien à faire.
        if self.acks.lock().unwrap().0 == voice || self.acks_loading.swap(true, Ordering::SeqCst) {
            return;
        }
        let dir = crate::applog::path().with_file_name("acks").join(url_escape(&voice));
        let _ = std::fs::create_dir_all(&dir);
        let mut clips = Vec::new();
        for (i, phrase) in ACK_PHRASES.iter().enumerate() {
            let file = dir.join(format!("{i}.f32"));
            let pcm = match std::fs::read(&file) {
                Ok(b) if b.len() >= 4 => b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                _ => match self.synth(phrase).await {
                    Ok(pcm) => {
                        let bytes: Vec<u8> = pcm.iter().flat_map(|x| x.to_le_bytes()).collect();
                        let _ = std::fs::write(&file, bytes);
                        pcm
                    }
                    Err(e) => {
                        crate::applog!("réponse préenregistrée « {phrase} » impossible : {e}");
                        continue;
                    }
                },
            };
            clips.push((phrase.to_string(), pcm));
        }
        crate::applog!("{} réponses préenregistrées prêtes ({voice})", clips.len());
        // Incomplètes (synthèse pas encore prête au démarrage) : on réessaiera au prochain réveil.
        let done = clips.len() == ACK_PHRASES.len();
        *self.acks.lock().unwrap() = (if done { voice } else { String::new() }, clips);
        self.acks_loading.store(false, Ordering::SeqCst);
    }

    /// Synthèse d'une phrase entière en mémoire (pour les réponses préenregistrées).
    async fn synth(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let cfg = self.config();
        let url = with_auth(&format!(
            "{}/api/tts_streaming?voice={}&format=PcmMessagePack&cfg_alpha=1.5",
            cfg.tts_url,
            url_escape(&cfg.voice)
        ));
        let (ws, _) = tokio_tungstenite::connect_async(url.as_str()).await?;
        let (mut sink, mut stream) = ws.split();
        for w in text.split_whitespace() {
            sink.send(Message::Binary(rmp_serde::to_vec_named(&TtsOut::Text { text: w.into() })?.into())).await?;
        }
        sink.send(Message::Binary(rmp_serde::to_vec_named(&TtsOut::Eos)?.into())).await?;
        let mut pcm = Vec::new();
        let collect = async {
            while let Some(Ok(msg)) = stream.next().await {
                if let Message::Binary(b) = msg {
                    if let Ok(TtsMsg::Audio { pcm: p }) = rmp_serde::from_slice::<TtsMsg>(&b) {
                        pcm.extend(p);
                    }
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(20), collect).await?;
        // On retire les silences de début et de fin pour une réponse immédiate.
        let first = pcm.iter().position(|x| x.abs() > 0.01).unwrap_or(0).saturating_sub(240);
        let last = pcm.iter().rposition(|x| x.abs() > 0.01).map(|i| (i + 2400).min(pcm.len())).unwrap_or(pcm.len());
        Ok(pcm[first..last].to_vec())
    }

    /// Envoie le micro (16 kHz) au détecteur de réveil tant que le mode mains libres est actif.
    async fn wake_loop(self: Arc<Self>, mut mic_rx: UnboundedReceiver<Vec<f32>>, mic_rate: u32) {
        let mut warned = false;
        loop {
            let cfg = self.config();
            if !cfg.hands_free {
                // Inactif : on vide simplement le flux.
                match tokio::time::timeout(Duration::from_millis(500), mic_rx.recv()).await {
                    Ok(None) => return,
                    _ => continue,
                }
            }
            let name = norm(&cfg.wake_word);
            let name = if name.is_empty() { "pi".to_string() } else { name };
            let me = self.clone();
            tauri::async_runtime::spawn(async move { me.ensure_acks().await });

            let url = format!("{}/?name={}", cfg.wake_url.trim_end_matches('/'), url_escape(&name));
            let ws = match tokio_tungstenite::connect_async(url.as_str()).await {
                Ok((ws, _)) => ws,
                Err(e) => {
                    if !warned {
                        warned = true;
                        crate::applog!("réveil injoignable : {e}");
                        let _ = self.app.emit(
                            "voice-error",
                            "Détecteur de réveil injoignable : lance scripts/voice-server.sh (service « wake »).",
                        );
                    }
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
                    while let Ok(Some(_)) = tokio::time::timeout_at(deadline, mic_rx.recv()).await {}
                    continue;
                }
            };
            warned = false;
            crate::applog!("réveil : connecté, nom « {name} »");
            let (mut sink, mut stream) = ws.split();
            let me = self.clone();
            let reader = tauri::async_runtime::spawn(async move {
                while let Some(Ok(msg)) = stream.next().await {
                    let Message::Text(txt) = msg else { continue };
                    let Ok(v) = serde_json::from_str::<Value>(txt.as_str()) else { continue };
                    match v["type"].as_str() {
                        Some("wake") => me.on_wake(),
                        Some("ready") if v["known"] == false => {
                            let _ = me.app.emit(
                                "voice-error",
                                format!("« {} » est inconnu du détecteur de réveil : choisis un autre nom.", v["name"]),
                            );
                        }
                        _ => {}
                    }
                }
            });
            let mut resampler = Resampler::to(mic_rate, WAKE_RATE);
            let mut out = Vec::new();
            while let Some(samples) = mic_rx.recv().await {
                out.clear();
                resampler.process(&samples, &mut out);
                let bytes: Vec<u8> = out
                    .iter()
                    .flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
                    .collect();
                if sink.send(Message::Binary(bytes.into())).await.is_err() || reader.inner().is_finished() {
                    break;
                }
                // Nom changé ou mode désactivé : on se reconnecte.
                let now = self.config();
                if !now.hands_free || norm(&now.wake_word) != name || now.wake_url != cfg.wake_url {
                    break;
                }
            }
            let _ = sink.close().await;
            reader.abort();
            crate::applog!("réveil : déconnecté");
        }
    }

    fn on_stt_step(&self, prs: &[f32]) {
        let Some(&p) = prs.get(PAUSE_HEAD) else { return };
        let ended = {
            let mut t = self.turn.lock().unwrap();
            t.pause_ema = 0.6 * t.pause_ema + 0.4 * p;
            t.steps_since_word = t.steps_since_word.saturating_add(1);
            // Mots retenus en attendant le nom, et plus rien depuis ~0,6 s : on les rend.
            if t.strip_name && !t.strip_buf.is_empty() && t.steps_since_word >= 8 {
                t.strip_name = false;
                let mut held = std::mem::take(&mut t.strip_buf);
                if let Some(end) = name_end(&held, &norm(&self.config.lock().unwrap().wake_word)) {
                    crate::applog!("nom retiré du texte (après une pause)");
                    held = after_name(held, end);
                }
                t.words_after_wake += held.len();
                t.speech_started = true;
                t.steps_since_word = 0;
                for (w, _) in held {
                    let _ = self.app.emit("stt-word", w);
                }
                return;
            }
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

    /// Faut-il une session de transcription ? (prise de parole, fin de phrase, ou mains libres)
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

    /// Une session STT par prise de parole : Kyutai n'est pas fait pour un flux continu de
    /// plusieurs dizaines de minutes (il finit par ne plus rendre de mots). Entre deux prises
    /// de parole, on garde juste les derniers instants du micro pour ne pas couper le début.
    async fn stt_loop(self: Arc<Self>, mut mic_rx: UnboundedReceiver<Vec<f32>>, mic_rate: u32) {
        let mut resampler = Resampler::new(mic_rate);
        let mut pre_roll: std::collections::VecDeque<f32> = std::collections::VecDeque::new();
        let mut out = Vec::new();
        let mut last_probe = Instant::now();
        self.probe_stt().await;
        loop {
            // 1. Attente d'une prise de parole (Espace ou nom de l'agent).
            while !self.wants_stt() {
                match tokio::time::timeout(Duration::from_millis(500), mic_rx.recv()).await {
                    Ok(Some(samples)) => {
                        out.clear();
                        resampler.process(&samples, &mut out);
                        pre_roll.extend(out.iter().copied());
                        while pre_roll.len() > PRE_ROLL_WAKE {
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
            let keep = {
                let mut t = self.turn.lock().unwrap();
                t.chunk_rms.clear();
                t.pre_roll
            };
            let skip = pre_roll.len().saturating_sub(keep);
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
                    let rms = (chunk.iter().map(|x| x * x).sum::<f32>() / STT_CHUNK as f32).sqrt();
                    self.turn.lock().unwrap().chunk_rms.push(rms);
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
                if self.duplex_on() && opened.elapsed() > duplex::STT_MAX_SESSION {
                    crate::applog!("STT : session duplex renouvelée");
                    break;
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

    // ---------- Synthèse ----------

    /// pi parle (une réponse préenregistrée « Oui ? » ne compte pas).
    fn agent_speaking(&self) -> bool {
        if self.tts.lock().unwrap().is_some() {
            return true;
        }
        let ack = self.turn.lock().unwrap().ack_until.is_some_and(|u| Instant::now() < u);
        !ack && !self.audio.playback.lock().unwrap().is_empty()
    }

    /// Coupe immédiatement la voix de l'agent.
    pub fn stop_speaking(&self) {
        self.tts_cut.store(self.tts_ids.load(Ordering::SeqCst), Ordering::SeqCst);
        let session = self.tts.lock().unwrap().take();
        self.audio.playback.lock().unwrap().clear();
        if let Some(s) = session {
            let mut t = self.turn.lock().unwrap();
            t.echo_words = s.words;
            t.echo_until = Some(Instant::now() + ECHO_TAIL);
        }
    }

    fn speak_word(self: &Arc<Self>, word: String) {
        let cfg = self.config();
        if !cfg.tts_enabled {
            return;
        }
        let mut guard = self.tts.lock().unwrap();
        if guard.as_ref().is_some_and(|s| s.ended) {
            // La phrase précédente finit sa lecture dans sa propre session ; ses mots restent
            // dans le filtre anti-écho.
            let old = guard.take().unwrap();
            let mut t = self.turn.lock().unwrap();
            t.echo_words = old.words;
            t.echo_until = Some(Instant::now() + ECHO_TAIL);
        }
        if guard.is_none() {
            *guard = Some(self.open_tts(&cfg));
        }
        let s = guard.as_mut().unwrap();
        s.words.insert(norm(&word));
        let _ = s.tx.send(TtsOut::Text { text: word });
    }

    fn speak_end(&self) {
        if let Some(s) = self.tts.lock().unwrap().as_mut() {
            if !s.ended {
                let _ = s.tx.send(TtsOut::Eos);
                s.ended = true;
            }
        }
    }

    /// Lit un texte complet (bouton « réécouter », repli sans `<oral>`).
    pub fn speak_text(self: &Arc<Self>, text: &str) {
        self.stop_speaking();
        for w in text.split_whitespace() {
            let w = speech::clean_word(w);
            if !w.is_empty() {
                self.speak_word(w);
            }
        }
        self.speak_end();
    }

    fn open_tts(self: &Arc<Self>, cfg: &VoiceConfig) -> TtsSession {
        let id = self.tts_ids.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, mut rx) = mpsc::unbounded_channel::<TtsOut>();
        let url = with_auth(&format!(
            "{}/api/tts_streaming?voice={}&format=PcmMessagePack&cfg_alpha=1.5",
            cfg.tts_url,
            url_escape(&cfg.voice)
        ));
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let ws = match tokio_tungstenite::connect_async(url.as_str()).await {
                Ok((ws, _)) => ws,
                Err(e) => {
                    crate::applog!("TTS injoignable : {e}");
                    let _ = me.app.emit("voice-error", format!("TTS injoignable : {e}"));
                    me.close_tts(id);
                    return;
                }
            };
            let (mut sink, mut stream) = ws.split();
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
            let me_r = me.clone();
            let reader = tauri::async_runtime::spawn(async move {
                let mut ready_tx = Some(ready_tx);
                while let Some(Ok(msg)) = stream.next().await {
                    let Message::Binary(bytes) = msg else { continue };
                    if me_r.tts_cut.load(Ordering::SeqCst) >= id {
                        break;
                    }
                    match rmp_serde::from_slice::<TtsMsg>(&bytes) {
                        Ok(TtsMsg::Audio { pcm }) => me_r.audio.playback.lock().unwrap().push(&pcm),
                        Ok(TtsMsg::Ready) => {
                            if let Some(t) = ready_tx.take() {
                                let _ = t.send(());
                            }
                        }
                        Ok(TtsMsg::Text { text }) => {
                            let _ = me_r.app.emit("tts-word", text);
                        }
                        Ok(TtsMsg::Error { message }) => {
                            let _ = me_r.app.emit("voice-error", format!("TTS : {message}"));
                        }
                        Ok(TtsMsg::Other) => {}
                        Err(e) => crate::applog!("TTS : message illisible : {e}"),
                    }
                }
                me_r.close_tts(id);
            });
            // Le serveur doit être prêt avant de recevoir le texte.
            if tokio::time::timeout(Duration::from_secs(10), ready_rx).await.is_err() {
                let _ = me.app.emit("voice-error", "TTS : pas de réponse du serveur");
                reader.abort();
                me.close_tts(id);
                return;
            }
            while let Some(m) = rx.recv().await {
                let eos = matches!(m, TtsOut::Eos);
                let Ok(bytes) = rmp_serde::to_vec_named(&m) else { continue };
                if sink.send(Message::Binary(bytes.into())).await.is_err() || eos {
                    break;
                }
            }
            // Sortie de boucle sans Eos = session interrompue : on ferme.
            if me.tts_cut.load(Ordering::SeqCst) >= id {
                let _ = sink.close().await;
                reader.abort();
            }
        });
        TtsSession { id, tx, words: HashSet::new(), ended: false }
    }

    fn close_tts(&self, id: u64) {
        let mut guard = self.tts.lock().unwrap();
        if guard.as_ref().is_some_and(|s| s.id == id) {
            let s = guard.take().unwrap();
            let mut t = self.turn.lock().unwrap();
            t.echo_words = s.words;
            t.echo_until = None; // l'audio peut encore être en lecture : `is_speaking` le couvre
        }
    }

    // ---------- Événements pi -> voix ----------

    pub fn on_pi_event(self: &Arc<Self>, ev: &Value) {
        match ev["type"].as_str() {
            Some("agent_start") => {
                let mut r = self.run.lock().unwrap();
                r.spoke = false;
                r.last_text.clear();
            }
            Some("message_start") if ev["message"]["role"] == "assistant" => {
                self.run.lock().unwrap().extractor.reset();
            }
            Some("message_update") => {
                let ame = &ev["assistantMessageEvent"];
                if ame["type"] == "text_delta" {
                    if let Some(d) = ame["delta"].as_str() {
                        let out = {
                            let mut r = self.run.lock().unwrap();
                            let out = r.extractor.push(d);
                            if r.extractor.started() {
                                r.spoke = true;
                            }
                            out
                        };
                        self.apply(out);
                    }
                }
            }
            Some("message_end") if ev["message"]["role"] == "assistant" => {
                let text: String = ev["message"]["content"]
                    .as_array()
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter(|b| b["type"] == "text")
                            .filter_map(|b| b["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                let out = {
                    let mut r = self.run.lock().unwrap();
                    if !text.trim().is_empty() {
                        r.last_text = text;
                    }
                    r.extractor.finish()
                };
                self.apply(out);
            }
            Some("agent_settled") => {
                let aborted = ev["aborted"].as_bool().unwrap_or(false);
                let fallback = {
                    let r = self.run.lock().unwrap();
                    (!r.spoke && !aborted && !r.last_text.is_empty()).then(|| speech::fallback_speech(&r.last_text))
                };
                if let Some(t) = fallback.filter(|t| !t.is_empty()) {
                    self.speak_text(&t);
                }
            }
            _ => {}
        }
    }

    fn apply(self: &Arc<Self>, out: Vec<Speech>) {
        for s in out {
            match s {
                Speech::Word(w) => self.speak_word(w),
                Speech::End => self.speak_end(),
            }
        }
    }

    // ---------- Niveaux pour l'égaliseur ----------

    async fn levels_loop(self: Arc<Self>) {
        let mut analyzer = Analyzer::new();
        let mut tick = tokio::time::interval(Duration::from_millis(22));
        let mut was_speaking = false;
        loop {
            tick.tick().await;
            let (out, out_rms, out_wave) = {
                let ring = self.audio.out_ring.lock().unwrap();
                let (b, r) = analyzer.bands(&ring);
                (b, r, Analyzer::wave(&ring))
            };
            let (inp, in_rms, in_wave) = {
                let ring = self.audio.in_ring.lock().unwrap();
                let (b, r) = analyzer.bands(&ring);
                (b, r, Analyzer::wave(&ring))
            };
            let _ = self.app.emit(
                "audio-levels",
                json!({ "out": out, "in": inp, "outRms": out_rms, "inRms": in_rms, "outWave": out_wave, "inWave": in_wave }),
            );
            let speaking = self.agent_speaking();
            {
                let now = Instant::now();
                let mut t = self.turn.lock().unwrap();
                t.mic_hist.push_back((now, in_rms));
                while t.mic_hist.front().is_some_and(|(at, _)| now.duration_since(*at) > Duration::from_millis(1500)) {
                    t.mic_hist.pop_front();
                }
            }
            if speaking != was_speaking {
                was_speaking = speaking;
                if !speaking {
                    let mut t = self.turn.lock().unwrap();
                    t.echo_until = Some(Instant::now() + ECHO_TAIL);
                }
                let _ = self.app.emit("voice-speaking", speaking);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_messages_are_tagged_maps() {
        let b = rmp_serde::to_vec_named(&TtsOut::Text { text: "salut".into() }).unwrap();
        let v: Value = rmp_serde::from_slice(&b).unwrap();
        assert_eq!(v, json!({ "type": "Text", "text": "salut" }));
        let b = rmp_serde::to_vec_named(&TtsOut::Eos).unwrap();
        assert_eq!(rmp_serde::from_slice::<Value>(&b).unwrap(), json!({ "type": "Eos" }));
        let b = rmp_serde::to_vec_named(&SttOut::Audio { pcm: &[0.5, -0.25] }).unwrap();
        assert_eq!(rmp_serde::from_slice::<Value>(&b).unwrap(), json!({ "type": "Audio", "pcm": [0.5, -0.25] }));
    }

    #[test]
    fn incoming_messages_parse() {
        let word = rmp_serde::to_vec_named(&json!({ "type": "Word", "text": "bonjour", "start_time": 0.5 })).unwrap();
        assert!(matches!(rmp_serde::from_slice::<SttMsg>(&word).unwrap(), SttMsg::Word { text, start_time } if text == "bonjour" && start_time == 0.5));
        let step = rmp_serde::to_vec_named(&json!({ "type": "Step", "step_idx": 3, "prs": [0.1, 0.2, 0.9, 0.95] })).unwrap();
        assert!(matches!(rmp_serde::from_slice::<SttMsg>(&step).unwrap(), SttMsg::Step { prs } if prs.len() == 4));
        let end = rmp_serde::to_vec_named(&json!({ "type": "EndWord", "stop_time": 1.0 })).unwrap();
        assert!(matches!(rmp_serde::from_slice::<SttMsg>(&end).unwrap(), SttMsg::Other));
        let audio = rmp_serde::to_vec_named(&json!({ "type": "Audio", "pcm": [0.0, 0.1] })).unwrap();
        assert!(matches!(rmp_serde::from_slice::<TtsMsg>(&audio).unwrap(), TtsMsg::Audio { pcm } if pcm.len() == 2));
    }

    #[test]
    fn echo_matching_tolerates_transcription_noise() {
        let words: HashSet<String> = ["pas", "de", "souci", "reconnaissance", "vocale"].iter().map(|s| s.to_string()).collect();
        assert!(is_echo(&words, "soucis"));
        assert!(is_echo(&words, "reconnaissances"));
        assert!(is_echo(&words, "pas"));
        assert!(!is_echo(&words, "attends"));
        assert!(!is_echo(&words, "stop"));
    }

    #[test]
    fn name_is_stripped_even_when_misheard() {
        let words = |s: &str| -> Vec<(String, f64)> { s.split_whitespace().map(|w| (w.to_string(), 0.0)).collect() };
        let strip = |s: &str, name: &str| -> Option<String> {
            let w = words(s);
            name_end(&w, name).map(|end| after_name(w, end).into_iter().map(|(w, _)| w).collect::<Vec<_>>().join(" "))
        };
        assert_eq!(strip("Jarvis,", "jarvis").as_deref(), Some(""));
        assert_eq!(strip("J'envis, c'est pour faire", "jarvis").as_deref(), Some("c'est pour faire"));
        assert_eq!(strip("Dis Jarvis ? ajoute", "jarvis").as_deref(), Some("ajoute"));
        // Transcriptions réelles relevées dans le journal
        assert_eq!(strip("J'envisse. Qu'est-ce qu'on fait", "jarvis").as_deref(), Some("Qu'est-ce qu'on fait"));
        assert_eq!(strip("J'ai un vice ? Tu penses pas", "jarvis").as_deref(), Some("Tu penses pas"));
        assert_eq!(strip("leur vise part sur les parts sur les tests", "jarvis").as_deref(), Some("part sur les parts sur les tests"));
        assert_eq!(strip("Jarviss? pars sur les tests", "jarvis").as_deref(), Some("pars sur les tests"));
        // Pas de nom : rien n'est retiré
        assert_eq!(strip("Qu'est-ce que tu me conseilles", "jarvis"), None);
        assert_eq!(strip("Vas-y, patche-le. Après, dis-moi", "jarvis"), None);
        assert_eq!(strip("Allez, vas-y.", "jarvis"), None);
        assert_eq!(strip("pi, lance les tests", "pi").as_deref(), Some("lance les tests"));
        assert_eq!(strip("je pense que", "pi"), None);
    }

    #[test]
    fn voice_param_is_escaped() {
        assert_eq!(url_escape("cml-tts/fr/a b.wav"), "cml-tts%2Ffr%2Fa%20b.wav");
    }
}
