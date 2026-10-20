<script lang="ts">
  import { FRENCH_VOICES, api, type AgentConfig, type VoiceConfig } from "../lib/api";
  import { PALETTES, loadThemePref, previewTokens, resolveMode, saveThemePref, type ModePref } from "../lib/theme";
  import { VIZ_STYLES, loadViz, saveViz, type VizStyle } from "../lib/viz";

  type Props = {
    config: VoiceConfig;
    onChange: (c: VoiceConfig) => void;
    onClose: () => void;
    models: { key: string; label: string }[];
    modelKey: string | null;
    onModel: (key: string) => void;
    agent: AgentConfig;
    onAgent: (a: AgentConfig) => void;
    root: string;
    onPromptSaved: () => void;
    onProviders: () => void;
    toast: (msg: string) => void;
  };
  let { config, onChange, onClose, models, modelKey, onModel, agent, onAgent, root, onPromptSaved, onProviders, toast }: Props =
    $props();

  type Tab = "agent" | "voice" | "look";
  let tab = $state<Tab>((localStorage.getItem("settingsTab") as Tab) ?? "agent");

  // ---------- Apparence ----------
  // La palette réellement appliquée fait foi (elle peut venir d'ailleurs que du réglage enregistré).
  let theme = $state({ ...loadThemePref(), palette: document.documentElement.dataset.palette ?? loadThemePref().palette });
  const previewMode = $derived(resolveMode(theme.mode));
  function setTheme(t: Partial<typeof theme>) {
    theme = { ...theme, ...t };
    saveThemePref(theme);
  }
  let viz = $state<VizStyle>(loadViz());
  function setViz(v: VizStyle) {
    viz = v;
    saveViz(v);
  }

  const MODES: { id: ModePref; label: string }[] = [
    { id: "system", label: "Système" },
    { id: "dark", label: "Sombre" },
    { id: "light", label: "Clair" },
  ];
  $effect(() => localStorage.setItem("settingsTab", tab));

  function set<K extends keyof VoiceConfig>(k: K, v: VoiceConfig[K]) {
    onChange({ ...config, [k]: v });
  }
  function setAgent<K extends keyof AgentConfig>(k: K, v: AgentConfig[K]) {
    onAgent({ ...agent, [k]: v });
  }

  // ---------- Prompt système (APPEND_SYSTEM.md) ----------
  let scope = $state<"global" | "project">("global");
  let promptPath = $state("");
  let saved = $state("");
  let draft = $state("");
  let projectOverrides = $state(false); // un prompt projet existe : il remplace le global
  let saving = $state(false);
  const dirty = $derived(draft !== saved);

  $effect(() => {
    const s = scope;
    const r = root;
    let cancelled = false;
    Promise.all([api.readPrompt(s, r || null), r ? api.readPrompt("project", r) : Promise.resolve(null)])
      .then(([f, proj]) => {
        if (cancelled) return;
        promptPath = f.path;
        saved = f.content;
        draft = f.content;
        projectOverrides = !!proj?.exists;
      })
      .catch((e) => !cancelled && toast(String(e)));
    return () => {
      cancelled = true;
    };
  });

  async function savePrompt() {
    saving = true;
    try {
      await api.writePrompt(scope, root || null, draft);
      saved = draft;
      if (scope === "project") projectOverrides = draft.trim() !== "";
      onPromptSaved();
    } catch (e) {
      toast(String(e));
    } finally {
      saving = false;
    }
  }

  function shortPath(p: string) {
    const home = p.match(/^\/home\/[^/]+/)?.[0];
    return home ? "~" + p.slice(home.length) : p;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="backdrop" onclick={onClose}></div>
<div class="panel" role="dialog" aria-label="Réglages">
  <div class="tabs">
    <button class:on={tab === "agent"} onclick={() => (tab = "agent")}>Agent</button>
    <button class:on={tab === "voice"} onclick={() => (tab = "voice")}>Voix</button>
    <button class:on={tab === "look"} onclick={() => (tab = "look")}>Apparence</button>
  </div>

  {#if tab === "agent"}
    <label class="field">
      <span>Nom</span>
      <input
        value={agent.name}
        placeholder="pi"
        onchange={(e) => setAgent("name", (e.target as HTMLInputElement).value.trim())}
      />
      <small>L'agent vocal se présente sous ce nom.</small>
    </label>

    {#if models.length}
      <label class="field">
        <span>Modèle</span>
        <select value={modelKey} onchange={(e) => onModel((e.target as HTMLSelectElement).value)}>
          {#each models as m (m.key)}
            <option value={m.key}>{m.label}</option>
          {/each}
        </select>
      </label>
    {/if}

    <div class="field">
      <div class="row">
        <span>Fournisseurs d'IA</span>
        <button class="try" onclick={onProviders}>Connecter / gérer…</button>
      </div>
      <small>Ouvre la connexion de pi (/login) : abonnement ou clé API.</small>
    </div>

    <div class="field">
      <div class="row">
        <span>Prompt système</span>
        <div class="seg">
          <button class:on={scope === "global"} onclick={() => (scope = "global")}>Global</button>
          <button class:on={scope === "project"} disabled={!root} onclick={() => (scope = "project")}>Ce projet</button>
        </div>
      </div>
      <textarea
        class="prompt"
        bind:value={draft}
        spellcheck="false"
        placeholder={scope === "global"
          ? "Consignes ajoutées au prompt de pi partout (personnalité, langue, style…)"
          : "Consignes pour ce projet uniquement. Vide = le prompt global s'applique."}
      ></textarea>
      <div class="row">
        <small class="path" title={promptPath}>{shortPath(promptPath)}</small>
        <button class="save" disabled={!dirty || saving} onclick={savePrompt}>
          {saving ? "…" : "Enregistrer"}
        </button>
      </div>
      {#if scope === "global" && projectOverrides}
        <small class="warn">Ce projet a son propre prompt : il remplace celui-ci ici.</small>
      {:else if scope === "project"}
        <small>Prioritaire sur le global pour ce projet (pi ne cumule pas les deux).</small>
      {/if}
    </div>

  {:else if tab === "look"}
    <div class="field">
      <span>Mode</span>
      <div class="seg wide">
        {#each MODES as m (m.id)}
          <button class:on={theme.mode === m.id} onclick={() => setTheme({ mode: m.id })}>{m.label}</button>
        {/each}
      </div>
    </div>
    <div class="field">
      <span>Visualisation de la voix</span>
      <div class="vizs">
        {#each VIZ_STYLES as v (v.id)}
          <button class="viz" class:on={viz === v.id} onclick={() => setViz(v.id)} title={v.description}>
            <svg viewBox="0 0 120 40" class="viz-art" aria-hidden="true">
              {#if v.id === "bars"}
                {#each [4, 8, 14, 20, 26, 20, 14, 26, 20, 14, 8, 4] as hh, i (i)}
                  <rect x={8 + i * 9} y={20 - hh / 2} width="4" height={hh} rx="2" />
                {/each}
              {:else if v.id === "wave"}
                <path
                  class="line"
                  d="M0 20 L6 18 L10 23 L14 15 L18 25 L22 12 L26 27 L30 16 L34 22 L38 9 L42 30 L46 14 L50 24 L54 11 L58 28 L62 17 L66 22 L70 13 L74 26 L78 18 L82 21 L86 15 L90 24 L94 19 L98 21 L104 19 L110 20 L120 20"
                />
              {:else}
                <path
                  class="fill"
                  d="M0 20 Q15 19 25 14 T50 6 T70 12 T95 15 T120 20 Q105 21 95 25 T70 28 T50 34 T25 26 T0 20 Z"
                />
              {/if}
            </svg>
            <span class="viz-name">{v.label}</span>
          </button>
        {/each}
      </div>
    </div>
    <div class="field">
      <span>Palette</span>
      <div class="palettes">
        {#each PALETTES as p (p.id)}
          {@const t = previewTokens(p, previewMode)}
          <button
            class="pal"
            class:on={theme.palette === p.id}
            onclick={() => setTheme({ palette: p.id })}
            title={p.description}
          >
            <div class="mini" style="background:{t.bg}; border-color:{t.line}">
              <div class="mini-head" style="background:{t['bg-2']}; border-color:{t.line}"></div>
              <div class="mini-eq">
                {#each [3, 6, 10, 14, 9, 5, 9, 14, 10, 6, 3] as h, i (i)}
                  <span style="height:{h}px; background:linear-gradient({t['agent-2']}, {t['agent-1']})"></span>
                {/each}
              </div>
              <div
                class="mini-user"
                style="background:linear-gradient(135deg, color-mix(in srgb, {t['user-1']} 30%, {t.bg}), color-mix(in srgb, {t['user-2']} 26%, {t.bg})); border-color:color-mix(in srgb, {t['user-2']} 45%, {t.bg})"
              ></div>
              <div class="mini-agent" style="background:{t.panel}; border-color:{t.line}">
                <i style="background:{t['text-3']}"></i><i style="background:{t['text-3']}; width:60%"></i>
              </div>
            </div>
            <div class="pal-name">{p.label}</div>
            <div class="pal-desc">{p.description}</div>
          </button>
        {/each}
      </div>
    </div>
  {:else}
    <label class="field">
      <span>Voix de {agent.name || "pi"}</span>
      <div class="voice-row">
        <select value={config.voice} onchange={(e) => set("voice", (e.target as HTMLSelectElement).value)}>
          {#each FRENCH_VOICES as v (v.id)}
            <option value={v.id}>{v.label}</option>
          {/each}
          {#if !FRENCH_VOICES.some((v) => v.id === config.voice)}
            <option value={config.voice}>{config.voice}</option>
          {/if}
        </select>
        <button
          class="try"
          onclick={() => api.speak(`Bonjour, je suis ${agent.name || "pi"}. Dis-moi ce que tu veux construire.`)}
        >
          Écouter
        </button>
      </div>
    </label>

    <label class="toggle">
      <input type="checkbox" checked={config.ttsEnabled} onchange={(e) => set("ttsEnabled", (e.target as HTMLInputElement).checked)} />
      <span><b>Lecture vocale</b><small>lit son résumé à la fin de chaque réponse</small></span>
    </label>

    <label class="toggle">
      <input type="checkbox" checked={config.voiceAgent} onchange={(e) => set("voiceAgent", (e.target as HTMLInputElement).checked)} />
      <span
        ><b>Agent vocal</b><small
          >ce que tu dis passe d'abord par lui : il répond tout de suite, même quand pi travaille, et transmet à pi ce
          qui est à faire. Le clavier va toujours directement à pi.</small
        ></span
      >
    </label>
    {#if config.voiceAgent}
      <label class="field">
        <span>Serveur de l'agent vocal (compatible OpenAI, harn par défaut)</span>
        <input value={config.agentUrl} onchange={(e) => set("agentUrl", (e.target as HTMLInputElement).value)} />
      </label>
    {/if}

    <div class="title sub">Serveurs Kyutai</div>
    <label class="field">
      <span>Transcription (STT)</span>
      <input value={config.sttUrl} onchange={(e) => set("sttUrl", (e.target as HTMLInputElement).value)} />
    </label>
    <label class="field">
      <span>Synthèse (TTS)</span>
      <input value={config.ttsUrl} onchange={(e) => set("ttsUrl", (e.target as HTMLInputElement).value)} />
    </label>
  {/if}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
  }
  .panel {
    position: absolute;
    top: 48px;
    right: 12px;
    z-index: 21;
    width: 420px;
    max-height: calc(100vh - 64px);
    overflow-y: auto;
    padding: 14px 16px 16px;
    border-radius: 14px;
    background: var(--panel);
    border: 1px solid var(--line-2);
    box-shadow: 0 20px 60px var(--shadow);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .tabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--line);
    margin: 0 -16px;
    padding: 0 12px;
  }
  .tabs button {
    padding: 6px 12px 9px;
    font-size: 13px;
    color: var(--text-3);
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--agent-1);
  }
  .title {
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-3);
    font-weight: 600;
  }
  .title.sub {
    margin-top: 4px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .field small {
    color: var(--text-3);
    font-size: 11.5px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .voice-row {
    display: flex;
    gap: 8px;
  }
  select,
  input:not([type="checkbox"]):not([type="range"]),
  textarea {
    flex: 1;
    background: var(--bg-2);
    border: 1px solid var(--line-2);
    border-radius: 8px;
    padding: 6px 8px;
    outline: 0;
    user-select: text;
  }
  input:focus,
  textarea:focus,
  select:focus {
    border-color: var(--agent-1);
  }
  .prompt {
    min-height: 170px;
    resize: vertical;
    font-family: var(--mono);
    font-size: 12px;
    line-height: 1.5;
  }
  .path {
    font-family: var(--mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .warn {
    color: var(--warn) !important;
  }
  .seg {
    display: flex;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 2px;
  }
  .seg button {
    padding: 3px 10px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-3);
  }
  .seg button.on {
    background: var(--panel-2);
    color: var(--text);
  }
  .seg.wide button {
    flex: 1;
  }
  .vizs {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }
  .viz {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 8px 6px 6px;
    border-radius: 10px;
    border: 1.5px solid var(--line);
    background: var(--bg-2);
  }
  .viz:hover {
    border-color: var(--line-2);
  }
  .viz.on {
    border-color: var(--agent-1);
    box-shadow: 0 0 0 3px var(--agent-soft);
  }
  .viz-art {
    width: 100%;
    height: 34px;
  }
  .viz-art rect {
    fill: var(--agent-1);
  }
  .viz-art .line {
    fill: none;
    stroke: var(--text-strong);
    stroke-width: 1.2;
    opacity: 0.7;
    filter: drop-shadow(0 2px 2px var(--agent-1));
  }
  .viz-art .fill {
    fill: var(--agent-soft);
    stroke: var(--agent-1);
    stroke-width: 1.5;
  }
  .viz-name {
    font-size: 12px;
    color: var(--text);
  }
  .palettes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .pal {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 6px 8px;
    border-radius: 12px;
    border: 1.5px solid var(--line);
    text-align: left;
  }
  .pal:hover {
    border-color: var(--line-2);
  }
  .pal.on {
    border-color: var(--agent-1);
    box-shadow: 0 0 0 3px var(--agent-soft);
  }
  .mini {
    position: relative;
    height: 84px;
    border-radius: 8px;
    border: 1px solid;
    overflow: hidden;
  }
  .mini-head {
    height: 12px;
    border-bottom: 1px solid;
  }
  .mini-eq {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
    height: 22px;
  }
  .mini-eq span {
    width: 3px;
    border-radius: 2px;
  }
  .mini-user {
    position: absolute;
    right: 8px;
    top: 40px;
    width: 46%;
    height: 12px;
    border-radius: 6px;
    border: 1px solid;
  }
  .mini-agent {
    position: absolute;
    left: 8px;
    top: 58px;
    width: 62%;
    height: 18px;
    border-radius: 6px;
    border: 1px solid;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 3px;
    padding: 0 6px;
  }
  .mini-agent i {
    display: block;
    height: 2px;
    border-radius: 2px;
    opacity: 0.6;
  }
  .pal-name {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text);
    padding: 0 2px;
  }
  .pal-desc {
    font-size: 11px;
    color: var(--text-3);
    padding: 0 2px;
    line-height: 1.35;
  }
  .seg button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .save {
    flex: none;
    padding: 5px 12px;
    border-radius: 8px;
    background: var(--accent-grad);
    color: var(--on-accent);
    font-size: 12.5px;
    font-weight: 600;
  }
  .save:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .try {
    padding: 0 12px;
    border-radius: 8px;
    border: 1px solid var(--line-2);
    color: var(--text-2);
    font-size: 12.5px;
  }
  .try:hover {
    border-color: var(--agent-1);
    color: var(--text);
  }
  .toggle {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    cursor: pointer;
  }
  .toggle input {
    margin-top: 3px;
    accent-color: var(--agent-1);
  }
  .toggle span {
    display: flex;
    flex-direction: column;
  }
  .toggle b {
    font-weight: 500;
    font-size: 13px;
  }
  .toggle small {
    color: var(--text-3);
    font-size: 12px;
  }
</style>
