<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    api,
    on,
    DEFAULT_AGENT,
    type AgentConfig,
    type Attachment,
    type AudioLevels,
    type GitStatus,
    type GitStatusKind,
    type ServersState,
    type SessionInfo,
    type VoiceConfig,
  } from "./lib/api";
  import { Session, splitOral } from "./lib/session.svelte";
  import { displayName } from "./lib/titles";
  import { APP_COMMANDS, fromPi, type Command } from "./lib/commands";
  import { isDemo, fillDemo, demoLevels, demoWave, DEMO_FILES, DEMO_GIT, DEMO_SESSIONS } from "./lib/demo";
  import { loadViz, type VizStyle } from "./lib/viz";
  import VoiceStage from "./components/VoiceStage.svelte";
  import Conversation from "./components/Conversation.svelte";
  import Composer from "./components/Composer.svelte";
  import Explorer from "./components/Explorer.svelte";
  import SessionsPanel from "./components/SessionsPanel.svelte";
  import Settings from "./components/Settings.svelte";
  import StatusBar from "./components/StatusBar.svelte";
  import ExtensionDialog from "./components/ExtensionDialog.svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import Ambient from "./components/Ambient.svelte";

  const AUTO_SEND_MS = 600;
  const STOP_WORDS = /^\s*(stop|stoppe|arr[êe]te|arr[êe]tez|annule)\b[\s.!]*$/i;

  const session = new Session();

  let root = $state("");
  let piUp = $state(false);
  let files = $state<string[]>([]);
  let git = $state<GitStatus>({ isRepo: false, branch: null, files: [] });
  let selected = $state<string | null>(null);
  let pinned = $state(false); // l'utilisateur a choisi un fichier : on ne suit plus pi automatiquement
  let refreshKey = $state(0);

  let levels = $state<AudioLevels>({ out: [], in: [], outRms: 0, inRms: 0 });
  let speaking = $state(false);
  let listening = $state(false);
  let sttUp = $state(false);
  let voiceCfg = $state<VoiceConfig | null>(null);
  let showSettings = $state(false);
  let vizStyle = $state<VizStyle>(loadViz());
  let servers = $state<ServersState>("starting");
  const voiceBooting = $derived(servers === "starting" && !sttUp);

  // ---------- Disposition : panneaux repliables et redimensionnables ----------
  type Layout = { left: boolean; right: boolean; leftW: number; rightW: number };
  let layout = $state<Layout>(loadLayout());
  $effect(() => localStorage.setItem("layout", JSON.stringify(layout)));

  function loadLayout(): Layout {
    const d: Layout = { left: true, right: true, leftW: 250, rightW: 460 };
    try {
      return { ...d, ...JSON.parse(localStorage.getItem("layout") ?? "{}") };
    } catch {
      return d;
    }
  }

  function startResize(side: "left" | "right", e: PointerEvent) {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = side === "left" ? layout.leftW : layout.rightW;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - x0;
      if (side === "left") layout.leftW = Math.min(420, Math.max(180, w0 + dx));
      else layout.rightW = Math.min(Math.round(window.innerWidth * 0.6), Math.max(300, w0 - dx));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      document.body.classList.remove("resizing");
    };
    document.body.classList.add("resizing");
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  // ---------- Sessions de pi dans ce dossier ----------
  let sessions = $state<SessionInfo[]>([]);
  let sessionPath = $state<string | null>(null);

  async function refreshSessions() {
    try {
      const st = await api.piSend({ type: "get_state" });
      sessionPath = st?.data?.sessionFile ?? null;
      sessions = sessionPath ? await api.listSessions(sessionPath) : [];
    } catch {
      /* pi pas encore prêt */
    }
  }

  // ---------- Assistant de démarrage : pi installé ? fournisseur connecté ? ----------
  let onboarding = $state<"install" | "login" | null>(null);
  let piVersion = $state<string | null>(null);

  function startProject() {
    const last = localStorage.getItem("lastProject");
    api.initialDir().then((arg) => {
      const dir = arg ?? (last && !last.startsWith("/tmp/") ? last : null);
      if (dir) openProject(dir);
    });
  }

  async function checkPi(): Promise<boolean> {
    const st = await api.piStatus();
    piVersion = st.version;
    if (!st.installed) onboarding = "install";
    return st.installed;
  }

  async function onPiInstalled() {
    if (!(await checkPi())) return;
    onboarding = null;
    startProject();
  }

  async function onProviderConnected() {
    onboarding = null;
    toast("Fournisseur connecté : pi redémarre.");
    if (root) await openProject(root, true);
  }

  // ---------- Dialogues des extensions de pi (/mcp, /llama…) ----------
  let uiRequests = $state<any[]>([]);

  function onExtensionUi(ev: any) {
    switch (ev.method) {
      case "notify":
        toast(ev.message ?? "");
        break;
      case "set_editor_text":
        draft = ev.text ?? "";
        break;
      case "select":
      case "confirm":
      case "input":
      case "editor":
        uiRequests = [...uiRequests, ev];
        break;
      // setStatus, setWidget, setTitle : affichages propres au terminal, ignorés ici.
    }
  }

  function respondUi(req: any, r: Record<string, unknown>) {
    uiRequests = uiRequests.filter((x) => x.id !== req.id);
    api.piWrite({ type: "extension_ui_response", id: req.id, ...r }).catch((e) => toast(String(e)));
  }

  // ---------- Mode zen : la voix seule, sur un fond d'ambiance ----------
  let zen = $state(localStorage.getItem("zen") === "1");
  $effect(() => localStorage.setItem("zen", zen ? "1" : "0"));
  const zenActive = $derived(zen && !onboarding);
  /** Dernier résumé vocal de l'agent (sous-titre du mode zen). */
  const lastOral = $derived.by(() => {
    for (let i = session.items.length - 1; i >= 0; i--) {
      const it = session.items[i];
      if (it.kind === "assistant") {
        const t = it.blocks.filter((b) => b?.type === "text").map((b) => b.text).join("\n");
        return splitOral(t).oral ?? "";
      }
    }
    return "";
  });

  // ---------- Banc d'essai de l'affichage (PIVOICE_BENCH=1) ----------
  async function runBench() {
    const stats = () => (window as any).__eqStats as { gaps: number[]; draws: number[] };
    const pct = (a: number[], p: number) => [...a].sort((x, y) => x - y)[Math.floor(a.length * p)] ?? 0;
    // Voix simulée, arrivant comme les vrais niveaux : ~45 messages/s.
    const zenBefore = localStorage.getItem("zen");
    speaking = true;
    const feed = setInterval(() => (levels = { out: demoLevels(performance.now()), in: [], outRms: 0.08, inRms: 0 }), 22);
    for (const [label, z] of [["normal", false], ["zen", true]] as const) {
      zen = z;
      await new Promise((r) => setTimeout(r, 1500)); // laisser la mise en page se stabiliser
      const s = stats();
      s.gaps.length = 0;
      s.draws.length = 0;
      await new Promise((r) => setTimeout(r, 5000));
      const g = s.gaps, d = s.draws;
      const fps = g.length / (g.reduce((a, b) => a + b, 0) / 1000);
      const janky = g.filter((x) => x > 25).length;
      api.log(
        `bench ${label}: ${fps.toFixed(1)} img/s · intervalle médian ${pct(g, 0.5).toFixed(1)} ms, p95 ${pct(g, 0.95).toFixed(1)} ms, max ${Math.max(...g).toFixed(0)} ms · ${janky}/${g.length} images > 25 ms · dessin médian ${pct(d, 0.5).toFixed(2)} ms, p95 ${pct(d, 0.95).toFixed(2)} ms`,
      );
    }
    // Scénario « pi écrit pendant qu'il parle » : une réponse markdown avec du code qui arrive par morceaux.
    zen = false;
    session.items = [{ kind: "assistant", id: "bench", blocks: [{ type: "text", text: "" }], streaming: true }];
    // Réponse déjà longue (~18 000 caractères), comme une vraie réponse détaillée.
    const seed = "Voici la fonction **corrigée** avec `fetchUser` et une liste :\n\n- point un\n- point deux\n\n```ts\nexport async function fetchUser(id: string) {\n  const r = await fetch(`/api/users/${id}`);\n  return r.json();\n}\n```\n\n";
    if (session.items[0].kind === "assistant") session.items[0].blocks[0].text = seed.repeat(100);
    const chunk = "Voici la fonction **corrigée** avec `fetchUser` et une liste :\n\n- point un\n- point deux\n\n```ts\nexport async function fetchUser(id: string) {\n  const r = await fetch(`/api/users/${id}`);\n  return r.json();\n}\n```\n\n";
    let k = 0;
    const stream = setInterval(() => {
      const it = session.items[0];
      if (it?.kind === "assistant") it.blocks[0].text += chunk.slice(k % chunk.length, (k % chunk.length) + 6), (k += 6);
    }, 30);
    await new Promise((r) => setTimeout(r, 1500));
    {
      const s = stats();
      s.gaps.length = 0;
      s.draws.length = 0;
      await new Promise((r) => setTimeout(r, 5000));
      const g = s.gaps;
      const fps = g.length / (g.reduce((a, b) => a + b, 0) / 1000);
      const len = session.items[0]?.kind === "assistant" ? session.items[0].blocks[0].text.length : 0;
      api.log(
        `bench streaming (${(window as any).__eqStats.live} égaliseur(s) actifs): ${fps.toFixed(1)} img/s · p95 ${pct(g, 0.95).toFixed(1)} ms, max ${Math.max(...g).toFixed(0)} ms · ${g.filter((x) => x > 25).length}/${g.length} images > 25 ms · message de ${len} caractères`,
      );
    }
    clearInterval(stream);
    clearInterval(feed);
    zen = zenBefore === "1";
    api.log("bench terminé");
  }

  // ---------- Pièces jointes ----------
  let attachments = $state<Attachment[]>([]);
  let dragOver = $state(false);

  async function addAttachments(paths: string[]) {
    for (const p of paths) {
      try {
        const a = await api.readAttachment(p);
        if (!attachments.some((x) => x.path && x.path === a.path)) attachments = [...attachments, a];
      } catch (e) {
        toast(`${p.split("/").pop()} : ${e}`);
      }
    }
  }

  async function pickAttachments() {
    const picked = await openDialog({ multiple: true, title: "Joindre des fichiers", defaultPath: root || undefined });
    if (picked) addAttachments(Array.isArray(picked) ? picked : [picked]);
  }

  /**
   * Message + pièces jointes au format de pi : les images vont au modèle (s'il les lit), les autres
   * fichiers sont cités par leur chemin et pi les lit lui-même avec son outil `read`.
   */
  function withAttachments(text: string): { message: string; images?: { type: "image"; data: string; mimeType: string }[] } {
    if (!attachments.length) return { message: text };
    const canSee = (modelInputs[modelKey ?? ""] ?? []).includes("image");
    const images = canSee ? attachments.filter((a) => a.data && a.mime) : [];
    const byPath = attachments.filter((a) => !images.includes(a));
    const lost = byPath.filter((a) => !a.path); // image collée, sans fichier, pour un modèle qui ne voit pas
    if (!canSee && attachments.some((a) => a.data)) toast("Ce modèle ne lit pas les images : elles sont transmises par leur chemin.");
    if (lost.length) toast("Image collée ignorée : ce modèle ne lit pas les images.");
    const files = byPath.filter((a) => a.path).map((a) => `- ${a.path}`);
    const message =
      (text || "Regarde les pièces jointes.") + (files.length ? `\n\nPièces jointes (lis-les avec read) :\n${files.join("\n")}` : "");
    attachments = [];
    return images.length
      ? { message, images: images.map((a) => ({ type: "image" as const, data: a.data!, mimeType: a.mime! })) }
      : { message };
  }

  // ---------- Commandes « / » ----------
  let piCommands = $state<Command[]>([]);
  let thinkingLevels = $state<string[]>([]);
  const commands = $derived([...APP_COMMANDS, ...piCommands]);

  async function loadCommands() {
    try {
      const [c, t] = await Promise.all([
        api.piSend({ type: "get_commands" }),
        api.piSend({ type: "get_available_thinking_levels" }),
      ]);
      piCommands = fromPi(c?.data?.commands ?? []);
      thinkingLevels = t?.data?.levels ?? [];
    } catch {
      /* pi pas prêt */
    }
  }

  /** Exécute une commande intégrée. Renvoie false si ce n'en est pas une (on l'envoie alors à pi). */
  async function runAppCommand(text: string): Promise<boolean> {
    const m = /^\/(\S+)\s*([\s\S]*)$/.exec(text);
    if (!m) return false;
    const [, name, rest] = m;
    const arg = rest.trim();
    switch (name) {
      case "new":
        await newSession();
        return true;
      case "stop":
        await abort();
        return true;
      case "resume":
        layout.left = true;
        return true;
      case "compact": {
        session.status = "Compaction du contexte…";
        const r = await api.piSend({ type: "compact", ...(arg ? { customInstructions: arg } : {}) });
        session.status = null;
        if (r?.success === false) toast(r.error ?? "Compaction impossible");
        else {
          const d = r?.data ?? {};
          session.notice(`Contexte compacté : ${Math.round((d.tokensBefore ?? 0) / 1000)}k → ${Math.round((d.estimatedTokensAfter ?? 0) / 1000)}k tokens.`);
        }
        return true;
      }
      case "model": {
        if (!arg) {
          showSettings = true;
          return true;
        }
        const q = arg.toLowerCase();
        const hit = models.find((x) => x.key.toLowerCase() === q) ?? models.find((x) => x.key.toLowerCase().includes(q) || x.label.toLowerCase().includes(q));
        if (hit) {
          await setModel(hit.key);
          session.notice(`Modèle : ${hit.label}`);
        } else toast(`Modèle introuvable : ${arg}`);
        return true;
      }
      case "thinking": {
        if (!arg) {
          toast(`Niveaux disponibles : ${thinkingLevels.join(", ") || "aucun"}`);
          return true;
        }
        const r = await api.piSend({ type: "set_thinking_level", level: arg });
        if (r?.success === false) toast(r.error ?? "Niveau refusé");
        else session.notice(`Réflexion : ${arg}`);
        return true;
      }
      case "name": {
        if (!arg) {
          toast("Donne un nom : /name mon sujet");
          return true;
        }
        await api.piSend({ type: "set_session_name", name: arg });
        if (sessionPath) titled.add(sessionPath);
        await refreshSessions();
        session.notice(`Session renommée : ${arg}`);
        return true;
      }
      case "session": {
        const r = await api.piSend({ type: "get_session_stats" });
        const d = r?.data;
        if (!d) return true;
        const ctx = d.contextUsage;
        const pct = ctx?.contextWindow ? Math.round((100 * ctx.tokens) / ctx.contextWindow) : null;
        session.notice(
          `${d.userMessages} demandes · ${d.toolCalls} outils · ${Math.round((d.tokens?.total ?? 0) / 1000)}k tokens · ${(d.cost ?? 0).toFixed(3)} $` +
            (pct !== null ? ` · contexte ${pct} %` : ""),
        );
        return true;
      }
      case "copy": {
        const r = await api.piSend({ type: "get_last_assistant_text" });
        const t = r?.data?.text;
        if (!t) return void toast("Aucune réponse à copier."), true;
        try {
          await navigator.clipboard.writeText(t);
          toast("Dernière réponse copiée.");
        } catch {
          toast("Copie impossible (presse-papiers indisponible).");
        }
        return true;
      }
      case "export": {
        const r = await api.piSend({ type: "export_html", ...(arg ? { outputPath: arg } : {}) });
        if (r?.success === false) toast(r.error ?? "Export impossible");
        else session.notice(`Session exportée : ${r?.data?.path}`);
        return true;
      }
    }
    return false;
  }

  // Titre court généré par le modèle après le premier échange, enregistré dans la session pi.
  const titled = new Set<string>();
  async function maybeTitle() {
    const path = sessionPath;
    if (!path || !root || titled.has(path)) return;
    if (sessions.find((s) => s.path === path)?.named) return void titled.add(path);
    const user = session.items.find((i) => i.kind === "user");
    const reply = session.items.find(
      (i) => i.kind === "assistant" && i.blocks.some((b) => b?.type === "text" && b.text.trim()),
    );
    if (!user || user.kind !== "user" || !reply || reply.kind !== "assistant") return;
    titled.add(path);
    const text = reply.blocks.filter((b) => b?.type === "text").map((b) => b.text).join("\n");
    const { body, oral } = splitOral(text);
    try {
      const title = await api.generateTitle(root, modelKey, user.text, body || oral || "");
      if (sessionPath !== path) return; // on a changé de session entre-temps
      await api.piSend({ type: "set_session_name", name: title });
      await refreshSessions();
    } catch (e) {
      api.log(`titre de session : ${e}`);
    }
  }

  async function newSession() {
    if (session.busy) return;
    await api.stopSpeaking();
    const r = await api.piSend({ type: "new_session" });
    if (r?.data?.cancelled) return toast("Nouvelle session annulée par une extension.");
    session.reset();
    await refreshSessions();
  }

  async function openSession(s: SessionInfo) {
    if (session.busy) return;
    await api.stopSpeaking();
    const r = await api.piSend({ type: "switch_session", sessionPath: s.path });
    if (r?.success === false) return toast(r.error ?? "Impossible d'ouvrir cette session");
    if (r?.data?.cancelled) return toast("Changement de session annulé par une extension.");
    const m = await api.piSend({ type: "get_messages" });
    session.loadHistory(m?.data?.messages ?? []);
    await refreshSessions();
  }

  async function trashSession(s: SessionInfo) {
    try {
      await api.trashSession(s.path);
    } catch (e) {
      toast(String(e));
    }
    await refreshSessions();
  }
  let demoRaf = 0;
  let woke = $state(0); // instant du dernier réveil par le nom (animation)

  let agent = $state<AgentConfig>(loadAgent());
  const agentName = $derived(displayName(agent.name));

  function loadAgent(): AgentConfig {
    try {
      return { ...DEFAULT_AGENT, ...JSON.parse(localStorage.getItem("agentConfig") ?? "{}") };
    } catch {
      return { ...DEFAULT_AGENT };
    }
  }

  function updateAgent(a: AgentConfig) {
    const renamed = a.name.trim() !== agent.name.trim();
    agent = a;
    localStorage.setItem("agentConfig", JSON.stringify(a));
    if (voiceCfg) updateVoice(voiceCfg); // nom de réveil et filtre suivent l'agent
    if (renamed && root) restartPi("Nom changé : pi redémarre pour le prendre en compte.");
  }

  /** Relance pi (nouveau nom, prompt système modifié…) en gardant le projet. */
  function restartPi(why: string) {
    if (!root) return;
    if (session.busy) {
      toast("pi travaille : la modification s'appliquera au prochain redémarrage.");
      return;
    }
    openProject(root, true).then(() => session.notice(why));
  }

  // Modèles disponibles dans la configuration de pi ; le dernier choisi est mémorisé.
  let models = $state<{ key: string; label: string }[]>([]);
  /** Types d'entrée acceptés par chaque modèle (« text », « image »). */
  let modelInputs = $state<Record<string, string[]>>({});
  let modelKey = $state<string | null>(null);
  const argOptions = $derived({
    model: models.map((m) => ({ value: m.key, label: m.label })),
    thinking: thinkingLevels.map((l) => ({ value: l, label: l })),
  });

  async function loadModels() {
    const st = await api.piSend({ type: "get_state" });
    const cur = st?.data?.model;
    const av = await api.piSend({ type: "get_available_models" });
    models = (av?.data?.models ?? []).map((m: any) => ({ key: `${m.provider}/${m.id}`, label: m.name ?? m.id }));
    modelInputs = Object.fromEntries((av?.data?.models ?? []).map((m: any) => [`${m.provider}/${m.id}`, m.input ?? ["text"]]));
    const keys = models.map((m) => m.key);
    const saved = localStorage.getItem("model");
    const wanted = saved && keys.includes(saved) ? saved : null;
    const curKey = cur ? `${cur.provider}/${cur.id}` : null;
    if (wanted && wanted !== curKey) await setModel(wanted, false);
    else {
      modelKey = curKey;
      session.model = cur?.name ?? cur?.id ?? null;
    }
    // Aucun modèle utilisable : pi n'a pas encore de fournisseur connecté.
    if (models.length === 0 && onboarding === null) onboarding = "login";
  }

  async function setModel(key: string, remember = true) {
    const i = key.indexOf("/");
    const r = await api.piSend({ type: "set_model", provider: key.slice(0, i), modelId: key.slice(i + 1) });
    if (r?.success === false) return toast(r.error ?? "Modèle refusé");
    modelKey = key;
    session.model = r?.data?.name ?? key.slice(i + 1);
    if (remember) localStorage.setItem("model", key);
  }

  let draft = $state("");
  const zenSubtitle = $derived(listening ? draft : speaking ? lastOral : "");
  const zenEnergy = $derived(listening ? levels.inRms * 10 : speaking ? levels.outRms * 8 : 0);
  let countdown = $state<number | null>(null);
  let countdownRaf = 0;

  let toasts = $state<{ id: number; text: string }[]>([]);
  let toastSeq = 0;

  const statusMap = $derived(Object.fromEntries(git.files.map((f) => [f.path, f.status])) as Record<string, GitStatusKind>);
  const touchedRel = $derived(new Set(session.touched.map(relPath)));
  const runningTool = $derived.by(() => {
    for (let i = session.items.length - 1; i >= 0; i--) {
      const it = session.items[i];
      if (it.kind === "tool" && it.status === "running") return it.name;
    }
    return null;
  });
  const projectName = $derived(root ? root.split("/").filter(Boolean).pop() : "");

  function relPath(p: string) {
    let r = p;
    if (root && r.startsWith(root + "/")) r = r.slice(root.length + 1);
    return r.replace(/^\.\//, "");
  }

  function toast(text: string) {
    const id = ++toastSeq;
    toasts.push({ id, text });
    setTimeout(() => (toasts = toasts.filter((t) => t.id !== id)), 6000);
  }

  // ---------- Projet ----------

  /** `resume` : relance pi sur le même projet en reprenant sa conversation (--continue). */
  async function openProject(dir: string, resume = false) {
    try {
      if (!resume) {
        session.reset();
        selected = null;
        pinned = false;
      }
      root = dir;
      await api.openProject(dir, agent.name.trim() || null, resume);
      piUp = true;
      // Les dossiers temporaires (tests) ne deviennent pas le « dernier projet ».
      if (!dir.startsWith("/tmp/")) localStorage.setItem("lastProject", dir);
      await refreshFiles();
      await loadModels();
      await refreshSessions();
      await loadCommands();
    } catch (e) {
      piUp = false;
      root = "";
      if (localStorage.getItem("lastProject") === dir) localStorage.removeItem("lastProject");
      toast(String(e));
    }
  }

  async function pickProject() {
    const dir = await openDialog({ directory: true, multiple: false, title: "Choisir le dossier du projet" });
    if (typeof dir === "string") openProject(dir);
  }

  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleRefresh() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(refreshFiles, 150);
  }

  async function refreshFiles() {
    if (!root) return;
    try {
      const [f, g] = await Promise.all([api.listFiles(root), api.gitStatus(root)]);
      files = f;
      git = g;
      refreshKey++;
    } catch (e) {
      toast(String(e));
    }
  }

  function selectFile(p: string) {
    selected = p;
    pinned = true;
  }

  session.onFileTouched = (p) => {
    if (!pinned) selected = relPath(p);
    scheduleRefresh();
  };

  // ---------- Voix ----------

  function cancelAuto() {
    if (countdown === null) return;
    cancelAnimationFrame(countdownRaf);
    countdown = null;
  }

  function startAuto() {
    cancelAuto();
    const t0 = performance.now();
    const step = (now: number) => {
      const k = (now - t0) / AUTO_SEND_MS;
      if (k >= 1) {
        countdown = null;
        send();
        return;
      }
      countdown = k;
      countdownRaf = requestAnimationFrame(step);
    };
    countdown = 0;
    countdownRaf = requestAnimationFrame(step);
  }

  async function toggleMic() {
    cancelAuto();
    if (listening) {
      await api.listen(false, true); // arrêt manuel : on envoie après les derniers mots
    } else {
      if (speaking) await api.stopSpeaking();
      await api.listen(true);
    }
  }

  async function stopVoice() {
    await api.stopSpeaking();
  }

  function onEscape() {
    if (countdown !== null) return cancelAuto();
    if (speaking) return void api.stopSpeaking();
    if (listening) return void api.listen(false, false);
    if (selected) return closeFile();
  }

  function closeFile() {
    selected = null;
    pinned = false; // de nouveau, l'explorateur suit les fichiers modifiés par pi
  }

  async function updateVoice(c: VoiceConfig) {
    c = { ...c, wakeWord: agent.name.trim() || "pi", filterDb: agent.filterDb };
    voiceCfg = c;
    localStorage.setItem("voiceConfig", JSON.stringify(c));
    await api.setVoiceConfig(c);
  }

  // ---------- Envoi à pi ----------

  async function send() {
    cancelAuto();
    const text = draft.trim();
    if (!text && !attachments.length) return;
    if (!piUp) {
      toast("pi n'est pas démarré : ouvre un projet d'abord.");
      return;
    }
    draft = "";
    if (speaking) await api.stopSpeaking();
    if (listening) await api.listen(false, false);
    try {
      // Commandes « / » : celles de l'app d'abord, sinon pi s'en charge (skills, modèles, extensions).
      if (text.startsWith("/")) {
        if (await runAppCommand(text)) return;
        const name = text.slice(1).split(/\s/)[0];
        if (!piCommands.some((c) => c.name === name)) {
          toast(`Commande inconnue : /${name}`);
          draft = text;
          return;
        }
      }
      if (session.busy) {
        if (STOP_WORDS.test(text) && !attachments.length) return abort();
        await api.piSend({ type: "prompt", ...withAttachments(text), streamingBehavior: "steer" });
      } else {
        if (STOP_WORDS.test(text) && !attachments.length) return;
        const r = await api.piSend({ type: "prompt", ...withAttachments(text) });
        if (r && r.success === false) toast(r.error ?? "pi a refusé la demande");
      }
    } catch (e) {
      toast(String(e));
      draft = text;
    }
  }

  async function abort() {
    try {
      await api.stopSpeaking();
      await api.piSend({ type: "clear_queue" });
      await api.piSend({ type: "abort" });
    } catch (e) {
      toast(String(e));
    }
  }

  // ---------- Événements ----------

  onMount(() => {
    const unlisten = [
      on<any>("pi-event", (ev) => {
        if (ev.type === "extension_ui_request") return onExtensionUi(ev);
        session.handle(ev);
        if (ev.type === "agent_settled") refreshSessions().then(maybeTitle);
        else if (ev.type === "session_info_changed") refreshSessions();
      }),
      on<{ code: string; stderr: string }>("pi-exit", ({ code, stderr }) => {
        piUp = false;
        session.busy = false;
        const why = stderr?.trim().split("\n").pop();
        session.notice(`pi s'est arrêté (${code})${why ? ` : ${why}` : ""}. Clique sur la pastille du modèle pour le relancer.`, "warn");
      }),
      on<AudioLevels>("audio-levels", (l) => (levels = l)),
      on<boolean>("voice-speaking", (s) => (speaking = s)),
      on<boolean>("voice-listening", (l) => (listening = l)),
      on<{ stt: boolean }>("voice-status", (s) => (sttUp = s.stt)),
      on<string>("voice-error", (e) => {
        // Pendant le démarrage des serveurs, « injoignable » est normal : pas de bulle d'erreur.
        if (servers === "starting" && /injoignable/i.test(e)) return;
        toast(e);
      }),
      on<ServersState>("voice-servers", (s) => {
        servers = s;
        if (s === "failed") toast("Les serveurs vocaux n'ont pas démarré : voir ~/.cache/pi-voice/voice-server.log");
      }),
      on("voice-wake", () => {
        cancelAuto();
        woke = performance.now();
      }),
      on<string>("stt-word", (w) => {
        cancelAuto();
        draft = draft && !/\s$/.test(draft) ? `${draft} ${w}` : draft + w;
      }),
      on<{ reason: string }>("stt-turn-end", ({ reason }) => {
        if (!draft.trim()) return;
        if (reason === "manual") send();
        else startAuto();
      }),
      on<string[]>("fs-changed", () => scheduleRefresh()),
    ];

    if (isDemo()) {
      // Navigateur, `?demo` : interface remplie pour juger le design (aucun backend).
      fillDemo(session);
      root = "/home/demo/projets/editeur";
      piUp = true;
      sttUp = true;
      files = DEMO_FILES;
      git = DEMO_GIT;
      sessions = DEMO_SESSIONS;
      sessionPath = DEMO_SESSIONS[0].path;
      session.touched = ["editeur-wysiwyg.html"];
      const dq = new URLSearchParams(location.search);
      if (dq.get("draft")) draft = dq.get("draft")!;
      if (dq.has("zen")) zen = true;
      if (dq.get("viz")) vizStyle = dq.get("viz") as VizStyle;
      if (dq.has("attach")) {
        // petit carré coloré en PNG (1x1 agrandi) pour la vignette
        const px = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
        attachments = [
          { name: "maquette.png", path: "/home/demo/maquette.png", size: 48213, mime: "image/png", data: px },
          { name: "cahier-des-charges.md", path: "/home/demo/cahier-des-charges.md", size: 5120, mime: null, data: null },
        ];
      }
      if (dq.get("onboarding")) onboarding = dq.get("onboarding") as "install" | "login";
      piCommands = [
        { name: "mcp", description: "Gérer les serveurs MCP : connexion, activation", source: "extension" },
        { name: "skill:review", description: "Relire le code modifié", source: "skill" },
        { name: "fix-tests", description: "Corriger les tests qui échouent", source: "prompt" },
      ];
      speaking = !dq.has("idle");
      if (dq.has("idle")) session.busy = false;
      voiceCfg = { sttUrl: "", ttsUrl: "", voice: "unmute-prod-website/developpeuse-3.wav", ttsEnabled: true, handsFree: true, wakeUrl: "", wakeWord: "jarvis", filterDb: 20, ackOnPause: true };
      agent = { name: "Jarvis", filterDb: 20 };
      const q = new URLSearchParams(location.search).get("settings");
      if (q) {
        localStorage.setItem("settingsTab", q);
        showSettings = true;
      }
      const tick = () => {
        levels = {
          out: speaking ? demoLevels(performance.now()) : [],
          in: [],
          outRms: 0.1,
          inRms: 0,
          outWave: speaking ? demoWave(performance.now()) : [],
        };
        demoRaf = requestAnimationFrame(tick);
      };
      tick();
      return () => cancelAnimationFrame(demoRaf);
    }

    (async () => {
      if (await api.benchMode()) return runBench();
      servers = await api.voiceServersState();
      sttUp = await api.sttConnected();
      const saved = localStorage.getItem("voiceConfig");
      const cfg = await api.getVoiceConfig();
      await updateVoice(saved ? { ...cfg, ...JSON.parse(saved) } : cfg);
      if (await checkPi()) startProject();
    })();

    // Glisser-déposer des fichiers sur la fenêtre : ils deviennent des pièces jointes.
    const unDrop = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") dragOver = true;
      else if (p.type === "leave") dragOver = false;
      else if (p.type === "drop") {
        dragOver = false;
        if (p.paths?.length) addAttachments(p.paths);
      }
    });
    unlisten.push(unDrop as unknown as (typeof unlisten)[number]);

    const onKey = (e: KeyboardEvent) => {
      const typing = (e.target as HTMLElement)?.closest("textarea, input, select");
      if (e.code === "Space" && !typing && !e.repeat) {
        e.preventDefault();
        toggleMic();
      } else if (e.key === "Escape" && !typing) {
        onEscape();
      } else if (e.key === "." && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        abort();
      } else if (e.key.toLowerCase() === "j" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        zen = !zen;
      } else if (e.key.toLowerCase() === "b" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        if (e.altKey) layout.right = !layout.right;
        else layout.left = !layout.left;
      } else if (e.key === "o" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        pickProject();
      }
    };
    window.addEventListener("keydown", onKey);
    const onViz = (e: Event) => (vizStyle = (e as CustomEvent<VizStyle>).detail);
    window.addEventListener("vizchange", onViz);

    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("vizchange", onViz);
      unlisten.forEach((p) => p.then((u) => u()));
    };
  });
