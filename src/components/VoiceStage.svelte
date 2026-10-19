<script lang="ts">
  import Equalizer from "./Equalizer.svelte";
  import type { VizStyle } from "../lib/viz";

  type Props = {
    agentLevels: number[];
    userLevels: number[];
    inRms: number;
    speaking: boolean;
    listening: boolean;
    busy: boolean;
    sttUp: boolean;
    voiceBooting: boolean;
    agentName: string;
    woke: number;
    handsFree: boolean;
    /** Conversation duplex : micro ouvert en continu. */
    duplex?: boolean;
    onMic: () => void;
    onStopVoice: () => void;
    /** Mode zen : la voix seule, en grand, au centre de la fenêtre. */
    zen?: boolean;
    /** Sous-titre du mode zen (ta transcription, ou ce que dit l'agent). */
    subtitle?: string;
    vizStyle?: VizStyle;
    agentWave?: number[];
    userWave?: number[];
  };
  let {
    agentLevels,
    userLevels,
    inRms,
    speaking,
    listening,
    busy,
    sttUp,
    voiceBooting,
    agentName,
    woke,
    handsFree,
    duplex = false,
    onMic,
    onStopVoice,
    zen = false,
    subtitle = "",
    vizStyle = "bars",
    agentWave = [],
    userWave = [],
  }: Props = $props();

  const label = $derived(
    !sttUp
      ? voiceBooting
        ? "Chargement de la voix… (~30 s)"
        : "Serveur de transcription injoignable"
      : duplex
        ? speaking
          ? `${agentName} parle — coupe-lui la parole quand tu veux`
          : "Conversation libre : parle quand tu veux"
        : listening
        ? "Je t'écoute…"
        : speaking
          ? handsFree
            ? `${agentName} parle — dis « ${agentName} » pour l'interrompre`
            : `${agentName} parle — Espace pour lui couper la parole`
          : busy
            ? `${agentName} travaille…`
            : "",
  );
  const ring = $derived(listening ? Math.min(1, inRms * 14) : 0);
</script>

