// Reconstruit la conversation de pi à partir de son flux d'événements RPC.

export type Block = { type: "text" | "thinking"; text: string };

export type Item =
  | { kind: "user"; id: string; text: string }
  | { kind: "assistant"; id: string; blocks: Block[]; streaming: boolean; error?: string }
  | {
      kind: "tool";
      id: string;
      name: string;
      args: Record<string, any>;
      status: "running" | "ok" | "error";
      output: string;
      durationMs?: number;
    }
  | { kind: "notice"; id: string; text: string; tone: "info" | "warn" }
  /** Échange avec l'agent vocal (pas un message de pi). */
  | { kind: "voice"; id: string; who: "user" | "agent"; text: string; action?: string };

const FILE_TOOLS = new Set(["write", "edit", "multiedit", "create", "str_replace"]);

let seq = 0;
const uid = (p: string) => `${p}-${++seq}`;

function textOf(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .filter((c: any) => c?.type === "text")
      .map((c: any) => c.text as string)
      .join("\n");
  }
  return "";
}

/** Chemin de fichier visé par un appel d'outil, s'il y en a un. */
export function toolPath(args: Record<string, any> | undefined): string | undefined {
  if (!args) return undefined;
  const p = args.path ?? args.file_path ?? args.filePath ?? args.filename;
  return typeof p === "string" ? p : undefined;
}

export class Session {
  items = $state<Item[]>([]);
  busy = $state(false);
  queue = $state<{ steering: string[]; followUp: string[] }>({ steering: [], followUp: [] });
  status = $state<string | null>(null);
  model = $state<string | null>(null);
  /** Fichiers modifiés par pi pendant cette session (chemins tels que donnés à l'outil). */
  touched = $state<string[]>([]);
  onFileTouched: ((path: string) => void) | null = null;

  private current: Extract<Item, { kind: "assistant" }> | null = null;

  reset() {
    this.items = [];
    this.busy = false;
    this.touched = [];
    this.current = null;
    this.status = null;
  }

  /** Reconstruit l'affichage depuis les messages d'une session reprise (`get_messages`). */
  loadHistory(messages: any[]) {
    this.reset();
    const items: Item[] = [];
    const tools = new Map<string, Extract<Item, { kind: "tool" }>>();
    for (const m of messages ?? []) {
      if (m?.role === "user") {
        const text = textOf(m.content);
        if (text.trim()) items.push({ kind: "user", id: uid("u"), text });
      } else if (m?.role === "assistant" && Array.isArray(m.content)) {
        const blocks: Block[] = m.content
          .filter((c: any) => c?.type === "text" || c?.type === "thinking")
          .map((c: any) => ({ type: c.type, text: c.type === "text" ? c.text : (c.thinking ?? "") }));
        if (blocks.some((b) => b.text.trim()) || m.errorMessage) {
          items.push({
            kind: "assistant",
            id: uid("a"),
            blocks,
            streaming: false,
            error: m.stopReason === "error" ? (m.errorMessage ?? "Erreur du modèle") : undefined,
          });
        }
        for (const c of m.content) {
          if (c?.type !== "toolCall") continue;
          const t: Extract<Item, { kind: "tool" }> = {
            kind: "tool",
            id: c.id ?? uid("t"),
            name: c.name,
            args: c.arguments ?? {},
            status: "ok",
            output: "",
          };
          tools.set(t.id, t);
          items.push(t);
        }
        if (m.model) this.model = m.model;
      } else if (m?.role === "toolResult") {
        const t = tools.get(m.toolCallId);
        if (t) {
          t.status = m.isError ? "error" : "ok";
          t.output = textOf(m.content);
        }
      }
    }
    this.items = items;
  }

  voice(who: "user" | "agent", text: string, action?: string) {
    this.items.push({ kind: "voice", id: uid("v"), who, text, action });
  }

