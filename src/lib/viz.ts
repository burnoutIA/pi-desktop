// Styles de visualisation de la voix (Réglages → Apparence).
export type VizStyle = "bars" | "wave" | "spectrum";

export const VIZ_STYLES: { id: VizStyle; label: string; description: string }[] = [
  { id: "bars", label: "Barres", description: "Une barre par fréquence, en miroir" },
  { id: "wave", label: "Onde", description: "La forme d'onde en trait fin lumineux" },
  { id: "spectrum", label: "Spectre", description: "Une courbe lisse et pleine" },
];

export function loadViz(): VizStyle {
  const v = localStorage.getItem("vizStyle");
  return v === "wave" || v === "spectrum" ? v : "bars";
}

export function saveViz(v: VizStyle) {
  localStorage.setItem("vizStyle", v);
  window.dispatchEvent(new CustomEvent("vizchange", { detail: v }));
}
