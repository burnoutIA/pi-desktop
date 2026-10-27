<script lang="ts">
  import Equalizer from "./Equalizer.svelte";
  import type { VizStyle } from "../lib/viz";

  type Props = {
    userLevels: number[];
    inRms: number;
    listening: boolean;
    busy: boolean;
    sttUp: boolean;
    voiceBooting: boolean;
    agentName: string;
    onMic: () => void;
    /** Mode zen : la voix seule, en grand, au centre de la fenêtre. */
    zen?: boolean;
    /** Sous-titre du mode zen (ta transcription). */
    subtitle?: string;
    vizStyle?: VizStyle;
    userWave?: number[];
  };
  let {
    userLevels,
    inRms,
    listening,
    busy,
    sttUp,
    voiceBooting,
    agentName,
    onMic,
    zen = false,
    subtitle = "",
    vizStyle = "bars",
    userWave = [],
  }: Props = $props();

  const label = $derived(
    !sttUp
      ? voiceBooting
        ? "Chargement de la voix… (~30 s)"
        : "Serveur de transcription injoignable"
      : listening
        ? "Je t'écoute…"
        : busy
          ? `${agentName} travaille…`
          : "",
  );
  const ring = $derived(listening ? Math.min(1, inRms * 14) : 0);
</script>

<div class="stage" class:zen class:wave={vizStyle === "wave"} class:listening class:idle={!zen && !listening && !busy && sttUp}>
  <div class="glow"></div>
  <div class="eq-wrap">
    <Equalizer
      user={userLevels}
      userActive={listening}
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
  </div>
  <div class="label" class:warn={!sttUp && !voiceBooting}>{label || (zen ? "Espace pour parler" : "")}</div>
  {#if zen}
    {#key listening}
      <div class="subtitle" class:user={listening}>{subtitle}</div>
    {/key}
  {/if}
</div>

<style>
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
    background: radial-gradient(closest-side, var(--user-soft), transparent);
    opacity: 0;
    transition: opacity 0.6s;
    pointer-events: none;
  }
  .stage.listening .glow {
    opacity: 1;
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
