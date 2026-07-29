<!--
  Shared modal shell.

  Every per-server action opens one of these (console, rename, server settings,
  new server, installer), so the scrim, the escape key, the focus trap, and the
  glass treatment live here once.

  Built on <dialog>: the browser gives the top layer, inert background, and
  Escape-to-close for free, which is a chunk of behaviour that is easy to get
  subtly wrong by hand.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  import Icon from "./Icon.svelte";

  let {
    open = $bindable(false),
    title,
    width = 480,
    dismissible = true,
    children,
    footer,
  }: {
    open: boolean;
    title: string;
    width?: number;
    /** Off for a modal the user has to act on: no close button, no backdrop
     *  dismiss, and Escape does nothing. The mandatory update prompt is the
     *  only one — everything else must stay closable. */
    dismissible?: boolean;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  let dialog = $state<HTMLDialogElement>();

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog
  bind:this={dialog}
  style="--modal-width: {width}px"
  onclose={() => (open = false)}
  oncancel={(e) => {
    // <dialog> closes on Escape by itself; this is the only way to refuse.
    if (!dismissible) e.preventDefault();
  }}
  onmousedown={(e) => {
    // Backdrop clicks land on the dialog element itself; anything inside is a
    // descendant. Using mousedown means a drag that starts inside and ends on
    // the backdrop does not dismiss the modal.
    if (dismissible && e.target === dialog) open = false;
  }}
>
  <header>
    <h2>{title}</h2>
    {#if dismissible}
      <button class="close" onclick={() => (open = false)} aria-label="關閉">
        <Icon name="close" size={16} />
      </button>
    {/if}
  </header>

  <div class="body">
    {@render children()}
  </div>

  {#if footer}
    <footer>{@render footer()}</footer>
  {/if}
</dialog>

<style>
  dialog {
    width: min(var(--modal-width), calc(100vw - 48px));
    max-height: calc(100vh - 96px);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface-veil);
    backdrop-filter: blur(28px) saturate(160%);
    color: var(--fg);
    box-shadow: var(--shadow-hi), var(--edge-highlight);
    overflow: hidden;
    animation: rise 180ms var(--ease);
  }

  dialog::backdrop {
    background: rgb(0 0 0 / 40%);
    animation: fade 180ms var(--ease);
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.98);
    }
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--gap-field);
    padding: 14px 14px 14px 20px;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    margin: 0;
    font-size: var(--font-section);
    font-weight: 600;
  }

  .close {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: var(--radius-input);
    background: transparent;
    color: var(--muted);
    cursor: default;
    transition: background-color 120ms var(--ease), color 120ms var(--ease);
  }

  .close:hover {
    background: var(--wash);
    color: var(--fg);
  }

  .body {
    padding: 20px;
    overflow: auto;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--gap-field);
    padding: 14px 20px;
    border-top: 1px solid var(--border);
  }
</style>
