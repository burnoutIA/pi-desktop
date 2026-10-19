<script lang="ts">
  import { tick } from "svelte";

  export type DuplexLine = { who: "user" | "agent"; text: string; cut?: boolean };
  type Props = {
    lines: DuplexLine[];
    active: boolean;
    /** Durée médiane d'un micro-tour sur les derniers tours (ms). */
    medianMs: number | null;
    pListen: number | null;
    agentName: string;
    onReset: () => void;
    onExit: () => void;
  };
  let { lines, active, medianMs, pListen, agentName, onReset, onExit }: Props = $props();

  let box: HTMLDivElement;
  $effect(() => {
    lines.length;
    lines[lines.length - 1]?.text;
    tick().then(() => box?.scrollTo({ top: box.scrollHeight }));
  });
</script>

<div class="duplex">
  <header>
    <div class="state" class:on={active}>
      <i></i>
      {active ? "Conversation libre — parle quand tu veux, coupe-la quand tu veux" : "En attente du LLM duplex et de la transcription…"}
    </div>
    <div class="stats">
      {#if medianMs !== null}<span title="Durée médiane d'un micro-tour (budget : 600 ms)">{medianMs} ms / tour</span>{/if}
      {#if pListen !== null}<span title="Probabilité d'écouter au dernier tour">P(écoute) {pListen.toFixed(2)}</span>{/if}
    </div>
    <button onclick={onReset}>Nouvelle conversation</button>
    <button onclick={onExit}>Quitter le mode duplex</button>
  </header>
  <div class="lines" bind:this={box}>
    {#if !lines.length}
      <p class="hint">
        Dis bonjour. {agentName} écoute en continu et décide seul quand répondre, toutes les 0,6 s.
        Le modèle est entraîné en anglais : il comprend le français, mais l'anglais marche mieux.
        Un casque évite qu'il s'entende lui-même.
      </p>
    {/if}
    {#each lines as l, i (i)}
      <div class="line {l.who}" class:cut={l.cut}>
        <span class="who">{l.who === "user" ? "Toi" : agentName}</span>
        <span class="text">{l.text}{#if l.cut}<em> — coupé</em>{/if}</span>
      </div>
    {/each}
  </div>
</div>

<style>
  .duplex {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 8px 16px 16px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    flex: 1;
    min-width: 220px;
  }
  .state i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-3);
  }
  .state.on i {
    background: var(--user-2);
    box-shadow: 0 0 8px var(--user-2);
  }
  .stats {
    display: flex;
    gap: 10px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    font-size: 0.9em;
  }
  header button {
    padding: 5px 10px;
    border-radius: 8px;
    border: 1px solid var(--line-2);
    background: var(--panel);
    color: var(--text);
    cursor: pointer;
  }
  .lines {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .hint {
    color: var(--text-3);
    max-width: 560px;
    line-height: 1.5;
  }
  .line {
    display: flex;
    gap: 10px;
    line-height: 1.5;
    max-width: 760px;
  }
  .who {
    flex: none;
    width: 72px;
    color: var(--text-3);
    font-size: 0.85em;
    padding-top: 2px;
  }
  .line.user .text {
    color: var(--text);
  }
  .line.agent .text {
    color: var(--text-2);
  }
  .line.cut em {
    color: var(--text-3);
    font-size: 0.85em;
  }
</style>
