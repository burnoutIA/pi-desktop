// watchdog — extension pi de Pi Voice : une commande bash longue ne bloque plus l'agent.
//
//   - `bash` remplace l'outil intégré (même exécution, même rendu). Si la commande dure plus de
//     `checkpointSeconds` (2 min par défaut), l'outil rend la main au modèle avec un point d'étape
//     (durée, CPU, mémoire, sortie récente) et la commande continue en arrière-plan sous un numéro de job ;
//   - `job_wait` attend encore (nouveau point d'étape ou résultat final), `job_kill` l'arrête :
//     c'est le modèle qui décide, il n'y a pas de durée maximale ;
//   - garde-fou mémoire, sur la RAM *disponible* de la machine (pas sur la taille du job) :
//       · sous `alertPercent` : le point d'étape arrive tout de suite ;
//       · sous `killPercent` (et jamais moins de `killFloorGb`) : le job qui pèse (> 25 % de la RAM,
//         ou qui grossit) est tué sans attendre le modèle ;
//     plus, si `systemd-run --user` marche, un plafond noyau (MemoryMax) au cas où pi lui-même tomberait ;
//   - `bash` avec `gpu: true` prête la carte graphique : si le modèle de l'agent est servi par harn sur
//     cette machine, harn le décharge (`/api/engine/stop`), la commande tourne seule sur la carte et le
//     tour s'arrête (`terminate`) : l'agent dort, rien ne surveille à part la durée `max_minutes` et le
//     garde-fou mémoire. À la fin, un message le réveille ; sa première requête fait recharger harn ;
//   - tout job fini que personne n'attend réveille l'agent de la même façon (message `watchdog-job`).
//
// Réglages : `<dossier agent>/watchdog.json` (onglet Agent des réglages), relu à chaque mesure.
// Chargée par l'app avec `--extension` (voir pi.rs) : le pi du terminal ne l'a pas.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { createBashToolDefinition, getAgentDir } from "@earendil-works/pi-coding-agent";
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { Type } from "typebox";

// ---------- Réglages ----------

type Config = { alertPercent: number; killPercent: number; killFloorGb: number; checkpointSeconds: number };
const DEFAULTS: Config = { alertPercent: 20, killPercent: 6, killFloorGb: 4, checkpointSeconds: 120 };

function config(): Config {
  try {
    const raw = JSON.parse(readFileSync(join(getAgentDir(), "watchdog.json"), "utf8"));
    const num = (k: keyof Config, min: number, max: number) => {
      const v = Number(raw?.[k]);
      return Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : DEFAULTS[k];
    };
    return {
      alertPercent: num("alertPercent", 1, 90),
      killPercent: num("killPercent", 1, 50),
      killFloorGb: num("killFloorGb", 0.5, 64),
      checkpointSeconds: num("checkpointSeconds", 10, 3600),
    };
  } catch {
    return DEFAULTS;
  }
}

// ---------- Mesures (/proc) ----------

const PAGE = 4096;
const TICKS = 100; // CLK_TCK sous Linux
const GB = 1024 ** 3;

function memInfo(): { total: number; available: number } {
  const txt = readFileSync("/proc/meminfo", "utf8");
  const kb = (k: string) => Number(new RegExp(`^${k}:\\s+(\\d+)`, "m").exec(txt)?.[1] ?? 0) * 1024;
  return { total: kb("MemTotal"), available: kb("MemAvailable") };
}

type Proc = { pid: number; ppid: number; pgrp: number; ticks: number; rss: number };

function readProcs(): Map<number, Proc> {
  const out = new Map<number, Proc>();
  for (const name of readdirSync("/proc")) {
    if (!/^\d+$/.test(name)) continue;
    try {
      const stat = readFileSync(`/proc/${name}/stat`, "utf8");
      const f = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      out.set(Number(name), {
        pid: Number(name),
        ppid: Number(f[1]),
        pgrp: Number(f[2]),
        ticks: Number(f[11]) + Number(f[12]),
        rss: Number(f[21]) * PAGE,
      });
    } catch {
      // processus disparu entre-temps
    }
  }
  return out;
}

