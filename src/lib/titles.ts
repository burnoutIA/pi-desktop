// Titre lisible tiré d'une phrase dictée : sans « dis », sans le nom de l'agent, avec une majuscule.
const FILLERS = /^(?:(?:dis|dis-moi|dites-moi|alors|ok|okay|bon|euh|eh|hey|allô|allo|écoute|stp|s'il te plaît)[\s,.!?]+)+/i;

export function cleanTitle(text: string, agentName: string): string {
  const name = agentName.trim().toLowerCase();
  let t = text.replace(/\s+/g, " ").trim();
  for (let i = 0; i < 3; i++) {
    const before = t;
    t = t.replace(FILLERS, "");
    // Le nom en tête, même mal transcrit (« J'envis, … ») : on retire ce premier mot.
    const [first, ...rest] = t.split(" ");
    const bare = (first ?? "").toLowerCase().replace(/[^a-zà-ÿ]/g, "");
    if (name && bare && (bare === name || (name.length >= 5 && distance(bare, name) <= 2))) t = rest.join(" ");
    t = t.replace(/^[\s,.!?:]+/, "");
    if (t === before) break;
  }
  t = t.replace(/[.,;:\s]+$/, "");
  if (!t) return text.trim();
  return t.charAt(0).toUpperCase() + t.slice(1);
}

function distance(a: string, b: string): number {
  const dp = Array.from({ length: b.length + 1 }, (_, j) => j);
  for (let i = 1; i <= a.length; i++) {
    let prev = dp[0];
    dp[0] = i;
    for (let j = 1; j <= b.length; j++) {
      const tmp = dp[j];
      dp[j] = Math.min(dp[j] + 1, dp[j - 1] + 1, prev + (a[i - 1] === b[j - 1] ? 0 : 1));
      prev = tmp;
    }
  }
  return dp[b.length];
}

/** Nom affiché de l'agent : majuscule initiale (« jarvis » -> « Jarvis »). */
export function displayName(name: string): string {
  const n = name.trim();
  return n ? n.charAt(0).toUpperCase() + n.slice(1) : "pi";
}
