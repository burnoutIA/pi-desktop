// harn — extension pi de Pi Voice : la liste des modèles de harn vient de harn lui-même.
//
//   - harn parle l'API OpenAI : `GET <baseUrl>/models` donne les modèles qu'il sert, avec leur nom,
//     leur contexte et s'ils voient les images (`meta`) ;
//   - l'adresse et la clé restent celles du provider `harn` de models.json ; sa liste `models`
//     ne sert plus que de repli quand harn ne répond pas, et pour le `maxTokens` d'un modèle déjà connu ;
//   - la liste est relue au lancement de pi et à chaque session (nouvelle ou reprise).
//
// Chargée par l'app avec `--extension` (voir pi.rs) : le pi du terminal ne l'a pas.

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { getAgentDir } from "@earendil-works/pi-coding-agent";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const PROVIDER = "harn";

function harnConfig(): any {
  try {
    return JSON.parse(readFileSync(join(getAgentDir(), "models.json"), "utf8"))?.providers?.[PROVIDER];
  } catch {
    return;
  }
}

async function liveModels(config: any) {
  const res = await fetch(`${String(config.baseUrl).replace(/\/$/, "")}/models`, {
    headers: config.apiKey ? { Authorization: `Bearer ${config.apiKey}` } : {},
    signal: AbortSignal.timeout(3000),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const known = new Map<string, any>((config.models ?? []).map((m: any) => [m.id, m]));
  return ((await res.json())?.data ?? []).map((m: any) => {
    const before = known.get(m.id);
    const contextWindow = m.meta?.context ?? before?.contextWindow ?? 32768;
    return {
      id: m.id,
      name: m.meta?.name ?? before?.name ?? m.id,
      reasoning: before?.reasoning ?? true,
      input: m.meta?.vision ? ["text", "image"] : ["text"],
      contextWindow,
      maxTokens: before?.maxTokens ?? Math.min(32768, contextWindow),
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    };
  });
}

export default async function (pi: ExtensionAPI) {
  const sync = async () => {
    const config = harnConfig();
    if (!config?.baseUrl) return;
    try {
      const models = await liveModels(config);
      pi.registerProvider(PROVIDER, { baseUrl: config.baseUrl, api: config.api ?? "openai-completions", models } as any);
    } catch {
      // harn éteint : la liste de models.json (ou la dernière lue) reste.
    }
  };
  await sync();
  pi.on("session_start", sync);
}
