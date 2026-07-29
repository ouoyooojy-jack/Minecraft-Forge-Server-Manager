<!--
  App settings.

  Only the theme so far, and it is real: it applies immediately and survives a
  restart. The .pen's other two fields (HTTP timeout, progress rate) are
  constants in the core with no settings module behind them yet — inputs that
  silently discard what the user types would be worse than not showing them.
-->
<script lang="ts">
  import Icon from "../lib/Icon.svelte";
  import { THEMES, theme, type ThemeName } from "../lib/theme.svelte";

  /**
   * A swatch has to paint the palette it *offers*, not the one in use, so these
   * are the only literal colours in the app. Keep them in step with app.css.
   */
  const SWATCH: Record<ThemeName, Record<string, string>> = {
    dark: { bg: "#131316", rail: "#0e0e11", surface: "#1b1b20", border: "#2b2b32", accent: "#e8e8ec" },
    light: { bg: "#f7f8f9", rail: "#eef0f3", surface: "#ffffff", border: "#e3e6ea", accent: "#16181c" },
    claude: { bg: "#faf9f7", rail: "#f2efea", surface: "#ffffff", border: "#e6e1d9", accent: "#b4530a" },
  };

  const swatchVars = (name: ThemeName) =>
    Object.entries(SWATCH[name])
      .map(([k, v]) => `--s-${k}:${v}`)
      .join(";");
</script>

<section class="page">
  <header><h1>設定</h1></header>

  <div class="setting">
    <h2>主題</h2>
    <p class="sub">立即套用，並記住到下次啟動。</p>

    <div class="grid">
      {#each THEMES as option (option.name)}
        <button
          class="tile"
          class:active={theme.name === option.name}
          onclick={() => theme.set(option.name)}
          aria-pressed={theme.name === option.name}
        >
          <!-- A miniature of the window, so you can see a palette without
               having to switch to it first. -->
          <span class="swatch" style={swatchVars(option.name)}>
            <span class="s-rail"></span>
            <span class="s-body">
              <span class="s-card"></span>
              <span class="s-accent"></span>
            </span>
          </span>
          <span class="name">
            {option.label}
            {#if theme.name === option.name}
              <span class="check"><Icon name="check" size={14} strokeWidth={2.5} /></span>
            {/if}
          </span>
          <span class="desc">{option.desc}</span>
        </button>
      {/each}
    </div>

    <p class="note">
      標題列的按鈕只在深色與淺色之間切換，並會記得你在這裡選的淺色主題。
    </p>
  </div>

  <p class="todo">HTTP 逾時與進度更新頻率目前是核心裡的常數，還沒有設定模組。</p>
</section>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: var(--gap-section);
    padding: 34px 40px 32px;
    overflow: auto;
  }

  h1 {
    margin: 0;
    font-size: var(--font-hero);
    font-weight: 600;
    letter-spacing: -0.4px;
  }

  .setting {
    max-width: 720px;
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface-veil);
    box-shadow: var(--shadow), var(--edge-highlight);
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .sub {
    margin: 4px 0 0;
    font-size: var(--font-small);
    color: var(--muted);
  }

  .grid {
    margin-top: 16px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: var(--gap-field);
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    background: transparent;
    text-align: left;
    transition: border-color 130ms var(--ease), background-color 130ms var(--ease);
  }
  .tile:hover {
    background: var(--wash);
  }
  .tile.active {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .swatch {
    height: 62px;
    display: flex;
    border: 1px solid var(--s-border);
    border-radius: 6px;
    background: var(--s-bg);
    overflow: hidden;
  }
  .s-rail {
    width: 14px;
    flex: none;
    background: var(--s-rail);
  }
  .s-body {
    flex: 1;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .s-card {
    height: 20px;
    border-radius: 4px;
    background: var(--s-surface);
    border: 1px solid var(--s-border);
  }
  .s-accent {
    height: 10px;
    width: 46px;
    border-radius: var(--radius-pill);
    background: var(--s-accent);
  }

  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 600;
  }
  .check {
    display: flex;
    color: var(--accent);
  }
  .desc {
    font-size: var(--font-tiny);
    color: var(--muted);
  }

  .note {
    margin: 16px 0 0;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: var(--font-tiny);
    color: var(--faint);
  }

  .todo {
    margin: 0;
    font-size: var(--font-tiny);
    color: var(--muted);
  }
</style>
