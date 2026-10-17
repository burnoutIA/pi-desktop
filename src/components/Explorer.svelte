<script lang="ts">
  // Explorateur du projet : arbre des fichiers / fichiers modifiés, puis aperçu (diff ou contenu).
  import type { GitStatusKind } from "../lib/api";
  import FileTree from "./FileTree.svelte";
  import DiffView from "./DiffView.svelte";

  type Props = {
    root: string;
    files: string[];
    status: Record<string, GitStatusKind>;
    touched: Set<string>;
    isRepo: boolean;
    branch: string | null;
    selected: string | null;
    refreshKey: number;
    onSelect: (path: string) => void;
    onBack: () => void;
    onInitGit: () => void;
  };
  let { root, files, status, touched, isRepo, branch, selected, refreshKey, onSelect, onBack, onInitGit }: Props =
    $props();

  let tab = $state<"files" | "changes">((localStorage.getItem("explorerTab") as "files" | "changes") ?? "changes");
  $effect(() => localStorage.setItem("explorerTab", tab));

  const changed = $derived(new Set([...Object.keys(status), ...touched]).size);
</script>

<div class="explorer">
  {#if selected}
    <DiffView {root} path={selected} status={status[selected]} {isRepo} {refreshKey} {onInitGit} {onBack} />
  {:else}
    <div class="tabs">
      <button class:on={tab === "changes"} onclick={() => (tab = "changes")}>
        Modifications{#if changed}<span class="count">{changed}</span>{/if}
      </button>
      <button class:on={tab === "files"} onclick={() => (tab = "files")}>Fichiers</button>
      <div class="spacer"></div>
      {#if branch}<span class="branch" title="Branche git">⎇ {branch}</span>{/if}
    </div>
    {#if !root}
      <div class="empty">Ouvre un projet pour voir ses fichiers.</div>
    {:else}
      {#if tab === "changes" && !isRepo}
        <div class="hint">
          Ce dossier n'est pas suivi par git : les diffs ne montrent que les nouveaux fichiers.
          <button onclick={onInitGit}>Initialiser git</button>
        </div>
      {/if}
      <div class="tree">
        <FileTree {files} {status} {touched} {selected} {onSelect} onlyChanged={tab === "changes"} />
      </div>
    {/if}
  {/if}
</div>

<style>
  .explorer {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 8px 10px 0;
    border-bottom: 1px solid var(--line);
  }
  .tabs button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px 8px;
    font-size: 12.5px;
    color: var(--text-3);
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--agent-1);
  }
  .count {
    background: var(--agent-1);
    color: var(--on-accent);
    border-radius: 999px;
    padding: 0 6px;
    font-size: 10.5px;
    line-height: 16px;
  }
  .spacer {
    flex: 1;
  }
  .branch {
    font-family: var(--mono);
    font-size: 11.5px;
    color: var(--text-3);
    padding-bottom: 6px;
    white-space: nowrap;
  }
  .tree {
    flex: 1;
    min-height: 0;
    padding-top: 6px;
  }
  .empty {
    margin: auto;
    color: var(--text-3);
    font-size: 13px;
  }
  .hint {
    margin: 10px 12px 0;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid var(--line);
    font-size: 12px;
    color: var(--text-3);
  }
  .hint button {
    display: block;
    margin-top: 6px;
    color: var(--agent-2);
    font-size: 12px;
  }
</style>
