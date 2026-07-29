<!--
  The app draws its own title bar (`decorations: false` in tauri.conf.json), so
  dragging and the three window buttons are this component's job.

  Dragging uses `data-tauri-drag-region` rather than a mousedown handler: the
  attribute hands the gesture to the OS, which is what makes snap-to-edge and
  double-click-to-maximise behave the way every other window does.
-->
<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import Icon from "./Icon.svelte";
  import { theme } from "./theme.svelte";

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

<header class="titlebar" data-tauri-drag-region>
  <span class="name" data-tauri-drag-region>Mc Server Manager</span>

  <div class="controls">
    <!--
      Light ↔ dark only. Which light palette it returns to is whatever was last
      chosen in Settings, so picking "claude" there is not undone by using this.
      The icon shows the mode you would switch *to*.
    -->
    <button
      class="ctl"
      onclick={() => theme.toggleMode()}
      title={theme.isDark ? "切換至淺色主題" : "切換至深色主題"}
      aria-label="切換深色或淺色主題"
    >
      <Icon name={theme.isDark ? "sun" : "moon"} size={15} />
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

  .name {
    font-size: var(--font-small);
    color: var(--muted);
    pointer-events: none;
  }

  .controls {
    display: flex;
    height: 100%;
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
