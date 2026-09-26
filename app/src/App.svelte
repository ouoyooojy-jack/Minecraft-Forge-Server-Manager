<!--
  App shell: title bar over a rail plus a content area, matching the `#window`
  frame every .pen file shares.

  Navigation is an index rather than a router. Four destinations that never
  take parameters do not need URL routing, and a router would put a history
  stack between the sidebar and the view for nothing.
-->
<script lang="ts">
  import Button from "./lib/Button.svelte";
  import Guide from "./lib/Guide.svelte";
  import Modal from "./lib/Modal.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import Downloads from "./views/Downloads.svelte";
  import Home from "./views/Home.svelte";
  import Remote from "./views/Remote.svelte";
  import ServerDetail from "./views/ServerDetail.svelte";
  import Settings from "./views/Settings.svelte";
  import UpdatePrompt from "./lib/UpdatePrompt.svelte";
  import type { ServerId } from "./lib/types";
  import { unsaved } from "./lib/unsaved.svelte";

  let selected = $state(0);
  /** Non-null while one server's detail page is open. It sits *inside* home
   *  rather than beside it: the rail still highlights 首頁, and leaving the
   *  page is a back button, not a fourth destination. */
  let detail = $state<ServerId | null>(null);

  /** The walkthrough, on the first launch only. Keyed by a flag rather than by
   *  "are there no servers yet", because someone who deleted their only server
   *  is not a new user and should not be walked through it again. */
  const GUIDE_SEEN = "guide.seen";
  let guide = $state(localStorage.getItem(GUIDE_SEEN) !== "1");
  $effect(() => {
    if (!guide) localStorage.setItem(GUIDE_SEEN, "1");
  });

  /** Where the user asked to go while the page had unsaved edits. */
  let leaving = $state<(() => void) | null>(null);
  let leaveOpen = $state(false);

  function go(next: () => void) {
    if (!unsaved.dirty) return next();
    leaving = next;
    leaveOpen = true;
  }

  function leaveAnyway() {
    unsaved.dirty = false;
    leaveOpen = false;
    leaving?.();
    leaving = null;
  }
</script>

<!-- Outside the window frame: it is modal over everything, including the
     title bar, and it must not depend on which page is showing. -->
<UpdatePrompt />
<Guide bind:open={guide} />

<div class="window">
  <TitleBar onguide={() => (guide = true)} />
  <div class="body">
    <Sidebar
      {selected}
      onnavigate={(index) => go(() => ((selected = index), (detail = null)))}
    />
    <main>
      {#if selected === 0 && detail !== null}
        <ServerDetail id={detail} back={() => go(() => (detail = null))} />
      {:else if selected === 0}
        <Home openServer={(id) => (detail = id)} />
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

<Modal bind:open={leaveOpen} title="有設定還沒儲存" width={420}>
  <p class="leave">離開這一頁，剛才改的設定就不會生效。要回去按「儲存」嗎？</p>
  {#snippet footer()}
    <Button onclick={leaveAnyway}>不儲存，離開</Button>
    <Button variant="primary" onclick={() => (leaveOpen = false)}>回去儲存</Button>
  {/snippet}
</Modal>

<style>
  .leave {
    color: var(--muted);
    font-size: var(--font-body);
    line-height: 1.7;
  }

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
