// Petite étiquette colorée par type de fichier, à la manière des icônes de VS Code.
export type FileIcon = { label: string; color: string };

const MAP: Record<string, FileIcon> = {
  html: { label: "<>", color: "var(--ft-html)" },
  htm: { label: "<>", color: "var(--ft-html)" },
  css: { label: "#", color: "var(--ft-css)" },
  scss: { label: "#", color: "var(--ft-css)" },
  js: { label: "JS", color: "var(--ft-js)" },
  mjs: { label: "JS", color: "var(--ft-js)" },
  cjs: { label: "JS", color: "var(--ft-js)" },
  jsx: { label: "JS", color: "var(--ft-js)" },
  ts: { label: "TS", color: "var(--ft-ts)" },
  tsx: { label: "TS", color: "var(--ft-ts)" },
  json: { label: "{}", color: "var(--ft-json)" },
  md: { label: "M↓", color: "var(--ft-md)" },
  rs: { label: "RS", color: "var(--ft-rs)" },
  py: { label: "PY", color: "var(--ft-py)" },
  svelte: { label: "S", color: "var(--ft-svelte)" },
  vue: { label: "V", color: "var(--ft-sh)" },
  sh: { label: "$", color: "var(--ft-sh)" },
  toml: { label: "⚙", color: "var(--ft-cfg)" },
  yml: { label: "⚙", color: "var(--ft-cfg)" },
  yaml: { label: "⚙", color: "var(--ft-cfg)" },
  lock: { label: "🔒", color: "var(--ft-cfg)" },
  svg: { label: "◇", color: "var(--ft-html)" },
  png: { label: "▣", color: "var(--ft-md)" },
  jpg: { label: "▣", color: "var(--ft-md)" },
};

export function fileIcon(path: string): FileIcon {
  const name = path.split("/").pop() ?? "";
  if (name.startsWith(".git")) return { label: "◆", color: "var(--ft-html)" };
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  return MAP[ext] ?? { label: "·", color: "var(--text-3)" };
}
