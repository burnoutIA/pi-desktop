<script lang="ts">
  // Les dots : agents pi autonomes. Chacun est un dossier (data/dots/<nom>/ pour ceux de l'app) et
  // tourne dans son conteneur. Un clic s'y branche (sa conversation s'affiche, comme une session) ;
  // on peut aussi en créer, les allumer, les éteindre, les supprimer.
  import type { DotAction, DotInfo } from "../lib/api";

  type Props = {
    dots: DotInfo[];
    /** Script `dot` trouvé : sinon le panneau demande son chemin. */
    cli: string | null;
    /** Dossiers des dots auxquels l'app est branchée. */
    connected: string[];
    /** Action en cours par dossier (« allumage… ») ; `new:<nom>` pour une création. */
    pending: Record<string, string>;
    /** Dernière erreur par dossier, affichée sous le nom. */
    errors: Record<string, string>;
    onConnect: (d: DotInfo) => void;
    onAction: (action: DotAction, d: DotInfo) => void;
    onCreate: (name: string, from: string | null) => Promise<boolean>;
    onOpen: (path: string) => void;
    onSetCli: (path: string) => void;
    onRefresh: () => void;
  };
  let { dots, cli, connected, pending, errors, onConnect, onAction, onCreate, onOpen, onSetCli, onRefresh }: Props = $props();

  let open = $state(localStorage.getItem("dotsOpen") !== "0");
  $effect(() => localStorage.setItem("dotsOpen", open ? "1" : "0"));

  const mine = $derived(dots.filter((d) => d.inApp));
  const elsewhere = $derived(dots.filter((d) => !d.inApp));

  let creating = $state(false);
  let newName = $state("");
  /** Dossier du dot à copier (vierge), ou "" : un fichier .dot à remplir. */
  let newFrom = $state<string | null>(null);
  const fromValue = $derived(newFrom ?? mine[0]?.dir ?? dots[0]?.dir ?? "");
  let cliPath = $state("");
  let menu = $state<string | null>(null);
  /** Position du menu, sous son bouton : en position fixe, la liste (qui défile) ne le coupe pas. */
  let menuAt = $state({ top: 0, right: 0 });
  let confirmRemove = $state<string | null>(null);

  const validName = $derived(/^[a-z][a-z0-9-]{0,30}$/.test(newName));
  const taken = $derived(mine.some((d) => d.name === newName));

  async function create() {
    if (!validName || taken) return;
    if (await onCreate(newName, fromValue || null)) {
      creating = false;
      newName = "";
      newFrom = null;
    }
  }

  function toggleMenu(e: MouseEvent, d: DotInfo) {
    e.stopPropagation();
    if (menu === d.dir) return void (menu = null);
    const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuAt = { top: b.bottom + 4, right: window.innerWidth - b.right };
    menu = d.dir;
  }

  function act(action: DotAction, d: DotInfo) {
    menu = null;
    if (action === "remove" && confirmRemove !== d.dir) {
      confirmRemove = d.dir;
      setTimeout(() => confirmRemove === d.dir && (confirmRemove = null), 4000);
      return;
    }
    confirmRemove = null;
    onAction(action, d);
  }

  function stateLabel(d: DotInfo) {
    if (pending[d.dir]) return pending[d.dir];
    if (!d.running) return "éteint";
    if (d.interface !== "rpc") return "mode terminal";
    return connected.includes(d.dir) ? "branché" : "allumé";
  }
</script>

<svelte:window onclick={() => (menu = null)} onresize={() => (menu = null)} />

