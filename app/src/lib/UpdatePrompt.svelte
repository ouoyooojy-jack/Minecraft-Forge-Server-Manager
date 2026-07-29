<!--
  Mandatory update prompt.

  Checked once at launch. If a newer version is published the modal opens and
  does not close: no X, no Escape, no backdrop click. The user takes the update
  or quits the app, and quitting only means seeing it again next launch.

  That is a deliberate choice, not laziness about the dismiss path — a server
  manager talks to a live Forge process and a versioned protocol, and letting
  an old client keep driving a moved target is how data gets damaged. The
  button is still a button, though: nothing installs itself behind the user's
  back.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";

  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";
  import { formatBytes } from "./format";

  let update = $state<Update | null>(null);
  let current = $state("");
  let open = $state(false);

  let busy = $state(false);
  let received = $state(0);
  let total = $state<number | null>(null);
  let error = $state<string | null>(null);

  const percent = $derived(total ? (received / total) * 100 : null);

  async function install() {
    if (!update || busy) return;
    busy = true;
    error = null;
    received = 0;
    total = null;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? null;
        if (event.event === "Progress") received += event.data.chunkLength;
      });
      // The installer has run; the app has to restart into the new build.
      await relaunch();
    } catch (e) {
      // Stays open with the button live: a failed download is exactly the
      // situation where the user needs to try again.
      error = e instanceof Error ? e.message : String(e);
      busy = false;
    }
  }

  onMount(async () => {
    // Dev has no signed release behind it, and a missing endpoint is not
    // something to shout about at every startup.
    if (import.meta.env.DEV) return;
    try {
      current = await getVersion();
      const found = await check();
      if (found) {
        update = found;
        open = true;
      }
    } catch (e) {
      // A machine that is offline, or a release feed that is down, must not
      // stop the app from opening.
      console.warn("update check failed:", e);
    }
  });
</script>

{#if update}
  <Modal bind:open title="有新版本" width={440} dismissible={false}>
    <p class="lead">
      <strong>{update.version}</strong> 已經發布，目前使用的是 {current}。
    </p>

    {#if update.body}
      <div class="notes">{update.body}</div>
    {/if}

    <p class="note">更新完成後 app 會自行重新啟動。伺服器資料與設定都不受影響。</p>

    {#if busy}
      <div class="bar" class:indeterminate={percent === null}>
        <div class="fill" style={percent !== null ? `width:${percent}%` : undefined}></div>
      </div>
      <p class="stat tabular">
        {#if percent !== null}
          {percent.toFixed(0)}% · {formatBytes(received)} / {formatBytes(total ?? 0)}
        {:else}
          {formatBytes(received)}
        {/if}
      </p>
    {/if}

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    {#snippet footer()}
      <Button variant="primary" onclick={install} disabled={busy}>
        {busy ? "更新中…" : "立即更新"}
      </Button>
    {/snippet}
  </Modal>
{/if}

<style>
  .lead {
    font-size: var(--font-body);
  }

  .notes {
    margin-top: 12px;
    max-height: 180px;
    overflow: auto;
    padding: 10px 12px;
    background: var(--surface-2);
    border-radius: var(--radius-input);
    color: var(--muted);
    font-size: var(--font-small);
    line-height: 1.7;
    /* Release notes arrive as plain text with real newlines in them. */
    white-space: pre-wrap;
    user-select: text;
  }

  .note {
    margin-top: 12px;
    color: var(--muted);
    font-size: var(--font-small);
    line-height: 1.6;
  }

  .bar {
    height: 6px;
    margin-top: 18px;
    border-radius: var(--radius-pill);
    background: var(--wash);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    width: 0;
    border-radius: var(--radius-pill);
    background: var(--accent);
    transition: width 160ms linear;
  }

  .bar.indeterminate .fill {
    width: 40%;
    animation: slide 1.1s var(--ease) infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(350%);
    }
  }

  .stat {
    margin-top: 8px;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .error {
    margin-top: 12px;
    padding: 10px 12px;
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border-radius: var(--radius-input);
    color: var(--error);
    font-size: var(--font-small);
  }
</style>
