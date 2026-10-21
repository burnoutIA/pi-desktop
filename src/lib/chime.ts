// Petit son de notification (WebAudio, aucun fichier) : un run en arrière-plan a fini, a planté
// ou attend ta réponse.

let ctx: AudioContext | null = null;

const NOTES: Record<"done" | "error" | "waiting", [number, number][]> = {
  // [fréquence Hz, départ s]
  done: [[660, 0], [880, 0.12]],
  waiting: [[880, 0], [880, 0.16]],
  error: [[440, 0], [330, 0.14]],
};

export function chime(kind: "done" | "error" | "waiting") {
  try {
    ctx ??= new AudioContext();
    const t0 = ctx.currentTime + 0.01;
    for (const [freq, at] of NOTES[kind]) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0, t0 + at);
      gain.gain.linearRampToValueAtTime(0.12, t0 + at + 0.015);
      gain.gain.exponentialRampToValueAtTime(0.0001, t0 + at + 0.32);
      osc.connect(gain).connect(ctx.destination);
      osc.start(t0 + at);
      osc.stop(t0 + at + 0.35);
    }
  } catch {
    /* audio indisponible : la notification visuelle suffit */
  }
}
