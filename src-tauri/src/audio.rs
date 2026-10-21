//! Entrée micro + sortie haut-parleur (cpal) et analyse spectrale pour l'égaliseur.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

/// Fréquence des modèles Kyutai.
pub const MODEL_RATE: u32 = 24_000;
pub const BANDS: usize = 40;
const FFT_SIZE: usize = 2048;

/// File d'échantillons TTS (24 kHz) lue par le callback de sortie.
#[derive(Default)]
pub struct Playback {
    queue: VecDeque<f32>,
    frac: f64,
}

impl Playback {
    pub fn push(&mut self, pcm: &[f32]) {
        self.queue.extend(pcm.iter().copied());
    }
    pub fn clear(&mut self) {
        self.queue.clear();
        self.frac = 0.0;
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    /// Échantillon suivant au débit de sortie (interpolation linéaire).
    fn next(&mut self, step: f64) -> f32 {
        let s = match (self.queue.front(), self.queue.get(1)) {
            (Some(&a), Some(&b)) => a + (b - a) * self.frac as f32,
            (Some(&a), None) => a,
            _ => return 0.0,
        };
        self.frac += step;
        while self.frac >= 1.0 && !self.queue.is_empty() {
            self.queue.pop_front();
            self.frac -= 1.0;
        }
        s
    }
}

/// Derniers échantillons entendus / captés, pour la FFT.
pub struct Ring {
    data: VecDeque<f32>,
    pub rate: u32,
}

impl Ring {
    fn new() -> Self {
        Self { data: VecDeque::from(vec![0.0; FFT_SIZE]), rate: 48_000 }
    }
    fn push(&mut self, s: f32) {
        if self.data.len() >= FFT_SIZE {
            self.data.pop_front();
        }
        self.data.push_back(s);
    }
}

pub struct AudioShared {
    /// Nombre d'échantillons reçus du micro (pour détecter un micro muet).
    pub mic_samples: std::sync::atomic::AtomicU64,
    pub playback: Mutex<Playback>,
    pub out_ring: Mutex<Ring>,
    pub in_ring: Mutex<Ring>,
}

impl AudioShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            mic_samples: std::sync::atomic::AtomicU64::new(0),
            playback: Mutex::new(Playback::default()),
            out_ring: Mutex::new(Ring::new()),
            in_ring: Mutex::new(Ring::new()),
        })
    }
}

/// Démarre les flux audio dans un thread dédié (les `Stream` cpal y restent vivants).
/// Retourne la fréquence d'échantillonnage du micro.
/// Chaque consommateur du micro (aujourd'hui, le STT seul) reçoit sa copie des échantillons.
pub fn start(shared: Arc<AudioShared>, mic_tx: Vec<UnboundedSender<Vec<f32>>>) -> Result<u32> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32, String>>();
    std::thread::Builder::new().name("audio".into()).spawn(move || {
        let host = cpal::default_host();
        let run = || -> Result<(cpal::Stream, cpal::Stream, u32)> {
            let out_dev = host.default_output_device().ok_or_else(|| anyhow!("aucune sortie audio"))?;
            let in_dev = host.default_input_device().ok_or_else(|| anyhow!("aucun micro"))?;
            let out_cfg = out_dev.default_output_config()?;
            let in_cfg = in_dev.default_input_config()?;
            let name = |d: &cpal::Device| d.description().map(|x| x.name().to_string()).unwrap_or_default();
            crate::applog!("audio : sortie « {} » {:?}", name(&out_dev), out_cfg);
            crate::applog!("audio : micro « {} » {:?}", name(&in_dev), in_cfg);
            let out_stream = match out_cfg.sample_format() {
                SampleFormat::F32 => build_output::<f32>(&out_dev, out_cfg.config(), shared.clone())?,
                SampleFormat::I16 => build_output::<i16>(&out_dev, out_cfg.config(), shared.clone())?,
                SampleFormat::U16 => build_output::<u16>(&out_dev, out_cfg.config(), shared.clone())?,
                f => return Err(anyhow!("format de sortie non géré : {f:?}")),
            };
            let in_rate = in_cfg.config().sample_rate;
            let in_stream = match in_cfg.sample_format() {
                SampleFormat::F32 => build_input::<f32>(&in_dev, in_cfg.config(), shared.clone(), mic_tx.clone())?,
                SampleFormat::I16 => build_input::<i16>(&in_dev, in_cfg.config(), shared.clone(), mic_tx.clone())?,
                SampleFormat::U16 => build_input::<u16>(&in_dev, in_cfg.config(), shared.clone(), mic_tx.clone())?,
                f => return Err(anyhow!("format micro non géré : {f:?}")),
            };
            out_stream.play()?;
            in_stream.play()?;
            Ok((out_stream, in_stream, in_rate))
        };
        match run() {
            Ok((_o, _i, rate)) => {
                let _ = ready_tx.send(Ok(rate));
                loop {
                    std::thread::park();
                }
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e.to_string()));
            }
        }
    })?;
    ready_rx.recv()?.map_err(|e| anyhow!(e))
}