/** Le shell d'un job : le processus marqué par son identifiant qui mène son groupe. */
function findLeader(marker: string, procs: Map<number, Proc>): number | undefined {
  const needle = `PI_WATCHDOG_JOB=${marker}\0`;
  for (const p of procs.values()) {
    if (p.pid !== p.pgrp) continue;
    try {
      if (readFileSync(`/proc/${p.pid}/environ`, "latin1").includes(needle)) return p.pid;
    } catch {
      // pas à nous, ou disparu
    }
  }
}

/** Le shell, ses descendants, et tout son groupe de processus. */
function jobProcs(leader: number, procs: Map<number, Proc>): Proc[] {
  const children = new Map<number, number[]>();
  for (const p of procs.values()) {
    if (!children.has(p.ppid)) children.set(p.ppid, []);
    children.get(p.ppid)!.push(p.pid);
  }
  const seen = new Set<number>();
  const stack = [leader];
  while (stack.length) {
    const pid = stack.pop()!;
    if (seen.has(pid) || !procs.has(pid)) continue;
    seen.add(pid);
    stack.push(...(children.get(pid) ?? []));
  }
  for (const p of procs.values()) if (p.pgrp === leader) seen.add(p.pid);
  return [...seen].map((pid) => procs.get(pid)!).filter(Boolean);
}

// ---------- Jobs ----------

type Sample = { t: number; rss: number; ticks: number };
type Outcome = { ok: true; value: any } | { ok: false; error: unknown };

type Job = {
  id: number;
  command: string;
  marker: string;
  startedAt: number;
  abort: AbortController;
  sink?: (u: any) => void; // onUpdate de l'appel qui attend le job (bash, puis job_wait)
  leader?: number;
  procs: number;
  samples: Sample[];
  tail: string;
  lastOutputAt?: number;
  outcome?: Outcome;
  finished: Promise<void>;
  killedReason?: string;
  wake?: (why: "alert") => void; // réveille l'appel qui attend
  waitRssAtStart?: number;
  collected?: boolean; // le résultat final a été rendu au modèle
  gpu?: { harn: string; deadline: number; maxMinutes: number }; // carte prêtée : l'agent dort
};

const jobs = new Map<number, Job>();
let nextId = 1;
let pendingMarker: string | undefined; // lu par le spawnHook, synchrone au début de execute()
let lowMemory = false; // sous le seuil d'alerte à la dernière mesure
let monitor: ReturnType<typeof setInterval> | undefined;

const fmtBytes = (b: number) => (b >= GB ? `${(b / GB).toFixed(1)} GB` : `${Math.round(b / 1024 ** 2)} MB`);
function fmtDuration(ms: number) {
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
  return h ? `${h}h ${String(m).padStart(2, "0")}m` : m ? `${m}m ${String(sec).padStart(2, "0")}s` : `${sec}s`;
}

/** Échantillon le plus ancien à au moins `ms` du dernier (ou le premier). */
function sampleAgo(job: Job, ms: number): Sample | undefined {
  const last = job.samples.at(-1);
  if (!last) return;
  for (let i = job.samples.length - 1; i >= 0; i--) if (last.t - job.samples[i].t >= ms) return job.samples[i];
  return job.samples[0];
}

function growth(job: Job, ms: number) {
  const last = job.samples.at(-1), old = sampleAgo(job, ms);
  return last && old ? last.rss - old.rss : 0;
}

function cpuPercent(job: Job, ms: number) {
  const last = job.samples.at(-1), old = sampleAgo(job, ms);
  if (!last || !old || last.t === old.t) return undefined;
  return Math.round(((last.ticks - old.ticks) / TICKS / ((last.t - old.t) / 1000)) * 100);
}