  /** L'état de pi en quelques lignes, pour l'agent vocal. */
  summary(): string {
    const lines: string[] = [];
    const lastUser = [...this.items].reverse().find((i) => i.kind === "user") as Extract<Item, { kind: "user" }> | undefined;
    if (this.busy) lines.push(`pi travaille sur : « ${lastUser?.text.slice(0, 300) ?? "?"} ».`);
    else lines.push(lastUser ? `pi est inactif. Dernière demande : « ${lastUser.text.slice(0, 300)} ».` : "pi est inactif, aucune demande pour l'instant.");
    const tools = this.items.filter((i) => i.kind === "tool").slice(-6) as Extract<Item, { kind: "tool" }>[];
    if (tools.length) {
      const st = { running: "en cours", ok: "ok", error: "erreur" };
      lines.push("Derniers outils : " + tools.map((t) => `${t.name} ${toolPath(t.args) ?? ""} (${st[t.status]})`.replace(/\s+\(/, " (")).join(", ") + ".");
    }
    const last = [...this.items].reverse().find((i) => i.kind === "assistant") as Extract<Item, { kind: "assistant" }> | undefined;
    const text = last?.blocks.filter((b) => b.type === "text").map((b) => b.text).join("\n").trim();
    if (text) lines.push(`Dernière réponse de pi : « ${text.length > 600 ? "…" + text.slice(-600) : text} ».`);
    if (this.queue.steering.length) lines.push(`Consignes en attente pour pi : ${this.queue.steering.length}.`);
    return lines.join("\n");
  }

  notice(text: string, tone: "info" | "warn" = "info") {
    this.items.push({ kind: "notice", id: uid("n"), text, tone });
  }

  handle(ev: any) {
    switch (ev.type) {
      case "agent_start":
        this.busy = true;
        this.status = null;
        break;
      case "agent_settled":
        this.busy = false;
        this.status = null;
        if (ev.aborted) this.notice("Interrompu.", "warn");
        break;
      case "message_start": {
        const m = ev.message;
        if (m?.role === "user") {
          this.items.push({ kind: "user", id: uid("u"), text: textOf(m.content) });
        } else if (m?.role === "assistant") {
          const item: Item = { kind: "assistant", id: uid("a"), blocks: [], streaming: true };
          this.items.push(item);
          // On garde la version réactive (proxy) de l'élément ajouté.
          this.current = this.items[this.items.length - 1] as Extract<Item, { kind: "assistant" }>;
        }
        break;
      }
      case "message_update": {
        const e = ev.assistantMessageEvent;
        const cur = this.current;
        if (!cur || !e) break;
        const idx = e.contentIndex ?? 0;
        const kind = e.type?.startsWith("thinking") ? "thinking" : "text";
        if (e.type === "text_start" || e.type === "thinking_start") {
          cur.blocks[idx] = { type: kind, text: "" };
        } else if (e.type === "text_delta" || e.type === "thinking_delta") {
          const b = (cur.blocks[idx] ??= { type: kind, text: "" });
          b.text += e.delta ?? "";
        } else if (e.type === "text_end" || e.type === "thinking_end") {
          if (typeof e.content === "string") cur.blocks[idx] = { type: kind, text: e.content };
        }
        break;
      }
      case "message_end": {
        const m = ev.message;
        if (m?.role !== "assistant" || !this.current) break;
        const cur = this.current;
        if (Array.isArray(m.content)) {
          cur.blocks = m.content
            .filter((c: any) => c?.type === "text" || c?.type === "thinking")
            .map((c: any) => ({ type: c.type, text: c.type === "text" ? c.text : (c.thinking ?? c.text ?? "") }));
        }
        cur.streaming = false;
        if (m.stopReason === "error" || m.errorMessage) cur.error = m.errorMessage ?? "Erreur du modèle";
        if (m.model) this.model = m.model;
        this.current = null;
        break;
      }
      case "tool_execution_start": {
        this.items.push({
          kind: "tool",
          id: ev.toolCallId ?? uid("t"),
          name: ev.toolName,
          args: ev.args ?? {},
          status: "running",
          output: "",
        });
        const p = toolPath(ev.args);
        if (p && FILE_TOOLS.has(String(ev.toolName).toLowerCase())) {
          if (!this.touched.includes(p)) this.touched.push(p);
          this.onFileTouched?.(p);
        }
        break;
      }
      case "tool_execution_update": {
        const t = this.findTool(ev.toolCallId);
        if (t) t.output = textOf(ev.partialResult?.content);
        break;
      }
      case "tool_execution_end": {
        const t = this.findTool(ev.toolCallId);
        if (t) {
          t.status = ev.isError ? "error" : "ok";
          t.output = textOf(ev.result?.content);
          t.durationMs = ev.durationMs;
        }
        const p = toolPath(t?.args);
        if (p && t && FILE_TOOLS.has(t.name.toLowerCase())) this.onFileTouched?.(p);
        break;
      }
      case "queue_update":
        this.queue = { steering: ev.steering ?? [], followUp: ev.followUp ?? [] };
        break;
      case "auto_retry_start":
        this.status = `Nouvel essai ${ev.attempt}/${ev.maxAttempts}…`;
        break;
      case "auto_retry_end":
        this.status = null;
        if (!ev.success && ev.finalError) this.notice(ev.finalError, "warn");
        break;
      case "compaction_start":
        this.status = "Compaction du contexte…";
        break;
      case "compaction_end":
        this.status = null;
        break;
    }
  }

  private findTool(id: string) {
    for (let i = this.items.length - 1; i >= 0; i--) {
      const it = this.items[i];
      if (it.kind === "tool" && it.id === id) return it;
    }
    return undefined;
  }
}

/** Sépare le texte affiché et le résumé oral `<oral>…</oral>`. */
export function splitOral(text: string): { body: string; oral: string | null } {
  const open = text.indexOf("<oral>");
  if (open < 0) return { body: text, oral: null };
  const close = text.indexOf("</oral>", open);
  const oral = text.slice(open + 6, close < 0 ? undefined : close).trim();
  const body = (text.slice(0, open) + (close < 0 ? "" : text.slice(close + 7))).trim();
  return { body, oral };
}
