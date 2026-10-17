<script lang="ts">
  import type { SessionInfo } from "../lib/api";
  import { cleanTitle } from "../lib/titles";

  type Props = {
    sessions: SessionInfo[];
    currentPath: string | null;
    busy: boolean;
    agentName: string;
    onOpen: (s: SessionInfo) => void;
    onNew: () => void;
    onTrash: (s: SessionInfo) => void;
  };
  let { sessions, currentPath, busy, agentName, onOpen, onNew, onTrash }: Props = $props();

  /** En dessous, une session est « courte » (un essai, une question isolée) et va dans un groupe replié. */
  const SHORT = 4;

  let confirm = $state<string | null>(null);
  let showShort = $state(localStorage.getItem("showShortSessions") === "1");
  $effect(() => localStorage.setItem("showShortSessions", showShort ? "1" : "0"));

  const title = (s: SessionInfo) => (s.named ? s.title : cleanTitle(s.title, agentName));
  const isShort = (s: SessionInfo) => s.messages <= SHORT && s.path !== currentPath;

  const short = $derived(sessions.filter(isShort));

  // Les sessions « vraies » regroupées par période, comme les apps de chat.
  const groups = $derived.by(() => {
    const now = new Date();
    const startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    const day = 86_400_000;
    const buckets: { label: string; items: SessionInfo[] }[] = [
      { label: "Aujourd'hui", items: [] },
      { label: "Hier", items: [] },
      { label: "7 derniers jours", items: [] },
      { label: "30 derniers jours", items: [] },
      { label: "Plus ancien", items: [] },
    ];
    for (const s of sessions) {
      if (isShort(s)) continue;
      const t = s.updated;
      const i = t >= startOfDay ? 0 : t >= startOfDay - day ? 1 : t >= startOfDay - 7 * day ? 2 : t >= startOfDay - 30 * day ? 3 : 4;
      buckets[i].items.push(s);
    }
    return buckets.filter((b) => b.items.length);
  });

  function time(ms: number) {
    const d = new Date(ms);
    if (new Date().toDateString() === d.toDateString()) {
      return d.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
    }
    return d.toLocaleDateString("fr-FR", { day: "numeric", month: "short" }).replace(".", "");
  }

  function askTrash(s: SessionInfo) {
    confirm = s.path;
    setTimeout(() => confirm === s.path && (confirm = null), 3000);
  }
</script>

{#snippet row(s: SessionInfo)}
  {@const current = s.path === currentPath}
  <div class="item" class:current>
    <button class="open" onclick={() => !current && onOpen(s)} disabled={busy && !current} title={s.title}>
      <span class="name">{title(s)}</span>
    </button>
    {#if confirm === s.path}
      <button class="trash sure" onclick={() => ((confirm = null), onTrash(s))}>Supprimer ?</button>
    {:else}
      <span class="time">{time(s.updated)}</span>
      {#if !current}
        <button class="trash" onclick={() => askTrash(s)} title="Mettre à la corbeille" aria-label="Supprimer">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            ><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14" /></svg
          >
        </button>
      {/if}
    {/if}
  </div>
{/snippet}

<div class="panel">
  <div class="head">
    <span class="title">Sessions</span>
    <button class="new" onclick={onNew} disabled={busy} title="Nouvelle session">
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"
        ><path d="M12 5v14M5 12h14" /></svg
      >
      Nouvelle
    </button>
  </div>
  <div class="list">
    {#each groups as g (g.label)}
      <div class="group">{g.label}</div>
      {#each g.items as s (s.path)}{@render row(s)}{/each}
    {/each}

    {#if short.length}
      <button class="group toggle" onclick={() => (showShort = !showShort)}>
        <span class="caret" class:rot={showShort}>›</span> Courtes <span class="n">{short.length}</span>
      </button>
      {#if showShort}
        {#each short as s (s.path)}{@render row(s)}{/each}
      {/if}
    {/if}

    {#if sessions.length === 0}
      <div class="none">Aucune session pour ce dossier.</div>
    {/if}
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
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
  .new {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 9px;
    border-radius: 7px;
    font-size: 12px;
    color: var(--text-2);
  }
  .new:hover:not(:disabled) {
    background: var(--panel-2);
    color: var(--text-strong);
  }
  .new:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 0 6px 12px;
  }
  .group {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 14px 8px 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-3);
    text-align: left;
  }
  .toggle:hover {
    color: var(--text-2);
  }
  .caret {
    display: inline-block;
    width: 10px;
    transition: transform 0.15s;
  }
  .caret.rot {
    transform: rotate(90deg);
  }
  .n {
    font-weight: 500;
    opacity: 0.8;
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    height: 30px;
    border-radius: 7px;
    padding-right: 6px;
  }
  .item:hover {
    background: var(--panel-2);
  }
  .item.current {
    background: var(--agent-soft);
  }
  .item.current::before {
    content: "";
    position: absolute;
    left: 0;
    top: 7px;
    bottom: 7px;
    width: 2px;
    border-radius: 2px;
    background: var(--agent-1);
  }
  .open {
    flex: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    padding: 0 8px 0 10px;
    text-align: left;
  }
  .open:disabled {
    cursor: default;
  }
  .name {
    width: 100%;
    font-size: 13px;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .item:hover .name {
    color: var(--text);
  }
  .current .name {
    color: var(--text-strong);
    font-weight: 600;
  }
  .time {
    flex: none;
    font-size: 11px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .trash {
    display: none;
    flex: none;
    padding: 3px 5px;
    border-radius: 6px;
    color: var(--text-3);
    font-size: 11px;
  }
  /* Au survol, l'heure laisse sa place à la corbeille. */
  .item:hover .time {
    display: none;
  }
  .item:hover .trash,
  .trash.sure {
    display: block;
  }
  .trash:hover,
  .trash.sure {
    color: var(--err);
  }
  .none {
    padding: 12px 10px;
    color: var(--text-3);
    font-size: 12.5px;
  }
</style>
