//! Diagnostic : liste les périphériques audio et enregistre 3 s depuis le micro par défaut.
//! cargo run --example audio_probe
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

fn main() {
    let host = cpal::default_host();
    println!("hôte : {:?}", host.id());
    for d in host.input_devices().unwrap() {
        let name = d.description().map(|x| format!("{x:?}")).unwrap_or_default();
        println!("  entrée : {name}");
    }
    let want = std::env::args().nth(1);
    let dev = match &want {
        Some(w) => host
            .input_devices()
            .unwrap()
            .find(|d| d.description().ok().and_then(|x| x.driver().map(|s| s.to_string())).as_deref() == Some(w.as_str()))
            .expect("périphérique introuvable"),
        None => host.default_input_device().expect("pas de micro par défaut"),
    };
    println!("micro par défaut : {:?}", dev.description());
    let cfg = dev.default_input_config().unwrap();
    println!("format : {cfg:?}");
    let stats = Arc::new(Mutex::new((0usize, 0f64)));
    let s2 = stats.clone();
    let stream = dev
        .build_input_stream(
            cfg.config(),
            move |data: &[f32], _| {
                let mut s = s2.lock().unwrap();
                s.0 += data.len();
                s.1 += data.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
            },
            |e| eprintln!("erreur : {e}"),
            None,
        )
        .unwrap();
    stream.play().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));
    let s = stats.lock().unwrap();
    println!("échantillons reçus en 3 s : {}  (rms {:.5})", s.0, (s.1 / s.0.max(1) as f64).sqrt());
}
