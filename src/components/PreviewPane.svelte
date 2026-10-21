<script lang="ts">
  // Aperçu : ce que pi montre (outil show_preview) ou un fichier ouvert depuis l'arbre.
  //  - adresse web (appli lancée sur localhost, site) : un vrai navigateur posé sur le panneau
  //    (browser.rs), qui affiche aussi les pages interdisant l'intégration ;
  //  - fichier local : page HTML servie par le serveur local dans un cadre, ou image.
  import { onMount } from "svelte";
  import { api, on } from "../lib/api";
  import type { Preview } from "../lib/preview";
  import { isImage } from "../lib/preview";

  type Props = {
    preview: Preview;
    /** Change quand des fichiers du projet changent : une page locale se recharge. */
    refreshKey: number;
    /** Chemin relatif au projet si le fichier en fait partie (bouton « Code »). */
    relPath: string | null;
    /** Une fenêtre de l'interface passe par-dessus : le navigateur (natif) se masque. */
    covered: boolean;
    onClose: () => void;
    onCode: (rel: string) => void;
  };
  let { preview, refreshKey, relPath, covered, onClose, onCode }: Props = $props();

  let reloads = $state(0);
  // Fichier local : rechargé quand le projet change (pi vient de le modifier). Une appli lancée
  // avec son serveur se recharge elle-même.
  let lastKey: number | null = null;
  $effect(() => {
    const k = refreshKey;
    if (lastKey !== null && k !== lastKey && preview.kind !== "url") reloads++;
    lastKey = k;
  });

  const image = $derived(preview.kind === "file" && isImage(preview.target));
  const label = $derived(
    preview.title ||
      (preview.kind === "url" ? preview.target.replace(/^https?:\/\//, "") : preview.target.split("/").pop() || preview.target),
  );
  const src = $derived(`${preview.url}${reloads ? `${preview.url.includes("?") ? "&" : "?"}r=${reloads}` : ""}`);

  // ---------- Navigateur (adresses web) ----------
  const web = $derived(preview.kind === "url");
  let slot = $state<HTMLDivElement | null>(null);
  /** Adresse réellement affichée (le navigateur suit liens et redirections). */
  let current = $state<string | null>(null);
  let last = "";

  const bounds = () => {
    const r = slot!.getBoundingClientRect();
    return { x: r.left, y: r.top, width: r.width, height: r.height };
  };

  // Suivre la place du panneau : redimensionnement, fenêtre agrandie, panneau voisin replié…
  function sync() {
    if (!web || !slot || covered) return;
    const b = bounds();
    const key = `${b.x}|${b.y}|${b.width}|${b.height}`;
    if (key === last) return;
    last = key;
    api.browserBounds(b).catch(() => {});
  }

  // Afficher / masquer selon l'adresse et ce qui passe par-dessus ; fermer pour un fichier local.
  $effect(() => {
    if (!web) {
      api.browserClose().catch(() => {});
      return;
    }
    if (!slot) return;
    const url = preview.url;
    if (covered) {
      api.browserHide().catch(() => {});
      return;
    }
    last = "";
    current = null;
    api.browserShow(url, bounds()).catch((e) => api.log(`navigateur de l'aperçu : ${e}`));
  });

  $effect(() => {
    if (!slot) return;
    const ro = new ResizeObserver(sync);
    ro.observe(slot);
    return () => ro.disconnect();
  });

  onMount(() => {
    const unlisten = on<string>("browser-url", (u) => {
      if (web) current = u;
    });
    window.addEventListener("resize", sync);
    const timer = setInterval(sync, 300);
    return () => {
      window.removeEventListener("resize", sync);
      clearInterval(timer);
      unlisten.then((f) => f());
      api.browserClose().catch(() => {});
    };
  });

  function reload() {
    if (web) api.browserReload().catch(() => {});
    else reloads++;
  }

  function openOutside() {
    const target = web ? (current ?? preview.target) : preview.target;
    if (target && target !== "about:blank") api.openExternal(target).catch(() => {});
  }
</script>

<div class="preview">
  <div class="head">
    <button class="icon" onclick={onClose} title="Fermer l'aperçu (Échap)" aria-label="Fermer l'aperçu">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"
        ><path d="M15 18l-6-6 6-6" /></svg
      >
    </button>
    <div class="title" title={preview.target}>
      <span class="tag">Aperçu</span>
      <span class="name">{label}</span>
      {#if web && current && current !== "about:blank" && current.replace(/\/$/, "") !== preview.target.replace(/\/$/, "")}
        <span class="where">{current.replace(/^https?:\/\//, "")}</span>
      {/if}
    </div>
    {#if relPath}
      <button class="text" onclick={() => onCode(relPath!)} title="Voir le code et le diff">Code</button>
    {/if}
    <button class="icon" onclick={reload} title="Recharger" aria-label="Recharger">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"
        ><path d="M21 12a9 9 0 1 1-2.6-6.4" /><path d="M21 3v6h-6" /></svg
      >
    </button>
    <button class="icon" onclick={openOutside} title="Ouvrir dans le navigateur" aria-label="Ouvrir dans le navigateur">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"
        ><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg
      >
    </button>
  </div>
  <div class="body">
    {#if web}
      <div class="slot" bind:this={slot}>
        {#if covered}<div class="covered">Aperçu masqué pendant que cette fenêtre est ouverte</div>{/if}
      </div>
    {:else if image}
      {#key src}
        <div class="img-wrap"><img {src} alt={label} /></div>
      {/key}
    {:else}
      {#key src}
        <iframe {src} title={label}></iframe>
      {/key}
    {/if}
  </div>
</div>

<style>
  .preview {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 12px 10px 14px;
    border-bottom: 1px solid var(--line);
  }
  .icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 7px;
    color: var(--text-2);
  }
  .icon:first-child {
    margin-left: -6px;
  }
  .icon:hover,
  .text:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .text {
    flex: none;
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .title {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .tag {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--agent-2);
  }
  .name {
    font-weight: 650;
    color: var(--text-strong);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .slot {
    flex: 1;
    display: grid;
    place-items: center;
  }
  .covered {
    color: var(--text-3);
    font-size: 12.5px;
  }
  .where {
    color: var(--text-3);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  iframe {
    flex: 1;
    border: 0;
    /* Une page sans fond défini s'affiche sur blanc, comme dans un navigateur. */
    background: #fff;
  }
  .img-wrap {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 16px;
    overflow: auto;
    background: repeating-conic-gradient(var(--panel-2) 0% 25%, transparent 0% 50%) 0 0 / 16px 16px;
  }
  img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: 6px;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.25);
  }
</style>
