<script lang="ts">
  // Fond d'ambiance du mode zen : de grandes nappes douces aux couleurs de la palette, qui
  // dérivent lentement et « respirent » avec ta voix (energy : 0 = silence, 1 = voix forte).
  //
  // Performances (WebKitGTK dessine souvent sans carte graphique) : pas de `filter: blur`
  // (mesuré : 16 img/s avec, la douceur vient déjà des dégradés radiaux), pas d'agrandissement
  // lié à la voix, et une respiration arrondie au 1/20e pour ne pas redessiner à chaque message.
  type Props = { energy: number; mood: "user" | "calm" };
  let { energy, mood }: Props = $props();

  let level = $state(0);
  let smooth = 0;
  $effect(() => {
    const target = Math.min(1, energy);
    smooth += (target - smooth) * (target > smooth ? 0.3 : 0.06);
    const q = Math.round(smooth * 20) / 20;
    if (q !== level) level = q;
  });
</script>

<div class="ambient {mood}" style="--e:{level}" aria-hidden="true">
  <div class="blob b1"></div>
  <div class="blob b2"></div>
  <div class="blob b3"></div>
</div>

<style>
  .ambient {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
    z-index: 0;
    isolation: isolate;
  }
  .blob {
    position: absolute;
    width: 70vmax;
    height: 70vmax;
    border-radius: 50%;
    opacity: calc(0.14 + var(--e) * 0.2);
    transition: opacity 0.35s ease;
  }
  .b1 {
    left: -24vmax;
    top: -28vmax;
    background: radial-gradient(closest-side, var(--agent-1), color-mix(in srgb, var(--agent-1) 35%, transparent) 45%, transparent);
    animation: drift1 38s ease-in-out infinite alternate;
  }
  .b2 {
    right: -26vmax;
    bottom: -32vmax;
    background: radial-gradient(closest-side, var(--agent-2), color-mix(in srgb, var(--agent-2) 35%, transparent) 45%, transparent);
    animation: drift2 46s ease-in-out infinite alternate;
  }
  .b3 {
    left: 22%;
    top: 26%;
    width: 50vmax;
    height: 50vmax;
    background: radial-gradient(closest-side, var(--user-1), transparent);
    opacity: calc(0.06 + var(--e) * 0.1);
    animation: drift3 54s ease-in-out infinite alternate;
  }
  /* Quand tu parles, ta couleur prend le dessus. */
  .user .b3 {
    opacity: calc(0.12 + var(--e) * 0.26);
  }
  .user .b1 {
    background: radial-gradient(closest-side, var(--user-2), color-mix(in srgb, var(--user-2) 35%, transparent) 45%, transparent);
  }
  .calm .blob {
    opacity: 0.1;
  }
  @keyframes drift1 {
    to {
      transform: translate(22vmax, 14vmax);
    }
  }
  @keyframes drift2 {
    to {
      transform: translate(-18vmax, -20vmax);
    }
  }
  @keyframes drift3 {
    0% {
      transform: translate(-10vmax, 6vmax);
    }
    100% {
      transform: translate(14vmax, -10vmax);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .blob {
      animation: none;
      transition: none;
    }
  }
</style>