function killJob(job: Job, reason: string) {
  job.killedReason ??= reason;
  job.abort.abort(); // pi tue le groupe du shell
  if (job.leader) {
    // et les descendants sortis du groupe (setsid…)
    for (const p of jobProcs(job.leader, readProcs())) {
      try {
        process.kill(p.pid, "SIGKILL");
      } catch {}
    }
  }
}

function tick() {
  const running = [...jobs.values()].filter((j) => !j.outcome);
  if (!running.length) {
    clearInterval(monitor);
    monitor = undefined;
    return;
  }
  const cfg = config();
  const procs = readProcs();
  const now = Date.now();
  for (const job of running) {
    job.leader ??= findLeader(job.marker, procs);
    if (!job.leader) continue;
    const set = jobProcs(job.leader, procs);
    job.procs = set.length;
    job.samples.push({ t: now, rss: set.reduce((a, p) => a + p.rss, 0), ticks: set.reduce((a, p) => a + p.ticks, 0) });
    while (job.samples.length > 2 && now - job.samples[0].t > 10 * 60_000) job.samples.shift();
  }

  // L'agent dort pendant un prêt de la carte : personne ne peut juger, la durée annoncée fait foi.
  for (const job of running) {
    if (job.gpu && now > job.gpu.deadline) {
      killJob(job, `stopped at the end of the ${job.gpu.maxMinutes} minutes (max_minutes) you set before going to sleep`);
    }
  }

  const mem = memInfo();
  const alertAt = (mem.total * cfg.alertPercent) / 100;
  const killAt = Math.max((mem.total * cfg.killPercent) / 100, cfg.killFloorGb * GB);

  if (mem.available < killAt) {
    // Un seul job par mesure : le plus gros de ceux qui pèsent vraiment ou grossissent.
    const culprit = running
      .filter((j) => {
        const rss = j.samples.at(-1)?.rss ?? 0;
        return rss > mem.total * 0.25 || growth(j, 60_000) > 256 * 1024 ** 2;
      })
      .sort((a, b) => (b.samples.at(-1)?.rss ?? 0) - (a.samples.at(-1)?.rss ?? 0))[0];
    if (culprit) {
      killJob(
        culprit,
        `killed by the memory watchdog: system memory fell to ${fmtBytes(mem.available)} available ` +
          `(limit ${fmtBytes(killAt)}) while this job used ${fmtBytes(culprit.samples.at(-1)?.rss ?? 0)}`,
      );
    }
  }

  const wasLow = lowMemory;
  lowMemory = mem.available < alertAt;
  for (const job of running) {
    // Un job qu'on vient de tuer n'est pas réveillé : son attente se termine sur le résultat final.
    if (!job.wake || !lowMemory || job.killedReason) continue;
    // Prévenir au passage du seuil, ou si le job a encore pris 2 Go depuis le début de l'attente :
    // sinon un modèle qui choisit d'attendre serait réveillé en boucle.
    const grew = (job.samples.at(-1)?.rss ?? 0) - (job.waitRssAtStart ?? 0) > 2 * GB;
    if (!wasLow || grew) job.wake("alert");
  }
}

function ensureMonitor() {
  monitor ??= setInterval(() => {
    try {
      tick();
    } catch {
      // une mesure ratée ne doit jamais casser l'agent
    }
  }, 3000);
}

