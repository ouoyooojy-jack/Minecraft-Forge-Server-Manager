<!--
  The two button shapes the .pen files use: an accent fill and a bordered
  surface. Defined once so the radius, height, and press feedback cannot drift
  between the download page and the modals.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    variant = "secondary",
    disabled = false,
    danger = false,
    onclick,
    children,
    ...rest
  }: {
    variant?: "primary" | "secondary";
    disabled?: boolean;
    danger?: boolean;
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
    [key: string]: unknown;
  } = $props();
</script>

<button class={variant} class:danger {disabled} {onclick} {...rest}>
  {@render children()}
</button>

<style>
  button {
    /* Row, not a block: an <Icon> is a block-level svg, so without this it
       stacks above the label instead of sitting beside it. */
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 40px;
    padding: 0 18px;
    border-radius: var(--radius-button);
    border: 1px solid var(--border);
    cursor: default;
    transition:
      background-color 120ms var(--ease),
      border-color 120ms var(--ease),
      opacity 120ms var(--ease),
      transform 80ms var(--ease);
  }

  button:active:not(:disabled) {
    /* A tiny press displacement is most of what makes a control feel physical. */
    transform: translateY(1px);
  }

  button:disabled {
    opacity: 0.45;
  }

  .primary {
    background: var(--accent);
    color: var(--accent-fg);
    border-color: transparent;
  }

  .primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .secondary {
    background: var(--surface-veil);
    color: var(--fg);
  }

  .secondary:hover:not(:disabled) {
    background: var(--wash-2);
    border-color: var(--muted);
  }

  .danger {
    color: var(--error);
    border-color: color-mix(in srgb, var(--error) 40%, transparent);
  }

  .danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--error) 14%, transparent);
    border-color: var(--error);
  }
</style>
