import { marked } from "marked";
import DOMPurify from "dompurify";
import { highlight } from "./highlight";

marked.use({
  gfm: true,
  breaks: false,
  renderer: {
    // Blocs de code colorés, avec l'étiquette du langage.
    code({ text, lang }) {
      const l = (lang ?? "").trim().split(/\s+/)[0];
      const label = l ? `<span class="code-lang">${l}</span>` : "";
      return `<pre class="code">${label}<code class="hljs">${highlight(text, l || null)}</code></pre>`;
    },
  },
});

export function renderMarkdown(src: string): string {
  return DOMPurify.sanitize(marked.parse(src, { async: false }) as string);
}

// ---------- Rendu par blocs, pour les réponses qui arrivent en streaming ----------
// Re-convertir tout un long message à chaque morceau reçu bloquait l'interface (~120 ms
// pour 20 000 caractères : l'égaliseur tombait à 8 img/s). On découpe en blocs (séparés par
// une ligne vide, hors blocs de code) : seuls les blocs nouveaux ou modifiés sont convertis.

const cache = new Map<string, string>();

/** Découpe un texte markdown en blocs indépendants (un bloc de code reste entier). */
export function splitBlocks(src: string): string[] {
  const blocks: string[] = [];
  let cur: string[] = [];
  let fence = false;
  for (const line of src.split("\n")) {
    if (/^\s*(```|~~~)/.test(line)) fence = !fence;
    if (!fence && line.trim() === "" && cur.length) {
      blocks.push(cur.join("\n"));
      cur = [];
    } else if (fence || line.trim() !== "" || cur.length) {
      cur.push(line);
    }
  }
  if (cur.length) blocks.push(cur.join("\n"));
  return blocks;
}

/** Rendu d'un bloc, mis en cache (un bloc terminé n'est converti qu'une fois). */
export function renderBlock(block: string): string {
  let html = cache.get(block);
  if (html === undefined) {
    html = renderMarkdown(block);
    if (cache.size > 2000) cache.delete(cache.keys().next().value!);
    cache.set(block, html);
  }
  return html;
}
