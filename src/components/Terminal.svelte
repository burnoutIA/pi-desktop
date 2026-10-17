<script lang="ts">
  // Terminal intégré (xterm.js) branché sur un pseudo-terminal Rust : sert à l'installeur de pi
  // et à son `/login`, qui n'existent qu'en mode terminal.
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { api, on } from "../lib/api";

  type Props = {
    program: string;
    args?: string[];
    cwd?: string | null;
    /** Texte tapé automatiquement une fois le programme prêt (ex. « /login\r »). */
    inject?: string;
    onExit?: (code: number) => void;
  };
  let { program, args = [], cwd = null, inject, onExit }: Props = $props();

  let host: HTMLDivElement;

  function theme() {
    const st = getComputedStyle(document.documentElement);
    const v = (n: string) => st.getPropertyValue(n).trim();
    return {
      background: v("--bg-2"),
      foreground: v("--text"),
      cursor: v("--agent-1"),
      cursorAccent: v("--bg-2"),
      selectionBackground: v("--agent-line"),
    };
  }

  onMount(() => {
    const term = new Terminal({
      fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--mono"),
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      theme: theme(),
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    fit.fit();

    let id: number | null = null;
    let injected = !inject;
    let idleTimer: ReturnType<typeof setTimeout> | undefined;
    const decoder = new TextDecoder();
    // Sortie arrivée avant qu'on connaisse l'identifiant du terminal : gardée de côté.
    const early: { id: number; data: string }[] = [];
    const handle = (e: { id: number; data: string }) => {
      if (id === null) return void early.push(e);
      if (e.id !== id) return;
      const bytes = Uint8Array.from(atob(e.data), (c) => c.charCodeAt(0));
      term.write(decoder.decode(bytes, { stream: true }));
      // Le programme a fini d'afficher son écran (plus rien depuis 700 ms) : on tape la commande.
      if (!injected) {
        clearTimeout(idleTimer);
        idleTimer = setTimeout(() => {
          if (!injected && id !== null) {
            injected = true;
            api.termWrite(id, inject!);
          }
        }, 700);
      }
    };
    const unlisten = [
      on<{ id: number; data: string }>("term-data", handle),
      on<{ id: number; code: number }>("term-exit", (e) => {
        if (e.id !== id) return;
        term.write("\r\n\x1b[2m[terminé]\x1b[0m\r\n");
        id = null;
        onExit?.(e.code);
      }),
    ];

    const onTheme = () => (term.options.theme = theme());
    window.addEventListener("themechange", onTheme);

    term.onData((d) => id !== null && api.termWrite(id, d));
    const ro = new ResizeObserver(() => {
      fit.fit();
      if (id !== null) api.termResize(id, term.cols, term.rows);
    });
    ro.observe(host);

    api
      .termSpawn(program, args, cwd, term.cols, term.rows)
      .then((n) => {
        id = n;
        early.splice(0).forEach(handle);
      })
      .catch((e) => term.write(`\x1b[31m${e}\x1b[0m\r\n`));
    term.focus();

    return () => {
      clearTimeout(idleTimer);
      ro.disconnect();
      window.removeEventListener("themechange", onTheme);
      unlisten.forEach((p) => p.then((u) => u()));
      if (id !== null) api.termKill(id);
      term.dispose();
    };
  });
</script>

<div class="term" bind:this={host}></div>

<style>
  .term {
    width: 100%;
    height: 100%;
    min-height: 0;
    padding: 8px 0 4px 10px;
    background: var(--bg-2);
  }
  .term :global(.xterm-viewport) {
    background: transparent !important;
  }
</style>
