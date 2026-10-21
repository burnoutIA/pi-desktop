//! Navigateur de l'aperçu : une vraie vue web posée par-dessus le panneau d'aperçu, pour les
//! adresses web (appli lancée sur localhost, site). Contrairement à une iframe, elle affiche aussi
//! les pages qui interdisent d'être intégrées (X-Frame-Options, frame-ancestors), comme qBittorrent.
//!
//! Sous Linux, Tauri empile ses vues enfants au lieu de les superposer : on crée donc la vue
//! WebKitGTK nous-mêmes, dans un `gtk::Overlay` qui enveloppe la vue de l'interface. L'interface
//! donne sa place à l'écran en pixels CSS, qui sont ceux de GTK (pas de zoom dans l'app).
//! Les objets GTK vivent sur le fil principal : chaque opération y passe par `with_webview`.

use anyhow::{anyhow, Result};
use gtk::prelude::*;
use std::cell::RefCell;
use tauri::{AppHandle, Emitter, Manager};
use webkit2gtk::{LoadEvent, WebViewExt};

#[derive(serde::Deserialize, Clone, Copy)]
pub struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

thread_local! {
    static VIEW: RefCell<Option<webkit2gtk::WebView>> = const { RefCell::new(None) };
}

/// Exécute `f` sur le fil principal avec la vue du navigateur (créée au besoin).
fn with_view(app: &AppHandle, create: bool, f: impl FnOnce(&webkit2gtk::WebView) + Send + 'static) -> Result<()> {
    let win = app.get_webview_window("main").ok_or_else(|| anyhow!("fenêtre principale introuvable"))?;
    let events = app.clone();
    win.with_webview(move |pw| {
        let existing = VIEW.with(|v| v.borrow().clone());
        let view = match existing {
            Some(v) => v,
            None if create => match build(&pw.inner(), events) {
                Some(v) => v,
                None => return,
            },
            None => return,
        };
        f(&view);
    })?;
    Ok(())
}

/// Enveloppe la vue de l'interface dans un calque et y ajoute la vue du navigateur.
fn build(main: &webkit2gtk::WebView, events: AppHandle) -> Option<webkit2gtk::WebView> {
    let parent = main.parent()?.downcast::<gtk::Container>().ok()?;
    let overlay = gtk::Overlay::new();
    parent.remove(main);
    overlay.add(main);
    match parent.downcast_ref::<gtk::Box>() {
        Some(b) => b.pack_start(&overlay, true, true, 0),
        None => parent.add(&overlay),
    }
    let view = webkit2gtk::WebView::new();
    view.set_halign(gtk::Align::Start);
    view.set_valign(gtk::Align::Start);
    // L'adresse affichée suit la navigation (liens, redirections, connexion…).
    view.connect_load_changed(move |v, ev| {
        if ev == LoadEvent::Finished {
            if let Some(uri) = v.uri() {
                let _ = events.emit("browser-url", uri.as_str());
            }
        }
    });
    overlay.add_overlay(&view);
    overlay.show_all();
    view.hide();
    main.grab_focus();
    VIEW.with(|v| *v.borrow_mut() = Some(view.clone()));
    Some(view)
}

fn place(view: &webkit2gtk::WebView, b: Bounds) {
    view.set_margin_start(b.x.round().max(0.0) as i32);
    view.set_margin_top(b.y.round().max(0.0) as i32);
    view.set_size_request(b.width.round().max(1.0) as i32, b.height.round().max(1.0) as i32);
}

/// Affiche `url` à l'emplacement `b`.
pub fn show(app: &AppHandle, url: &str, b: Bounds) -> Result<()> {
    anyhow::ensure!(url.starts_with("http://") || url.starts_with("https://"), "adresse non web : {url}");
    let url = url.to_string();
    with_view(app, true, move |view| {
        place(view, b);
        if view.uri().as_deref() != Some(url.as_str()) {
            view.load_uri(&url);
        }
        view.show();
    })
}

/// Nouvelle place à l'écran (panneau redimensionné, fenêtre agrandie…).
pub fn bounds(app: &AppHandle, b: Bounds) -> Result<()> {
    with_view(app, false, move |view| place(view, b))
}

/// Masque la vue (une fenêtre de l'interface passe par-dessus).
pub fn hide(app: &AppHandle) -> Result<()> {
    with_view(app, false, |view| view.hide())
}

/// Ferme l'aperçu : la page est déchargée (scripts, sons…) et la vue masquée.
pub fn close(app: &AppHandle) -> Result<()> {
    with_view(app, false, |view| {
        view.hide();
        view.load_uri("about:blank");
    })
}

pub fn reload(app: &AppHandle) -> Result<()> {
    with_view(app, false, |view| view.reload())
}