fn build_output<T>(dev: &cpal::Device, config: StreamConfig, shared: Arc<AudioShared>) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    let step = MODEL_RATE as f64 / rate as f64;
    shared.out_ring.lock().unwrap().rate = rate;
    let stream = dev.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let mut pb = shared.playback.lock().unwrap();
            let mut ring = shared.out_ring.lock().unwrap();
            for frame in data.chunks_mut(channels) {
                let s = pb.next(step);
                ring.push(s);
                let v = T::from_sample(s);
                for x in frame.iter_mut() {
                    *x = v;
                }
            }
        },
        |e| crate::applog!("audio : erreur sortie : {e}"),
        None,
    )?;
    Ok(stream)
}

fn build_input<T>(
    dev: &cpal::Device,
    config: StreamConfig,
    shared: Arc<AudioShared>,
    tx: Vec<UnboundedSender<Vec<f32>>>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    shared.in_ring.lock().unwrap().rate = config.sample_rate;
    let stream = dev.build_input_stream(
        config,
        move |data: &[T], _| {
            let mono: Vec<f32> = data
                .chunks(channels)
                .map(|f| f.iter().map(|&s| f32::from_sample(s)).sum::<f32>() / channels as f32)
                .collect();
            {
                let mut ring = shared.in_ring.lock().unwrap();
                for &s in &mono {
                    ring.push(s);
                }
            }
            shared.mic_samples.fetch_add(mono.len() as u64, std::sync::atomic::Ordering::Relaxed);
            if let Some((last, rest)) = tx.split_last() {
                for t in rest {
                    let _ = t.send(mono.clone());
                }
                let _ = last.send(mono);
            }
        },
        |e| crate::applog!("audio : erreur micro : {e}"),
        None,
    )?;
    Ok(stream)
}

/// Convertit un flux mono de `from` Hz vers `to` Hz (24 kHz par défaut) : interpolation linéaire + passe-bas simple.
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
    lp: f32,
}

impl Resampler {
    pub fn new(from: u32) -> Self {
        Self::to(from, MODEL_RATE)
    }
    pub fn to(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, pos: 0.0, prev: 0.0, lp: 0.0 }
    }
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        // Passe-bas d'ordre 1 pour limiter le repliement quand on divise la fréquence.
        let alpha = if self.step > 1.0 { (1.0 / self.step) as f32 } else { 1.0 };
        for &raw in input {
            self.lp += alpha * (raw - self.lp);
            let cur = self.lp;
            while self.pos < 1.0 {
                out.push(self.prev + (cur - self.prev) * self.pos as f32);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = cur;
        }
    }
}

