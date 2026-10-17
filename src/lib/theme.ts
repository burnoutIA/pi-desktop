// Palettes et mode clair / sombre. Chaque palette définit l'accent de
// l'agent (sa voix, ses boutons) et l'accent de l'utilisateur (ta voix, tes bulles), sur des fonds gris neutres, dans les
// deux modes. Les jetons sont posés en variables CSS sur <html> ; app.css en dérive les
// transparences (--agent-soft, --user-line…) avec color-mix.

export type Mode = "dark" | "light";
export type ModePref = Mode | "system";

type Tokens = Record<string, string>;

/** Jetons communs à toutes les palettes, par mode. */
const BASE: Record<Mode, Tokens> = {
  dark: {
    "text-strong": "#ffffff",
    text: "#e9ecf4",
    "text-2": "#b3bacb",
    "text-3": "#7a8398",
    ok: "#34d399",
    warn: "#fbbf24",
    err: "#f87171",
    modified: "#e2c08d",
    "add-bg": "rgba(52, 211, 153, 0.12)",
    "add-fg": "#7ee2b8",
    "del-bg": "rgba(248, 113, 113, 0.12)",
    "del-fg": "#ffa3a3",
    shadow: "rgba(0, 0, 0, 0.5)",
    "on-accent": "#ffffff",
    "on-user": "#1a0f14",
    // Surfaces : le « chrome » (en-tête, barre d'état) est plus sombre que tout le reste.
    chrome: "color-mix(in srgb, var(--bg) 62%, #000)",
    "soft-pct": "15%",
    "soft-2-pct": "10%",
    // Coloration syntaxique (esprit VS Code Dark+)
    "syn-keyword": "#c586c0",
    "syn-control": "#569cd6",
    "syn-string": "#ce9178",
    "syn-number": "#b5cea8",
    "syn-comment": "#6a9955",
    "syn-func": "#dcdcaa",
    "syn-type": "#4ec9b0",
    "syn-tag": "#569cd6",
    "syn-attr": "#9cdcfe",
    "syn-var": "#9cdcfe",
    // Outils de l'agent
    "tool-bash": "#89d185",
    "tool-read": "#75beff",
    "tool-edit": "#e2c08d",
    "tool-search": "#c586c0",
    // Icônes de fichiers par type
    "ft-html": "#e8743b",
    "ft-css": "#a074c4",
    "ft-js": "#f1e05a",
    "ft-ts": "#4a9eda",
    "ft-json": "#cbcb41",
    "ft-md": "#519aba",
    "ft-rs": "#dea584",
    "ft-py": "#4b8bbe",
    "ft-svelte": "#ff5a2b",
    "ft-sh": "#89e051",
    "ft-cfg": "#8f9bb3",
  },
  light: {
    "text-strong": "#0b0d14",
    text: "#1f2430",
    "text-2": "#4b5366",
    "text-3": "#7a8294",
    ok: "#059669",
    warn: "#b45309",
    err: "#dc2626",
    modified: "#9a6700",
    "add-bg": "rgba(16, 185, 129, 0.13)",
    "add-fg": "#046c4e",
    "del-bg": "rgba(239, 68, 68, 0.11)",
    "del-fg": "#b91c1c",
    shadow: "rgba(30, 35, 60, 0.14)",
    "on-accent": "#ffffff",
    "on-user": "#ffffff",
    chrome: "color-mix(in srgb, var(--bg-2) 88%, var(--line-2))",
    // Sur fond clair, une teinte à 15 % paraît lourde (surtout avec un accent très sombre).
    "soft-pct": "9%",
    "soft-2-pct": "7%",
    // Coloration syntaxique (esprit VS Code Light+)
    "syn-keyword": "#af00db",
    "syn-control": "#0000ff",
    "syn-string": "#a31515",
    "syn-number": "#098658",
    "syn-comment": "#008000",
    "syn-func": "#795e26",
    "syn-type": "#267f99",
    "syn-tag": "#800000",
    "syn-attr": "#e50000",
    "syn-var": "#001080",
    "tool-bash": "#16825d",
    "tool-read": "#0066b8",
    "tool-edit": "#a46a00",
    "tool-search": "#8e3fbf",
    "ft-html": "#d0531f",
    "ft-css": "#7f4fad",
    "ft-js": "#a88f00",
    "ft-ts": "#2b6cb0",
    "ft-json": "#8a7e00",
    "ft-md": "#3a7ea1",
    "ft-rs": "#b5652b",
    "ft-py": "#346b9b",
    "ft-svelte": "#e1400d",
    "ft-sh": "#3f8f1f",
    "ft-cfg": "#6b7488",
  },
};

