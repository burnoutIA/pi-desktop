// Données de démonstration, uniquement hors de l'app (navigateur, `?demo`) : pour juger le
// design et les palettes sans backend.
import type { Session } from "./session.svelte";
import type { GitStatus, SessionInfo } from "./api";

export const isDemo = () => !("__TAURI_INTERNALS__" in window) && new URLSearchParams(location.search).has("demo");

export function fillDemo(session: Session) {
  session.handle({ type: "agent_start" });
  session.items = [
    { kind: "user", id: "d1", text: "Jarvis, ajoute un mode sombre à l'éditeur et sauvegarde le choix." },
    {
      kind: "assistant",
      id: "d2",
      streaming: false,
      blocks: [
        { type: "thinking", text: "Je regarde d'abord la structure du fichier…" },
        { type: "text", text: "Je regarde comment l'éditeur gère ses styles." },
      ],
    },
    { kind: "tool", id: "d3", name: "read", args: { path: "editeur-wysiwyg.html" }, status: "ok", output: "<!doctype html>…", durationMs: 4 },
    { kind: "tool", id: "d4", name: "edit", args: { path: "editeur-wysiwyg.html" }, status: "ok", output: "1 remplacement", durationMs: 12 },
    { kind: "tool", id: "d5", name: "bash", args: { command: "npx prettier --check editeur-wysiwyg.html" }, status: "running", output: "" },
    {
      kind: "assistant",
      id: "d6",
      streaming: false,
      blocks: [
        {
          type: "text",
          text:
            "C'est fait. J'ai ajouté :\n\n- un **bouton** dans la barre d'outils ;\n- des variables CSS `--bg` et `--text` ;\n- la mémorisation dans `localStorage`.\n\n<oral>Le mode sombre est en place, avec un bouton dans la barre d'outils, et ton choix est gardé d'une fois sur l'autre.</oral>",
        },
      ],
    },
    { kind: "user", id: "d7", text: "Parfait, et les raccourcis clavier ?" },
  ];
  session.model = "deepseek-v4-pro";
}

export const DEMO_FILES = ["editeur-wysiwyg.html", "README.md", "src/app.css", "src/main.ts", "src/lib/theme.ts", "package.json"];

export const DEMO_GIT: GitStatus = {
  isRepo: true,
  branch: "main",
  files: [
    { path: "editeur-wysiwyg.html", status: "modified" },
    { path: "src/lib/theme.ts", status: "untracked" },
    { path: "README.md", status: "modified" },
  ],
};

const now = Date.now();
export const DEMO_SESSIONS: SessionInfo[] = [
  { path: "/demo/1.jsonl", id: "1", title: "Mode sombre de l'éditeur", updated: now - 60_000, messages: 12, named: true },
  { path: "/demo/5.jsonl", id: "5", title: "dis bonjour à toute l'équipe.", updated: now - 20 * 60_000, messages: 4, named: false },
  { path: "/demo/6.jsonl", id: "6", title: "Jarvis, comment ça va ?", updated: now - 40 * 60_000, messages: 2, named: false },
  { path: "/demo/2.jsonl", id: "2", title: "Critique du projet", updated: now - 3 * 3600_000, messages: 20, named: true },
  { path: "/demo/3.jsonl", id: "3", title: "Micro-cravate pour le gaming", updated: now - 26 * 3600_000, messages: 44, named: true },
  { path: "/demo/4.jsonl", id: "4", title: "dis-moi, mise en place du TTS", updated: now - 4 * 86_400_000, messages: 31, named: false },
];

/** Forme d'onde simulée (millièmes) : quelques harmoniques modulées, comme une voix. */
export function demoWave(t: number): number[] {
  const env = 0.55 + 0.45 * Math.sin(t / 380);
  return Array.from({ length: 512 }, (_, i) => {
    const x = i / 512;
    const v =
      Math.sin(2 * Math.PI * (x * 9 + t / 90)) * 0.5 +
      Math.sin(2 * Math.PI * (x * 23 + t / 47)) * 0.28 +
      Math.sin(2 * Math.PI * (x * 61 + t / 29)) * 0.14 +
      (Math.random() - 0.5) * 0.1;
    return Math.round(v * env * 280 * (0.6 + 0.4 * Math.sin(Math.PI * x)));
  });
}

/** Niveaux d'égaliseur qui imitent une voix (pour la capture « l'agent parle »). */
export function demoLevels(t: number): number[] {
  return Array.from({ length: 40 }, (_, i) => {
    const formant = Math.exp(-((i - 8) ** 2) / 30) * 0.9 + Math.exp(-((i - 20) ** 2) / 40) * 0.6;
    const wobble = 0.55 + 0.45 * Math.sin(t / 140 + i * 0.7) * Math.sin(t / 310 + i * 0.3);
    return Math.max(0, Math.min(1, formant * wobble + 0.05));
  });
}