{#key woke}
  <div class="wake-flash" class:on={woke > 0}></div>
{/key}
<div class="stage" class:zen class:wave={vizStyle === "wave"} class:speaking class:listening class:idle={!zen && !speaking && !listening && !busy && sttUp}>
  <div class="glow"></div>
  <div class="eq-wrap">
    <Equalizer
      agent={agentLevels}
      user={userLevels}
      agentActive={speaking}
      userActive={listening}
      {agentWave}
      {userWave}
      style={vizStyle}
    />
  </div>
  <div class="controls">
    <button
      class="mic"
      class:on={listening}
      class:off={!sttUp}
      style="--ring:{ring}"
      onclick={onMic}
      title={listening ? "Terminer et envoyer" : "Parler (Espace)"}
      aria-label="Micro"
    >
      <span class="halo"></span>
      {#if listening}
        <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="3" /></svg>
      {:else}
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
          ><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0" /><path d="M12 18v3" /></svg
        >
      {/if}
    </button>
    {#if speaking}
      <button class="hush" onclick={onStopVoice} title="Couper la voix (Échap)">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
          ><path d="M11 5 6 9H2v6h4l5 4V5Z" /><path d="m22 9-6 6M16 9l6 6" /></svg
        >
      </button>
    {/if}
  </div>
  <div class="label" class:warn={!sttUp && !voiceBooting}>{label || (zen ? (handsFree ? `Dis « ${agentName}, … »` : "Espace pour parler") : "")}</div>
  {#if zen}
    {#key listening}
      <div class="subtitle" class:user={listening}>{subtitle}</div>
    {/key}
  {/if}
</div>

<style>
  .wake-flash {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 5;
  }
  .wake-flash.on {
    animation: wake 0.9s ease-out;
    box-shadow: inset 0 0 0 2px var(--user-2), inset 0 0 60px var(--user-line);
    opacity: 0;
  }
  @keyframes wake {
    0% {
      opacity: 1;
    }
    100% {
      opacity: 0;
    }
  }
  /* ---------- Mode zen ---------- */
  .stage.zen {
    isolation: isolate;
    flex: 1;
    justify-content: center;
    gap: 22px;
    border-bottom: 0;
    padding: 24px 40px 60px;
    z-index: 1;
    overflow: visible;
  }
  .stage.zen .glow {
    display: none;
  }
  .stage.zen .eq-wrap {
    position: relative;
    z-index: 2;
    width: min(92%, 980px);
    height: 220px;
    transform: translateZ(0);
  }
  /* L'onde traverse toute la largeur, d'un bord à l'autre (comme un oscilloscope). */
  .stage.zen.wave .eq-wrap {
    width: calc(100% + 80px);
    height: 260px;
  }
  .stage.zen .mic {
    width: 84px;
    height: 84px;
    background: color-mix(in srgb, var(--panel) 70%, transparent);
    backdrop-filter: blur(8px);
  }
  .stage.zen .label {
    font-size: 14px;
    letter-spacing: 0.02em;
  }
  .subtitle {
    max-width: min(760px, 90%);
    min-height: 3.2em;
    text-align: center;
    font-size: 19px;
    line-height: 1.55;
    color: var(--text-strong);
    opacity: 0.85;
    animation: fadein 0.5s ease;
  }
  .subtitle.user {
    color: color-mix(in srgb, var(--user-1) 70%, var(--text-strong));
  }
  @keyframes fadein {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  .stage {
    position: relative;
    padding: 18px 28px 14px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    border-bottom: 1px solid var(--line);
    overflow: hidden;
  }
  .glow {
    position: absolute;
    inset: -40% 10% auto;
    height: 200px;
    background: radial-gradient(closest-side, var(--agent-glow), transparent);
    opacity: 0;
    transition: opacity 0.6s;
    pointer-events: none;
  }
  .stage.speaking .glow {
    opacity: 1;
  }
  .stage.listening .glow {
    opacity: 1;
    background: radial-gradient(closest-side, var(--user-soft), transparent);
  }
  .eq-wrap {
    width: min(100%, 760px);
    height: 108px;
    transition: height 0.35s ease;
  }
  /* Au repos, le bloc voix se fait discret ; il s'ouvre dès qu'on parle ou que l'agent travaille. */
  .stage.idle {
    padding-top: 10px;
    padding-bottom: 10px;
    gap: 6px;
  }
  .stage.idle .eq-wrap {
    height: 40px;
  }
  .stage.idle .label {
    display: none;
  }
  .controls {
    position: relative;
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .mic {
    position: relative;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    color: var(--text-strong);
    background: linear-gradient(145deg, var(--panel), var(--panel-2));
    border: 1.5px solid var(--line-2);
    box-shadow:
      0 10px 30px var(--shadow),
      inset 0 1px 0 rgba(255, 255, 255, 0.05);
    transition:
      transform 0.15s,
      background 0.25s,
      border-color 0.25s;
  }
  .mic:hover {
    transform: translateY(-1px);
    border-color: var(--agent-1);
  }
  .mic:active {
    transform: scale(0.96);
  }
  .mic.on {
    background: linear-gradient(145deg, var(--user-1), var(--user-2));
    border-color: transparent;
    color: var(--on-user);
  }
  .mic.off {
    opacity: 0.55;
  }
  .halo {
    position: absolute;
    inset: -6px;
    border-radius: 50%;
    border: 2px solid var(--user-2);
    opacity: calc(var(--ring) * 0.9);
    transform: scale(calc(1 + var(--ring) * 0.35));
    transition:
      transform 0.08s linear,
      opacity 0.08s linear;
    pointer-events: none;
  }
  .hush {
    position: absolute;
    left: calc(100% + 14px);
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    color: var(--text-2);
    border: 1px solid var(--line-2);
    background: var(--panel);
  }
  .hush:hover {
    color: var(--text);
    border-color: var(--agent-1);
  }
  .label {
    font-size: 12.5px;
    color: var(--text-3);
    letter-spacing: 0.01em;
    min-height: 18px;
  }
  .label.warn {
    color: var(--warn);
  }
</style>
