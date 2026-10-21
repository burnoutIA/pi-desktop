// preview — extension pi de Pi Voice : l'agent montre à la personne ce qu'il vient de faire.
//
//   - un outil, `show_preview` : un fichier (page HTML, image, PDF…) ou une adresse web
//     (l'appli lancée sur http://localhost:5173, par exemple) ;
//   - l'app suit les appels de pi : à la fin de celui-ci, elle ouvre le panneau d'aperçu
//     sur la cible, résolue en chemin absolu dans `details`.
//
// Chargée par l'app avec `--extension` (voir pi.rs) : le pi du terminal ne l'a pas.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { existsSync, statSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";
import { Type } from "typebox";

const fail = (text: string) => ({ content: [{ type: "text" as const, text }], details: undefined, isError: true });

export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "show_preview",
    label: "show_preview",
    description:
      "Show the person something visual in the preview panel of their app: an HTML page or an image you made, " +
      "a PDF, or a running web app or website by its URL. The panel opens beside the conversation. " +
      "For a URL, the panel is a full web browser built into the app (the person may call it \"le navigateur\" or \"l'aperçu\"), " +
      "not an iframe: pages that forbid framing (X-Frame-Options, CSP frame-ancestors) display normally, and the person can click, log in and use them.",
    promptSnippet: "show_preview: show the person an HTML page, an image, a running web app or a website in the app's built-in browser",
    promptGuidelines: [
      "After making or changing something visual (an HTML page, an image, a chart, a web interface), call `show_preview` so the person sees it, without waiting to be asked.",
      "For a web app with a dev server, start the server in the background first (e.g. `npm run dev > /tmp/dev.log 2>&1 &`), wait until it answers, then call `show_preview` with its local URL.",
      "A static HTML file needs no server: pass its path to `show_preview`.",
      "The preview panel is a real browser: any URL works, including pages that refuse to be framed (X-Frame-Options, CSP frame-ancestors). Never build a proxy or rewrite headers to show a page: pass its original URL as is. If you set up such a proxy earlier in the conversation, stop using it.",
      "When the person asks to open or see a page \"dans le navigateur\", use `show_preview`; open their own desktop browser (xdg-open) only if they explicitly ask for it.",
      "Your Playwright browser (mcp__playwright__ tools) opens in a visible window on the person's screen (\"le navigateur de pi\"): they see what you do and can click and type in it. When a page needs a login, a captcha, a payment or a decision only the person can make, ask them to do it in that window, never ask for their password, then continue once they say it is done.",
    ],
    parameters: Type.Object({
      target: Type.String({ description: "Path of the file to show (relative to your working directory, or absolute), or an http(s) URL" }),
      title: Type.Optional(Type.String({ description: "Short title shown above the preview" })),
    }),
    async execute(_id: string, args: any, _signal: any, _onUpdate: any, ctx: any) {
      const target = String(args.target ?? "").trim();
      if (!target) return fail("No target given.");
      if (/^https?:\/\//i.test(target)) {
        return {
          content: [{ type: "text" as const, text: `Opened ${target} in the app's built-in browser (a real browser, not an iframe: framing restrictions do not apply). The person sees it beside the conversation and can use it.` }],
          details: { kind: "url", target, title: args.title ?? null },
          isError: false,
        };
      }
      const cwd = ctx?.cwd ?? process.cwd();
      const path = isAbsolute(target) ? target : resolve(cwd, target.replace(/^file:\/\//, ""));
      if (!existsSync(path)) return fail(`File not found: ${path}`);
      const kind = statSync(path).isDirectory() ? "dir" : "file";
      return {
        content: [{ type: "text" as const, text: `Shown to the person in the preview panel: ${target}` }],
        details: { kind, target: path, title: args.title ?? null },
        isError: false,
      };
    },
  });
}
