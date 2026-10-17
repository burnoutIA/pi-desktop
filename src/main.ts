import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { api } from "./lib/api";
import { applyTheme, loadThemePref, saveThemePref, watchSystemMode } from "./lib/theme";

// Thème appliqué avant le premier rendu (pas de flash de l'ancien thème).
const pref = loadThemePref();
// Hors de l'app (navigateur, captures) : ?palette=braise&mode=light pour prévisualiser un thème.
if (!("__TAURI_INTERNALS__" in window)) {
  const q = new URLSearchParams(location.search);
  if (q.get("palette")) pref.palette = q.get("palette")!;
  if (q.get("mode")) pref.mode = q.get("mode") as typeof pref.mode;
  if (q.get("palette") || q.get("mode")) saveThemePref(pref);
}
applyTheme(pref);
watchSystemMode();

// Les erreurs de l'interface vont dans le journal de l'app (~/.cache/pi-voice/app.log).
window.addEventListener("error", (e) => api.log(`${e.message} (${e.filename}:${e.lineno})`));
window.addEventListener("unhandledrejection", (e) => api.log(`promesse rejetée : ${String(e.reason)}`));

mount(App, { target: document.getElementById("app")! });
