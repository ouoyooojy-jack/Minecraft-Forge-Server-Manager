<!--
  App shell: title bar over a rail plus a content area, matching the `#window`
  frame every .pen file shares.

  Navigation is an index rather than a router. Three destinations that never
  take parameters and never need a back button do not need URL routing, and a
  router would put a history stack between the sidebar and the view for nothing.
-->
<script lang="ts">
  import Sidebar from "./lib/Sidebar.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import Downloads from "./views/Downloads.svelte";
  import Home from "./views/Home.svelte";
  import Remote from "./views/Remote.svelte";
  import ServerDetail from "./views/ServerDetail.svelte";
  import Settings from "./views/Settings.svelte";
  import UpdatePrompt from "./lib/UpdatePrompt.svelte";
  import type { ServerId } from "./lib/types";

  let selected = $state(0);
  /** Non-null while one server's detail page is open. It sits *inside* home
   *  rather than beside it: the rail still highlights 首頁, and leaving the
   *  page is a back button, not a fourth destination. */
  let detail = $state<ServerId | null>(null);
</script>

<!-- Outside the window frame: it is modal over everything, including the
     title bar, and it must not depend on which page is showing. -->
<UpdatePrompt />

<div class="window">
  <TitleBar />
  <div class="body">
    <Sidebar bind:selected onnavigate={() => (detail = null)} />
    <main>
      {#if selected === 0 && detail !== null}
        <ServerDetail id={detail} back={() => (detail = null)} />
      {:else if selected === 0}
        <Home
          goToDownloads={() => (selected = 1)}
          openServer={(id) => (detail = id)}
        />
      {:else if selected === 1}
        <Downloads />
      {:else if selected === 2}
        <Remote />
      {:else}
        <Settings />
      {/if}
    </main>
  </div>
</div>

<style>
  .window {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-veil);
    /* Matches the Windows 11 corner radius; the window itself is transparent,
       so the rounding shows the Mica backdrop rather than a clipped rectangle. */
    border-radius: var(--radius-window);
    overflow: hidden;
  }

  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  main {
    flex: 1;
    min-width: 0;
  }
</style>
