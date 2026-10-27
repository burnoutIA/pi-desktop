import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Réglages de la dictée : pi ne parle pas, seule ta voix est transcrite. */
export type VoiceConfig = {
  sttUrl: string;
};

export type GitFile = { path: string; status: GitStatusKind };
export type GitStatusKind = "modified" | "added" | "deleted" | "renamed" | "untracked" | "conflict";
export type GitStatus = { isRepo: boolean; branch: string | null; files: GitFile[] };

/** Réglages de l'agent (stockés côté interface). */
export type AgentConfig = {
  name: string;
};

export const DEFAULT_AGENT: AgentConfig = { name: "" };

/** Serveurs vocaux : déjà lancés à la main, en démarrage par l'app, prêts, ou en échec. */
export type ServersState = "external" | "starting" | "ready" | "failed";

/** Pièce jointe : image (envoyée au modèle) ou fichier (transmis par son chemin). */
export type Attachment = { name: string; path: string; size: number; mime: string | null; data: string | null };

export type SessionInfo = { path: string; id: string; title: string; updated: number; messages: number; named: boolean };

/** Un dot (un dossier : `<nom>.dot`, skills/, extensions/, state/), tel que le décrit `dot list --json`. */
export type DotInfo = {
  name: string;
  running: boolean;
  vault: boolean;
  interface: string;
  dir: string;
  /** Son fichier `.dot` (réglages, providers, secrets). */
  file: string;
  socket: string;
  description: string;
  /** Il vit dans l'app (data/dots/), et non ailleurs sur le disque. */
  inApp: boolean;
};
export type DotsInfo = { cli: string | null; skill: boolean; home: string };

export type DotAction = "start" | "stop" | "remove" | "use-rpc" | "import";

export type PromptFile = { path: string; content: string; exists: boolean };

/** Niveaux du micro pour l'égaliseur. */
export type AudioLevels = {
  in: number[];
  inRms: number;
  /** Forme d'onde récente en millièmes (vide dans le silence). */
  inWave?: number[];
};

export const api = {
  /** Lance (ou relance) le pi d'un run. `session` : fichier de session à reprendre. */
  piStart: (run: string, cwd: string, agentName: string | null, opts: { resume?: boolean; session?: string | null; extraPrompt?: string | null } = {}) =>
    invoke<void>("pi_start", { run, cwd, agentName, resume: opts.resume ?? false, session: opts.session ?? null, extraPrompt: opts.extraPrompt ?? null }),
  /** Rejoint un dot (`dot @nom rpc`) à la place d'un pi local. */
  piStartDot: (run: string, dir: string, agentName: string | null) => invoke<void>("pi_start_dot", { run, dir, agentName }),
  dotsInfo: () => invoke<DotsInfo>("dots_info"),
  dotsSetCli: (path: string) => invoke<void>("dots_set_cli", { path }),
  dotsList: () => invoke<DotInfo[]>("dots_list"),
  /** Nouveau dot dans l'app : copie vierge du dot du dossier `from`, ou `.dot` à remplir. */
  dotsCreate: (name: string, from: string | null) => invoke<string>("dots_create", { name, from }),
  dotsAction: (action: DotAction, dir: string) => invoke<string>("dots_action", { action, dir }),
  piStop: (run: string) => invoke<void>("pi_stop", { run }),
  watchDir: (cwd: string) => invoke<void>("watch_dir", { cwd }),
  quitApp: () => invoke<void>("quit_app"),
  worktreeCreate: (root: string, label: string) => invoke<{ path: string; branch: string; base: string }>("worktree_create", { root, label }),
  worktreeInfo: (root: string, path: string, branch: string, base: string) =>
    invoke<{ ahead: number; dirty: boolean; exists: boolean }>("worktree_info", { root, path, branch, base }),
  worktreeMerge: (root: string, path: string, branch: string, message: string) =>
    invoke<{ merged: boolean; conflicts: string[] }>("worktree_merge", { root, path, branch, message }),
  worktreeRemove: (root: string, path: string, branch: string, deleteBranch: boolean) =>
    invoke<void>("worktree_remove", { root, path, branch, deleteBranch }),
  readPrompt: (scope: "global" | "project", root: string | null) => invoke<PromptFile>("read_prompt", { scope, root }),
  writePrompt: (scope: "global" | "project", root: string | null, content: string) =>
    invoke<void>("write_prompt", { scope, root, content }),
  piSend: (run: string, cmd: Record<string, unknown>) => invoke<any>("pi_send", { run, cmd }),
  piWrite: (run: string, record: Record<string, unknown>) => invoke<void>("pi_write", { run, record }),
  listen: (on: boolean, submit = false) => invoke<void>("voice_listen", { on, submit }),
  getVoiceConfig: () => invoke<VoiceConfig>("voice_get_config"),
  setVoiceConfig: (config: VoiceConfig) => invoke<void>("voice_set_config", { config }),
  sttConnected: () => invoke<boolean>("voice_stt_connected"),
  listFiles: (root: string) => invoke<string[]>("list_files", { root }),
  gitStatus: (root: string) => invoke<GitStatus>("git_status", { root }),
  gitDiff: (root: string, path: string) => invoke<string>("git_diff", { root, path }),
  readFile: (root: string, path: string) => invoke<string>("read_file", { root, path }),
  gitInit: (root: string) => invoke<void>("git_init", { root }),
  initialDir: () => invoke<string | null>("initial_dir"),
  listSessions: (current: string) => invoke<SessionInfo[]>("list_sessions", { current }),
  trashSession: (path: string) => invoke<void>("trash_session", { path }),
  generateTitle: (cwd: string, model: string | null, user: string, assistant: string) =>
    invoke<string>("generate_title", { cwd, model, user, assistant }),
  voiceServersState: () => invoke<ServersState>("voice_servers_state"),
  piStatus: () => invoke<{ installed: boolean; path: string | null; version: string | null }>("pi_status"),
  authMtime: () => invoke<number>("auth_mtime"),
  termSpawn: (program: string, args: string[], cwd: string | null, cols: number, rows: number) =>
    invoke<number>("term_spawn", { program, args, cwd, cols, rows }),
  termWrite: (id: number, data: string) => invoke<void>("term_write", { id, data }),
  termResize: (id: number, cols: number, rows: number) => invoke<void>("term_resize", { id, cols, rows }),
  termKill: (id: number) => invoke<void>("term_kill", { id }),
  readAttachment: (path: string) => invoke<Attachment>("read_attachment", { path }),
  previewUrl: (root: string, path: string) => invoke<string>("preview_url", { root, path }),
  browserShow: (url: string, bounds: Bounds) => invoke<void>("browser_show", { url, bounds }),
  browserBounds: (bounds: Bounds) => invoke<void>("browser_bounds", { bounds }),
  browserHide: () => invoke<void>("browser_hide"),
  browserClose: () => invoke<void>("browser_close"),
  browserReload: () => invoke<void>("browser_reload"),
  openExternal: (target: string) => invoke<void>("open_external", { target }),
  benchMode: () => invoke<boolean>("bench_mode"),
  log: (msg: string) => invoke<void>("log_frontend", { msg }).catch(() => {}),
};

/** Place d'un élément dans la fenêtre, en pixels CSS. */
export type Bounds = { x: number; y: number; width: number; height: number };

export function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload));
}

