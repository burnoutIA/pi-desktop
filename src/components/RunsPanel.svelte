<script lang="ts">
  // Runs ouverts : un par projet, plus ceux qui travaillent en parallèle sur une branche.
  // Une pastille dit où en est chacun ; un clic l'affiche (les autres continuent de tourner).
  import type { Run } from "../lib/run.svelte";

  type Props = {
    runs: Run[];
    visibleId: string | null;
    name: (r: Run) => string;
    /** Le projet affiché est suivi par git : on peut lancer un travail en parallèle. */
    canParallel: boolean;
    onSelect: (r: Run) => void;
    onClose: (r: Run) => void;
    onParallel: () => void;
  };
  let { runs, visibleId, name, canParallel, onSelect, onClose, onParallel }: Props = $props();

  let confirm = $state<string | null>(null);

  type State = "busy" | "waiting" | "error" | "done" | "idle" | "paused";
  function stateOf(r: Run): State {
    if (!r.piUp) return r.attention === "error" ? "error" : "paused";
    if (r.uiRequests.length) return "waiting";
    if (r.session.busy) return "busy";
    if (r.attention === "error") return "error";
    if (r.attention === "done") return "done";
    return "idle";
  }
  const LABEL: Record<State, string> = {
    busy: "travaille",
    waiting: "attend ta réponse",
    error: "erreur",
    done: "a terminé",
    idle: "prêt",
    paused: "en pause",
  };

  function close(r: Run) {
    if (r.session.busy && confirm !== r.id) {
      confirm = r.id;
      setTimeout(() => confirm === r.id && (confirm = null), 3000);
      return;
    }
    confirm = null;
    onClose(r);
  }
</script>

{#if runs.length}
  <div class="runs">
    <div class="head">
      <span class="title">En cours</span>
      <button
        class="par"
        onclick={onParallel}
        disabled={!canParallel}
        title={canParallel
          ? "Nouvelle session sur ce projet, en parallèle, sur sa propre branche"
          : "Ouvre un projet suivi par git pour travailler en parallèle"}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
          ><circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="8" r="2" /><path d="M6 7v10M18 10c0 4-6 3-11 7" /></svg
        >
        En parallèle
      </button>
    </div>
    <div class="list">
      {#each runs as r (r.id)}
        {@const st = stateOf(r)}
        <div class="item" class:current={r.id === visibleId}>
          <button class="open" onclick={() => onSelect(r)} title="{name(r)} : {LABEL[st]}">
            <span class="dot {st}" aria-label={LABEL[st]}></span>
            {#if r.branch}<span class="br" aria-hidden="true">⎇</span>{/if}
            <span class="name">{name(r)}</span>
          </button>
          {#if confirm === r.id}
            <button class="x sure" onclick={() => close(r)}>Arrêter ?</button>
          {:else if r.piUp}
            <button
              class="x"
              onclick={() => close(r)}
              title={r.branch ? "Mettre en pause (le travail de la branche est gardé)" : "Fermer ce projet (arrête son pi)"}
              aria-label="Fermer"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"
                ><path d="M6 6l12 12M18 6L6 18" /></svg
              >
            </button>
          {/if}
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .runs {
    flex: none;
    max-height: 40%;
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--line);
    padding-bottom: 6px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 10px 6px 14px;
  }
  .title {
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-3);
    font-weight: 650;
  }
  .par {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 9px;
    border-radius: 7px;
    font-size: 12px;
    color: var(--text-2);
  }
  .par:hover:not(:disabled) {
    background: var(--panel-2);
    color: var(--text);
  }
  .par:disabled {
    opacity: 0.45;
  }
  .list {
    overflow-y: auto;
    padding: 0 6px;
  }
  .item {
    display: flex;
    align-items: center;
    border-radius: 8px;
  }
  .item:hover,
  .item.current {
    background: var(--panel-2);
  }
  .item.current .name {
    color: var(--text-strong);
    font-weight: 600;
  }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    text-align: left;
    font-size: 13px;
    color: var(--text-2);
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .br {
    color: var(--text-3);
    font-size: 12px;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-3);
  }
  .dot.busy {
    background: var(--agent-2);
    box-shadow: 0 0 8px var(--agent-2);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .dot.waiting {
    background: var(--modified);
    box-shadow: 0 0 8px var(--modified);
    animation: pulse 0.9s ease-in-out infinite;
  }
  .dot.error {
    background: var(--del-fg);
  }
  .dot.done {
    background: var(--add-fg);
  }
  .dot.paused {
    background: transparent;
    border: 1.5px solid var(--text-3);
  }
  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }
  .x {
    flex: none;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    margin-right: 4px;
    border-radius: 6px;
    color: var(--text-3);
    opacity: 0;
  }
  .item:hover .x,
  .x.sure {
    opacity: 1;
  }
  .x:hover {
    color: var(--text);
    background: var(--line);
  }
  .x.sure {
    width: auto;
    padding: 0 8px;
    font-size: 11.5px;
    color: var(--del-fg);
  }
</style>