/// Gain d'affichage automatique : ramène le niveau de la voix (micro ou synthèse) à une même
/// cible, pour que l'égaliseur réagisse autant à l'utilisateur qu'à l'IA.
pub struct DisplayGain {
    env: f32,
}

impl DisplayGain {
    const TARGET: f32 = 0.1;
    /// En dessous, c'est du silence ou du bruit de fond : le niveau mesuré ne bouge pas.
    const VOICE: f32 = 0.002;
    /// Retombée du niveau mesuré à chaque appel (~22 ms) : demi-vie d'environ 3 s.
    const DECAY: f32 = 0.995;

    pub fn new() -> Self {
        Self { env: Self::TARGET }
    }

    pub fn update(&mut self, rms: f32) -> f32 {
        if rms > Self::VOICE {
            self.env = rms.max(self.env * Self::DECAY);
        }
        (Self::TARGET / self.env).clamp(0.5, 12.0)
    }
}

/// Calcule des niveaux par bande (0..1, échelle log en fréquence).
pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buf: Vec<Complex<f32>>,
}

impl Analyzer {
    pub fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos())
            .collect();
        Self { fft, window, buf: vec![Complex::default(); FFT_SIZE] }
    }

    /// Forme d'onde récente (style « Onde ») : les 1024 derniers échantillons, un sur deux,
    /// en millièmes (entiers : message plus léger), amplifiés de `gain`. Vide dans le silence.
    pub fn wave(ring: &Ring, gain: f32) -> Vec<i16> {
        let n = ring.data.len();
        let rms = (ring.data.iter().map(|s| s * s).sum::<f32>() / n.max(1) as f32).sqrt();
        if rms < 2e-4 {
            return Vec::new();
        }
        ring.data.iter().skip(n.saturating_sub(1024)).step_by(2).map(|s| ((s * gain).clamp(-1.0, 1.0) * 1000.0) as i16).collect()
    }

    /// Retourne (bandes, rms brut). `gain` amplifie le signal avant l'affichage.
    pub fn bands(&mut self, ring: &Ring, gain: f32) -> (Vec<f32>, f32) {
        let mut sum = 0.0;
        for (i, (&s, w)) in ring.data.iter().zip(&self.window).enumerate() {
            sum += s * s;
            self.buf[i] = Complex::new(s * w * gain, 0.0);
        }
        let rms = (sum / FFT_SIZE as f32).sqrt();
        if rms < 2e-4 {
            return (vec![0.0; BANDS], rms);
        }
        self.fft.process(&mut self.buf);
        let bin_hz = ring.rate as f32 / FFT_SIZE as f32;
        let (lo, hi) = (90.0f32, 7500.0f32);
        let bands = (0..BANDS)
            .map(|b| {
                let f0 = lo * (hi / lo).powf(b as f32 / BANDS as f32);
                let f1 = lo * (hi / lo).powf((b + 1) as f32 / BANDS as f32);
                let i0 = ((f0 / bin_hz) as usize).max(1);
                let i1 = ((f1 / bin_hz) as usize).max(i0 + 1).min(FFT_SIZE / 2);
                let peak = self.buf[i0..i1].iter().map(|c| c.norm()).fold(0.0f32, f32::max);
                let db = 20.0 * (peak / (FFT_SIZE as f32 / 4.0) + 1e-9).log10();
                ((db + 65.0) / 55.0).clamp(0.0, 1.0)
            })
            .collect();
        (bands, rms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_48k_to_24k_halves_length() {
        let mut r = Resampler::new(48_000);
        let mut out = Vec::new();
        r.process(&vec![0.5; 4800], &mut out);
        assert!((out.len() as i32 - 2400).abs() <= 1, "{}", out.len());
    }

    #[test]
    fn playback_upsamples() {
        let mut p = Playback::default();
        p.push(&vec![1.0; 240]);
        let n = (0..1000).take_while(|_| p.next(0.5) != 0.0).count();
        assert!((n as i32 - 480).abs() <= 2, "{n}");
    }
}