export type Palette = {
  id: string;
  label: string;
  description: string;
  dark: Tokens;
  light: Tokens;
};

// Fonds : toujours des gris (au plus une teinte à peine perceptible, jamais du bleu ou du vert).
// Les palettes se distinguent surtout par leurs accents.
/** Gris à peine chaud (chroma très faible) : on sent une chaleur, sans voir de couleur. */
const WARM_GRAY: Record<Mode, Tokens> = {
  dark: { bg: "#11100f", "bg-2": "#171615", panel: "#1c1b19", "panel-2": "#232120", line: "#2e2c29", "line-2": "#3b3835", "eq-idle": "#5c5853" },
  light: { bg: "#fffefd", "bg-2": "#eeedea", panel: "#f7f6f4", "panel-2": "#efeeeb", line: "#e0deda", "line-2": "#d0cdc8", "eq-idle": "#c2beb7" },
};

/** Gris à peine vert-de-gris : on sent la nuance, sans voir de vert. */
const SAGE_GRAY: Record<Mode, Tokens> = {
  dark: { bg: "#0f110f", "bg-2": "#151715", panel: "#1a1c1a", "panel-2": "#212321", line: "#2b2e2b", "line-2": "#383b38", "eq-idle": "#575b57" },
  light: { bg: "#fdfefd", "bg-2": "#eceeeb", panel: "#f5f7f4", "panel-2": "#edefec", line: "#dee1dc", "line-2": "#cdd1cb", "eq-idle": "#bcc1ba" },
};

const NEUTRAL: Record<Mode, Tokens> = {
  dark: { bg: "#0f0f11", "bg-2": "#151517", panel: "#19191c", "panel-2": "#202024", line: "#2a2a2f", "line-2": "#37373e", "eq-idle": "#55555e" },
  light: { bg: "#ffffff", "bg-2": "#ececee", panel: "#f6f6f7", "panel-2": "#eeeef0", line: "#dedee2", "line-2": "#cdcdd3", "eq-idle": "#bdbdc4" },
};

