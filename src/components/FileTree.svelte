<script lang="ts">
  import type { GitStatusKind } from "../lib/api";
  import { fileIcon } from "../lib/fileicon";

  type Node = { name: string; path: string; dir: boolean; children: Node[] };
  type Props = {
    root: string;
    files: string[];
    status: Record<string, GitStatusKind>;
    touched: Set<string>;
    selected: string | null;
    onSelect: (path: string) => void;
    /** Vue « Modifications » : seulement les fichiers changés (git ou touchés par pi). */
    onlyChanged: boolean;
  };
  let { root, files, status, touched, selected, onSelect, onlyChanged }: Props = $props();

  // Arbre complet : tout est fermé par défaut, les dossiers ouverts sont mémorisés par projet.
  const storeKey = $derived(`openDirs:${root}`);
  let opened = $state<Set<string>>(new Set());
  $effect(() => {
    try {
      opened = new Set(JSON.parse(localStorage.getItem(storeKey) ?? "[]"));
    } catch {
      opened = new Set();
    }
  });
  function toggleOpened(path: string) {
    const next = new Set(opened);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    opened = next;
    try {
      localStorage.setItem(storeKey, JSON.stringify([...next]));
    } catch {}
  }

  // Vue « Modifications » : tout ouvert par défaut, repli temporaire.
  let collapsed = $state<Record<string, boolean>>({});

  // Dossiers contenant des changements : affichés ouverts et marqués.
  const dirtyDirs = $derived.by(() => {
    const s = new Set<string>();
    for (const p of [...Object.keys(status), ...touched]) {
      const parts = p.split("/");
      for (let i = 1; i < parts.length; i++) s.add(parts.slice(0, i).join("/"));
    }
    return s;
  });

  const tree = $derived.by(() => {
    const root: Node = { name: "", path: "", dir: true, children: [] };
    const all = new Set(files);
    for (const p of Object.keys(status)) all.add(p); // fichiers supprimés inclus
    const list = [...all].filter((p) => !onlyChanged || status[p] || touched.has(p));
    for (const p of list) {
      const parts = p.split("/");
      let cur = root;
      for (let i = 0; i < parts.length; i++) {
        const isDir = i < parts.length - 1;
        const path = parts.slice(0, i + 1).join("/");
        let next = cur.children.find((c) => c.name === parts[i] && c.dir === isDir);
        if (!next) {
          next = { name: parts[i], path, dir: isDir, children: [] };
          cur.children.push(next);
        }
        cur = next;
      }
    }
    const sort = (n: Node) => {
      n.children.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
      n.children.forEach(sort);
    };
    sort(root);
    return root;
  });

  const BADGE: Record<GitStatusKind, string> = {
    modified: "M",
    added: "A",
    deleted: "D",
    renamed: "R",
    untracked: "U",
    conflict: "!",
  };

  function isOpen(n: Node) {
    if (!onlyChanged) return opened.has(n.path);
    return !collapsed[n.path];
  }

  function toggle(n: Node) {
    if (onlyChanged) collapsed[n.path] = isOpen(n);
    else toggleOpened(n.path);
  }
</script>

<div class="tree">
  <div class="list">
    {#snippet branch(nodes: Node[], depth: number)}
      {#each nodes as n (n.path + (n.dir ? "/" : ""))}
        {#if n.dir}
          <button
            class="item dir"
            class:dirty={dirtyDirs.has(n.path)}
            style="padding-left:{10 + depth * 14}px"
            onclick={() => toggle(n)}
          >
            <span class="caret" class:rot={isOpen(n)}>›</span>
            <span class="name">{n.name}</span>
          </button>
          {#if isOpen(n)}{@render branch(n.children, depth + 1)}{/if}
        {:else}
          {@const st = status[n.path]}
          <button
            class="item file {st ?? ''}"
            class:sel={selected === n.path}
            class:touched={touched.has(n.path)}
            style="padding-left:{16 + depth * 14}px"
            onclick={() => onSelect(n.path)}
            title={n.path}
          >
            <span class="ficon" style="color:{fileIcon(n.path).color}">{fileIcon(n.path).label}</span>
            <span class="name">{n.name}</span>
            {#if st}<span class="badge {st}">{BADGE[st]}</span>{/if}
          </button>
        {/if}
      {/each}
    {/snippet}
    {@render branch(tree.children, 0)}
    {#if tree.children.length === 0}
      <div class="none">{onlyChanged ? "Aucune modification" : "Dossier vide"}</div>
    {/if}
  </div>
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .list {
    overflow: auto;
    flex: 1;
    padding-bottom: 12px;
  }
  .item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 6px;
    text-align: left;
    padding: 3px 10px;
    font-size: 13px;
    color: var(--text-2);
    border-radius: 0;
    position: relative;
  }
  .item:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .item.sel {
    background: var(--agent-soft);
    color: var(--text);
  }
  .item.sel::before {
    content: "";
    position: absolute;
    left: 0;
    top: 3px;
    bottom: 3px;
    width: 2px;
    background: var(--agent-1);
    border-radius: 2px;
  }
  .dir .name {
    color: var(--text-2);
  }
  .dir.dirty .name {
    color: var(--text);
  }
  .caret {
    width: 10px;
    display: inline-block;
    color: var(--text-3);
    transition: transform 0.15s;
  }
  .caret.rot {
    transform: rotate(90deg);
  }
  .ficon {
    flex: none;
    width: 18px;
    text-align: center;
    font-family: var(--mono);
    font-size: 10px;
    font-weight: 800;
    letter-spacing: -0.04em;
  }
  .dir .name {
    font-weight: 500;
  }
  .name {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .file.modified .name {
    color: var(--modified);
  }
  .file.added .name,
  .file.untracked .name {
    color: var(--add-fg);
  }
  .file.deleted .name {
    color: var(--del-fg);
    text-decoration: line-through;
  }
  .file.touched .name::after {
    content: "";
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-left: 6px;
    border-radius: 50%;
    background: var(--agent-2);
    box-shadow: 0 0 8px var(--agent-2);
    vertical-align: middle;
  }
  .badge {
    font-family: var(--mono);
    font-size: 10.5px;
    font-weight: 700;
    width: 16px;
    text-align: center;
  }
  .badge.modified {
    color: var(--modified);
  }
  .badge.added,
  .badge.untracked {
    color: var(--add-fg);
  }
  .badge.deleted {
    color: var(--del-fg);
  }
  .none {
    padding: 12px 14px;
    color: var(--text-3);
    font-size: 12.5px;
  }
</style>
