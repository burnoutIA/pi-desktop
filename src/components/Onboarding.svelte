<script lang="ts">
  // Assistant de démarrage : installer le pi de l'app (runtime/pi), puis le connecter à un
  // fournisseur d'IA (`/login`). Les deux étapes tournent dans le terminal intégré.
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import Terminal from "./Terminal.svelte";

  type Props = {
    step: "install" | "login";
    piVersion: string | null;
    /** pi vient d'être installé (ou le terminal a été fermé) : l'app revérifie. */
    onInstalled: () => void;
    /** Un identifiant a été enregistré : l'app redémarre pi et recharge les modèles. */
    onConnected: () => void;
    onSkip: () => void;
  };
  let { step, piVersion, onInstalled, onConnected, onSkip }: Props = $props();

  let running = $state(false);

  // Connexion : on guette l'écriture d'un identifiant dans auth.json.
  onMount(() => {
    let base = -1;
    const timer = setInterval(async () => {
      if (step !== "login" || !running) return;
      const m = await api.authMtime();
      if (base < 0) base = m;
      else if (m !== base) {
        running = false;
        onConnected();
      }
    }, 1500);
    return () => clearInterval(timer);
  });
</script>

<div class="onboarding">
  <div class="card">
    <div class="hello">Bienvenue dans Pi Voice</div>
    <p class="intro">
      Pi Voice est une interface vocale pour <b>pi</b>, un agent de code qui tourne sur ta machine. Deux étapes
      avant de commencer :
    </p>

    <ol class="steps">
      <li class:done={step === "login"} class:active={step === "install"}>
        <span class="num">{step === "login" ? "✓" : "1"}</span>
        <div>
          <div class="st-title">Installer pi et ses outils</div>
          <div class="st-desc">
            {#if step === "login"}
              Installé{piVersion ? ` (version ${piVersion})` : ""}.
            {:else}
              Pi Voice installe sa propre copie de pi, sans toucher à un pi déjà installé sur ta machine, avec
              deux outils : ketch pour chercher sur internet et Playwright pour piloter un navigateur. Il faut
              Node.js 22 ou plus.
            {/if}
          </div>
        </div>
      </li>
      <li class:active={step === "login"}>
        <span class="num">2</span>
        <div>
          <div class="st-title">Connecter un fournisseur d'IA</div>
          <div class="st-desc">
            Un abonnement (Claude, ChatGPT, GitHub Copilot…) ou une clé API (DeepSeek, OpenRouter, Mistral…). Choisis
            dans le menu de pi : il ouvre ton navigateur ou te demande ta clé, puis Pi Voice continue tout seul.
          </div>
        </div>
      </li>
    </ol>

    {#if running}
      <div class="term-wrap">
        {#if step === "install"}
          <Terminal program="scripts/setup-pi.sh" onExit={() => ((running = false), onInstalled())} />
        {:else}
          <Terminal program="pi" inject={"/login\r"} onExit={() => (running = false)} />
        {/if}
      </div>
      <div class="hint">
        {step === "install"
          ? "Le terminal se ferme à la fin de l'installation."
          : "Tape /login si le menu n'apparaît pas. Pi Voice reprend dès que l'identifiant est enregistré."}
      </div>
    {:else}
      <div class="actions">
        <button class="primary" onclick={() => (running = true)}>
          {step === "install" ? "Installer pi" : "Connecter un fournisseur"}
        </button>
        {#if step === "login"}
          <button class="ghost" onclick={onSkip} title="Si tes clés passent par des variables d'environnement">
            Passer
          </button>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .onboarding {
    height: 100%;
    overflow-y: auto;
    display: flex;
    justify-content: center;
    padding: 28px 22px;
  }
  .card {
    width: min(760px, 100%);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .hello {
    font-size: 24px;
    font-weight: 750;
    color: var(--text-strong);
  }
  .intro {
    margin: 0;
    color: var(--text-2);
  }
  .intro b {
    color: var(--text-strong);
  }
  .steps {
    list-style: none;
    margin: 4px 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .steps li {
    display: flex;
    gap: 12px;
    padding: 12px 14px;
    border-radius: 12px;
    border: 1px solid var(--line);
    opacity: 0.6;
  }
  .steps li.active {
    opacity: 1;
    border-color: var(--agent-line);
    background: var(--agent-faint);
  }
  .steps li.done {
    opacity: 1;
  }
  .num {
    flex: none;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-size: 12px;
    font-weight: 700;
    background: var(--panel-2);
    color: var(--text-2);
  }
  .active .num {
    background: var(--agent-1);
    color: var(--on-accent);
  }
  .done .num {
    background: var(--ok);
    color: #fff;
  }
  .st-title {
    font-weight: 650;
    color: var(--text-strong);
  }
  .st-desc {
    font-size: 13px;
    color: var(--text-2);
  }
  .term-wrap {
    height: 380px;
    border-radius: 10px;
    overflow: hidden;
    border: 1px solid var(--line-2);
  }
  .hint {
    font-size: 12px;
    color: var(--text-3);
  }
  .actions {
    display: flex;
    gap: 10px;
  }
  .primary {
    padding: 9px 18px;
    border-radius: 10px;
    background: var(--accent-grad);
    color: var(--on-accent);
    font-weight: 650;
  }
  .ghost {
    padding: 9px 14px;
    border-radius: 10px;
    color: var(--text-2);
  }
  .ghost:hover {
    background: var(--panel-2);
  }
</style>
