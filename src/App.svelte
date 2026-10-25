<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    api,
    on,
    DEFAULT_AGENT,
    FRENCH_VOICES,
    type AgentConfig,
    type Attachment,
    type AudioLevels,
    type DotAction,
    type DotInfo,
    type GitStatus,
    type GitStatusKind,
    type ServersState,
    type SessionInfo,
    type VoiceConfig,
  } from "./lib/api";
  import { Session, splitOral } from "./lib/session.svelte";
  import { Run, branchPrompt, loadBranchRuns, saveBranchRuns } from "./lib/run.svelte";
  import { chime } from "./lib/chime";
  import { displayName } from "./lib/titles";
  import { APP_COMMANDS, fromPi, type Command } from "./lib/commands";
  import { isDemo, fillDemo, demoLevels, demoWave, DEMO_FILES, DEMO_GIT, DEMO_SESSIONS } from "./lib/demo";
  import { loadViz, type VizStyle } from "./lib/viz";
  import VoiceStage from "./components/VoiceStage.svelte";
  import Conversation from "./components/Conversation.svelte";
  import Composer from "./components/Composer.svelte";
  import Explorer from "./components/Explorer.svelte";
  import { isPreviewable, type Preview } from "./lib/preview";
  import SessionsPanel from "./components/SessionsPanel.svelte";
  import RunsPanel from "./components/RunsPanel.svelte";
  import DotsPanel from "./components/DotsPanel.svelte";
  import BranchBar from "./components/BranchBar.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import Settings from "./components/Settings.svelte";
  import StatusBar from "./components/StatusBar.svelte";
  import ExtensionDialog from "./components/ExtensionDialog.svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import Ambient from "./components/Ambient.svelte";

  const AUTO_SEND_MS = 600;
  const STOP_WORDS = /^\s*(stop|stoppe|arr[êe]te|arr[êe]tez|annule)\b[\s.!]*$/i;

  // ---------- Runs : un pi par projet (ou par branche de travail), un seul affiché ----------
  let runs = $state<Run[]>(loadBranchRuns());
  let visibleId = $state<string | null>(null);
  const run = $derived(runs.find((r) => r.id === visibleId) ?? null);
  /** Session affichée (vide tant qu'aucun run n'est ouvert). */
  const idleSession = new Session();
  const session = $derived(run?.session ?? idleSession);
  /** Dossier affiché : celui où travaille le pi du run visible (projet ou branche). */
  const root = $derived(run?.cwd ?? "");
  const piUp = $derived(run?.piUp ?? false);
  $effect(() => saveBranchRuns(runs));

  let files = $state<string[]>([]);
  let git = $state<GitStatus>({ isRepo: false, branch: null, files: [] });
  let selected = $state<string | null>(null);
  /** Panneau d'aperçu (page, appli lancée, image) : prioritaire sur le diff tant qu'il est ouvert. */
  let preview = $state<Preview | null>(null);
  let pinned = $state(false); // l'utilisateur a choisi un fichier : on ne suit plus pi automatiquement
  let refreshKey = $state(0);
  /** Change seulement quand un vrai fichier du projet change (pas git) : recharge l'aperçu local. */
  let previewKey = $state(0);

  let levels = $state<AudioLevels>({ out: [], in: [], outRms: 0, inRms: 0 });
  let speaking = $state(false);
  let listening = $state(false);
  let sttUp = $state(false);
  let voiceCfg = $state<VoiceConfig | null>(null);
  /** Le brouillon vient de la voix (et non du clavier) : il passera par l'agent vocal. */
  let voiceDraft = false;
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
    // Le pointeur reste à la poignée pendant tout le glissement, même au-dessus de la page de
    // l'aperçu (une iframe garderait sinon les mouvements pour elle).
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    const x0 = e.clientX;
    const w0 = side === "left" ? layout.leftW : layout.rightW;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - x0;
      if (side === "left") layout.leftW = Math.min(420, Math.max(180, w0 + dx));
      else layout.rightW = Math.min(Math.round(window.innerWidth * 0.6), Math.max(300, w0 - dx));
    };
    const up = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      handle.removeEventListener("lostpointercapture", up);
      document.body.classList.remove("resizing");
    };
    document.body.classList.add("resizing");
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    handle.addEventListener("lostpointercapture", up);
  }

  // ---------- Sessions de pi dans le dossier du run ----------
  const sessions = $derived(run?.sessions ?? []);
  const sessionPath = $derived(run?.sessionPath ?? null);

  async function refreshSessions(r: Run | null = run) {
    if (!r) return;
    try {
      const st = await r.send({ type: "get_state" });
      r.sessionPath = st?.data?.sessionFile ?? null;
      // Un dot garde sa conversation dans son conteneur : pas de sessions locales à lister.
      r.sessions = r.sessionPath && !r.dot ? await api.listSessions(r.sessionPath) : [];
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
    if (run) await startRun(run, { resume: true });
  }

  // ---------- Dialogues des extensions de pi (/mcp, /llama…) ----------
  const uiRequests = $derived(run?.uiRequests ?? []);

  function onExtensionUi(r: Run, ev: any) {
    const shown = r.id === visibleId;
    switch (ev.method) {
      case "notify":
        toast(shown ? (ev.message ?? "") : `${runName(r)} : ${ev.message ?? ""}`);
        break;
      case "set_editor_text":
        if (shown) draft = ev.text ?? "";
        break;
      case "select":
      case "confirm":
      case "input":
      case "editor":
        r.uiRequests = [...r.uiRequests, ev];
        // En arrière-plan, pi est bloqué jusqu'à ta réponse : on prévient.
        if (!shown) notifyBackground(r, "waiting");
        break;
      // setStatus, setWidget, setTitle : affichages propres au terminal, ignorés ici.
    }
  }

  function respondUi(req: any, res: Record<string, unknown>) {
    const r = run;
    if (!r) return;
    r.uiRequests = r.uiRequests.filter((x) => x.id !== req.id);
    if (r.attention === "waiting" && !r.uiRequests.length) r.attention = null;
    r.write({ type: "extension_ui_response", id: req.id, ...res }).catch((e) => toast(String(e)));
  }

  // ---------- Mode zen : la voix seule, sur un fond d'ambiance ----------
  let zen = $state(localStorage.getItem("zen") === "1");
  $effect(() => localStorage.setItem("zen", zen ? "1" : "0"));
  const zenActive = $derived(zen && !onboarding);
  // L'aperçu web est une vraie fenêtre native : rien ne peut passer par-dessus, le composer ne déborde pas de ce côté.
  const overRight = $derived(layout.right && preview?.kind !== "url");
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

  async function loadCommands(r: Run) {
    try {
      const [c, t] = await Promise.all([
        r.send({ type: "get_commands" }),
        r.send({ type: "get_available_thinking_levels" }),
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
        const r = await run!.send({ type: "compact", ...(arg ? { customInstructions: arg } : {}) });
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
        if (hit && run) {
          await setModel(run, hit.key);
          session.notice(`Modèle : ${hit.label}`);
        } else toast(`Modèle introuvable : ${arg}`);
        return true;
      }
      case "thinking": {
        if (!arg) {
          toast(`Niveaux disponibles : ${thinkingLevels.join(", ") || "aucun"}`);
          return true;
        }
        const r = await run!.send({ type: "set_thinking_level", level: arg });
        if (r?.success === false) toast(r.error ?? "Niveau refusé");
        else session.notice(`Réflexion : ${arg}`);
        return true;
      }
      case "name": {
        if (!arg) {
          toast("Donne un nom : /name mon sujet");
          return true;
        }
        await run!.send({ type: "set_session_name", name: arg });
        if (run?.sessionPath) run.titled.add(run.sessionPath);
        await refreshSessions();
        session.notice(`Session renommée : ${arg}`);
        return true;
      }
      case "session": {
        const r = await run!.send({ type: "get_session_stats" });
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
        const r = await run!.send({ type: "get_last_assistant_text" });
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
        const r = await run!.send({ type: "export_html", ...(arg ? { outputPath: arg } : {}) });
        if (r?.success === false) toast(r.error ?? "Export impossible");
        else session.notice(`Session exportée : ${r?.data?.path}`);
        return true;
      }
    }
    return false;
  }

  // Titre court généré par le modèle après le premier échange, enregistré dans la session pi.
  async function maybeTitle(r: Run) {
    const path = r.sessionPath;
    if (!path || r.titled.has(path) || r.dot) return;
    if (r.sessions.find((x) => x.path === path)?.named) {
      r.titled.add(path);
      if (r.branch && !r.label) r.label = r.sessions.find((x) => x.path === path)?.title ?? "";
      return;
    }
    const user = r.session.items.find((i) => i.kind === "user");
    const reply = r.session.items.find(
      (i) => i.kind === "assistant" && i.blocks.some((b) => b?.type === "text" && b.text.trim()),
    );
    if (!user || user.kind !== "user" || !reply || reply.kind !== "assistant") return;
    r.titled.add(path);
    const text = reply.blocks.filter((b) => b?.type === "text").map((b) => b.text).join("\n");
    const { body, oral } = splitOral(text);
    try {
      const title = await api.generateTitle(r.cwd, r.modelKey, user.text, body || oral || "");
      if (r.sessionPath !== path) return; // on a changé de session entre-temps
      await r.send({ type: "set_session_name", name: title });
      if (r.branch) r.label = title;
      await refreshSessions(r);
    } catch (e) {
      api.log(`titre de session : ${e}`);
    }
  }

  async function newSession() {
    if (!run) return;
    // pi travaille : la nouvelle session part en parallèle, sur une branche.
    if (session.busy) return run.dot ? toast("Le dot travaille : attends qu'il ait fini, ou arrête-le.") : startParallel();
    await api.stopSpeaking();
    const r = await run.send({ type: "new_session" });
    if (r?.data?.cancelled) return toast("Nouvelle session annulée par une extension.");
    session.reset();
    await refreshSessions();
  }

  async function openSession(s: SessionInfo) {
    if (session.busy || !run) return;
    await api.stopSpeaking();
    const r = await run.send({ type: "switch_session", sessionPath: s.path });
    if (r?.success === false) return toast(r.error ?? "Impossible d'ouvrir cette session");
    if (r?.data?.cancelled) return toast("Changement de session annulé par une extension.");
    const m = await run.send({ type: "get_messages" });
    session.loadHistory(m?.data?.messages ?? []);
    await refreshSessions();
    await loadModels(run, false);
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
    if (voiceCfg) updateVoice(voiceCfg); // le nom de l'agent vocal suit celui de l'agent
    if (renamed && run) restartPi("Nom changé : pi redémarre pour le prendre en compte.");
  }

  /** Relance le pi du run affiché (nouveau nom, prompt système modifié…) en gardant sa conversation. */
  function restartPi(why: string) {
    const r = run;
    if (!r) return;
    if (r.session.busy) {
      toast("pi travaille : la modification s'appliquera au prochain redémarrage.");
      return;
    }
    startRun(r, { resume: true }).then(() => r.session.notice(why));
  }

  // Modèles disponibles dans la configuration de pi (les mêmes pour tous les runs). Chaque run a
  // le sien ; le modèle par défaut (réglages) sert aux nouvelles sessions.
  let models = $state<{ key: string; label: string }[]>([]);
  /** Types d'entrée acceptés par chaque modèle (« text », « image »). */
  let modelInputs = $state<Record<string, string[]>>({});
  /** Modèles servis sur cette machine (localhost) : ils occupent la carte graphique. */
  let modelLocal = $state<Record<string, boolean>>({});
  let defaultModel = $state<string | null>(localStorage.getItem("model"));
  const modelKey = $derived(run?.modelKey ?? null);
  const argOptions = $derived({
    model: models.map((m) => ({ value: m.key, label: m.label })),
    thinking: thinkingLevels.map((l) => ({ value: l, label: l })),
  });
  /** Le modèle local choisi tourne déjà pour un autre run : la carte sera partagée. */
  const modelShared = $derived.by(() => {
    const k = run?.modelKey;
    if (!k || !modelLocal[k]) return [];
    return runs.filter((r) => r !== run && r.piUp && r.modelKey === k).map(runName);
  });

  /** `applyDefault` : nouvelle session, on lui donne le modèle par défaut. */
  async function loadModels(r: Run, applyDefault: boolean) {
    const st = await r.send({ type: "get_state" });
    const cur = st?.data?.model;
    const av = await r.send({ type: "get_available_models" });
    const list = av?.data?.models ?? [];
    models = list.map((m: any) => ({ key: `${m.provider}/${m.id}`, label: m.name ?? m.id }));
    modelInputs = Object.fromEntries(list.map((m: any) => [`${m.provider}/${m.id}`, m.input ?? ["text"]]));
    modelLocal = Object.fromEntries(
      list.map((m: any) => [`${m.provider}/${m.id}`, /\/\/(127\.0\.0\.1|localhost|\[::1\])\b/.test(m.baseUrl ?? "")]),
    );
    const keys = models.map((m) => m.key);
    const wanted = applyDefault && defaultModel && keys.includes(defaultModel) ? defaultModel : null;
    const curKey = cur ? `${cur.provider}/${cur.id}` : null;
    if (wanted && wanted !== curKey) await setModel(r, wanted);
    else {
      r.modelKey = curKey;
      r.session.model = cur?.name ?? cur?.id ?? null;
    }
    // Aucun modèle utilisable : pi n'a pas encore de fournisseur connecté.
    if (models.length === 0 && onboarding === null && !r.dot) onboarding = "login";
  }

  /** Modèle d'un run : s'applique à son prochain tour, sans toucher aux autres runs. */
  async function setModel(r: Run, key: string) {
    const i = key.indexOf("/");
    const res = await r.send({ type: "set_model", provider: key.slice(0, i), modelId: key.slice(i + 1) });
    if (res?.success === false) return toast(res.error ?? "Modèle refusé");
    r.modelKey = key;
    r.session.model = res?.data?.name ?? key.slice(i + 1);
  }

  function setDefaultModel(key: string) {
    defaultModel = key;
    localStorage.setItem("model", key);
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
  const projectName = $derived(run ? run.projectName : "");

  function relPath(p: string) {
    let r = p;
    // Chemins vus depuis le conteneur d'un dot : /dot/workspace/… est son state/workspace/.
    if (run?.dot && r.startsWith("/dot/workspace/")) r = r.slice("/dot/workspace/".length);
    if (root && r.startsWith(root + "/")) r = r.slice(root.length + 1);
    return r.replace(/^\.\//, "");
  }

  function toast(text: string) {
    const id = ++toastSeq;
    toasts.push({ id, text });
    setTimeout(() => (toasts = toasts.filter((t) => t.id !== id)), 6000);
  }

  // ---------- Projet et runs ----------

  /** Nom d'un run pour les listes et notifications : projet, et branche le cas échéant. */
  function runName(r: Run) {
    return r.branch ? `${r.projectName} · ${r.label || r.branch.branch.replace(/^pi\//, "")}` : r.projectName;
  }

  /** Lance (ou relance) le pi d'un run. Sans `resume` ni `session`, c'est une nouvelle session. */
  async function startRun(r: Run, opts: { resume?: boolean; session?: string | null } = {}) {
    try {
      if (r.dot) await api.piStartDot(r.id, r.dot.dir, agent.name.trim() || null);
      else await api.piStart(r.id, r.cwd, agent.name.trim() || null, { ...opts, extraPrompt: branchPrompt(r) });
      r.piUp = true;
      const fresh = !r.dot && !opts.resume && !opts.session;
      if (fresh) r.session.reset();
      else {
        // Un dot vit sans nous (Telegram, autonomie) : on reprend toujours sa conversation à jour.
        const m = await r.send({ type: "get_messages" });
        if (r.dot || !r.session.items.length) r.session.loadHistory(m?.data?.messages ?? []);
      }
      await loadModels(r, fresh);
      await refreshSessions(r);
      await loadCommands(r);
    } catch (e) {
      r.piUp = false;
      throw e;
    }
  }

  /** Affiche un run : l'explorateur, la voix et le micro passent sur lui ; les autres continuent. */
  async function showRun(r: Run) {
    if (visibleId === r.id) return;
    cancelAuto();
    if (listening) await api.listen(false, false);
    visibleId = r.id;
    selected = null;
    preview = null;
    pinned = false;
    files = [];
    git = { isRepo: false, branch: null, files: [] };
    await api.voiceSetActiveRun(r.id);
    if (!r.branch && !r.dot && !r.root.startsWith("/tmp/")) localStorage.setItem("lastProject", r.root);
    try {
      await api.watchDir(r.cwd);
      await refreshFiles();
    } catch (e) {
      api.log(`affichage du run ${r.id} : ${e}`);
    }
    // Revenu sur un run qui a fini en arrière-plan : on lit son résumé, s'il n'a pas été lu.
    const oral = r.pendingOral;
    r.pendingOral = null;
    if (r.attention !== "waiting") r.attention = null;
    if (oral && voiceCfg?.ttsEnabled) api.speak(oral);
  }

  /** Ouvre un projet : son run s'il tourne déjà, sinon un nouveau. Le run affiché continue. */
  async function openProject(dir: string) {
    let r = runs.find((x) => x.root === dir && !x.branch && !x.dot);
    if (!r) {
      r = new Run(dir);
      runs = [...runs, r];
      try {
        await startRun(r, { resume: false });
      } catch (e) {
        runs = runs.filter((x) => x !== r);
        if (localStorage.getItem("lastProject") === dir) localStorage.removeItem("lastProject");
        return toast(String(e));
      }
    }
    await showRun(runs.find((x) => x.id === r!.id)!);
  }

  /** Un run sur branche resté en pause (app relancée) : on relance son pi sur sa dernière session. */
  async function selectRun(r: Run) {
    if (!r.piUp) {
      try {
        await startRun(r, { resume: true });
      } catch (e) {
        toast(String(e));
        // Une branche dont le dossier a disparu s'affiche quand même : sa barre permet de la
        // garder ou de l'abandonner.
        if (!r.branch) return;
      }
    }
    await showRun(r);
  }

  /** Nouveau run sur le projet affiché, en parallèle, sur sa propre branche. */
  async function startParallel() {
    const base = run;
    if (!base) return;
    if (!git.isRepo && !base.branch) return toast("Ce projet n'est pas suivi par git : impossible de lancer un travail en parallèle sur une branche.");
    try {
      const stamp = new Date().toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" }).replace(":", "h");
      const wt = await api.worktreeCreate(base.root, `session ${stamp}`);
      const r = new Run(base.root, wt);
      runs = [...runs, r];
      await startRun(r, { resume: false });
      await showRun(r);
      r.session.notice(`Nouvelle session en parallèle, sur la branche ${wt.branch} (copie du dernier commit de ${wt.base}). Le travail en cours continue de son côté.`);
    } catch (e) {
      toast(`Travail en parallèle impossible : ${e}`);
    }
  }

  /** Arrête le pi d'un run. Un run sur branche reste dans la liste (en pause) : son travail attend d'être fusionné. */
  async function closeRun(r: Run) {
    await api.piStop(r.id);
    r.piUp = false;
    r.session.busy = false;
    if (!r.branch) runs = runs.filter((x) => x !== r);
    if (visibleId === r.id && !r.branch) {
      visibleId = null;
      const next = runs.find((x) => x.piUp) ?? null;
      if (next) await showRun(next);
    }
  }

  /** Fin d'un run sur branche (fusionné, gardé ou abandonné) : il quitte la liste. */
  async function dropBranchRun(r: Run) {
    await api.piStop(r.id);
    runs = runs.filter((x) => x !== r);
    api.voiceAgentReset(r.id);
    if (visibleId === r.id) {
      visibleId = null;
      await openProject(r.root);
    }
  }

  /** Un run en arrière-plan a fini, planté, ou attend ta réponse : pastille, notification et petit son. */
  function notifyBackground(r: Run, kind: "done" | "error" | "waiting") {
    r.attention = kind;
    chime(kind);
    const what = { done: "a terminé", error: "s'est arrêté sur une erreur", waiting: "attend ta réponse" }[kind];
    toast(`${runName(r)} ${what}.`);
  }

  // ---------- Dots : agents autonomes dans leur conteneur (script `dot` du dépôt dots) ----------
  let dots = $state<DotInfo[]>([]);
  let dotsCli = $state<string | null>(null);
  /** Action en cours et dernière erreur, par dossier de dot. */
  let dotPending = $state<Record<string, string>>({});
  let dotErrors = $state<Record<string, string>>({});

  /** Erreur d'un dot : gardée sous son nom (une bulle passe trop vite), et journalisée. */
  function dotError(dir: string, e: unknown | null) {
    const next = { ...dotErrors };
    if (e === null) delete next[dir];
    else {
      const msg = String(e).replace(/^Error:\s*/, "");
      next[dir] = msg;
      api.log(`dot ${dir} : ${msg}`);
    }
    dotErrors = next;
  }
  const connectedDots = $derived(runs.filter((r) => r.dot && r.piUp).map((r) => r.dot!.dir));

  async function refreshDots() {
    try {
      dotsCli = (await api.dotsInfo()).cli;
      dots = dotsCli ? await api.dotsList() : [];
    } catch (e) {
      api.log(`liste des dots : ${e}`);
    }
  }

  async function setDotsCli(path: string) {
    try {
      await api.dotsSetCli(path);
      await refreshDots();
      // Le pi local reçoit le skill des dots et DOTS_CLI à son prochain démarrage.
      if (dotsCli) restartPi("Dots configurés : pi a redémarré pour pouvoir les gérer.");
    } catch (e) {
      toast(String(e));
    }
  }

  function setDotPending(dir: string, what: string | null) {
    const next = { ...dotPending };
    if (what) next[dir] = what;
    else delete next[dir];
    dotPending = next;
  }

  const DOT_DOING: Record<DotAction, string> = {
    start: "allumage…",
    stop: "extinction…",
    remove: "suppression…",
    "use-rpc": "passage en RPC…",
    import: "rangement…",
  };

  /** Allumer, éteindre, supprimer, passer en RPC, ranger dans l'app. Renvoie false en cas d'échec. */
  async function dotAction(action: DotAction, d: DotInfo): Promise<boolean> {
    setDotPending(d.dir, DOT_DOING[action]);
    dotError(d.dir, null);
    try {
      // On se débranche d'abord (sinon la coupure s'afficherait comme un plantage).
      const r = runs.find((x) => x.dot?.dir === d.dir);
      if (r && action !== "start" && action !== "use-rpc") await closeRun(r);
      if (action === "use-rpc") {
        await api.dotsAction("use-rpc", d.dir);
        // Le mode d'interface est lu au démarrage du dot : on le rallume s'il tournait.
        if (d.running) {
          if (r) await closeRun(r);
          await api.dotsAction("stop", d.dir);
          await api.dotsAction("start", d.dir);
        }
        toast(`« ${d.name} » est en mode RPC${d.running ? " (rallumé)" : ""}.`);
      } else if (action === "import") {
        // Ses conteneurs portent l'empreinte de son dossier : on l'éteint, on le range, on le rallume.
        if (d.running) await api.dotsAction("stop", d.dir);
        const dest = await api.dotsAction("import", d.dir);
        if (d.running) await api.dotsAction("start", dest);
        toast(`« ${d.name} » est rangé dans l'app${d.running ? " (rallumé)" : ""}.`);
      } else {
        await api.dotsAction(action, d.dir);
        if (action === "remove") toast(`Dot « ${d.name} » supprimé.`);
      }
      return true;
    } catch (e) {
      dotError(d.dir, e);
      return false;
    } finally {
      setDotPending(d.dir, null);
      await refreshDots();
    }
  }

  async function createDot(name: string, from: string | null): Promise<boolean> {
    const key = `new:${name}`;
    setDotPending(key, "création…");
    try {
      await api.dotsCreate(name, from);
      toast(
        from
          ? `Dot « ${name} » créé (mêmes accès et savoir-faire, sans son passé). Clique dessus pour l'allumer.`
          : `Dot « ${name} » créé : remplis son fichier ${name}.dot (menu ⋯ → Modifier sa configuration).`,
      );
      return true;
    } catch (e) {
      toast(`Création impossible : ${e}`);
      return false;
    } finally {
      setDotPending(key, null);
      await refreshDots();
    }
  }

  /** Se branche sur un dot (en l'allumant s'il le faut) et affiche sa conversation. */
  async function openDot(d: DotInfo) {
    const existing = runs.find((x) => x.dot?.dir === d.dir);
    if (existing?.piUp) return showRun(existing);
    if (dotPending[d.dir]) return;
    dotError(d.dir, null);
    if (d.interface !== "rpc") {
      return dotError(d.dir, "Mode terminal : choisis « Passer en mode RPC » dans son menu (⋯) pour lui parler d'ici.");
    }
    if (!d.running && !(await dotAction("start", d))) return;
    const r = existing ?? Run.forDot({ name: d.name, dir: d.dir });
    if (!existing) runs = [...runs, r];
    setDotPending(d.dir, "connexion…");
    try {
      await startRun(r, { resume: true });
      await showRun(r);
    } catch (e) {
      if (!existing) runs = runs.filter((x) => x !== r);
      dotError(d.dir, e);
    } finally {
      setDotPending(d.dir, null);
      await refreshDots();
    }
  }

  async function pickProject() {
    const dir = await openDialog({ directory: true, multiple: false, title: "Choisir le dossier du projet" });
    if (typeof dir === "string") openProject(dir);
  }

  // ---------- Fermeture de l'app pendant que des runs travaillent ----------
  let closeAsk = $state<string[] | null>(null);

  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleRefresh() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(refreshFiles, 150);
  }

  async function refreshFiles() {
    const dir = root;
    if (!dir) return;
    try {
      const [f, g] = await Promise.all([api.listFiles(dir), api.gitStatus(dir)]);
      if (dir !== root) return; // on a changé de run entre-temps
      files = f;
      git = g;
      refreshKey++;
    } catch (e) {
      toast(String(e));
    }
  }

  function selectFile(p: string) {
    if (root && isPreviewable(p)) {
      void showPreview({ kind: "file", target: `${root}/${p}`, title: null });
      return;
    }
    openCode(p);
  }

  /** Le diff / code d'un fichier, même s'il est prévisualisable (bouton « Code » de l'aperçu). */
  function openCode(p: string) {
    preview = null;
    selected = p;
    pinned = true;
  }

  async function showPreview(d: { kind: Preview["kind"]; target: string; title: string | null }) {
    try {
      const url = d.kind === "url" ? d.target : await api.previewUrl(root, d.target);
      preview = { ...d, url };
      selected = null;
      layout.right = true;
    } catch (e) {
      toast(`Aperçu impossible : ${e}`);
    }
  }

  function closePreview() {
    preview = null;
    pinned = false;
  }


  /** Branche les rappels de la session d'un run : seuls ceux du run affiché touchent l'explorateur. */
  function hookRun(r: Run) {
    r.session.onFileTouched = (p) => {
      if (r.id !== visibleId) return;
      // Pendant un aperçu, pi peut continuer d'écrire : on garde l'aperçu (il se recharge).
      if (!pinned && !preview) selected = relPath(p);
      scheduleRefresh();
    };
    r.session.onShow = (d) => {
      if (r.id === visibleId) void showPreview(d);
    };
  }
  $effect(() => {
    for (const r of runs) if (!r.session.onShow) hookRun(r);
  });

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
    if (preview) return closePreview();
    if (selected) return closeFile();
  }

  function closeFile() {
    selected = null;
    pinned = false; // de nouveau, l'explorateur suit les fichiers modifiés par pi
  }

  async function updateVoice(c: VoiceConfig) {
    // Anciens réglages du mode mains libres (retiré) : on ne les renvoie plus.
    const { handsFree, wakeUrl, wakeWord, filterDb, ackOnPause, ...rest } = c as VoiceConfig & Record<string, unknown>;
    c = { ...rest, agentName: agent.name.trim() || "pi" } as VoiceConfig;
    // Voix d'un ancien moteur (TTS Kyutai 1,6B) : remplacée par la voix française de Pocket TTS.
    if (!FRENCH_VOICES.some((v) => v.id === c.voice)) c = { ...c, voice: "estelle" };
    voiceCfg = c;
    localStorage.setItem("voiceConfig", JSON.stringify(c));
    await api.setVoiceConfig(c);
  }

  // ---------- Envoi à pi ----------

  /** Envoie un texte au pi d'un run : consigne en cours de route s'il travaille, nouvelle demande sinon. */
  async function sendToPi(r: Run | null, text: string) {
    if (!r?.piUp) return toast("pi n'est pas démarré : ouvre un projet d'abord.");
    try {
      if (r.session.busy) await r.send({ type: "prompt", message: text, streamingBehavior: "steer" });
      else {
        const res = await r.send({ type: "prompt", message: text });
        if (res && res.success === false) toast(res.error ?? "pi a refusé la demande");
      }
    } catch (e) {
      toast(String(e));
    }
  }

  /** Arrête le pi d'un run sans couper la voix (l'agent vocal est peut-être en train de le dire). */
  async function stopPi(r: Run | null) {
    if (!r) return;
    try {
      await r.send({ type: "clear_queue" });
      await r.send({ type: "abort" });
    } catch (e) {
      toast(String(e));
    }
  }

  /** Dernière phrase dictée envoyée à l'agent vocal (repli vers pi s'il ne répond pas). */
  let agentAsked = "";
  /** Réponse de l'agent vocal attendue (pas encore d'action reçue). */
  let agentPending = false;
  const wordCount = (s: string) => s.trim().split(/\s+/).filter(Boolean).length;
  /** Action de la réponse en cours de l'agent vocal, affichée avec sa réponse. */
  let pendingAction: string | null = null;

  async function send() {
    cancelAuto();
    const text = draft.trim();
    if (!text && !attachments.length) return;
    const fromVoice = voiceDraft;
    voiceDraft = false;
    if (fromVoice && voiceCfg?.voiceAgent && !attachments.length && !text.startsWith("/")) {
      draft = "";
      if (listening) await api.listen(false, false);
      session.voice("user", text);
      if (STOP_WORDS.test(text)) return abort(); // « stop » : pas besoin de l'agent pour ça
      // L'agent n'a pas encore décidé quoi faire de la phrase précédente : on les envoie ensemble,
      // au lieu de perdre la première.
      const ask = agentPending && agentAsked ? `${agentAsked} ${text}` : text;
      agentAsked = ask;
      agentPending = true;
      if (run) await api.voiceAgentAsk(run.id, ask, session.summary(), modelKey);
      return;
    }
    if (!run || !piUp) {
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
        await run.send({ type: "prompt", ...withAttachments(text), streamingBehavior: "steer" });
      } else {
        if (STOP_WORDS.test(text) && !attachments.length) return;
        const r = await run.send({ type: "prompt", ...withAttachments(text) });
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
      await stopPi(run);
    } catch (e) {
      toast(String(e));
    }
  }

  // ---------- Événements ----------

  onMount(() => {
    const unlisten = [
      on<{ run: string; event: any }>("pi-event", ({ run: id, event: ev }) => {
        const r = runs.find((x) => x.id === id);
        if (!r) return;
        if (ev.type === "extension_ui_request") return onExtensionUi(r, ev);
        // Relais d'un dot : une autre interface (Telegram, ./dot chat) a déjà répondu à la question.
        if (ev.type === "hub_ui_answered") {
          r.uiRequests = r.uiRequests.filter((x) => x.id !== ev.id);
          if (r.attention === "waiting" && !r.uiRequests.length) r.attention = null;
          return;
        }
        // Le pi du dot redémarre (skill ou extension modifiés) : ce qui était en cours est perdu.
        if (ev.type === "hub_pi_restarting") {
          r.session.busy = false;
          r.uiRequests = [];
          r.session.notice("Le dot redémarre son pi (il recharge ses skills et extensions)…");
          // Le relais relance pi au bout de 3 s ; d'ici là il ne transmet rien. Ensuite on recharge
          // ses commandes (skills) et sa conversation.
          setTimeout(async () => {
            if (!r.piUp || r.session.busy) return;
            await loadCommands(r);
            const m = await r.send({ type: "get_messages" }).catch(() => null);
            if (m?.data?.messages && !r.session.busy) r.session.loadHistory(m.data.messages);
          }, 8000);
          return;
        }
        r.session.handle(ev);
        if (ev.type === "agent_settled") {
          refreshSessions(r).then(() => maybeTitle(r));
          if (!r.dot && dotsCli) refreshDots();
          if (r.id !== visibleId) {
            // Fini en arrière-plan : son résumé oral attend qu'on revienne sur lui.
            const last = [...r.session.items].reverse().find((i) => i.kind === "assistant");
            const failed = last?.kind === "assistant" && !!last.error;
            if (last?.kind === "assistant") {
              const t = last.blocks.filter((b) => b?.type === "text").map((b) => b.text).join("\n");
              r.pendingOral = splitOral(t).oral || null;
            }
            if (!ev.aborted) notifyBackground(r, failed ? "error" : "done");
          }
        } else if (ev.type === "session_info_changed") refreshSessions(r);
      }),
      on<{ run: string; code: string; stderr: string }>("pi-exit", ({ run: id, code, stderr }) => {
        const r = runs.find((x) => x.id === id);
        if (!r) return;
        r.piUp = false;
        r.session.busy = false;
        const why = stderr?.trim().split("\n").pop();
        if (r.dot) {
          r.session.notice(`Connexion au dot perdue${why ? ` : ${why}` : ""}. Clique sur lui dans la liste des dots pour te rebrancher.`, "warn");
          refreshDots();
        } else r.session.notice(`pi s'est arrêté (${code})${why ? ` : ${why}` : ""}. Clique sur la pastille du modèle pour le relancer.`, "warn");
        if (r.id !== visibleId) notifyBackground(r, "error");
      }),
      on<string[]>("close-requested", (ids) => (closeAsk = ids)),
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
      on<string>("stt-word", (w) => {
        cancelAuto();
        voiceDraft = true;
        draft = draft && !/\s$/.test(draft) ? `${draft} ${w}` : draft + w;
      }),
      on<{ reason: string }>("stt-turn-end", ({ reason }) => {
        if (!draft.trim()) return;
        if (reason === "manual") send();
        else startAuto();
      }),
      on<string[]>("fs-changed", (paths) => {
        if (paths.some((p) => !p.startsWith(".git/"))) previewKey++;
        scheduleRefresh();
      }),
      on<{ run: string; kind: string; action?: string; instruction?: string; text?: string; message?: string }>("voice-agent", (e) => {
        // La réponse va au run à qui la phrase était adressée, même si on a changé de projet depuis.
        const target = runs.find((x) => x.id === e.run) ?? null;
        if (e.kind === "action") {
          agentPending = false;
          const asked = agentAsked;
          let instruction = e.instruction || asked;
          // Consigne bien plus longue que ce que tu as dit : l'agent a brodé, on envoie tes mots.
          if (wordCount(instruction) > Math.max(2 * wordCount(asked), wordCount(asked) + 12)) instruction = asked;
          if (e.action === "transmettre") sendToPi(target, instruction);
          else if (e.action === "arreter") stopPi(target);
          if (e.action !== "repondre") pendingAction = e.action ?? null;
        } else if (e.kind === "reply") {
          if (e.text) target?.session.voice("agent", e.text, pendingAction ?? undefined);
          pendingAction = null;
        } else if (e.kind === "error") {
          agentPending = false;
          toast(`Agent vocal injoignable (${e.message}) : demande envoyée directement à pi.`);
          if (e.text) sendToPi(target, e.text);
        }
      }),
    ];

    if (isDemo()) {
      // Navigateur, `?demo` : interface remplie pour juger le design (aucun backend).
      const demo = new Run("/home/demo/projets/editeur");
      fillDemo(demo.session);
      demo.piUp = true;
      demo.sessions = DEMO_SESSIONS;
      demo.sessionPath = DEMO_SESSIONS[0].path;
      demo.session.touched = ["editeur-wysiwyg.html"];
      demo.modelKey = "harn/qwen3-27b";
      models = [
        { key: "harn/qwen3-27b", label: "Qwen3 27B (local)" },
        { key: "deepseek/deepseek-v4-pro", label: "DeepSeek V4 Pro" },
      ];
      modelLocal = { "harn/qwen3-27b": true };
      // Autres runs : un projet qui tourne en arrière-plan sur le même modèle local, une branche.
      const other = new Run("/home/demo/projets/site-vitrine");
      other.piUp = true;
      other.modelKey = "harn/qwen3-27b";
      other.session.busy = true;
      const br = new Run("/home/demo/projets/editeur", { path: "/tmp/wt", branch: "pi/export-csv-3f2a", base: "main" }, "Export CSV");
      br.piUp = true;
      br.attention = "done";
      runs = [demo, other, br];
      visibleId = demo.id;
      dotsCli = "/home/demo/dots/dot";
      const D = "/home/demo/pi-desktop/data/dots";
      const demoDot = (name: string, description: string, running: boolean, iface: string, inApp = true) => {
        const dir = inApp ? `${D}/${name}` : `/home/demo/ailleurs/${name}`;
        return { name, description, running, vault: running, interface: iface, dir, file: `${dir}/${name}.dot`, socket: "", inApp };
      };
      dots = [
        demoDot("perso", "Mon assistant personnel. Brief du matin (agenda et mails), et ce que je lui demande.", true, "rpc"),
        demoDot("veille", "Veille techno : nouveautés pi, Tauri et modèles locaux, résumé chaque lundi.", false, "rpc"),
        demoDot("compta", "Factures et relevés : classe, rapproche, prépare la déclaration.", true, "tui"),
        demoDot("ancien", "Un dot créé hors de l'app.", false, "rpc", false),
      ];
      dotErrors = new URLSearchParams(location.search).has("doterr") ? { [`${D}/veille`]: "le dot « veille » ne répond pas (pas de socket)" } : {};
      sttUp = true;
      files = DEMO_FILES;
      git = DEMO_GIT;
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
      if (dq.has("idle")) demo.session.busy = false;
      voiceCfg = { sttUrl: "", ttsUrl: "", voice: "estelle", ttsEnabled: true, agentName: "jarvis", voiceAgent: true, agentUrl: "" };
      agent = { name: "Jarvis" };
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
      await refreshDots();
    })();
    // Les dots s'allument et s'éteignent aussi ailleurs (Telegram, terminal, l'agent) : on suit.
    const dotsTimer = setInterval(() => dotsCli && refreshDots(), 20_000);

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
      clearInterval(dotsTimer);
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
        modelKey={defaultModel}
        onModel={setDefaultModel}
        {agent}
        onAgent={updateAgent}
        {root}
        onPromptSaved={() => restartPi("Prompt système enregistré : pi a redémarré.")}
        onProviders={() => ((showSettings = false), (onboarding = "login"))}
        {toast}
        {dotsCli}
        onDotsCli={setDotsCli}
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
        <div class="left-col">
          <RunsPanel
            {runs}
            {visibleId}
            name={runName}
            canParallel={!!run && !run.dot && (git.isRepo || !!run.branch)}
            onSelect={selectRun}
            onClose={closeRun}
            onParallel={startParallel}
          />
          <DotsPanel
            {dots}
            cli={dotsCli}
            connected={connectedDots}
            pending={dotPending}
            errors={dotErrors}
            onConnect={openDot}
            onAction={dotAction}
            onCreate={createDot}
            onOpen={(p) => api.openExternal(p).catch((e) => toast(String(e)))}
            onSetCli={setDotsCli}
            onRefresh={refreshDots}
          />
          {#if !run?.dot}
          <SessionsPanel
            {sessions}
            currentPath={sessionPath}
            busy={session.busy}
            canParallel={!!run && (git.isRepo || !!run.branch)}
            {agentName}
            onOpen={openSession}
            onNew={newSession}
            onTrash={trashSession}
          />
          {/if}
        </div>
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="resizer right-edge" onpointerdown={(e) => startResize("left", e)}></div>
      {/if}
    </aside>

    <section
      class="center"
      class:zen={zenActive}
      class:only-left={layout.left && !overRight}
      class:only-right={overRight && !layout.left}
      class:no-over={!layout.left && !overRight}
    >
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
          />
        {/if}
      </div>
      {#if !onboarding && !zenActive && run?.branch}
        {#key run.id}
          <BranchBar
            {run}
            busy={session.busy}
            refreshKey={refreshKey}
            {toast}
            onAskPi={(text) => sendToPi(run, text)}
            onDone={dropBranchRun}
          />
        {/key}
      {/if}
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
        {models}
        {modelKey}
        modelShared={modelShared}
        modelLocal={!!(modelKey && modelLocal[modelKey])}
        onModel={(k) => run && setModel(run, k)}
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
          {preview}
          {previewKey}
          covered={showSettings || uiRequests.length > 0 || dragOver}
          {refreshKey}
          onSelect={selectFile}
          onClosePreview={closePreview}
          onCode={openCode}
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
    {runningTool}
    project={projectName ?? ""}
    branch={git.branch}
    model={session.model}
    onRestart={() => run && startRun(run, { resume: true }).catch((e) => toast(String(e)))}
    onSettings={() => (showSettings = true)}
  />

  {#if uiRequests.length}
    {#key uiRequests[0].id}
      <ExtensionDialog req={uiRequests[0]} onRespond={(r) => respondUi(uiRequests[0], r)} />
    {/key}
  {/if}

  {#if closeAsk}
    <ConfirmDialog
      title="Des travaux sont en cours"
      message={`${closeAsk.map((id) => runs.find((r) => r.id === id)).filter(Boolean).map((r) => runName(r!)).join(", ") || "Un run"} travaille encore. Quitter arrête ${closeAsk.length > 1 ? "ces travaux" : "ce travail"} en plein milieu.`}
      confirm="Arrêter et quitter"
      cancel="Annuler"
      danger
      onConfirm={() => api.quitApp()}
      onCancel={() => (closeAsk = null)}
    />
  {/if}

  {#if dragOver}
    <div class="drop-veil"><div class="drop-box">Dépose pour joindre à ta demande</div></div>
  {/if}

  <!-- Le navigateur de l'aperçu est natif et passe devant tout : les bulles se décalent à sa gauche. -->
  <div class="toasts" style:right={preview?.kind === "url" && layout.right && !zenActive ? `${layout.rightW + 18}px` : null}>
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
  :global(body.resizing iframe) {
    pointer-events: none;
  }
  :global(body.resizing) {
    cursor: col-resize;
  }
  aside {
    position: relative;
    overflow: hidden;
  }
  .left-col {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
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
  /* Le composer garde une largeur minimale : si la colonne est trop étroite,
     il passe par-dessus le(s) panneau(x) ouvert(s) au lieu de rétrécir. */
  main .center > :global(.composer) {
    z-index: 10;
    align-self: center;
    width: calc(100% - 36px);
    min-width: min(720px, calc(100vw - 36px));
    margin-left: 0;
    margin-right: 0;
  }
  main .center.only-left > :global(.composer) {
    align-self: flex-end;
    margin-right: 18px;
  }
  main .center.no-over > :global(.composer) {
    min-width: 0;
  }
  main .center.only-right > :global(.composer) {
    align-self: flex-start;
    margin-left: 18px;
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
