<script lang="ts">
  // Zone de transcript : le texte dicté s'écrit ici en direct et reste modifiable avant l'envoi.
  // « / » ouvre l'autocomplétion des commandes (celles de pi et celles de l'app).
  import { suggest, type Command } from "../lib/commands";
  import type { Attachment } from "../lib/api";
  import { fileIcon } from "../lib/fileicon";

  type Props = {
    value: string;
    listening: boolean;
    busy: boolean;
    countdown: number | null; // 0..1 pendant l'envoi automatique
    onSend: () => void;
    onCancelAuto: () => void;
    onEscape: () => void;
    onAbort: () => void;
    agentName: string;
    commands: Command[];
    argOptions: Record<string, { value: string; label: string }[]>;
    attachments: Attachment[];
    onAttach: () => void;
    /** Modèle de cette session (indépendant des autres sessions). */
    models: { key: string; label: string }[];
    modelKey: string | null;
    /** Le modèle est servi sur cette machine (carte graphique). */
    modelLocal: boolean;
    /** Autres runs qui utilisent déjà ce modèle local. */
    modelShared: string[];
    onModel: (key: string) => void;
  };
  let {
    value = $bindable(),
    listening,
    busy,
    countdown,
    onSend,
    onCancelAuto,
    onEscape,
    onAbort,
    agentName,
    commands,
    argOptions,
    attachments = $bindable(),
    onAttach,
    models,
    modelKey,
    modelLocal,
    modelShared,
    onModel,
  }: Props = $props();

  // Image collée (Ctrl+V) : jointe directement, sans passer par un fichier.
  function onPaste(e: ClipboardEvent) {
    for (const item of e.clipboardData?.items ?? []) {
      if (!item.type.startsWith("image/")) continue;
      const file = item.getAsFile();
      if (!file) continue;
      e.preventDefault();
      const reader = new FileReader();
      reader.onload = () => {
        const data = String(reader.result).split(",")[1] ?? "";
        const ext = item.type.split("/")[1] ?? "png";
        attachments = [...attachments, { name: `image collée.${ext}`, path: "", size: file.size, mime: item.type, data }];
      };
      reader.readAsDataURL(file);
    }
  }

  function remove(i: number) {
    attachments = attachments.filter((_, j) => j !== i);
  }

  function size(n: number) {
    return n < 1024 ? `${n} o` : n < 1024 * 1024 ? `${Math.round(n / 1024)} Ko` : `${(n / 1024 / 1024).toFixed(1)} Mo`;
  }

  let ta: HTMLTextAreaElement;
  let active = $state(0);
  let dismissed = $state<string | null>(null); // texte pour lequel l'utilisateur a fermé la liste (Échap)

  const suggestions = $derived(value === dismissed ? [] : suggest(value, commands, argOptions));
  $effect(() => {
    void suggestions.length;
    active = 0;
  });

  function accept(i = active) {
    const s = suggestions[i];
    if (!s) return;
    value = s.insert;
    ta?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (suggestions.length) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const n = suggestions.length;
        active = (active + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
        return;
      }
      // Tab complète toujours ; Entrée complète tant que la commande n'est pas encore écrite en entier.
      const exact = suggestions[active] && value.trimEnd() === suggestions[active].insert.trimEnd();
      if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey && !exact)) {
        e.preventDefault();
        accept();
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        dismissed = value;
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      onSend();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onEscape();
    } else {
      onCancelAuto();
    }
  }

  // Ajuste la hauteur au contenu et garde le curseur à la fin pendant la dictée.
  $effect(() => {
    void value;
    if (!ta) return;
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, 180) + "px";
    if (listening && document.activeElement !== ta) ta.scrollTop = ta.scrollHeight;
  });

  const placeholder = $derived(
    listening ? "Parle, le texte s'écrit ici…" : busy ? `Une précision pour ${agentName} pendant qu'il travaille ?` : `Écris ou dicte une demande à ${agentName}…`,
  );
</script>