</script>

<div class="app">
  <header>
    <button
      class="icon-btn"
      class:active={layout.left}
      title="Sessions (Ctrl+B)"
      aria-label="Afficher les sessions"
      onclick={() => (layout.left = !layout.left)}
    >
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
        ><rect x="3" y="4" width="18" height="16" rx="3" /><path d="M9 4v16" />{#if layout.left}<path
            d="M5 8h2M5 11h2"
            stroke-linecap="round"
          />{/if}</svg
      >
    </button>
    <div class="brand">
      <svg width="20" height="20" viewBox="0 0 24 24"
        ><defs
          ><linearGradient id="lg" x1="0" x2="1" y1="0" y2="1"
            ><stop offset="0" style="stop-color: var(--agent-1)" /><stop offset="1" style="stop-color: var(--agent-2)" /></linearGradient
          ></defs
        ><g fill="url(#lg)"
          ><rect x="3" y="10" width="3" height="5" rx="1.5" /><rect x="8" y="7" width="3" height="11" rx="1.5" /><rect
            x="13"
            y="4"
            width="3"
            height="17"
            rx="1.5"
          /><rect x="18" y="8" width="3" height="9" rx="1.5" /></g
        ></svg
      >
      <span>Pi Voice</span>
    </div>
    <button class="project" onclick={pickProject} title="Ouvrir un dossier (Ctrl+O)">
      {#if root}
        <span class="pname">{projectName}</span>
        {#if git.branch && !layout.right}<span class="branch">⎇ {git.branch}</span>{/if}
      {:else}
        <span class="pname muted">Ouvrir un projet…</span>
      {/if}
    </button>
    <div class="spacer"></div>
    <button class="icon-btn" title="Réglages voix" aria-label="Réglages" onclick={() => (showSettings = !showSettings)}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
        ><path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0" /><circle cx="16" cy="6" r="2" /><circle cx="10" cy="12" r="2" /><circle
          cx="18"
          cy="18"
          r="2"
        /></svg
      >
    </button>
    <button
      class="icon-btn"
      class:active={zen}
      title={zen ? "Quitter le mode zen (Ctrl+J)" : "Mode zen : la voix seule (Ctrl+J)"}
      aria-label="Mode zen"
      onclick={() => (zen = !zen)}
    >
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"
        ><path d="M3 12h2M7 8v8M11 5v14M15 8v8M19 11v2" /></svg
      >
    </button>
    <button
      class="icon-btn"
      class:active={layout.right}
      title="Explorateur (Ctrl+Alt+B)"
      aria-label="Afficher l'explorateur"
      onclick={() => (layout.right = !layout.right)}
    >
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
        ><rect x="3" y="4" width="18" height="16" rx="3" /><path d="M15 4v16" />{#if layout.right}<path
            d="M17 8h2M17 11h2"
            stroke-linecap="round"
          />{/if}</svg
      >
    </button>
    {#if showSettings && voiceCfg}
      <Settings
        config={voiceCfg}
        onChange={updateVoice}
        onClose={() => (showSettings = false)}
        {models}
        {modelKey}
        onModel={(k) => setModel(k)}
        {agent}
        onAgent={updateAgent}
        {root}
        onPromptSaved={() => restartPi("Prompt système enregistré : pi a redémarré.")}
        onProviders={() => ((showSettings = false), (onboarding = "login"))}
        {toast}
      />
    {/if}
  </header>

  <main
    style="grid-template-columns: {layout.left && !zenActive ? layout.leftW : 0}px minmax(380px, 1fr) {layout.right &&
    !zenActive
      ? layout.rightW
      : 0}px"
  >
    <aside class="left" class:closed={!layout.left || zenActive}>
      {#if layout.left && !zenActive}
        <SessionsPanel
          {sessions}
          currentPath={sessionPath}
          busy={session.busy}
          {agentName}
          onOpen={openSession}
          onNew={newSession}
          onTrash={trashSession}
        />
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="resizer right-edge" onpointerdown={(e) => startResize("left", e)}></div>
      {/if}
    </aside>

    <section class="center" class:zen={zenActive}>
      {#if zenActive}
        <Ambient energy={zenEnergy} mood={listening ? "user" : speaking ? "agent" : "calm"} />
      {/if}
      <VoiceStage
        zen={zenActive}
        subtitle={zenSubtitle}
        {vizStyle}
        agentWave={levels.outWave ?? []}
        userWave={levels.inWave ?? []}
        agentLevels={levels.out}
        userLevels={levels.in}
        inRms={levels.inRms}
        {speaking}
        {listening}
        busy={session.busy}
        {sttUp}
        {voiceBooting}
        {agentName}
        {woke}
        handsFree={voiceCfg?.handsFree ?? false}
        onMic={toggleMic}
        onStopVoice={stopVoice}
      />
      <div class="feed" class:hidden={zenActive}>
        {#if onboarding}
          <Onboarding
            step={onboarding}
            {piVersion}
            onInstalled={onPiInstalled}
            onConnected={onProviderConnected}
            onSkip={() => (onboarding = null)}
          />
        {:else}
          <Conversation
            {session}
            {root}
            onOpenProject={pickProject}
            onOpenFile={(p) => selectFile(relPath(p))}
            onReplay={(t) => api.speak(t)}
            {agentName}
            handsFree={voiceCfg?.handsFree ?? false}
          />
        {/if}
      </div>
      {#if !onboarding && !zenActive}
      <Composer
        bind:value={draft}
        {listening}
        busy={session.busy}
        {countdown}
        onSend={send}
        onCancelAuto={cancelAuto}
        {onEscape}
        onAbort={abort}
        {agentName}
        {commands}
        {argOptions}
        bind:attachments
        onAttach={pickAttachments}
      />
      {/if}
    </section>

    <aside class="right" class:closed={!layout.right || zenActive}>
      {#if layout.right && !zenActive}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="resizer left-edge" onpointerdown={(e) => startResize("right", e)}></div>
        <Explorer
          {root}
          {files}
          status={statusMap}
          touched={touchedRel}
          isRepo={git.isRepo}
          branch={git.branch}
          {selected}
          {refreshKey}
          onSelect={selectFile}
          onBack={closeFile}
          onInitGit={async () => {
            await api.gitInit(root);
            refreshFiles();
          }}
        />
      {/if}
    </aside>
  </main>

  <StatusBar
    {agentName}
    {piUp}
    hasProject={!!root}
    onOpenProject={pickProject}
    busy={session.busy}
    {speaking}
    {listening}
    {sttUp}
    {voiceBooting}
    handsFree={voiceCfg?.handsFree ?? false}
    {runningTool}
    project={projectName ?? ""}
    branch={git.branch}
    model={session.model}
    onRestart={() => root && openProject(root, true)}
    onSettings={() => (showSettings = true)}
  />

  {#if uiRequests.length}
    {#key uiRequests[0].id}
      <ExtensionDialog req={uiRequests[0]} onRespond={(r) => respondUi(uiRequests[0], r)} />
    {/key}
  {/if}

  {#if dragOver}
    <div class="drop-veil"><div class="drop-box">Dépose pour joindre à ta demande</div></div>
  {/if}

  <div class="toasts">
    {#each toasts as t (t.id)}
      <div class="toast">{t.text}</div>
    {/each}
  </div>
</div>

<style>
  .app {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  header {
    position: relative;
    height: 46px;
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px 0 16px;
    border-bottom: 1px solid var(--line);
    background: var(--chrome);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 750;
    letter-spacing: 0.01em;
    color: var(--text-strong);
  }
  .project {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 12px;
    border-radius: 8px;
    border: 1px solid var(--line);
    margin-left: 8px;
  }
  .project:hover {
    border-color: var(--line-2);
    background: var(--panel);
  }
  .pname {
    font-weight: 600;
    color: var(--text-strong);
  }
  .muted {
    color: var(--text-3);
  }
  .branch {
    font-size: 12px;
    color: var(--text-3);
    font-family: var(--mono);
  }
  .spacer {
    flex: 1;
  }
  main {
    flex: 1;
    min-height: 0;
    display: grid;
    transition: grid-template-columns 0.18s ease;
  }
  :global(body.resizing) main {
    transition: none;
  }
  :global(body.resizing) {
    cursor: col-resize;
  }
  aside {
    position: relative;
    overflow: hidden;
  }
  aside.closed {
    border: 0 !important;
  }
  .resizer {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 6px;
    z-index: 3;
    cursor: col-resize;
  }
  .resizer:hover {
    background: linear-gradient(90deg, transparent 2px, var(--agent-1) 2px, var(--agent-1) 4px, transparent 4px);
  }
  .right-edge {
    right: -3px;
  }
  .left-edge {
    left: -3px;
  }
  .icon-btn.active {
    color: var(--text);
  }
  .left {
    border-right: 1px solid var(--line);
    background: var(--bg-2);
    min-height: 0;
  }
  .center {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }
  .feed {
    flex: 1;
    min-height: 0;
  }
  .feed.hidden {
    display: none;
  }
  .center {
    position: relative;
  }
  .center.zen {
    overflow: hidden;
  }
  .right {
    border-left: 1px solid var(--line);
    background: var(--bg-2);
    min-height: 0;
    min-width: 0;
  }
  .drop-veil {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
    pointer-events: none;
  }
  .drop-box {
    padding: 28px 40px;
    border-radius: 16px;
    border: 2px dashed var(--agent-1);
    background: var(--panel);
    color: var(--text-strong);
    font-weight: 650;
    box-shadow: 0 20px 60px var(--shadow);
  }
  .toasts {
    position: fixed;
    bottom: 40px;
    right: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 50;
  }
  .toast {
    max-width: 380px;
    padding: 10px 14px;
    border-radius: 10px;
    background: var(--panel-2);
    border: 1px solid var(--line-2);
    box-shadow: 0 12px 30px var(--shadow);
    font-size: 13px;
    animation: rise 0.2s ease-out;
  }
  @keyframes rise {
    from {
      transform: translateY(8px);
      opacity: 0;
    }
  }
</style>
