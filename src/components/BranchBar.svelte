<script lang="ts">
  // Run sur une branche de travail : où en est la branche, et que faire de son travail quand il
  // est prêt (fusionner dans la branche du projet, garder la branche, ou abandonner).
  import { api } from "../lib/api";
  import type { Run } from "../lib/run.svelte";

  type Props = {
    run: Run;
    busy: boolean;
    /** Change quand les fichiers bougent : l'état de la branche est relu. */
    refreshKey: number;
    toast: (msg: string) => void;
    /** Consigne envoyée au pi de ce run (résoudre un conflit). */
    onAskPi: (text: string) => void;
    /** Le run est terminé (fusionné, gardé ou abandonné) : il quitte la liste. */
    onDone: (r: Run) => void;
  };
  let { run, busy, refreshKey, toast, onAskPi, onDone }: Props = $props();

  const br = $derived(run.branch!);
  let info = $state<{ ahead: number; dirty: boolean; exists: boolean } | null>(null);
  let conflicts = $state<string[]>([]);
  let working = $state(false);
  let confirm = $state<"drop" | null>(null);

  $effect(() => {
    void refreshKey;
    void busy;
    api
      .worktreeInfo(run.root, br.path, br.branch, br.base)
      .then((i) => (info = i))
      .catch(() => (info = null));
  });

  const hasWork = $derived(!!info && (info.ahead > 0 || info.dirty));

  async function merge() {
    working = true;
    try {
      const out = await api.worktreeMerge(run.root, br.path, br.branch, run.label || br.branch);
      if (out.merged) {
        await api.worktreeRemove(run.root, br.path, br.branch, true);
        toast(`Travail de ${br.branch} fusionné dans ${br.base}.`);
        onDone(run);
      } else {
        conflicts = out.conflicts;
      }
    } catch (e) {
      toast(String(e));
    } finally {
      working = false;
    }
  }

  async function keep() {
    working = true;
    try {
      await api.worktreeRemove(run.root, br.path, br.branch, false);
      toast(`Branche ${br.branch} gardée : fusionne-la avec git quand tu veux.`);
      onDone(run);
    } catch (e) {
      toast(String(e));
    } finally {
      working = false;
    }
  }

  async function drop() {
    if (confirm !== "drop" && hasWork) {
      confirm = "drop";
      setTimeout(() => confirm === "drop" && (confirm = null), 3000);
      return;
    }
    working = true;
    try {
      await api.worktreeRemove(run.root, br.path, br.branch, true);
      toast(`Branche ${br.branch} abandonnée.`);
      onDone(run);
    } catch (e) {
      toast(String(e));
    } finally {
      working = false;
    }
  }

  function askPi() {
    onAskPi(
      `La fusion de ta branche ${br.branch} dans ${br.base} donne des conflits sur : ${conflicts.join(", ")}. ` +
        `Dans ton dossier, fusionne ${br.base} dans ta branche (git merge ${br.base}), résous ces conflits en gardant ` +
        `les deux intentions, vérifie que tout marche, puis fais un commit. Dis-moi quand c'est prêt.`,
    );
    conflicts = [];
  }
</script>

<div class="bar">
  <div class="where">
    <span class="br">⎇ {br.branch}</span>
    <span class="arrow">→ {br.base}</span>
    {#if info && !info.exists}
      <span class="note warn">dossier de travail introuvable</span>
    {:else if info}
      <span class="note">
        {info.ahead ? `${info.ahead} commit${info.ahead > 1 ? "s" : ""}` : ""}{info.ahead && info.dirty ? " · " : ""}{info.dirty
          ? "modifications non enregistrées"
          : ""}{!hasWork ? "rien à fusionner pour l'instant" : ""}
      </span>
    {/if}
  </div>
  {#if conflicts.length}
    <div class="conflict">
      <span>Conflit sur {conflicts.join(", ")} : rien n'a bougé dans le projet.</span>
      <button class="primary" onclick={askPi} disabled={busy}>Demander à pi de résoudre</button>
    </div>
  {:else}
    <div class="actions">
      <button class="primary" onclick={merge} disabled={busy || working || !hasWork} title={busy ? "pi travaille encore sur cette branche" : ""}>
        Fusionner dans {br.base}
      </button>
      <button onclick={keep} disabled={busy || working} title="Arrêter ce run en gardant la branche, pour la fusionner plus tard avec git">Garder la branche</button>
      <button class:danger={confirm === "drop"} onclick={drop} disabled={busy || working}>
        {confirm === "drop" ? "Tout jeter ?" : "Abandonner"}
      </button>
    </div>
  {/if}
</div>

<style>
  .bar {
    margin: 0 18px 8px;
    padding: 8px 10px 8px 14px;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 14px;
    border: 1px solid var(--line-2);
    border-radius: 12px;
    background: var(--panel);
    font-size: 12.5px;
  }
  .where {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex-wrap: wrap;
  }
  .br {
    font-family: var(--mono);
    color: var(--agent-2);
  }
  .arrow {
    font-family: var(--mono);
    color: var(--text-3);
  }
  .note {
    color: var(--text-3);
  }
  .note.warn {
    color: var(--del-fg);
  }
  .actions,
  .conflict {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .conflict span {
    color: var(--modified);
  }
  button {
    padding: 4px 10px;
    border-radius: 8px;
    border: 1px solid var(--line-2);
    color: var(--text-2);
    font-size: 12.5px;
  }
  button:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--text-3);
  }
  button.primary {
    border-color: var(--agent-1);
    color: var(--text-strong);
  }
  button.danger {
    border-color: var(--del-fg);
    color: var(--del-fg);
  }
  button:disabled {
    opacity: 0.45;
  }
</style>