function report(job: Job, why: "checkpoint" | "alert"): string {
  const cfg = config();
  const mem = memInfo();
  const last = job.samples.at(-1);
  const cpu = cpuPercent(job, 30_000);
  const g = growth(job, 60_000);
  const quiet = Date.now() - (job.lastOutputAt ?? job.startedAt);
  const lines = job.tail.trimEnd().split("\n").slice(-15).join("\n");
  const parts = [
    `Job ${job.id} is still running after ${fmtDuration(Date.now() - job.startedAt)}. It keeps running in the background.`,
    `Command: ${job.command.length > 300 ? job.command.slice(0, 300) + "…" : job.command}`,
    last
      ? `CPU: ${cpu ?? "?"}% (last 30 s) · Memory: ${fmtBytes(last.rss)} (${g >= 0 ? "+" : ""}${fmtBytes(g)} in the last minute) · Processes: ${job.procs}`
      : "CPU / memory: not measured yet.",
    job.lastOutputAt
      ? `Output: last new output ${fmtDuration(quiet)} ago.` + (lines ? ` Last lines:\n${lines}` : "")
      : `Output: none since the start (${fmtDuration(quiet)}).`,
    `System memory: ${fmtBytes(mem.available)} available of ${fmtBytes(mem.total)} (${Math.round((mem.available / mem.total) * 100)}%).`,
  ];
  if (why === "alert" || mem.available < (mem.total * cfg.alertPercent) / 100) {
    parts.push(
      `WARNING: system memory is low (alert below ${cfg.alertPercent}%). If this job is what fills it, kill it now; ` +
        `the watchdog kills it by itself below ${cfg.killPercent}%.`,
    );
  }
  parts.push(
    `Decide now, and say in one short sentence why: job_wait {"id": ${job.id}} to keep waiting (pass "seconds", up to 3600, ` +
      `for long work that is clearly progressing), or job_kill {"id": ${job.id}} if it looks stuck (no output, CPU busy, memory growing) ` +
      `or is no longer needed.`,
  );
  return parts.join("\n");
}

/** Attend la fin du job, un réveil (alerte mémoire), le délai, ou l'abandon du tour. */
async function waitJob(job: Job, ms: number, signal?: AbortSignal): Promise<"done" | "checkpoint" | "alert" | "aborted"> {
  if (job.outcome) return "done";
  job.waitRssAtStart = job.samples.at(-1)?.rss ?? 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let onAbort: (() => void) | undefined;
  try {
    return await Promise.race([
      job.finished.then(() => "done" as const),
      new Promise<"checkpoint">((r) => (timer = setTimeout(() => r("checkpoint"), ms))),
      new Promise<"alert">((r) => (job.wake = r)),
      new Promise<"aborted">((r) => {
        if (!signal) return;
        onAbort = () => r("aborted");
        if (signal.aborted) onAbort();
        else signal.addEventListener("abort", onAbort, { once: true });
      }),
    ]);
  } finally {
    clearTimeout(timer);
    job.wake = undefined;
    if (onAbort) signal?.removeEventListener("abort", onAbort);
  }
}

/** Le résultat final du job, tel que l'outil bash d'origine l'aurait rendu. */
function finalResult(job: Job, prefix?: string) {
  jobs.delete(job.id);
  job.collected = true;
  const o = job.outcome!;
  const note = [prefix, job.killedReason && `Job ${job.id} was ${job.killedReason}.`].filter(Boolean).join("\n");
  if (o.ok) {
    if (!note) return o.value;
    const content = [{ type: "text", text: note }, ...(o.value.content ?? [])];
    return { ...o.value, content };
  }
  const msg = o.error instanceof Error ? o.error.message : String(o.error);
  throw new Error(note ? `${note}\n\n${msg}` : msg);
}

// ---------- Extension ----------

/** Plafond noyau par commande, si systemd en session utilisateur est là (pas dans un conteneur). */
const systemdScope = (() => {
  try {
    return spawnSync("systemd-run", ["--user", "--scope", "--quiet", "--collect", "--", "true"], { timeout: 5000 }).status === 0;
  } catch {
    return false;
  }
})();

