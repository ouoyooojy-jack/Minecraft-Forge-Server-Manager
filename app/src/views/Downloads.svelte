<!--
  Downloads: pick a Forge build, fetch it, manage the jars on disk.

  Two dropdowns rather than one list because Forge has published thousands of
  builds — picking a Minecraft line first cuts that to a readable number.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import Button from "../lib/Button.svelte";
  import Icon from "../lib/Icon.svelte";
  import {
    cancelDownload,
    deleteDownload,
    downloadForge,
    listDownloads,
    listForgeVersions,
    onCoreEvent,
  } from "../lib/api";
  import { formatBytes, formatSpeed } from "../lib/format";
  import {
    downloadPercent,
    errorMessage,
    type DownloadProgress,
    type DownloadedFile,
    type ForgeVersionGroup,
  } from "../lib/types";

  let groups = $state<ForgeVersionGroup[]>([]);
  let loadingVersions = $state(true);
  let versionError = $state<string | null>(null);

  let major = $state("");
  let version = $state("");

  let files = $state<DownloadedFile[]>([]);
  let error = $state<string | null>(null);

  /** In-flight and just-finished transfers, keyed by id. */
  let active = $state<Record<number, DownloadProgress>>({});

  const versions = $derived(groups.find((g) => g.mcMajor === major)?.versions ?? []);
  const activeList = $derived(Object.values(active));

  /** Matching on the version string avoids restating the core's naming rule. */
  const alreadyOnDisk = $derived(
    !!version && files.some((f) => f.name.includes(version)),
  );
  /**
   * Any transfer at all, not just this version.
   *
   * The core only runs one at a time — two writers share a `.part` file and
   * truncate each other — so the button has to reflect that rule rather than
   * let the user discover it as an error.
   */
  const busy = $derived(activeList.some((d) => d.state.kind === "running"));

  const totalBytes = $derived(files.reduce((sum, f) => sum + f.bytes, 0));

  function label(d: DownloadProgress) {
    return d.what.kind === "forgeInstaller"
      ? `Forge ${d.what.version}`
      : `JRE ${d.what.major}`;
  }

  /** `forge-1.20.1-47.2.0-installer.jar` → `Forge 1.20.1-47.2.0`. */
  function prettyName(name: string) {
    const m = name.match(/^forge-(.+)-installer\.jar$/i);
    return m ? `Forge ${m[1]}` : name;
  }

  async function loadVersions(refresh = false) {
    loadingVersions = true;
    versionError = null;
    try {
      groups = await listForgeVersions(refresh);
      if (!groups.some((g) => g.mcMajor === major)) {
        major = groups[0]?.mcMajor ?? "";
        version = groups[0]?.versions[0] ?? "";
      }
    } catch (e) {
      versionError = errorMessage(e);
    } finally {
      loadingVersions = false;
    }
  }

  async function refreshFiles() {
    try {
      files = await listDownloads();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  function pickMajor(value: string) {
    major = value;
    // The previously selected build belongs to the old line; default to the
    // newest of the new one rather than leaving a stale, mismatched value.
    version = groups.find((g) => g.mcMajor === value)?.versions[0] ?? "";
  }

  async function start() {
    if (!version) return;
    try {
      await downloadForge(version);
      error = null;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function remove(file: DownloadedFile) {
    try {
      await deleteDownload(file.path);
      await refreshFiles();
    } catch (e) {
      error = errorMessage(e);
    }
  }

  onMount(() => {
    loadVersions();
    refreshFiles();

    const unlisten = onCoreEvent((event) => {
      if (event.type !== "download") return;
      active[event.id] = event;

      if (event.state.kind !== "running") {
        // A finished transfer stays on screen briefly so the outcome is
        // readable, then clears itself. Failures linger longer — the message
        // is the whole point of showing them.
        refreshFiles();
        const linger = event.state.kind === "failed" ? 8000 : 2500;
        setTimeout(() => delete active[event.id], linger);
      }
    });
    return () => void unlisten.then((fn) => fn());
  });
</script>

<section class="page">
  <header>
    <div>
      <h1>下載</h1>
      <p class="sub">Forge 官方 Maven 的所有發行版本。</p>
    </div>
    <button
      class="ghost"
      onclick={() => loadVersions(true)}
      disabled={loadingVersions}
      title="重新取得版本清單"
    >
      <Icon name="refresh" size={15} />
      重新整理
    </button>
  </header>

  <div class="picker">
    {#if versionError}
      <div class="banner">
        <span>{versionError}</span>
        <Button onclick={() => loadVersions(true)}>重試</Button>
      </div>
    {:else}
      <label class="field">
        <span>Minecraft version</span>
        <div class="select">
          <select
            value={major}
            disabled={loadingVersions}
            onchange={(e) => pickMajor(e.currentTarget.value)}
          >
            {#each groups as group (group.mcMajor)}
              <option value={group.mcMajor}>{group.mcMajor}</option>
            {/each}
          </select>
          <Icon name="chevron-down" size={14} />
        </div>
      </label>

      <label class="field">
        <span>Forge version</span>
        <div class="select wide">
          <select bind:value={version} disabled={loadingVersions || !versions.length}>
            {#each versions as v (v)}
              <option value={v}>{v}</option>
            {/each}
          </select>
          <Icon name="chevron-down" size={14} />
        </div>
      </label>

      <Button
        variant="primary"
        disabled={loadingVersions || !version || busy || alreadyOnDisk}
        onclick={start}
      >
        {#if loadingVersions}
          載入版本中…
        {:else if alreadyOnDisk}
          已下載
        {:else if busy}
          下載中…
        {:else}
          下載安裝檔
        {/if}
      </Button>
    {/if}
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if activeList.length}
    <div class="transfers">
      {#each activeList as d (d.id)}
        {@const percent = downloadPercent(d)}
        <div class="transfer" class:failed={d.state.kind === "failed"}>
          <div class="line">
            <span class="name">{label(d)}</span>
            <span class="status tabular">
              {#if d.state.kind === "running"}
                {percent === null ? formatBytes(d.received) : `${percent.toFixed(0)}%`}
                · {formatSpeed(d.bytesPerSec)}
              {:else if d.state.kind === "done"}
                完成
              {:else if d.state.kind === "cancelled"}
                已取消
              {:else}
                {d.state.message}
              {/if}
            </span>
            {#if d.state.kind === "running"}
              <button class="ghost small" onclick={() => cancelDownload(d.id)}>取消</button>
            {/if}
          </div>

          <!-- Indeterminate when the server sent no Content-Length: a made-up
               percentage is worse than admitting the size is unknown. -->
          <div class="bar" class:indeterminate={percent === null && d.state.kind === "running"}>
            <div
              class="fill"
              class:done={d.state.kind === "done"}
              style={percent !== null ? `width:${percent}%` : undefined}
            ></div>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <div class="group-head">
    <h2>已下載</h2>
    <span class="n tabular">{files.length}</span>
    {#if files.length}
      <span class="size tabular">{formatBytes(totalBytes)}</span>
    {/if}
    <span class="line-rule"></span>
  </div>

  {#if files.length === 0}
    <p class="empty">還沒有安裝檔。選一個版本按「下載安裝檔」。</p>
  {:else}
    <ul class="files">
      {#each files as file (file.path)}
        <li>
          <span class="jar">
            <span class="jar-name">{prettyName(file.name)}</span>
            <span class="jar-file">{file.name}</span>
          </span>
          <span class="jar-size tabular">{formatBytes(file.bytes)}</span>
          <button
            class="icon-btn"
            onclick={() => remove(file)}
            title="刪除"
            aria-label="刪除 {file.name}"
          >
            <Icon name="trash" size={16} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
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

  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--gap-section);
  }
  /* A wordmark, not a heading. On a geometric face the authority comes from
     the drawing — even stroke, circular bowls — and a heavy weight destroys
     exactly that by thickening the monoline into a slab. Weight stays at 500
     and the letters are pulled together instead; that tightening is what makes
     a large geometric line read as set rather than merely enlarged. */
  h1 {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--font-hero);
    font-weight: 500;
    letter-spacing: -0.02em;
  }
  .sub {
    margin: 4px 0 0;
    font-size: var(--font-small);
    color: var(--muted);
  }

  .ghost {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 34px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    background: var(--surface-veil);
    color: var(--muted);
    font-size: var(--font-small);
    transition: color 120ms var(--ease), background-color 120ms var(--ease);
  }
  .ghost:hover:not(:disabled) {
    background: var(--wash);
    color: var(--fg);
  }
  .ghost:disabled {
    opacity: 0.5;
  }
  .ghost.small {
    height: 26px;
    padding: 0 10px;
    font-size: var(--font-tiny);
  }

  /* ── picker ──────────────────────────────────────────── */

  .picker {
    display: flex;
    align-items: flex-end;
    gap: var(--gap-field);
    flex-wrap: wrap;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field > span {
    font-size: var(--font-tiny);
    color: var(--muted);
  }

  /* Native <select> for the keyboard and the OS popup; the chevron is ours
     because the platform one cannot be recoloured to follow the theme. */
  .select {
    position: relative;
    display: flex;
    align-items: center;
  }
  .select select {
    appearance: none;
    width: 170px;
    height: 42px;
    padding: 0 34px 0 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    background: var(--surface-veil);
    color: var(--fg);
    font: inherit;
    transition: border-color 130ms var(--ease);
  }
  .select.wide select {
    width: 230px;
  }
  .select select:focus {
    border-color: var(--accent);
    outline: none;
  }
  .select select:disabled {
    opacity: 0.5;
  }
  .select :global(svg) {
    position: absolute;
    right: 12px;
    color: var(--muted);
    pointer-events: none;
  }

  .banner {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--gap-field);
    padding: 12px 14px;
    border: 1px solid color-mix(in srgb, var(--error) 40%, transparent);
    border-radius: var(--radius-button);
    background: color-mix(in srgb, var(--error) 10%, transparent);
    color: var(--error);
    font-size: var(--font-small);
  }

  .error {
    margin: 0;
    color: var(--error);
    font-size: var(--font-small);
  }
  .empty {
    margin: 0;
    color: var(--muted);
  }

  /* ── transfers ───────────────────────────────────────── */

  .transfers {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .transfer {
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    background: var(--surface-veil);
  }
  .transfer.failed {
    border-color: color-mix(in srgb, var(--error) 40%, transparent);
  }
  .transfer .line {
    display: flex;
    align-items: center;
    gap: var(--gap-field);
    margin-bottom: 10px;
  }
  .transfer .name {
    font-size: var(--font-small);
    font-weight: 600;
  }
  .transfer .status {
    margin-left: auto;
    font-size: var(--font-tiny);
    color: var(--muted);
  }
  .transfer.failed .status {
    color: var(--error);
  }

  .bar {
    height: 6px;
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
  .fill.done {
    width: 100%;
    background: var(--online);
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
      transform: translateX(250%);
    }
  }

  /* ── file list ───────────────────────────────────────── */

  .group-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .group-head h2 {
    margin: 0;
    font-size: var(--font-tiny);
    font-weight: 700;
    letter-spacing: 0.9px;
    text-transform: uppercase;
    color: var(--muted);
  }
  .group-head .n {
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    display: grid;
    place-items: center;
    border-radius: var(--radius-pill);
    background: var(--wash);
    font-size: var(--font-tiny);
    color: var(--muted);
  }
  .group-head .size {
    font-size: var(--font-tiny);
    color: var(--faint);
  }
  .group-head .line-rule {
    flex: 1;
    height: 1px;
    background: var(--border);
  }

  .files {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 720px;
  }
  .files li {
    display: flex;
    align-items: center;
    gap: var(--gap-field);
    height: 56px;
    padding: 0 8px 0 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    background: var(--surface-veil);
    transition: border-color 130ms var(--ease);
  }
  .files li:hover {
    border-color: color-mix(in srgb, var(--muted) 45%, transparent);
  }

  .jar {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .jar-name {
    font-size: var(--font-small);
    font-weight: 600;
  }
  .jar-file {
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
    color: var(--faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .jar-size {
    font-size: var(--font-small);
    color: var(--muted);
  }

  .icon-btn {
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: var(--radius-button);
    background: transparent;
    color: var(--muted);
    transition: background-color 120ms var(--ease), color 120ms var(--ease);
  }
  .icon-btn:hover {
    background: color-mix(in srgb, var(--error) 14%, transparent);
    color: var(--error);
  }
</style>