<div class="composer" class:listening>
  {#if suggestions.length}
    <div class="suggest" role="listbox">
      {#each suggestions as s, i (s.insert)}
        <button
          class="sug"
          class:on={i === active}
          role="option"
          aria-selected={i === active}
          onmousedown={(e) => (e.preventDefault(), accept(i))}
          onmouseenter={() => (active = i)}
        >
          <span class="sug-name">{s.label}</span>
          {#if s.hint}<span class="sug-hint">{s.hint}</span>{/if}
          {#if s.description}<span class="sug-desc">{s.description}</span>{/if}
          {#if s.source}<span class="sug-src {s.source}">{{ app: "app", skill: "skill", prompt: "modèle", extension: "extension" }[s.source]}</span>{/if}
        </button>
      {/each}
      <div class="sug-help"><kbd>↑↓</kbd> choisir · <kbd>Tab</kbd> compléter · <kbd>Échap</kbd> fermer</div>
    </div>
  {/if}
  {#if countdown !== null}
    <div class="auto">
      <div class="bar" style="transform:scaleX({countdown})"></div>
    </div>
  {/if}
  <button class="clip" onclick={onAttach} title="Joindre des fichiers ou des images (ou glisse-les ici, ou Ctrl+V)" aria-label="Joindre">
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
      ><path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" /></svg
    >
  </button>
  <div class="input-col">
    {#if attachments.length}
      <div class="atts">
        {#each attachments as a, i (a.name + i)}
          <div class="att" title={a.path || a.name}>
            {#if a.data && a.mime}
              <img src="data:{a.mime};base64,{a.data}" alt={a.name} />
            {:else}
              <span class="att-icon" style="color:{fileIcon(a.name).color}">{fileIcon(a.name).label}</span>
            {/if}
            <span class="att-name">{a.name}</span>
            <span class="att-size">{size(a.size)}</span>
            <button class="att-x" onclick={() => remove(i)} aria-label="Retirer">×</button>
          </div>
        {/each}
      </div>
    {/if}
    <textarea
      class="selectable"
      bind:this={ta}
      bind:value
      rows="1"
      {placeholder}
      onkeydown={onKey}
      onpointerdown={onCancelAuto}
      onpaste={onPaste}
    ></textarea>
    {#if models.length}
      <div class="meta">
        <label class="model" title="Modèle de cette session : les autres sessions gardent le leur. Il s'applique au prochain tour.">
          {#if modelKey}<span class="chip" class:local={modelLocal}>{modelLocal ? "local" : "cloud"}</span>{/if}
          <select value={modelKey} onchange={(e) => onModel((e.target as HTMLSelectElement).value)}>
            {#if !modelKey}<option value="" disabled selected>Modèle…</option>{/if}
            {#each models as m (m.key)}
              <option value={m.key}>{m.label}</option>
            {/each}
          </select>
        </label>
        {#if modelShared.length}
          <span class="shared" title="Deux runs sur le même modèle local se partagent la carte graphique : ils ralentissent tous les deux.">
            Déjà utilisé par {modelShared.join(", ")} : la carte graphique sera partagée.
          </span>
        {/if}
      </div>
    {/if}
  </div>
  <div class="actions">
    {#if countdown !== null}
      <button class="ghost" onclick={onCancelAuto}>Corriger</button>
    {/if}
    {#if busy}
      <button class="ghost stop" onclick={onAbort} title="Arrêter (Ctrl+.)">Arrêter</button>
    {/if}
    <button class="send" disabled={!value.trim() && !attachments.length} onclick={onSend} title={busy ? "Envoyer comme consigne en cours de route" : "Envoyer"}>
      {busy ? "Réorienter" : "Envoyer"}
      <kbd>↵</kbd>
    </button>
  </div>
</div>

<style>
  .composer {
    position: relative;
    margin: 0 18px 16px;
    padding: 10px 10px 10px 16px;
    display: flex;
    align-items: flex-end;
    gap: 10px;
    background: var(--panel);
    border: 1px solid var(--line-2);
    border-radius: 16px;
    box-shadow: 0 12px 40px var(--shadow);
    transition: border-color 0.2s;
  }
  .suggest {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(100% + 8px);
    z-index: 10;
    max-height: 340px;
    overflow-y: auto;
    padding: 6px;
    border-radius: 12px;
    background: var(--panel);
    border: 1px solid var(--line-2);
    box-shadow: 0 16px 40px var(--shadow);
  }
  .sug {
    width: 100%;
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 6px 10px;
    border-radius: 8px;
    text-align: left;
  }
  .sug.on {
    background: var(--agent-soft);
  }
  .sug-name {
    font-family: var(--mono);
    font-size: 12.5px;
    font-weight: 650;
    color: var(--agent-1);
    white-space: nowrap;
  }
  .sug-hint {
    font-family: var(--mono);
    font-size: 11.5px;
    color: var(--text-3);
    white-space: nowrap;
  }
  .sug-desc {
    flex: 1;
    min-width: 0;
    font-size: 12.5px;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sug-src {
    flex: none;
    font-size: 10.5px;
    padding: 1px 7px;
    border-radius: 999px;
    color: var(--text-3);
    border: 1px solid var(--line-2);
  }
  .sug-src.skill {
    color: var(--tool-search);
    border-color: color-mix(in srgb, var(--tool-search) 40%, transparent);
  }
  .sug-src.prompt {
    color: var(--tool-read);
    border-color: color-mix(in srgb, var(--tool-read) 40%, transparent);
  }
  .sug-src.extension {
    color: var(--tool-bash);
    border-color: color-mix(in srgb, var(--tool-bash) 40%, transparent);
  }
  .sug-help {
    padding: 6px 10px 2px;
    font-size: 11px;
    color: var(--text-3);
    border-top: 1px solid var(--line);
    margin-top: 4px;
  }
  .composer:focus-within {
    border-color: var(--agent-1);
  }
  .composer.listening {
    border-color: var(--user-2);
  }
  .auto {
    position: absolute;
    left: 14px;
    right: 14px;
    top: -1px;
    height: 2px;
    overflow: hidden;
    border-radius: 2px;
  }
  .bar {
    height: 100%;
    background: linear-gradient(90deg, var(--user-1), var(--user-2));
    transform-origin: left;
  }
  .clip {
    flex: none;
    align-self: flex-end;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    margin-left: -8px;
    border-radius: 9px;
    color: var(--text-3);
  }
  .clip:hover {
    background: var(--panel-2);
    color: var(--text-strong);
  }
  .input-col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 11.5px;
    color: var(--text-3);
  }
  .model {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .model select {
    max-width: 260px;
    padding: 1px 4px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text-2);
    font-size: 11.5px;
    cursor: pointer;
  }
  .model select:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .chip {
    padding: 0 6px;
    border-radius: 999px;
    border: 1px solid var(--line-2);
    font-size: 10px;
    line-height: 15px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .chip.local {
    border-color: var(--agent-1);
    color: var(--agent-2);
  }
  .shared {
    color: var(--modified);
  }
  .atts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding-top: 2px;
  }
  .att {
    display: flex;
    align-items: center;
    gap: 7px;
    max-width: 260px;
    padding: 4px 6px 4px 4px;
    border-radius: 9px;
    background: var(--panel-2);
    border: 1px solid var(--line);
    font-size: 12px;
  }
  .att img {
    width: 30px;
    height: 30px;
    object-fit: cover;
    border-radius: 6px;
  }
  .att-icon {
    width: 30px;
    text-align: center;
    font-family: var(--mono);
    font-size: 11px;
    font-weight: 800;
  }
  .att-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .att-size {
    flex: none;
    color: var(--text-3);
    font-size: 11px;
  }
  .att-x {
    flex: none;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    color: var(--text-3);
    line-height: 1;
  }
  .att-x:hover {
    background: var(--line-2);
    color: var(--text-strong);
  }
  textarea {
    flex: 1;
    resize: none;
    border: 0;
    outline: 0;
    background: transparent;
    padding: 6px 0;
    line-height: 1.5;
    max-height: 180px;
    font-size: 14.5px;
  }
  textarea::placeholder {
    color: var(--text-3);
  }
  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .ghost {
    padding: 6px 10px;
    border-radius: 9px;
    color: var(--text-2);
    font-size: 13px;
  }
  .ghost:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .stop:hover {
    color: var(--err);
  }
  .send {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 7px 14px;
    border-radius: 10px;
    background: var(--accent-grad);
    color: var(--on-accent);
    font-weight: 600;
    font-size: 13px;
  }
  .send:disabled {
    background: var(--panel-2);
    color: var(--text-3);
    cursor: default;
  }
  kbd {
    font-family: var(--mono);
    font-size: 11px;
    opacity: 0.75;
  }
</style>