const shellQuote = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`;

// ---------- Prêt de la carte graphique (harn) ----------

async function getJson(url: string, init?: RequestInit): Promise<any> {
  const res = await fetch(url, { ...init, signal: AbortSignal.timeout(60_000) });
  if (!res.ok) throw new Error(`${url} : HTTP ${res.status}`);
  return res.json().catch(() => ({}));
}

/** L'adresse de harn si le modèle de l'agent est servi par harn sur cette machine, sinon rien. */
async function harnOf(model: any): Promise<string | undefined> {
  try {
    const url = new URL(String(model?.baseUrl ?? ""));
    if (!["127.0.0.1", "localhost", "::1", "[::1]"].includes(url.hostname)) return;
    const status = await getJson(`${url.origin}/api/status`);
    return "active" in status && "models" in status ? url.origin : undefined;
  } catch {
    return;
  }
}

function freeVram(): string {
  const r = spawnSync("nvidia-smi", ["--query-gpu=memory.used,memory.total", "--format=csv,noheader,nounits"], { encoding: "utf8", timeout: 5000 });
  const [used, total] = (r.stdout ?? "").trim().split("\n")[0]?.split(",").map(Number) ?? [];
  return Number.isFinite(used) && Number.isFinite(total) ? `${((total - used) / 1024).toFixed(1)} GB of ${(total / 1024).toFixed(1)} GB VRAM free` : "VRAM not measured";
}

/** Décharge le modèle de harn, et attend qu'il soit vraiment arrêté. */
async function lendGpu(harn: string) {
  await getJson(`${harn}/api/engine/stop`, { method: "POST" });
  for (let i = 0; i < 60; i++) {
    const s = await getJson(`${harn}/api/status`).catch(() => null);
    if (s && (!s.active || s.active.status === "stopped")) break;
    await new Promise((r) => setTimeout(r, 1000));
  }
  await new Promise((r) => setTimeout(r, 1500)); // le temps que le pilote rende la mémoire
}

/** Le texte du résultat final, pour le message de réveil. */
function outcomeText(job: Job): string {
  const o = job.outcome!;
  const body = o.ok
    ? (o.value?.content ?? []).map((c: any) => c?.text ?? "").join("\n")
    : o.error instanceof Error ? o.error.message : String(o.error);
  return body.length > 12_000 ? `…${body.slice(-12_000)}` : body;
}

export default function (pi: ExtensionAPI) {
  const base = createBashToolDefinition(process.cwd(), {
    spawnHook: ({ command, cwd, env }) => {
      const marker = pendingMarker ?? "none";
      if (systemdScope) {
        const cfg = config();
        const total = memInfo().total;
        const reserve = Math.max((total * cfg.killPercent) / 100, cfg.killFloorGb * GB);
        const max = Math.max(GB, Math.floor(total - reserve));
        command = `exec systemd-run --user --scope --quiet --collect -p MemoryMax=${max} -- bash -c ${shellQuote(command)}`;
      }
      return { command, cwd, env: { ...env, PI_WATCHDOG_JOB: marker } };
    },
  });

  let lastCtx: any;

  /** Un job fini que personne n'attend : son résultat réveille l'agent (ou l'attend s'il travaille). */
  function notifyWhenUnclaimed(job: Job) {
    job.finished.then(() =>
      setTimeout(() => {
        if (job.collected || job.wake) return; // rendu par bash/job_wait, ou une attente le récupère
        jobs.delete(job.id);
        job.collected = true;
        const head = job.gpu
          ? `[watchdog] You are awake again. Job ${job.id}, which you ran with the GPU while you slept, finished after ${fmtDuration(Date.now() - job.startedAt)}. ` +
            "harn reloaded your model on this request."
          : `[watchdog] Background job ${job.id} finished after ${fmtDuration(Date.now() - job.startedAt)}.`;
        const killed = job.killedReason ? `\nIt was ${job.killedReason}.` : "";
        const text = `${head}${killed}\nCommand: ${job.command.slice(0, 300)}\n\n${outcomeText(job)}`;
        const idle = lastCtx?.isIdle?.() ?? true;
        try {
          // `details` : ce que l'app affiche à la personne (session.svelte.ts), le texte est pour le modèle.
          const details = { job: job.id, gpu: Boolean(job.gpu), seconds: Math.round((Date.now() - job.startedAt) / 1000), killed: Boolean(job.killedReason) };
          pi.sendMessage({ customType: "watchdog-job", content: text, display: true, details }, idle ? { triggerTurn: true } : { deliverAs: "steer" });
        } catch {
          // session fermée entre-temps
        }
      }, 200),
    );
  }

  const parameters = Type.Object({
    ...(base.parameters as any).properties,
    gpu: Type.Optional(
      Type.Boolean({
        description:
          "true when the command needs the GPU (running a model, CUDA tests, training…). Your own model runs on that GPU: " +
          "it is unloaded, the command runs alone on the GPU, your turn ends, and you are woken up with the result.",
      }),
    ),
    max_minutes: Type.Optional(
      Type.Number({ description: "With gpu: the command is killed after this many minutes, since nobody watches it while you sleep. Default 60." }),
    ),
  });

  pi.registerTool({
    ...base,
    parameters,
    description:
      `${base.description} If the command runs longer than about ${Math.round(DEFAULTS.checkpointSeconds / 60)} minutes, ` +
      "you get a progress report instead of the result and the command keeps running as a background job: " +
      "then use job_wait or job_kill. Set gpu: true for a command that needs the GPU.",
    promptGuidelines: [
      ...(base.promptGuidelines ?? []),
      "A long bash command returns a progress report (CPU, memory, recent output) and keeps running as a job. There is no time limit: " +
        "if it is progressing (new output, steady memory), keep waiting with job_wait, with a longer `seconds` for long builds or downloads. " +
        "If it is stuck (no new output while CPU is busy, memory growing without bound) or no longer useful, stop it with job_kill. " +
        "Always say in one short sentence why you wait or kill.",
      "Do not end your turn while a job you started is still running, unless it is meant to keep running (a server): wait for it or kill it.",
      "Your own model may run on this machine's GPU, served by harn. Never stop, restart or reconfigure harn or its llama-server to get the GPU: " +
        "run the command with bash and gpu: true (plus max_minutes) instead. You then sleep while it runs and are woken up with its result. " +
        "Prepare everything first (build, files, a short check that the command works), then make it the last call of your turn.",
    ],
    async execute(toolCallId: string, params: any, signal: AbortSignal | undefined, onUpdate: any, ctx: any) {
      lastCtx = ctx ?? lastCtx;
      const { gpu, max_minutes, ...shellParams } = params ?? {};
      params = shellParams;
      let lend: Job["gpu"];
      if (gpu) {
        const harn = await harnOf(ctx?.model);
        if (harn) {
          try {
            await lendGpu(harn);
          } catch (e) {
            throw new Error(`Could not free the GPU: harn did not unload your model (${e instanceof Error ? e.message : e}). The command was not run.`);
          }
          const maxMinutes = Math.min(24 * 60, Math.max(1, Number(max_minutes) || 60));
          lend = { harn, maxMinutes, deadline: Date.now() + maxMinutes * 60_000 };
        }
      }
      const id = nextId++;
      const job: Job = {
        id,
        command: String(params?.command ?? ""),
        marker: `${process.pid}-${id}-${Date.now()}`,
        startedAt: Date.now(),
        abort: new AbortController(),
        sink: onUpdate,
        procs: 0,
        samples: [],
        tail: "",
        finished: Promise.resolve(),
      };
      const update = (u: any) => {
        const text = u?.content?.find?.((c: any) => c.type === "text")?.text;
        if (text) {
          job.tail = text.slice(-4000);
          job.lastOutputAt = Date.now();
        }
        job.sink?.(u);
      };
      pendingMarker = job.marker;
      let run: Promise<any>;
      try {
        run = base.execute(toolCallId, params, job.abort.signal, update, ctx);
      } finally {
        pendingMarker = undefined;
      }
      job.finished = run.then(
        (value) => void (job.outcome = { ok: true, value }),
        (error) => void (job.outcome = { ok: false, error }),
      );
      jobs.set(id, job);
      ensureMonitor();
      notifyWhenUnclaimed(job);

      if (lend) {
        // L'agent s'endort : plus aucune requête au modèle (sinon harn le recharge et reprend la carte).
        job.gpu = lend;
        job.sink = undefined;
        return {
          content: [
            {
              type: "text",
              text:
                `Your model was unloaded from the GPU (${freeVram()}). Job ${id} now runs with the GPU to itself, ` +
                `for at most ${lend.maxMinutes} minutes. Your turn ends here: you are asleep, since your own model ran on this GPU. ` +
                "When the job ends you will be woken up with its result, and harn reloads your model by itself.",
            },
          ],
          details: undefined,
          terminate: true,
        };
      }

      const why = await waitJob(job, config().checkpointSeconds * 1000, signal);
      if (why === "aborted") {
        killJob(job, "stopped because the turn was aborted");
        await job.finished;
      }
      if (job.outcome) return finalResult(job);
      job.sink = undefined; // la suite passe par job_wait
      return { content: [{ type: "text", text: report(job, why === "alert" ? "alert" : "checkpoint") }], details: undefined };
    },
  });

  const getJob = (id: unknown) => {
    const job = jobs.get(Number(id));
    if (!job) {
      const known = [...jobs.keys()];
      const sent = Number(id) < nextId ? ` Job ${id} has already finished and its result was already given to you (in a tool result or a [watchdog] message).` : "";
      throw new Error(`No running job ${id}.${sent} ${known.length ? `Running jobs: ${known.join(", ")}.` : "No job is running."}`);
    }
    return job;
  };

  pi.registerTool({
    name: "job_wait",
    label: "job_wait",
    description:
      "Wait for a background job started by a long bash command. Returns its final output if it finishes, " +
      "or a new progress report after `seconds` (default: the usual checkpoint interval), or earlier if system memory gets low.",
    promptSnippet: "job_wait: keep waiting for a long bash command that runs as a background job",
    parameters: Type.Object({
      id: Type.Number({ description: "Job number, from the progress report" }),
      seconds: Type.Optional(
        Type.Number({ description: "How long to wait before the next progress report, 0 to 3600. 0 = report now." }),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, onUpdate: any) {
      const job = getJob(args.id);
      const secs = Math.min(3600, Math.max(0, Number(args.seconds ?? config().checkpointSeconds) || 0));
      job.sink = onUpdate;
      try {
        const why = secs === 0 ? (job.outcome ? "done" : "checkpoint") : await waitJob(job, secs * 1000, signal);
        if (job.outcome) {
          return finalResult(job, `Job ${job.id} finished after ${fmtDuration(Date.now() - job.startedAt)}.`);
        }
        if (why === "aborted") {
          killJob(job, "stopped because the turn was aborted");
          await job.finished;
          return finalResult(job);
        }
        return { content: [{ type: "text", text: report(job, why === "alert" ? "alert" : "checkpoint") }], details: undefined };
      } finally {
        job.sink = undefined;
      }
    },
  });

  pi.registerTool({
    name: "job_kill",
    label: "job_kill",
    description: "Stop a background job started by a long bash command (its whole process tree), and return its output so far.",
    promptSnippet: "job_kill: stop a background job that is stuck or no longer needed",
    parameters: Type.Object({
      id: Type.Number({ description: "Job number, from the progress report" }),
      reason: Type.Optional(Type.String({ description: "Why you stop it, in a few words" })),
    }),
    async execute(_id: string, args: any) {
      const job = getJob(args.id);
      killJob(job, `stopped by you${args.reason ? ` (${args.reason})` : ""}`);
      await Promise.race([job.finished, new Promise((r) => setTimeout(r, 10_000))]);
      if (!job.outcome) {
        return { content: [{ type: "text", text: `Job ${job.id}: kill sent, but it has not exited yet.` }], details: undefined, isError: true };
      }
      try {
        return finalResult(job);
      } catch (e) {
        // Une commande tuée « échoue » toujours : ici c'est voulu, pas une erreur de l'outil.
        return { content: [{ type: "text", text: e instanceof Error ? e.message : String(e) }], details: undefined };
      }
    },
  });

  pi.on("session_shutdown", async () => {
    for (const job of jobs.values()) if (!job.outcome) killJob(job, "stopped because the session ended");
    jobs.clear();
  });
}