{#snippet row(d: DotInfo)}
  <div class="item" class:current={connected.includes(d.dir)}>
    <button class="open" onclick={() => onConnect(d)} disabled={!!pending[d.dir]} title={d.description || d.dir}>
      <span class="led" class:on={d.running} class:busy={!!pending[d.dir]} class:tui={d.running && d.interface !== "rpc"}></span>
      <span class="text">
        <span class="line">
          <span class="name">{d.name}</span>
          <span class="st">{stateLabel(d)}</span>
        </span>
        {#if d.description}<span class="desc">{d.description}</span>{/if}
      </span>
    </button>
    {#if confirmRemove === d.dir}
      <button class="x sure" onclick={() => act("remove", d)}>Supprimer ?</button>
    {:else}
      <button class="x" onclick={(e) => toggleMenu(e, d)} title="Actions" aria-label="Actions du dot" disabled={!!pending[d.dir]}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"
          ><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></svg
        >
      </button>
    {/if}
    {#if menu === d.dir}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="menu" style:top="{menuAt.top}px" style:right="{menuAt.right}px" onclick={(e) => e.stopPropagation()}>
        {#if d.running}
          <button onclick={() => act("stop", d)}>Éteindre</button>
        {:else}
          <button onclick={() => act("start", d)}>Allumer</button>
        {/if}
        {#if d.interface !== "rpc"}
          <button onclick={() => act("use-rpc", d)} title="DOT_INTERFACE: rpc, nécessaire pour lui parler depuis l'app"
            >Passer en mode RPC{d.running ? " (le rallume)" : ""}</button
          >
        {/if}
        {#if !d.inApp}
          <button onclick={() => act("import", d)} title="Déplace son dossier dans data/dots/ de l'app"
            >Ranger dans l'app{d.running ? " (le rallume)" : ""}</button
          >
        {/if}
        <button onclick={() => ((menu = null), onOpen(`${d.dir}/state/livrables`))}>Ses livrables</button>
        <button onclick={() => ((menu = null), onOpen(d.dir))}>Ouvrir son dossier</button>
        <button onclick={() => ((menu = null), onOpen(d.file))} title="Description, réglages, providers et secrets (rallumer pour appliquer)"
          >Modifier sa configuration</button
        >
        <button class="danger" onclick={() => act("remove", d)}>Supprimer…</button>
      </div>
    {/if}
  </div>
  {#if errors[d.dir] && !pending[d.dir]}<small class="err" title={errors[d.dir]}>{errors[d.dir]}</small>{/if}
{/snippet}

<div class="dots">
  <div class="head">
    <button class="title" onclick={() => (open = !open)} aria-expanded={open}>
      <svg class="chev" class:open width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"
        ><path d="M9 6l6 6-6 6" /></svg
      >
      Dots
    </button>
    {#if open && cli}
      <div class="tools">
        <button class="mini" onclick={onRefresh} title="Actualiser la liste" aria-label="Actualiser">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
            ><path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6" /></svg
          >
        </button>
        <button class="mini" class:on={creating} onclick={() => (creating = !creating)} title="Créer un dot" aria-label="Créer un dot">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"
            ><path d="M12 5v14M5 12h14" /></svg
          >
        </button>
      </div>
    {/if}
  </div>

  {#if open}
    {#if !cli}
      <div class="setup">
        <small>Script <code>dot</code> introuvable. Indique son chemin (dépôt dots) :</small>
        <div class="row">
          <input bind:value={cliPath} placeholder="/chemin/vers/dots/dot" spellcheck="false" />
          <button class="ok" disabled={!cliPath.trim()} onclick={() => onSetCli(cliPath.trim())}>OK</button>
        </div>
      </div>
    {:else}
      {#if creating}
        <form class="create" onsubmit={(e) => (e.preventDefault(), create())}>
          <!-- svelte-ignore a11y_autofocus -->
          <input bind:value={newName} placeholder="nom (minuscules, chiffres, tirets)" spellcheck="false" autofocus />
          <div class="row">
            <select value={fromValue} onchange={(e) => (newFrom = (e.target as HTMLSelectElement).value)} title="Point de départ">
              {#each dots as d (d.dir)}<option value={d.dir}>à partir de « {d.name} »</option>{/each}
              <option value="">vide (à remplir)</option>
            </select>
            <button class="ok" type="submit" disabled={!validName || taken || !!pending[`new:${newName}`]}>
              {pending[`new:${newName}`] ? "…" : "Créer"}
            </button>
          </div>
          {#if newName && !validName}<small class="warn">Minuscules, chiffres et tirets ; commence par une lettre.</small>
          {:else if taken}<small class="warn">Ce nom existe déjà.</small>
          {:else}<small class="hint">{fromValue ? "Mêmes accès, modèle et savoir-faire ; ni mémoire, ni Telegram." : "Il faudra remplir son modèle et ses secrets avant de l'allumer."}</small>{/if}
        </form>
      {/if}

      <div class="list">
        {#each mine as d (d.dir)}
          {@render row(d)}
        {:else}
          {#if !elsewhere.length}<small class="empty">Aucun dot. Crée-en un avec +, ou demande-le à l'agent.</small>{/if}
        {/each}
        {#if elsewhere.length}
          <div class="sub" title="Dots connus du script dot, rangés ailleurs sur le disque">Ailleurs</div>
          {#each elsewhere as d (d.dir)}
            {@render row(d)}
          {/each}
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .dots {
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
    padding: 12px 10px 6px 10px;
  }
  .title {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-3);
    font-weight: 650;
    padding: 0 4px;
  }
  .chev {
    transition: transform 0.15s;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .tools {
    display: flex;
    gap: 2px;
  }
  .mini {
    display: grid;
    place-items: center;
    width: 24px;
    height: 22px;
    border-radius: 6px;
    color: var(--text-3);
  }
  .mini:hover,
  .mini.on {
    background: var(--panel-2);
    color: var(--text);
  }
  .setup,
  .create {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 2px 12px 8px 14px;
  }
  .setup small,
  .empty {
    color: var(--text-3);
    font-size: 12px;
  }
  .empty {
    padding: 2px 14px 6px;
  }
  .warn {
    color: var(--del-fg);
    font-size: 11.5px;
  }
  .row {
    display: flex;
    gap: 6px;
  }
  input,
  select {
    flex: 1;
    min-width: 0;
    padding: 5px 8px;
    border-radius: 7px;
    border: 1px solid var(--line);
    background: var(--panel);
    color: var(--text);
    font-size: 12.5px;
  }
  .ok {
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 12px;
    background: var(--panel-2);
    color: var(--text);
  }
  .ok:disabled {
    opacity: 0.45;
  }
  .list {
    overflow-y: auto;
    padding: 0 6px;
  }
  .item {
    position: relative;
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
    align-items: flex-start;
    gap: 8px;
    padding: 6px 8px;
    text-align: left;
    font-size: 13px;
    color: var(--text-2);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .line {
    display: flex;
    align-items: baseline;
    gap: 6px;
  }
  .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .desc {
    font-size: 11.5px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    padding: 8px 8px 2px;
    font-size: 10.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .hint {
    color: var(--text-3);
    font-size: 11.5px;
  }
  .st {
    margin-left: auto;
    flex: none;
    font-size: 11px;
    color: var(--text-3);
  }
  .led {
    flex: none;
    margin-top: 5px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 1.5px solid var(--text-3);
  }
  .led.on {
    border: none;
    background: var(--add-fg);
  }
  .led.tui {
    background: var(--modified);
  }
  .led.busy {
    border: none;
    background: var(--agent-2);
    animation: pulse 1.1s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.4;
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
  .err {
    display: block;
    padding: 0 10px 6px 24px;
    font-size: 11.5px;
    line-height: 1.35;
    color: var(--del-fg);
  }
  .menu {
    position: fixed;
    z-index: 50;
    min-width: 190px;
    display: flex;
    flex-direction: column;
    padding: 4px;
    border-radius: 9px;
    border: 1px solid var(--line);
    background: var(--chrome);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
  }
  .menu button {
    text-align: left;
    padding: 6px 10px;
    border-radius: 6px;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .menu button:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .menu .danger {
    color: var(--del-fg);
  }
</style>