export const PALETTES: Palette[] = [
  {
    id: "graphite",
    label: "Graphite",
    description: "Sobre : argent, ta voix ambre.",
    dark: {
      ...NEUTRAL.dark,
      "agent-1": "#e4e4e7", "agent-2": "#a1a1aa", "user-1": "#fbbf24", "user-2": "#f59e0b",
      "on-accent": "#111113", "on-user": "#1f1503",
    },
    light: {
      ...NEUTRAL.light,
      "agent-1": "#27272a", "agent-2": "#71717a", "user-1": "#d97706", "user-2": "#b45309",
    },
  },
  {
    id: "braise",
    label: "Braise",
    description: "Orange et or, ta voix turquoise.",
    dark: {
      ...WARM_GRAY.dark,
      "agent-1": "#fb923c", "agent-2": "#facc15", "user-1": "#5eead4", "user-2": "#38bdf8",
      "on-accent": "#1c1206", "on-user": "#05201d",
    },
    light: {
      ...WARM_GRAY.light,
      "agent-1": "#ea580c", "agent-2": "#ca8a04", "user-1": "#0d9488", "user-2": "#0284c7",
    },
  },
  {
    id: "brique",
    label: "Brique",
    description: "Terre cuite et corail, ta voix sable.",
    dark: {
      ...WARM_GRAY.dark,
      "agent-1": "#e2673f", "agent-2": "#f59a7a", "user-1": "#e9cf9e", "user-2": "#d6b37a",
      "on-accent": "#ffffff", "on-user": "#231a0b",
    },
    light: {
      ...WARM_GRAY.light,
      "agent-1": "#c2410c", "agent-2": "#e0704f", "user-1": "#a87d3a", "user-2": "#8c6526",
    },
  },
  {
    id: "sauge",
    label: "Sauge",
    description: "Sauge et menthe, ta voix pêche.",
    dark: {
      ...SAGE_GRAY.dark,
      "agent-1": "#8fbf9f", "agent-2": "#b6e3c6", "user-1": "#fbbf95", "user-2": "#f59e72",
      "on-accent": "#0e1a12", "on-user": "#2a1406",
    },
    light: {
      ...SAGE_GRAY.light,
      "agent-1": "#3f7a54", "agent-2": "#5c9a72", "user-1": "#d9783f", "user-2": "#c25a26",
    },
  },
  {
    id: "prune",
    label: "Prune",
    description: "Prune et orchidée, ta voix abricot.",
    dark: {
      ...NEUTRAL.dark,
      "agent-1": "#c084fc", "agent-2": "#f0abfc", "user-1": "#fdba74", "user-2": "#fb923c",
      "on-accent": "#1d0b2e", "on-user": "#2a1404",
    },
    light: {
      ...NEUTRAL.light,
      "agent-1": "#8b3fd9", "agent-2": "#b84fc9", "user-1": "#e07a1f", "user-2": "#c2620a",
    },
  },
  {
    id: "miel",
    label: "Miel",
    description: "Ambre et miel, ta voix lavande.",
    dark: {
      ...WARM_GRAY.dark,
      "agent-1": "#f2b33d", "agent-2": "#f7d27c", "user-1": "#b9a7f5", "user-2": "#9d86ee",
      "on-accent": "#241703", "on-user": "#140c2e",
    },
    light: {
      ...WARM_GRAY.light,
      "agent-1": "#b7791f", "agent-2": "#d69e2e", "user-1": "#7357d6", "user-2": "#5b3fc4",
    },
  },
  {
    id: "rubis",
    label: "Rubis",
    description: "Rubis et framboise, ta voix glacier.",
    dark: {
      ...NEUTRAL.dark,
      "agent-1": "#f0506e", "agent-2": "#ff8fa3", "user-1": "#cfe3ea", "user-2": "#a9c7d1",
      "on-accent": "#ffffff", "on-user": "#0e1a1f",
    },
    light: {
      ...NEUTRAL.light,
      "agent-1": "#d1294b", "agent-2": "#e8566f", "user-1": "#557a87", "user-2": "#3f6170",
    },
  },
];

const PREF_KEY = "theme";

export type ThemePref = { palette: string; mode: ModePref };

export function loadThemePref(): ThemePref {
  try {
    const p = JSON.parse(localStorage.getItem(PREF_KEY) ?? "{}");
    return { palette: PALETTES.some((x) => x.id === p.palette) ? p.palette : "graphite", mode: p.mode ?? "dark" };
  } catch {
    return { palette: "graphite", mode: "dark" };
  }
}

export function resolveMode(pref: ModePref): Mode {
  if (pref !== "system") return pref;
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/** Applique palette et mode sur <html> (sans rechargement). */
export function applyTheme(pref: ThemePref) {
  const palette = PALETTES.find((p) => p.id === pref.palette) ?? PALETTES[0];
  const mode = resolveMode(pref.mode);
  const tokens = { ...BASE[mode], ...palette[mode] };
  const root = document.documentElement;
  for (const [k, v] of Object.entries(tokens)) root.style.setProperty(`--${k}`, v);
  root.style.colorScheme = mode;
  root.dataset.mode = mode;
  root.dataset.palette = palette.id;
  window.dispatchEvent(new CustomEvent("themechange"));
}

export function saveThemePref(pref: ThemePref) {
  localStorage.setItem(PREF_KEY, JSON.stringify(pref));
  applyTheme(pref);
}

/** Suit le réglage clair / sombre du système quand le mode « Système » est choisi. */
export function watchSystemMode() {
  window.matchMedia?.("(prefers-color-scheme: light)").addEventListener("change", () => {
    const pref = loadThemePref();
    if (pref.mode === "system") applyTheme(pref);
  });
}

/** Jetons d'une palette pour un mode, pour les aperçus. */
export function previewTokens(p: Palette, mode: Mode): Tokens {
  return { ...BASE[mode], ...p[mode] };
}
