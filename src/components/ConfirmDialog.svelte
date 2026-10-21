<script lang="ts">
  // Petite fenêtre de confirmation (quitter l'app pendant que des runs travaillent…).
  type Props = {
    title: string;
    message: string;
    confirm: string;
    cancel: string;
    danger?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  };
  let { title, message, confirm, cancel, danger = false, onConfirm, onCancel }: Props = $props();
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onCancel()} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="veil" onclick={onCancel}>
  <div class="box" role="alertdialog" aria-modal="true" aria-labelledby="cd-title" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <div class="title" id="cd-title">{title}</div>
    <p>{message}</p>
    <div class="actions">
      <button onclick={onCancel}>{cancel}</button>
      <button class="go" class:danger onclick={onConfirm}>{confirm}</button>
    </div>
  </div>
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: grid;
    place-items: center;
    padding: 16px;
    background: color-mix(in srgb, var(--bg) 55%, transparent);
  }
  .box {
    width: min(440px, 100%);
    padding: 20px 22px 16px;
    border-radius: 14px;
    border: 1px solid var(--line-2);
    background: var(--panel);
    box-shadow: 0 24px 60px var(--shadow);
  }
  .title {
    font-weight: 650;
    color: var(--text-strong);
    margin-bottom: 8px;
  }
  p {
    margin: 0 0 18px;
    color: var(--text-2);
    font-size: 13.5px;
    line-height: 1.5;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  button {
    padding: 7px 14px;
    border-radius: 9px;
    border: 1px solid var(--line-2);
    color: var(--text-2);
  }
  button:hover {
    color: var(--text);
  }
  .go {
    border-color: var(--agent-1);
    color: var(--text-strong);
  }
  .go.danger {
    border-color: var(--del-fg);
    color: var(--del-fg);
  }
</style>
