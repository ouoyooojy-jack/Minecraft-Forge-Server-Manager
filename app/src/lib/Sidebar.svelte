<!--
  A column of four words.

  No fill behind the rail and no highlight block behind the current
  destination: the page and the rail are the same surface, and the only mark
  is a 2px rule on the active label. Alignment does the separating, which is
  the whole premise of this design — a slab of tinted background would be a
  second, redundant boundary.

  The icons are gone with the collapse: a rail this narrow that always shows
  its labels has nothing to collapse to, and an icon beside a word it repeats
  is decoration.
-->
<script lang="ts">
  const DESTS = ["首頁", "下載", "遠端", "設定"];

  let {
    selected,
    onnavigate,
  }: {
    selected: number;
    /** Fires on every rail click, including one that picks the destination
     *  already showing — that click is how you leave a sub-page. The shell
     *  decides whether to go: a page with unsaved edits asks first. */
    onnavigate: (index: number) => void;
  } = $props();
</script>

<nav aria-label="主導覽">
  {#each DESTS as label, index (label)}
    <button
      class="item"
      class:active={selected === index}
      onclick={() => onnavigate(index)}
      aria-current={selected === index ? "page" : undefined}
    >
      {label}
    </button>
  {/each}
</nav>

<style>
  nav {
    width: var(--sidebar-width);
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 26px 0 0 20px;
    background: var(--rail-veil);
  }

  .item {
    height: 30px;
    display: flex;
    align-items: center;
    padding: 0 10px;
    /* The marker sits in the padding the whole column shares, so the labels
       stay on one baseline whether or not they carry it. */
    border-left: 2px solid transparent;
    margin-left: -2px;
    background: none;
    border-top: 0;
    border-right: 0;
    border-bottom: 0;
    color: var(--muted);
    font: inherit;
    font-size: var(--font-body);
    text-align: left;
    transition: color 120ms var(--ease);
  }

  .item:hover {
    color: var(--fg);
  }

  .item.active {
    border-left-color: var(--accent);
    color: var(--fg);
    font-weight: 500;
  }
</style>
