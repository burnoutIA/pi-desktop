// Coloration syntaxique (highlight.js, langages courants seulement pour garder l'app légère).
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import css from "highlight.js/lib/languages/css";
import go from "highlight.js/lib/languages/go";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import ini from "highlight.js/lib/languages/ini";
import sql from "highlight.js/lib/languages/sql";
import diff from "highlight.js/lib/languages/diff";

for (const [name, lang] of Object.entries({
  bash, css, go, javascript, json, markdown, python, rust, typescript, xml, yaml, ini, sql, diff,
})) {
  hljs.registerLanguage(name, lang);
}
hljs.registerAliases(["sh", "shell", "zsh", "console"], { languageName: "bash" });
hljs.registerAliases(["html", "svelte", "vue", "svg"], { languageName: "xml" });
hljs.registerAliases(["js", "jsx", "mjs", "cjs"], { languageName: "javascript" });
hljs.registerAliases(["ts", "tsx"], { languageName: "typescript" });
hljs.registerAliases(["py"], { languageName: "python" });
hljs.registerAliases(["rs"], { languageName: "rust" });
hljs.registerAliases(["yml"], { languageName: "yaml" });
hljs.registerAliases(["toml"], { languageName: "ini" });
hljs.registerAliases(["md"], { languageName: "markdown" });

const EXT: Record<string, string> = {
  sh: "bash", bash: "bash", zsh: "bash", css: "css", scss: "css", go: "go", js: "javascript", mjs: "javascript",
  cjs: "javascript", jsx: "javascript", json: "json", md: "markdown", py: "python", rs: "rust", ts: "typescript",
  tsx: "typescript", html: "xml", htm: "xml", svelte: "xml", vue: "xml", svg: "xml", xml: "xml", yml: "yaml",
  yaml: "yaml", toml: "ini", ini: "ini", sql: "sql",
};

export function langFromPath(path: string): string | null {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return EXT[ext] ?? null;
}

function escape(s: string) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** HTML coloré d'un bloc de code (échappé si le langage est inconnu). */
export function highlight(code: string, lang: string | null | undefined): string {
  const l = lang && hljs.getLanguage(lang) ? lang : null;
  if (!l) return escape(code);
  try {
    return hljs.highlight(code, { language: l, ignoreIllegals: true }).value;
  } catch {
    return escape(code);
  }
}

/**
 * Colore un fichier entier puis le découpe en lignes, en refermant / rouvrant les balises
 * à chaque saut de ligne (un commentaire ou une chaîne sur plusieurs lignes garde sa couleur).
 */
export function highlightLines(code: string, lang: string | null): string[] {
  const html = highlight(code, lang);
  const lines: string[] = [];
  const open: string[] = [];
  let cur = "";
  const re = /(<span[^>]*>)|(<\/span>)|(\n)|([^<\n]+|<)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(html))) {
    if (m[1]) {
      open.push(m[1]);
      cur += m[1];
    } else if (m[2]) {
      open.pop();
      cur += m[2];
    } else if (m[3]) {
      lines.push(cur + "</span>".repeat(open.length));
      cur = open.join("");
    } else {
      cur += m[4];
    }
  }
  lines.push(cur + "</span>".repeat(open.length));
  return lines;
}
