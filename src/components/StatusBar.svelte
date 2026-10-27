<script lang="ts">
  // Barre d'état façon VS Code : sa couleur dit d'un coup d'œil ce qui se passe.
  type Props = {
    agentName: string;
    piUp: boolean;
    hasProject: boolean;
    busy: boolean;
    listening: boolean;
    sttUp: boolean;
    voiceBooting: boolean;
    runningTool: string | null;
    project: string;
    branch: string | null;
    model: string | null;
    onRestart: () => void;
    onOpenProject: () => void;
    onSettings: () => void;
  };
  let p: Props = $props();

  const state = $derived(
    !p.hasProject ? "noproject" : !p.piUp ? "down" : p.listening ? "listening" : p.busy ? "busy" : "idle",
  );
  const label = $derived(
    {
      noproject: "Aucun projet ouvert — Ctrl+O",
      down: "pi arrêté — cliquer pour relancer",
      listening: "Je t'écoute",
      busy: p.runningTool ? `${p.agentName} travaille · ${p.runningTool}` : `${p.agentName} réfléchit…`,
      idle: "Prêt",
    }[state],
  );
</script>

<footer class="status {state}">
  <button
    class="cell main"
    onclick={() => (state === "down" ? p.onRestart() : state === "noproject" ? p.onOpenProject() : null)}
    disabled={state !== "down" && state !== "noproject"}
  >
    <span class="pulse"></span>
    {label}
  </button>
  {#if p.project}
    <span class="cell">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"
        ><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" /></svg
      >
      {p.project}
    </span>
    {#if p.branch}
      <span class="cell mono">⎇ {p.branch}</span>
    {/if}
  {/if}
  <span class="spacer"></span>
  <button class="cell" onclick={p.onSettings} title="Réglages de la voix">⌨ Espace pour parler</button>
  <span
    class="cell"
    title={p.sttUp ? "Voix prête" : p.voiceBooting ? "Chargement des modèles vocaux" : "Serveur de transcription injoignable"}
  >
    <span class="dot" class:ok={p.sttUp} class:boot={p.voiceBooting} class:err={!p.sttUp && !p.voiceBooting}></span>
    {p.voiceBooting ? "voix…" : "voix"}
  </span>
  <button class="cell mono" onclick={p.onSettings} title="Modèle (réglages, onglet Agent)">{p.model ?? "pi"}</button>
</footer>

<style>
  .status {
    flex: none;
    display: flex;
    align-items: stretch;
    height: 26px;
    font-size: 12px;
    background: var(--chrome);
    color: var(--text-2);
    border-top: 1px solid var(--line);
    transition: background 0.25s, color 0.25s;
  }
  .cell {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    white-space: nowrap;
    color: inherit;
  }
  button.cell:not(:disabled):hover {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }
  button.cell:disabled {
    cursor: default;
  }
  .mono {
    font-family: var(--mono);
    font-size: 11.5px;
  }
  .spacer {
    flex: 1;
  }
  .main {
    font-weight: 600;
    padding: 0 12px;
  }
  .pulse {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--agent-1);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.err {
    background: var(--err);
  }
  .dot.boot {
    background: var(--warn);
    animation: pulse 1s ease-in-out infinite;
  }

  /* Le bloc d'état prend la couleur de ce qui se passe. */
  .listening .main {
    background: var(--user-2);
    color: var(--on-user);
  }
  .busy .main {
    color: var(--on-accent);
    background: repeating-linear-gradient(
        -45deg,
        transparent 0 10px,
        color-mix(in srgb, #fff 14%, transparent) 10px 20px
      ),
      var(--accent-grad);
    background-size: 28px 28px, 100% 100%;
    animation: stripes 0.9s linear infinite;
  }
  .down .main {
    background: var(--err);
    color: #fff;
  }
  .listening .pulse,
  .busy .pulse,
  .down .pulse {
    background: currentColor;
    animation: pulse 1s ease-in-out infinite;
  }
  .idle .pulse {
    box-shadow: 0 0 6px var(--agent-1);
  }
  @keyframes stripes {
    to {
      background-position: 28px 0, 0 0;
    }
  }
</style>
