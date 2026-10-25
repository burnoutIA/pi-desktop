// Un run : un pi qui tourne sur un projet, ou sur une branche de travail du projet (git worktree)
// quand un autre run occupe déjà le projet. Chaque run a sa conversation, son modèle, ses
// dialogues en attente ; il continue de tourner quand on en affiche un autre.

import { api, type SessionInfo } from "./api";
import { Session } from "./session.svelte";

export type Branch = {
  /** Dossier de travail de la branche (data/worktrees/…). */
  path: string;
  branch: string;
  /** Branche du projet où le travail sera fusionné. */
  base: string;
};

/** Pourquoi un run en arrière-plan demande ton attention. */
/** Run d'un dot : sa conversation vit dans son conteneur, l'app s'y branche. */
export type DotRef = { name: string; dir: string };

export type Attention = "done" | "error" | "waiting" | null;

export class Run {
  readonly id: string;
  /** Dossier du projet. */
  readonly root: string;
  readonly branch: Branch | null;
  readonly dot: DotRef | null;
  /** Nom affiché (titre de la session pour une branche). */
  label = $state("");
  session = new Session();
  piUp = $state(false);
  sessions = $state<SessionInfo[]>([]);
  sessionPath = $state<string | null>(null);
  /** Modèle de ce run (« fournisseur/id »). */
  modelKey = $state<string | null>(null);
  /** Dialogues des extensions de pi en attente de réponse. */
  uiRequests = $state<any[]>([]);
  attention = $state<Attention>(null);
  /** Résumé oral produit pendant que le run était en arrière-plan : lu quand on y revient. */
  pendingOral = $state<string | null>(null);
  /** Sessions déjà titrées (titre généré une fois). */
  titled = new Set<string>();

  constructor(root: string, branch: Branch | null = null, label = "", dot: DotRef | null = null) {
    this.root = root;
    this.branch = branch;
    this.dot = dot;
    this.id = dot ? `d:${dot.dir}` : branch ? `b:${branch.path}` : `p:${root}`;
    this.label = label;
  }

  /** Un dot : son espace de travail (`state/workspace`, `/dot/workspace` dans le conteneur). */
  static forDot(dot: DotRef) {
    return new Run(`${dot.dir}/state/workspace`, null, "", dot);
  }

  /** Dossier où travaille pi. */
  get cwd() {
    return this.branch?.path ?? this.root;
  }

  get projectName() {
    if (this.dot) return this.dot.name;
    return this.root.split("/").filter(Boolean).pop() ?? this.root;
  }

  send(cmd: Record<string, unknown>) {
    return api.piSend(this.id, cmd);
  }

  write(record: Record<string, unknown>) {
    return api.piWrite(this.id, record);
  }
}

/** Consignes données à pi quand il travaille sur une branche. */
export function branchPrompt(r: Run): string | null {
  if (!r.branch) return null;
  return (
    `Tu travailles dans un dossier de travail git séparé (${r.branch.path}), sur la branche ${r.branch.branch}, ` +
    `copie du projet ${r.root} prise au dernier commit de ${r.branch.base}. Un autre travail peut avancer en même temps ` +
    `dans le projet principal : reste dans ton dossier. Les fichiers non suivis par git (node_modules, .env, builds…) ` +
    `n'y sont pas : installe les dépendances si tu en as besoin. Ne fusionne pas toi-même dans ${r.branch.base} : ` +
    `la personne le fera depuis l'application quand ton travail sera prêt.`
  );
}

/** Runs sur branche gardés d'une ouverture de l'app à l'autre (leur travail n'est pas encore fusionné). */
const KEY = "branchRuns";

export function loadBranchRuns(): Run[] {
  try {
    const list = JSON.parse(localStorage.getItem(KEY) ?? "[]") as { root: string; branch: Branch; label: string }[];
    return list.map((x) => new Run(x.root, x.branch, x.label));
  } catch {
    return [];
  }
}

export function saveBranchRuns(runs: Run[]) {
  try {
    const list = runs.filter((r) => r.branch).map((r) => ({ root: r.root, branch: r.branch, label: r.label }));
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    /* stockage indisponible */
  }
}
