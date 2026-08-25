<!--
  "This needs Java N — get it?"

  Raised by whatever launch just failed, not by a check on startup. Java is
  40-60 MB and most machines running a Minecraft server already have one, so
  nothing is fetched until a real launch has actually been blocked by it.

  On success it calls `onready`, which is the caller's retry — the user pressed
  a button that said 啟動, and finishing the download should finish that, not
  hand them the same button again.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";
  import { cancelDownload, installJava, onCoreEvent } from "./api";
  import { formatBytes, formatSpeed } from "./format";
  import {
    downloadPercent,
    errorMessage,
    isCoreError,
    type DownloadProgress,
  } from "./types";

  let {
    open = $bindable(false),
    major,
    onready,
  }: {
    open: boolean;
    /** Java major version the blocked launch needs. */
    major: number;
    /** Retry whatever failed. Called once the runtime is installed. */
    onready: () => void;
  } = $props();

  let busy = $state(false);
  let progress = $state<DownloadProgress | null>(null);
  let error = $state<string | null>(null);

  const percent = $derived(progress ? downloadPercent(progress) : null);

  async function download() {
    busy = true;
    error = null;
    try {
      await installJava(major);
      open = false;
      onready();
    } catch (e) {
      // Cancelling is the user's own doing; the dialog stays as it was rather
      // than accusing them of a failure.
      if (!isCoreError(e) || e.kind !== "downloadAborted") error = errorMessage(e);
    } finally {
      busy = false;
      progress = null;
    }
  }

  /** Stop the transfer if one is in flight; otherwise just close. */
  function cancel() {
    if (busy && progress) {
      cancelDownload(progress.id);
      return;
    }
    open = false;
  }

  onMount(() => {
    const unlisten = onCoreEvent((event) => {
      if (event.type === "download" && event.what.kind === "jre") progress = event;
    });
    return () => void unlisten.then((fn) => fn());
  });
</script>

<Modal bind:open title="需要 Java {major}" width={440}>
  <p class="lead">
    這個版本要用 Java {major} 才能執行，這台電腦上沒有找到。
  </p>
  <p class="note">
    會從 Adoptium 下載官方的 JRE（40–60 MB，視版本而定），只給這個 app 用，不會動到系統既有的 Java。
  </p>

  {#if busy}
    <div class="bar" class:indeterminate={percent === null}>
      <div class="fill" style={percent !== null ? `width:${percent}%` : undefined}></div>
    </div>
    <p class="stat tabular">
      {#if progress}
        {percent === null ? formatBytes(progress.received) : `${percent.toFixed(0)}%`}
        · {formatSpeed(progress.bytesPerSec)}
      {:else}
        準備中…
      {/if}
    </p>
  {/if}

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#snippet footer()}
    <!-- While a transfer is running, cancelling means cancelling *it* — tens of megabytes
         is long enough that closing the dialog and leaving it running would be
         the wrong reading of the button. -->
    <Button onclick={cancel} disabled={busy && !progress}>取消</Button>
    <Button variant="primary" onclick={download} disabled={busy}>
      {busy ? "下載中…" : `下載 Java ${major}`}
    </Button>
  {/snippet}
</Modal>

<style>
  .lead {
    font-size: var(--font-body);
  }

  .note {
    margin-top: 8px;
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

  /* No Content-Length means no honest percentage — a bar that moves without
     claiming a number is better than one that invents one. */
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
