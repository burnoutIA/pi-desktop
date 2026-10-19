import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type VoiceConfig = {
  sttUrl: string;
  ttsUrl: string;
  voice: string;
  ttsEnabled: boolean;
  handsFree: boolean;
  wakeUrl: string;
  wakeWord: string;
  filterDb: number;
  ackOnPause: boolean;
  /** Agent vocal : répond tout de suite et transmet à pi ce qui est à faire, même quand pi travaille. */
  voiceAgent: boolean;
  /** Serveur compatible OpenAI de l'agent vocal (harn par défaut). */
  agentUrl: string;
};

export type GitFile = { path: string; status: GitStatusKind };
export type GitStatusKind = "modified" | "added" | "deleted" | "renamed" | "untracked" | "conflict";
export type GitStatus = { isRepo: boolean; branch: string | null; files: GitFile[] };

/** Réglages de l'agent (stockés côté interface). */
export type AgentConfig = {
  name: string;
  /** Mains libres : écart de volume toléré par rapport à ta voix au moment du nom (dB). */
  filterDb: number;
};

export const DEFAULT_AGENT: AgentConfig = { name: "", filterDb: 20 };

/** Serveurs vocaux : déjà lancés à la main, en démarrage par l'app, prêts, ou en échec. */
export type ServersState = "external" | "starting" | "ready" | "failed";

/** Pièce jointe : image (envoyée au modèle) ou fichier (transmis par son chemin). */
export type Attachment = { name: string; path: string; size: number; mime: string | null; data: string | null };

export type SessionInfo = { path: string; id: string; title: string; updated: number; messages: number; named: boolean };

export type PromptFile = { path: string; content: string; exists: boolean };

export type AudioLevels = {
  out: number[];
  in: number[];
  outRms: number;
  inRms: number;
  /** Forme d'onde récente en millièmes (vide dans le silence). */
  outWave?: number[];
  inWave?: number[];
};

export const api = {
  openProject: (cwd: string, agentName: string | null, resume = false) =>
    invoke<void>("open_project", { cwd, agentName, resume }),
  readPrompt: (scope: "global" | "project", root: string | null) => invoke<PromptFile>("read_prompt", { scope, root }),
  writePrompt: (scope: "global" | "project", root: string | null, content: string) =>
    invoke<void>("write_prompt", { scope, root, content }),
  piSend: (cmd: Record<string, unknown>) => invoke<any>("pi_send", { cmd }),
  piWrite: (record: Record<string, unknown>) => invoke<void>("pi_write", { record }),
  listen: (on: boolean, submit = false) => invoke<void>("voice_listen", { on, submit }),
  stopSpeaking: () => invoke<void>("voice_stop_speaking"),
  speak: (text: string) => invoke<void>("voice_speak", { text }),
  voiceAgentAsk: (text: string, piState: string, piModel: string | null) =>
    invoke<void>("voice_agent_ask", { text, piState, piModel }),
  voiceAgentReset: () => invoke<void>("voice_agent_reset"),
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
  benchMode: () => invoke<boolean>("bench_mode"),
  log: (msg: string) => invoke<void>("log_frontend", { msg }).catch(() => {}),
};

export function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload));
}

/** Voix françaises : Pocket TTS (par défaut), puis celles du TTS Kyutai 1,6B (PIVOICE_TTS=kyutai). */
export const FRENCH_VOICES: { id: string; label: string }[] = [
  { id: "estelle", label: "Estelle (Pocket TTS, processeur)" },
  { id: "unmute-prod-website/developpeuse-3.wav", label: "Développeuse" },
  { id: "unmute-prod-website/fabieng-enhanced-v2.wav", label: "Fabien" },
  { id: "cml-tts/fr/2154_2576_000020-0003_enhanced.wav", label: "CML · voix 1" },
  { id: "cml-tts/fr/1406_1028_000009-0003_enhanced.wav", label: "CML · voix 2" },
  { id: "cml-tts/fr/12977_10625_000037-0001_enhanced.wav", label: "CML · voix 3" },
  { id: "cml-tts/fr/2216_1745_000007-0001_enhanced.wav", label: "CML · voix 4" },
  { id: "cml-tts/fr/296_1028_000022-0001_enhanced.wav", label: "CML · voix 5" },
];
