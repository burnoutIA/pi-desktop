export type DiffLine = { kind: "add" | "del" | "ctx" | "hunk"; text: string; old?: number; new?: number; count?: number };

/** Transforme un diff unifié git en lignes affichables avec numéros. */
export function parseDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = [];
  let o = 0;
  let n = 0;
  let inHunk = false;
  for (const line of diff.split("\n")) {
    const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,(\d+))? @@(.*)$/.exec(line);
    if (m) {
      o = +m[1];
      n = +m[2];
      inHunk = true;
      // Au lieu de « @@ -12,7 +12,9 @@ » : la plage de lignes et le contexte (fonction…).
      out.push({ kind: "hunk", text: m[4].trim(), new: n, count: m[3] === undefined ? 1 : +m[3] });
      continue;
    }
    if (!inHunk) continue; // en-têtes diff --git / index / --- / +++
    if (line.startsWith("+")) out.push({ kind: "add", text: line.slice(1), new: n++ });
    else if (line.startsWith("-")) out.push({ kind: "del", text: line.slice(1), old: o++ });
    else if (line.startsWith("\\")) continue; // « No newline at end of file »
    else if (line.length || o || n) out.push({ kind: "ctx", text: line.slice(1), old: o++, new: n++ });
  }
  while (out.length && out[out.length - 1].kind === "ctx" && out[out.length - 1].text === "") out.pop();
  return out;
}

export function diffStats(lines: DiffLine[]) {
  let add = 0;
  let del = 0;
  for (const l of lines) {
    if (l.kind === "add") add++;
    else if (l.kind === "del") del++;
  }
  return { add, del };
}
