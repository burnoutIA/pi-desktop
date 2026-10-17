<script lang="ts">
  import { tick } from "svelte";
  import type { Item, Session } from "../lib/session.svelte";
  import { splitOral, toolPath } from "../lib/session.svelte";
  import { renderBlock, splitBlocks } from "../lib/markdown";

  type Props = {
    session: Session;
    root: string;
    onOpenFile: (path: string) => void;
    onReplay: (text: string) => void;
    agentName: string;
    handsFree: boolean;
    onOpenProject: () => void;
  };
  let { session, root, onOpenFile, onReplay, agentName, handsFree, onOpenProject }: Props = $props();

  let scroller: HTMLDivElement;
  let stick = true;
  let open = $state<Record<string, boolean>>({});

  function onScroll() {
    const el = scroller;
    stick = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  }

  // Suit le bas de la conversation tant que l'utilisateur n'est pas remonté.
  $effect(() => {
    session.items.length;
    lastLen();
    if (stick) tick().then(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  });

  function lastLen() {
    const last = session.items[session.items.length - 1];
    if (!last) return 0;
    if (last.kind === "assistant") return last.blocks.reduce((n, b) => n + (b?.text.length ?? 0), 0);
    if (last.kind === "tool") return last.output.length;
    return 0;
  }

  function rel(p: string) {
    if (root && p.startsWith(root)) return p.slice(root.length).replace(/^\/+/, "");
    return p;
  }

  function toolSummary(it: Extract<Item, { kind: "tool" }>) {
    const a = it.args ?? {};
    const p = toolPath(a);
    if (p) return rel(p);
    if (typeof a.command === "string") return a.command;
    if (typeof a.pattern === "string") return a.pattern;
    if (typeof a.query === "string") return a.query;
    const s = JSON.stringify(a);
    return s.length > 90 ? s.slice(0, 90) + "…" : s;
  }

  // Couleur par famille d'outils, comme les catégories d'une palette de commandes.
  const TOOL_COLOR: Record<string, string> = {
    bash: "var(--tool-bash)",
    read: "var(--tool-read)",
    ls: "var(--tool-read)",
    write: "var(--tool-edit)",
    edit: "var(--tool-edit)",
    multiedit: "var(--tool-edit)",
    grep: "var(--tool-search)",
    find: "var(--tool-search)",
  };

  const TOOL_ICON: Record<string, string> = {
    bash: "❯",
    read: "◫",
    write: "✎",
    edit: "✎",
    grep: "⌕",
    find: "⌕",
    ls: "☰",
  };

  function textBlocks(it: Extract<Item, { kind: "assistant" }>) {
    return it.blocks.filter((b) => b && b.type === "text").map((b) => b.text).join("\n\n");
  }
  function thinking(it: Extract<Item, { kind: "assistant" }>) {
    return it.blocks.filter((b) => b && b.type === "thinking").map((b) => b.text).join("\n\n");
  }
</script>

<div class="feed" bind:this={scroller} onscroll={onScroll}>
  {#if session.items.length === 0}
    <div class="empty">
      <div class="empty-title">{root ? `Parle à ${agentName}` : "Ouvre un projet"}</div>
      <div class="empty-sub">
        {#if !root}
          {agentName} travaille dans un dossier de ton choix.
          <button class="open-project" onclick={onOpenProject}>Choisir un dossier <kbd>Ctrl+O</kbd></button>
        {:else if handsFree}
          Dis « {agentName}, … » suivi de ta demande, ou appuie sur <kbd>Espace</kbd>.
        {:else}
          Appuie sur <kbd>Espace</kbd> (ou clique sur le micro), dis ce que tu veux, corrige le texte si besoin.
        {/if}
      </div>
    </div>
  {/if}

  {#each session.items as it (it.id)}
    {#if it.kind === "user"}
      <div class="row user">
        <div class="bubble user-b selectable">{it.text}</div>
      </div>
    {:else if it.kind === "assistant"}
      {@const think = thinking(it)}
      {@const { body, oral } = splitOral(textBlocks(it))}
      <div class="row">
        <div class="assistant">
          {#if think}
            <button class="think" onclick={() => (open[it.id] = !open[it.id])}>
              <span class="caret" class:rot={open[it.id]}>›</span> Réflexion
              {#if it.streaming && !body}<span class="shimmer">en cours…</span>{/if}
            </button>
            {#if open[it.id]}<div class="think-body selectable">{think}</div>{/if}
          {/if}
          {#if body}
            <!-- Un élément par bloc : pendant le streaming, seul le dernier bloc change. -->
            <div class="md">
              {#each splitBlocks(body) as block, bi (bi)}
                <div class="md-block">{@html renderBlock(block)}</div>
              {/each}
            </div>
          {/if}
          {#if oral}
            <div class="oral">
              <button class="oral-btn" title="Réécouter" onclick={() => onReplay(oral)}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
                  ><path d="M11 5 6 9H2v6h4l5 4V5Z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path
                    d="M19 5a10 10 0 0 1 0 14"
                  /></svg
                >
              </button>
              <span class="selectable">{oral}</span>
            </div>
          {/if}
          {#if it.streaming && !body && !think}
            <div class="typing"><span></span><span></span><span></span></div>
          {/if}
          {#if it.error}<div class="error">{it.error}</div>{/if}
        </div>
      </div>
    {:else if it.kind === "tool"}
      {@const p = toolPath(it.args)}
      <div class="row">
        <div class="tool {it.status}" style="--tc:{TOOL_COLOR[it.name] ?? 'var(--agent-1)'}">
          <button class="tool-head" onclick={() => (open[it.id] = !open[it.id])}>
            <span class="tool-icon">{TOOL_ICON[it.name] ?? "◆"}</span>
            <span class="tool-name">{it.name}</span>
            <span class="tool-arg">{toolSummary(it)}</span>
            {#if it.status === "running"}
              <span class="spinner"></span>
            {:else if it.durationMs != null}
              <span class="tool-ms">{it.durationMs < 1000 ? `${it.durationMs} ms` : `${(it.durationMs / 1000).toFixed(1)} s`}</span>
            {/if}
          </button>
          {#if p && (it.name === "write" || it.name === "edit")}
            <button class="tool-link" onclick={() => onOpenFile(p)}>voir le diff →</button>
          {/if}
          {#if open[it.id] && it.output}
            <pre class="tool-out selectable">{it.output.length > 6000 ? it.output.slice(0, 6000) + "\n…" : it.output}</pre>
          {/if}
        </div>
      </div>
    {:else}
      <div class="notice {it.tone}">{it.text}</div>
    {/if}
  {/each}

  {#if session.status}
    <div class="notice info">{session.status}</div>
  {/if}
  {#each session.queue.steering as q, i (i)}
    <div class="row user"><div class="bubble user-b queued">{q}<span class="q-tag">en attente</span></div></div>
  {/each}
</div>

<style>
  .feed {
    height: 100%;
    overflow-y: auto;
    padding: 18px 22px 28px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .empty {
    margin: auto;
    text-align: center;
    max-width: 380px;
    color: var(--text-3);
  }
  .empty-title {
    font-size: 22px;
    color: var(--text-strong);
    margin-bottom: 6px;
    font-weight: 600;
  }
  .open-project {
    display: flex;
    width: fit-content;
    align-items: center;
    gap: 8px;
    margin: 14px auto 0;
    padding: 8px 16px;
    border-radius: 10px;
    background: var(--accent-grad);
    color: var(--on-accent);
    font-weight: 600;
  }
  kbd {
    font-family: var(--mono);
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 5px;
    border: 1px solid var(--line-2);
    background: var(--panel-2);
  }
  .row {
    display: flex;
  }
  .row.user {
    justify-content: flex-end;
  }
  .bubble {
    max-width: 78%;
    padding: 9px 14px;
    border-radius: 16px;
    white-space: pre-wrap;
  }
  .user-b {
    background: linear-gradient(135deg, var(--user-soft), var(--user-2-soft));
    border: 1px solid var(--user-line);
    border-bottom-right-radius: 5px;
  }
  .queued {
    opacity: 0.65;
    border-style: dashed;
  }
  .q-tag {
    margin-left: 8px;
    font-size: 11px;
    color: var(--text-3);
  }
  .assistant {
    max-width: 92%;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .think {
    align-self: flex-start;
    color: var(--text-3);
    font-size: 12px;
    display: inline-flex;
    gap: 6px;
    align-items: center;
  }
  .caret {
    display: inline-block;
    transition: transform 0.15s;
  }
  .caret.rot {
    transform: rotate(90deg);
  }
  .think-body {
    font-size: 12.5px;
    color: var(--text-3);
    border-left: 2px solid var(--line-2);
    padding-left: 10px;
    white-space: pre-wrap;
  }
  .shimmer {
    animation: pulse 1.4s ease-in-out infinite;
  }
  .oral {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 14px;
    border-radius: 14px;
    background: linear-gradient(135deg, var(--agent-soft), var(--agent-2-soft));
    border: 1px solid var(--agent-line);
    color: var(--text);
  }
  .oral-btn {
    flex: none;
    color: var(--agent-2);
    margin-top: 2px;
  }
  .typing {
    display: flex;
    gap: 4px;
    padding: 6px 2px;
  }
  .typing span {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--agent-1);
    animation: pulse 1s infinite;
  }
  .typing span:nth-child(2) {
    animation-delay: 0.15s;
  }
  .typing span:nth-child(3) {
    animation-delay: 0.3s;
  }
  .error {
    color: var(--err);
    font-size: 13px;
  }
  .tool {
    width: min(100%, 720px);
    border: 1px solid var(--line);
    border-left: 3px solid var(--tc);
    background: var(--panel);
    border-radius: 10px;
    overflow: hidden;
  }
  .tool.error {
    border-color: var(--err-line);
  }
  .tool-head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 12px;
    text-align: left;
    font-size: 12.5px;
  }
  .tool-head:hover {
    background: var(--panel-2);
  }
  .tool-icon {
    color: var(--tc);
    width: 14px;
    text-align: center;
  }
  .tool.error .tool-icon {
    color: var(--err);
  }
  .tool-name {
    font-weight: 650;
    color: var(--tc);
  }
  .tool-arg {
    flex: 1;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tool-ms {
    color: var(--text-3);
    font-family: var(--mono);
    font-size: 11px;
  }
  .tool-link {
    font-size: 12px;
    color: var(--agent-2);
    padding: 0 12px 8px 36px;
  }
  .tool-out {
    margin: 0;
    max-height: 280px;
    overflow: auto;
    font-family: var(--mono);
    font-size: 11.5px;
    padding: 10px 12px;
    border-top: 1px solid var(--line);
    background: var(--bg-2);
    color: var(--text-2);
    white-space: pre-wrap;
  }
  .spinner {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid var(--line-2);
    border-top-color: var(--agent-1);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .notice {
    align-self: center;
    font-size: 12px;
    color: var(--text-3);
    padding: 3px 12px;
    border-radius: 999px;
    border: 1px solid var(--line);
  }
  .notice.warn {
    color: var(--warn);
    border-color: var(--warn-line);
  }
</style>
