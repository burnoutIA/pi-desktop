// Commandes « slash » de la zone de saisie : celles de pi (skills, modèles de prompt, extensions),
// envoyées telles quelles, et les commandes intégrées de pi en terminal, refaites ici via le RPC.
export type CommandSource = "app" | "skill" | "prompt" | "extension";

export type Command = {
  name: string;
  description: string;
  source: CommandSource;
  /** Aide sur les arguments, affichée après le nom. */
  args?: string;
};

export const APP_COMMANDS: Command[] = [
  { name: "new", description: "Nouvelle session", source: "app" },
  { name: "compact", args: "[consignes]", description: "Résumer la conversation pour libérer du contexte", source: "app" },
  { name: "model", args: "[modèle]", description: "Changer de modèle", source: "app" },
  { name: "thinking", args: "[niveau]", description: "Niveau de réflexion du modèle", source: "app" },
  { name: "name", args: "[nom]", description: "Renommer la session", source: "app" },
  { name: "session", description: "Tokens, coût et contexte utilisés", source: "app" },
  { name: "copy", description: "Copier la dernière réponse", source: "app" },
  { name: "export", args: "[chemin]", description: "Exporter la session en HTML", source: "app" },
  { name: "resume", description: "Afficher les sessions", source: "app" },
  { name: "stop", description: "Arrêter l'agent", source: "app" },
];

/** Commandes renvoyées par `get_commands` (le nom des skills commence par « skill: »). */
export function fromPi(list: any[]): Command[] {
  return (list ?? []).map((c) => ({
    name: String(c.name),
    description: c.description ?? "",
    source: (c.source === "skill" || c.source === "prompt" ? c.source : "extension") as CommandSource,
  }));
}

export type Suggestion = { insert: string; label: string; hint?: string; description?: string; source?: CommandSource };

/**
 * Suggestions pour le texte saisi : noms de commandes après « / », puis valeurs connues
 * pour certains arguments (modèles, niveaux de réflexion).
 */
export function suggest(
  text: string,
  commands: Command[],
  argOptions: Record<string, { value: string; label: string }[]>,
): Suggestion[] {
  const cmd = /^\/(\S*)$/.exec(text);
  if (cmd) {
    const q = cmd[1].toLowerCase();
    const scored = commands
      .map((c) => {
        const n = c.name.toLowerCase();
        const score = n.startsWith(q) ? 0 : n.split(/[:\-_]/).some((p) => p.startsWith(q)) ? 1 : n.includes(q) ? 2 : -1;
        return { c, score };
      })
      .filter((x) => x.score >= 0)
      .sort((a, b) => a.score - b.score || a.c.name.localeCompare(b.c.name));
    return scored.slice(0, 12).map(({ c }) => ({
      insert: `/${c.name} `,
      label: `/${c.name}`,
      hint: c.args,
      description: c.description,
      source: c.source,
    }));
  }
  const arg = /^\/(\S+)\s+(\S*)$/.exec(text);
  if (arg && argOptions[arg[1]]) {
    const q = arg[2].toLowerCase();
    return argOptions[arg[1]]
      .filter((o) => o.value.toLowerCase().includes(q) || o.label.toLowerCase().includes(q))
      .slice(0, 12)
      .map((o) => ({ insert: `/${arg[1]} ${o.value}`, label: o.label, hint: o.value === o.label ? undefined : o.value }));
  }
  return [];
}
