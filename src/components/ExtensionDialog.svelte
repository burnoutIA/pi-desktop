<script lang="ts">
  // Dialogue demandé par une extension de pi (choix, confirmation, saisie). Sans réponse,
  // pi resterait bloqué : Échap ou le délai annulent proprement.
  import { onMount, untrack } from "svelte";

  type Request = {
    id: string;
    method: "select" | "confirm" | "input" | "editor";
    title?: string;
    message?: string;
    options?: string[];
    placeholder?: string;
    prefill?: string;
    timeout?: number;
  };
  type Props = { req: Request; onRespond: (r: Record<string, unknown>) => void };
  let { req, onRespond }: Props = $props();

  // Le dialogue est recréé pour chaque demande : on part de son texte pré-rempli.
  let text = $state(untrack(() => req.prefill ?? ""));
  let active = $state(0);
  let input: HTMLInputElement | HTMLTextAreaElement | undefined = $state();

  const cancel = () => onRespond({ cancelled: true });

  onMount(() => {
    input?.focus();
    const t = req.timeout ? setTimeout(cancel, req.timeout) : undefined;
    return () => clearTimeout(t);
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (req.method === "select" && req.options?.length) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const n = req.options.length;
        active = (active + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
      } else if (e.key === "Enter") {
        e.preventDefault();
        onRespond({ value: req.options[active] });
      }
    } else if (req.method === "input" && e.key === "Enter") {
      e.preventDefault();
      onRespond({ value: text });
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="veil">
  <div class="dialog" role="dialog" aria-label={req.title ?? "Extension"}>
    <div class="from">Extension de pi</div>
    {#if req.title}<div class="title">{req.title}</div>{/if}
    {#if req.message}<div class="msg">{req.message}</div>{/if}

    {#if req.method === "select"}
      <div class="options">
        {#each req.options ?? [] as o, i (o)}
          <button class="opt" class:on={i === active} onmouseenter={() => (active = i)} onclick={() => onRespond({ value: o })}>
            {o}
          </button>
        {/each}
      </div>
    {:else if req.method === "input"}
      <input bind:this={input} bind:value={text} placeholder={req.placeholder ?? ""} />
    {:else if req.method === "editor"}
      <textarea bind:this={input} bind:value={text} rows="10"></textarea>
    {/if}

    <div class="actions">
      <button class="ghost" onclick={cancel}>Annuler</button>
      {#if req.method === "confirm"}
        <button class="ghost" onclick={() => onRespond({ confirmed: false })}>Non</button>
        <button class="primary" onclick={() => onRespond({ confirmed: true })}>Oui</button>
      {:else if req.method === "input" || req.method === "editor"}
        <button class="primary" onclick={() => onRespond({ value: text })}>Valider</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 55%, transparent);
    backdrop-filter: blur(2px);
  }
  .dialog {
    width: min(520px, calc(100vw - 32px));
    max-height: 80vh;
    overflow: auto;
    padding: 18px;
    border-radius: 14px;
    background: var(--panel);
    border: 1px solid var(--line-2);
    box-shadow: 0 24px 70px var(--shadow);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .from {
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--tool-bash);
    font-weight: 650;
  }
  .title {
    font-size: 15px;
    font-weight: 650;
    color: var(--text-strong);
  }
  .msg {
    color: var(--text-2);
    white-space: pre-wrap;
    user-select: text;
  }
  .options {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .opt {
    text-align: left;
    padding: 8px 10px;
    border-radius: 8px;
    color: var(--text);
  }
  .opt.on {
    background: var(--agent-soft);
    color: var(--text-strong);
  }
  input,
  textarea {
    background: var(--bg-2);
    border: 1px solid var(--line-2);
    border-radius: 8px;
    padding: 8px 10px;
    outline: 0;
    user-select: text;
    font-family: var(--mono);
    font-size: 12.5px;
    resize: vertical;
  }
  input:focus,
  textarea:focus {
    border-color: var(--agent-1);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
  .ghost {
    padding: 6px 12px;
    border-radius: 8px;
    color: var(--text-2);
  }
  .ghost:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .primary {
    padding: 6px 14px;
    border-radius: 8px;
    background: var(--accent-grad);
    color: var(--on-accent);
    font-weight: 600;
  }
</style>
