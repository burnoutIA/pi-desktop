<script lang="ts">
  // Visualisation du micro, en trois styles :
  //  - « bars »     : une barre par bande de fréquence, en miroir (graves au centre) ;
  //  - « wave »     : la forme d'onde brute, en trait fin lumineux (esprit oscilloscope) ;
  //  - « spectrum » : le spectre en courbe lisse et pleine, en miroir.
  import { onMount } from "svelte";
  import type { VizStyle } from "../lib/viz";

  type Props = {
    user: number[];
    userActive: boolean;
    /** Forme d'onde récente (millièmes), pour le style « wave ». */
    userWave?: number[];
    style?: VizStyle;
  };
  let { user, userActive, userWave = [], style = "bars" }: Props = $props();

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;

  onMount(() => {
    const ctx = canvas.getContext("2d")!;
    let raf = 0;
    let shownU: number[] = [];
    let t0 = performance.now();

    // Couleurs relues à chaque changement de thème (palette ou mode clair / sombre).
    let palette: Record<string, string> = {};
    const readColors = () => {
      const st = getComputedStyle(document.documentElement);
      for (const n of ["--user-1", "--user-2", "--eq-idle"]) palette[n] = st.getPropertyValue(n).trim();
      palette.mode = document.documentElement.dataset.mode ?? "dark";
    };
    readColors();
    window.addEventListener("themechange", readColors);
    const col = (name: string) => palette[name];

    const resize = () => {
      const r = wrap.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.round(r.width * dpr);
      canvas.height = Math.round(r.height * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    const ro = new ResizeObserver(resize);
    ro.observe(wrap);
    resize();

    const ease = (shown: number[], target: number[], active: boolean) => {
      const n = target.length;
      if (shown.length !== n) shown = new Array(n).fill(0);
      for (let i = 0; i < n; i++) {
        const v = active ? target[i] : 0;
        // Montée rapide, retombée douce : ça « respire » comme une voix.
        const k = v > shown[i] ? 0.5 : 0.14;
        shown[i] += (v - shown[i]) * k;
      }
      return shown;
    };

    // ---------- Style « wave » : la forme d'onde, en trait fin lumineux ----------
    let shownWU: number[] = [];
    /** Lissage d'une image à l'autre (les niveaux arrivent à ~45/s, l'écran à 60/s). */
    const easeWave = (shown: number[], target: number[], active: boolean) => {
      const n = target.length || shown.length;
      if (shown.length !== n) shown = new Array(n).fill(0);
      for (let i = 0; i < n; i++) {
        const v = active && target.length ? target[i] / 1000 : 0;
        shown[i] += (v - shown[i]) * 0.55;
      }
      return shown;
    };
    const quiet = (a: number[]) => a.every((v) => Math.abs(v) < 0.002);
    const drawWave = (vals: number[], w: number, h: number, color: string, glow: string, alpha: number) => {
      const mid = h / 2;
      const n = vals.length;
      ctx.beginPath();
      for (let i = 0; i < n; i++) {
        // Gain + saturation douce : la voix remplit bien la hauteur sans écrêter net.
        const y = mid - Math.tanh(vals[i] * 2.4) * (h / 2 - 6);
        const x = (i / (n - 1)) * w;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.lineWidth = 1.3;
      ctx.lineJoin = "round";
      ctx.strokeStyle = color;
      ctx.globalAlpha = alpha;
      ctx.shadowColor = glow;
      ctx.shadowBlur = palette.mode === "light" ? 4 : 10;
      ctx.shadowOffsetY = 3;
      ctx.stroke();
      ctx.shadowBlur = 0;
      ctx.shadowOffsetY = 0;
      ctx.globalAlpha = 1;
    };
    const drawFlat = (w: number, h: number) => {
      ctx.beginPath();
      ctx.moveTo(0, h / 2);
      ctx.lineTo(w, h / 2);
      ctx.lineWidth = 1;
      ctx.strokeStyle = col("--eq-idle");
      ctx.globalAlpha = 0.55;
      ctx.stroke();
      ctx.globalAlpha = 1;
    };

    // ---------- Style « spectrum » : courbe lisse et pleine, en miroir ----------
    const drawSpectrum = (vals: number[], w: number, h: number, c1: string, c2: string, alpha: number) => {
      const n = vals.length;
      if (!n) return;
      // Graves au centre, aigus vers les bords (comme les barres).
      const pts: number[] = [];
      for (let j = 0; j < n * 2; j++) pts.push(vals[j < n ? n - 1 - j : j - n]);
      const mid = h / 2;
      const amp = (v: number) => Math.max(1.5, v * (h / 2 - 4));
      const path = (sign: 1 | -1) => {
        ctx.moveTo(0, mid);
        for (let j = 0; j < pts.length; j++) {
          const x0 = (j / (pts.length - 1)) * w;
          const x1 = ((j + 1) / (pts.length - 1)) * w;
          const y0 = mid - sign * amp(pts[j]);
          const y1 = mid - sign * amp(pts[Math.min(j + 1, pts.length - 1)]);
          ctx.quadraticCurveTo(x0, y0, (x0 + x1) / 2, (y0 + y1) / 2);
        }
        ctx.lineTo(w, mid);
      };
      const grad = ctx.createLinearGradient(0, 0, w, 0);
      grad.addColorStop(0, c2);
      grad.addColorStop(0.5, c1);
      grad.addColorStop(1, c2);
      ctx.globalAlpha = alpha * 0.28;
      ctx.fillStyle = grad;
      ctx.beginPath();
      path(1);
      path(-1);
      ctx.fill();
      ctx.globalAlpha = alpha;
      ctx.strokeStyle = grad;
      ctx.lineWidth = 2;
      ctx.shadowColor = c1;
      ctx.shadowBlur = palette.mode === "light" ? 4 : 12;
      for (const sign of [1, -1] as const) {
        ctx.beginPath();
        path(sign);
        ctx.stroke();
      }
      ctx.shadowBlur = 0;
      ctx.globalAlpha = 1;
    };

    const drawLayer = (vals: number[], w: number, h: number, c1: string, c2: string, alpha: number) => {
      const n = vals.length;
      if (!n) return;
      const total = n * 2;
      const gap = Math.max(3, w / total / 2.4);
      const bw = Math.max(2, (w - gap * (total - 1)) / total);
      const mid = h / 2;
      const grad = ctx.createLinearGradient(0, 0, w, 0);
      grad.addColorStop(0, c2);
      grad.addColorStop(0.5, c1);
      grad.addColorStop(1, c2);
      ctx.fillStyle = grad;
      ctx.globalAlpha = alpha;
      ctx.shadowColor = c1;
      ctx.shadowBlur = palette.mode === "light" ? 6 : 18; // un halo fort bave sur fond clair
      for (let j = 0; j < total; j++) {
        // j < n : moitié gauche (aigus -> graves), sinon moitié droite (graves -> aigus)
        const band = j < n ? n - 1 - j : j - n;
        const v = vals[band];
        const bh = Math.max(bw, v * (h - 8));
        const x = j * (bw + gap);
        ctx.beginPath();
        ctx.roundRect(x, mid - bh / 2, bw, bh, bw / 2);
        ctx.fill();
      }
      ctx.shadowBlur = 0;
      ctx.globalAlpha = 1;
    };

    // Mesure (banc d'essai) : intervalle entre images et temps de dessin.
    const stats = ((window as any).__eqStats ??= { gaps: [] as number[], draws: [] as number[], live: 0 });
    stats.live++;
    let last = 0;
    const frame = (now: number) => {
      const t0draw = performance.now();
      if (last) stats.gaps.push(now - last);
      last = now;
      const w = canvas.width / (window.devicePixelRatio || 1);
      const h = canvas.height / (window.devicePixelRatio || 1);
      ctx.clearRect(0, 0, w, h);
      if (style === "wave") {
        shownWU = easeWave(shownWU, userWave, userActive);
        if (quiet(shownWU)) drawFlat(w, h);
        else drawWave(shownWU, w, h, col("--user-1"), col("--user-2"), 0.75);
        stats.draws.push(performance.now() - t0draw);
        raf = requestAnimationFrame(frame);
        return;
      }
      shownU = ease(shownU, user, userActive);

      if (shownU.every((v) => v < 0.01)) {
        // Repos : ligne de points qui respire très légèrement.
        const n = Math.max(user.length, 40) * 2;
        const breathe = 0.35 + 0.15 * Math.sin((now - t0) / 900);
        ctx.fillStyle = col("--eq-idle");
        ctx.globalAlpha = breathe;
        const gap = w / n;
        for (let j = 0; j < n; j++) {
          ctx.beginPath();
          ctx.arc(gap * (j + 0.5), h / 2, 1.6, 0, Math.PI * 2);
          ctx.fill();
        }
        ctx.globalAlpha = 1;
      } else {
        const layer = style === "spectrum" ? drawSpectrum : drawLayer;
        layer(shownU, w, h, col("--user-1"), col("--user-2"), 1);
      }
      stats.draws.push(performance.now() - t0draw);
      if (stats.gaps.length > 2000) (stats.gaps.splice(0, 1000), stats.draws.splice(0, 1000));
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      stats.live--;
      ro.disconnect();
      window.removeEventListener("themechange", readColors);
    };
  });
</script>

<div class="eq" bind:this={wrap}>
  <canvas bind:this={canvas}></canvas>
</div>

<style>
  .eq {
    position: relative;
    width: 100%;
    height: 100%;
  }
  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
