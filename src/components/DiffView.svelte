<script lang="ts">
  import { api, type GitStatusKind } from "../lib/api";
  import { parseDiff, diffStats } from "../lib/diff";
  import { fileIcon } from "../lib/fileicon";
  import { highlight, highlightLines, langFromPath } from "../lib/highlight";

  type Props = {
    root: string;
    path: string | null;
    status: GitStatusKind | undefined;
    isRepo: boolean;
    refreshKey: number;
    onInitGit: () => void;
    onBack: () => void;
  };
  let { root, path, status, isRepo, refreshKey, onInitGit, onBack }: Props = $props();

  let tab = $state<"diff" | "file">("diff");
  let diff = $state("");
  let content = $state("");
  let error = $state<string | null>(null);
  let loading = $state(false);

  const lines = $derived(parseDiff(diff));
  const lang = $derived(path ? langFromPath(path) : null);
  const fileLines = $derived(highlightLines(content, lang));
  // Fichier entièrement nouveau : on l'affiche comme un fichier normal, pas comme un mur de « + ».
  const isNew = $derived(
    lines.length > 0 && lines.every((l) => l.kind === "add" || l.kind === "hunk") && /^@@ -0,0 /m.test(diff),
  );
  const newLines = $derived(
    isNew ? highlightLines(lines.filter((l) => l.kind === "add").map((l) => l.text).join("\n"), lang) : [],
  );
  const icon = $derived(path ? fileIcon(path) : null);
  const stats = $derived(diffStats(lines));

  $effect(() => {
    // Recharge quand le fichier, l'onglet ou le contenu sur disque changent.
    const p = path;
    const t = tab;
    void refreshKey;
    if (!p || !root) return;
    let cancelled = false;
    loading = true;
    const job = t === "diff" ? api.gitDiff(root, p) : api.readFile(root, p);
    job
      .then((txt) => {
        if (cancelled) return;
        error = null;
        if (t === "diff") diff = txt;
        else content = txt;
      })
      .catch((e) => !cancelled && (error = String(e)))
      .finally(() => !cancelled && (loading = false));
    return () => {
      cancelled = true;
    };
  });

  const name = $derived(path?.split("/").pop() ?? "");
  const dir = $derived(path?.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
</script>

<div class="diff">
  {#if !path}
    <div class="placeholder">
      <div class="ph-title">Aucun fichier sélectionné</div>
      <div class="ph-sub">Les fichiers que pi modifie s'ouvrent ici automatiquement.</div>
      {#if !isRepo && root}
        <button class="init" onclick={onInitGit}>Initialiser git pour suivre les changements</button>
      {/if}
    </div>
  {:else}
    <div class="head">
      <button class="back" onclick={onBack} title="Retour à la liste (Échap)" aria-label="Retour">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
          ><path d="M15 18l-6-6 6-6" /></svg
        >
      </button>
      <div class="file">
        {#if icon}<span class="ficon" style="color:{icon.color}">{icon.label}</span>{/if}
        <span class="fname">{name}</span>
        {#if dir}<span class="fdir">{dir}</span>{/if}
      </div>
      {#if tab === "diff" && isNew}
        <span class="badge-new">Nouveau fichier · {stats.add} lignes</span>
      {:else if tab === "diff" && (stats.add || stats.del)}
        <span class="stats"><span class="a">+{stats.add}</span> <span class="d">−{stats.del}</span></span>
      {/if}
      <div class="tabs">
        <button class:on={tab === "diff"} onclick={() => (tab = "diff")}>Diff</button>
        <button class:on={tab === "file"} onclick={() => (tab = "file")}>Fichier</button>
      </div>
    </div>
    <div class="body selectable" class:dim={loading}>
      {#if error}
        <div class="msg err">{error}</div>
      {:else if tab === "diff"}
        {#if isNew}
          <table class="newfile">
            <tbody>
              {#each newLines as l, i (i)}
                <tr class="ctx"><td class="ln">{i + 1}</td><td class="code">{@html l}</td></tr>
              {/each}
            </tbody>
          </table>
        {:else if lines.length === 0}
          <div class="msg">
            {status === "deleted" ? "Fichier supprimé." : "Aucune modification par rapport au dernier commit."}
          </div>
        {:else}
          <table>
            <tbody>
              {#each lines as l, i (i)}
                {#if l.kind === "hunk"}
                  <tr class="hunk">
                    <td colspan="3">
                      Lignes {l.new}–{(l.new ?? 0) + Math.max(0, (l.count ?? 1) - 1)}{#if l.text}<span class="ctxname"
                          >{l.text}</span
                        >{/if}
                    </td>
                  </tr>
                {:else}
                  <tr class={l.kind}>
                    <td class="ln">{l.old ?? ""}</td>
                    <td class="ln">{l.new ?? ""}</td>
                    <td class="code"><span class="sign">{l.kind === "add" ? "+" : l.kind === "del" ? "−" : " "}</span
                      >{@html highlight(l.text, lang)}</td
                    >
                  </tr>
                {/if}
              {/each}
            </tbody>
          </table>
        {/if}
      {:else}
        <table>
          <tbody>
            {#each fileLines as l, i (i)}
              <tr class="ctx"><td class="ln">{i + 1}</td><td class="code">{@html l}</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  {/if}
</div>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .placeholder {
    margin: auto;
    text-align: center;
    color: var(--text-3);
    padding: 24px;
  }
  .ph-title {
    color: var(--text-2);
    font-weight: 600;
    margin-bottom: 4px;
  }
  .init {
    margin-top: 14px;
    padding: 7px 14px;
    border-radius: 8px;
    border: 1px solid var(--line-2);
    color: var(--text-2);
  }
  .init:hover {
    border-color: var(--agent-1);
    color: var(--text);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--line);
  }
  .back {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    margin-left: -6px;
    border-radius: 7px;
    color: var(--text-2);
  }
  .back:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .file {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .fname {
    font-weight: 650;
    color: var(--text-strong);
    white-space: nowrap;
  }
  .fdir {
    color: var(--text-3);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
  }
  .stats {
    font-family: var(--mono);
    font-size: 12px;
  }
  .a {
    color: var(--add-fg);
  }
  .d {
    color: var(--del-fg);
  }
  .tabs {
    display: flex;
    background: var(--bg-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 2px;
  }
  .tabs button {
    padding: 3px 10px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-3);
  }
  .tabs button.on {
    background: var(--panel-2);
    color: var(--text);
  }
  .body {
    flex: 1;
    overflow: auto;
    font-family: var(--mono);
    font-size: 12px;
    line-height: 1.6;
    transition: opacity 0.15s;
  }
  .body.dim {
    opacity: 0.6;
  }
  .msg {
    padding: 18px;
    color: var(--text-3);
    font-family: var(--sans);
  }
  .msg.err {
    color: var(--err);
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  td {
    padding: 0 10px;
    vertical-align: top;
  }
  .ln {
    width: 1%;
    min-width: 40px;
    text-align: right;
    color: var(--text-3);
    opacity: 0.7;
    user-select: none;
  }
  .code {
    white-space: pre;
    color: var(--text-2);
  }
  .sign {
    display: inline-block;
    width: 14px;
    opacity: 0.7;
    user-select: none;
  }
  tr.add {
    background: var(--add-bg);
  }
  tr.add .sign {
    color: var(--add-fg);
    opacity: 1;
  }
  tr.del {
    background: var(--del-bg);
  }
  tr.del .sign {
    color: var(--del-fg);
    opacity: 1;
  }
  tr.add .ln,
  tr.del .ln {
    opacity: 1;
  }
  tr.add .ln {
    color: var(--add-fg);
  }
  tr.del .ln {
    color: var(--del-fg);
  }
  .badge-new {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--add-fg);
    background: var(--add-bg);
    padding: 2px 8px;
    border-radius: 999px;
    white-space: nowrap;
  }
  .newfile .ln {
    box-shadow: inset -2px 0 0 color-mix(in srgb, var(--add-fg) 45%, transparent);
  }
  .ctxname {
    margin-left: 10px;
    color: var(--text-3);
    font-family: var(--mono);
  }
  .ficon {
    font-family: var(--mono);
    font-size: 10.5px;
    font-weight: 800;
    align-self: center;
  }
  tr.hunk td {
    color: var(--text-2);
    background: var(--panel-2);
    padding: 3px 12px;
    font-family: var(--sans);
    font-size: 11.5px;
    font-weight: 600;
    border-top: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
  }
</style>
