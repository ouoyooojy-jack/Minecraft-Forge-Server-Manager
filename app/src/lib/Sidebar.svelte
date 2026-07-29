<!--
  The 48px rail from the .pen files, which opens to 180px on the hamburger.

  Labels are always in the DOM, not swapped in on expand: rendering them and
  clipping the rail keeps the icons from shifting sideways as the width
  animates, and leaves the accessible name present at both widths.
-->
<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";

  type Dest = { label: string; icon: IconName };

  const DESTS: Dest[] = [
    { label: "主頁", icon: "home" },
    { label: "下載", icon: "download" },
    { label: "遠端", icon: "server" },
    { label: "設定", icon: "settings-2" },
  ];

  let {
    selected = $bindable(0),
    onnavigate,
  }: {
    selected: number;
    /** Fires on every rail click, including one that picks the destination
     *  already showing — that click is how you leave a sub-page. */
    onnavigate?: (index: number) => void;
  } = $props();
  let expanded = $state(false);
</script>

<nav class="rail" class:expanded aria-label="主導覽">
  <button
    class="item"
    onclick={() => (expanded = !expanded)}
    aria-expanded={expanded}
    title={expanded ? "收合選單" : "展開選單"}
  >
    <span class="glyph"><Icon name="menu" /></span>
    <span class="label">收合</span>
  </button>

  <div class="spacer"></div>

  {#each DESTS as dest, index (dest.icon)}
    <button
      class="item"
      class:active={selected === index}
      onclick={() => ((selected = index), onnavigate?.(index))}
      aria-current={selected === index ? "page" : undefined}
      title={dest.label}
    >
      <span class="glyph"><Icon name={dest.icon} /></span>
      <span class="label">{dest.label}</span>
    </button>
  {/each}
</nav>

<style>
  .rail {
    width: var(--sidebar-width);
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 18px 0;
    background: var(--rail-veil);
    overflow: hidden;
    transition: width 200ms var(--ease);
  }

  .rail.expanded {
    width: var(--sidebar-width-open);
    align-items: stretch;
    padding: 18px 12px;
  }

  .spacer {
    height: 4px;
  }

  /* A raised tile rather than a tinted row: the active destination reads as a
     surface lifted out of the rail, which works at both widths without a
     separate marker element to position. */
  .item {
    height: 40px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius-button);
    background: transparent;
    color: var(--muted);
    white-space: nowrap;
    transition: background-color 130ms var(--ease), color 130ms var(--ease),
      border-color 130ms var(--ease);
  }

  .rail:not(.expanded) .item {
    width: 40px;
  }

  /* Fixed-width box around the glyph so the icon sits at the same x at both
     rail widths and does not slide during the transition. */
  .glyph {
    width: 38px;
    display: grid;
    place-items: center;
    flex: none;
  }

  .label {
    font-size: var(--font-body);
    opacity: 0;
    transition: opacity 140ms var(--ease);
  }

  .rail.expanded .label {
    opacity: 1;
  }

  .item:hover {
    background: var(--wash);
    color: var(--fg);
  }

  .item.active {
    background: var(--surface);
    border-color: var(--border);
    color: var(--accent);
    box-shadow: var(--shadow);
  }
</style>
