<!--
  The app draws its own title bar (`decorations: false` in tauri.conf.json), so
  dragging and the three window buttons are this component's job.

  Dragging uses `data-tauri-drag-region` rather than a mousedown handler: the
  attribute hands the gesture to the OS, which is what makes snap-to-edge and
  double-click-to-maximise behave the way every other window does.
-->
<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import Guide from "./Guide.svelte";
  import Icon from "./Icon.svelte";
  import { THEMES, theme } from "./theme.svelte";

  let menuOpen = $state(false);
  let guide = $state(false);

  /**
   * Resolved per click, not once at module load. `getCurrentWindow()` needs the
   * Tauri runtime, so calling it up front makes the whole component — and with
   * it the entire shell — fail to mount when the page is opened in a plain
   * browser at localhost:1420. Deferring it keeps that a usable way to iterate
   * on the layout with devtools and instant reload.
   */
  function windowAction(act: (w: ReturnType<typeof getCurrentWindow>) => void) {
    return () => {
      try {
        act(getCurrentWindow());
      } catch {
        // No Tauri runtime: browser preview, where window controls are moot.
      }
    };
  }
</script>

<svelte:window onclick={() => (menuOpen = false)} />

<header class="titlebar" data-tauri-drag-region>
  <span class="name" data-tauri-drag-region>Mc Server Manager</span>

  <div class="controls">
    <!--
      The theme lives here rather than in Settings: it is a thing people flip
      by feel, several times a day, and a preference you have to navigate to is
      one you stop changing. The icon shows the palette in use, not the one you
      would switch to — this is a menu, not a toggle.
    -->
    <div class="theme">
      <button
        class="ctl"
        onclick={(e) => (e.stopPropagation(), (menuOpen = !menuOpen))}
        aria-expanded={menuOpen}
        aria-haspopup="menu"
        title="主題"
        aria-label="主題"
      >
        <Icon name={theme.choice === "system" ? "contrast" : theme.isDark ? "moon" : "sun"} size={15} />
      </button>

      {#if menuOpen}
        <div class="menu" role="menu">
          {#each THEMES as option (option.name)}
            <button
              class="option"
              class:active={theme.choice === option.name}
              role="menuitemradio"
              aria-checked={theme.choice === option.name}
              onclick={() => (theme.set(option.name), (menuOpen = false))}
            >
              <Icon name={option.icon} size={15} />
              {option.label}
            </button>
          {/each}
        </div>
      {/if}
    </div>
    <!--
      Beside the theme for the same reason: both are about the app rather than
      about any one page, and neither belongs to a destination in the rail.
      The walkthrough also opens itself on a first launch — this is how you get
      back to it afterwards.
    -->
    <button class="ctl" onclick={() => (guide = true)} title="使用說明" aria-label="使用說明">
      <Icon name="help" size={15} />
    </button>
    <button
      class="ctl"
      onclick={windowAction((w) => w.minimize())}
      title="最小化"
      aria-label="最小化"
    >
      <Icon name="minimize" size={15} />
    </button>
    <button
      class="ctl"
      onclick={windowAction((w) => w.toggleMaximize())}
      title="最大化"
      aria-label="最大化"
    >
      <Icon name="maximize" size={13} />
    </button>
    <button
      class="ctl close"
      onclick={windowAction((w) => w.close())}
      title="關閉"
      aria-label="關閉"
    >
      <Icon name="close" size={15} />
    </button>
  </div>
</header>

<Guide bind:open={guide} />

<style>
  .titlebar {
    height: var(--titlebar-height);
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-left: 12px;
    /* The window's own rounding, so the drag strip does not square off the
       top corners over the Mica backdrop. */
    border-radius: var(--radius-window) var(--radius-window) 0 0;
  }

  /* The app's own name, so it gets the wordmark treatment the headings get —
     at title-bar size that means the tracking and the weight, not the scale.
     It stays quiet in colour: this is the strip you drag the window by, not a
     place to shout. */
  .name {
    font-family: var(--font-display);
    font-size: 13px;
    font-weight: 600;
    /* Barely any: at 13px Comfortaa's round bowls are already close, and
       pulling them further turns `rm` into one shape. */
    letter-spacing: -0.005em;
    color: var(--muted);
    pointer-events: none;
  }

  .controls {
    display: flex;
    height: 100%;
  }

  .theme {
    position: relative;
    height: 100%;
  }

  .menu {
    position: absolute;
    top: 100%;
    right: 0;
    z-index: 20;
    min-width: 168px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    box-shadow: var(--shadow-modal);
  }

  .option {
    height: var(--h-control);
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-button);
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: var(--font-body);
    text-align: left;
  }

  .option:hover {
    background: var(--wash);
  }

  .option.active {
    background: var(--wash-2);
  }

  .ctl {
    width: 44px;
    height: 100%;
    display: grid;
    place-items: center;
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: default;
    transition: background-color 120ms var(--ease), color 120ms var(--ease);
  }

  .ctl:hover {
    background: var(--wash);
    color: var(--fg);
  }

  .ctl:active {
    background: var(--wash-2);
  }

  /* Close is the one destructive control up here; colouring it on hover is the
     convention every Windows app follows, and the only affordance separating
     it from minimise at a glance. */
  .close:hover,
  .close:active {
    background: #c42b1c;
    color: #fff;
  }

  .ctl:last-child {
    border-top-right-radius: var(--radius-window);
  }
</style>
