<!--
  Home: the server list.

  Servers are grouped by whether they are up, because "which of my servers is
  running" is the question this screen exists to answer. Search and the scope
  filter are list controls, not a dashboard — nothing here belongs to another
  page.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";

  import Button from "../lib/Button.svelte";
  import Icon from "../lib/Icon.svelte";
  import JavaPrompt from "../lib/JavaPrompt.svelte";
  import Modal from "../lib/Modal.svelte";
  import { formatMemory, formatUptime } from "../lib/format";
  import {
    crashReport,
    createServerFromInstaller,
    getServerConfig,
    importServer,
    listDownloads,
    listServers,
    onCoreEvent,
    saveServerConfig,
    startServer,
    stopServer,
  } from "../lib/api";
  import { isActive, primaryAction, STATE_LABEL } from "../lib/serverState";
  import {
    errorMessage,
    isCoreError,
    type DownloadedFile,
    type ServerSummary,
  } from "../lib/types";

  let {
    goToDownloads,
    openServer,
  }: {
    /** Lets the empty-installer state send the user somewhere useful. */
    goToDownloads: () => void;
    /** Opens one server's own page. */
    openServer: (id: ServerSummary["id"]) => void;
  } = $props();

  /** Up, or on its way there — the grouping the header counts. */
  const isUp = (s: ServerSummary) => isActive(s.state);

  let servers = $state<ServerSummary[]>([]);

  /**
   * Why each crashed server crashed, keyed by id.
   *
   * Fetched per card rather than carried on the summary: the diagnosis reads a
   * crash report off disk, and doing that for every server on every refresh
   * would put a file scan behind a list that repaints once a second.
   */
  let crashes = $state<Record<string, string>>({});
  let error = $state<string | null>(null);

  /** Seconds since the last refresh, for the uptime readouts. */
  let ticks = $state(0);

  let query = $state("");
  let scope = $state<"all" | "running" | "stopped">("all");

  let menuOpen = $state(false);

  // ── create-and-install ──────────────────────────────────
  let addOpen = $state(false);
  let newName = $state("");
  let installers = $state<DownloadedFile[]>([]);
  let installer = $state("");

  /**
   * An installer jar dropped straight onto the dialog.
   *
   * It is not in the downloads folder, so it cannot be one of the radio
   * options — it is held separately and shown as an extra row while it lasts.
   */
  let droppedInstaller = $state<DownloadedFile | null>(null);
  /** Non-null while an install is running; the text describes the step. */
  let phase = $state<string | null>(null);
  let lastLog = $state<string | null>(null);

  /** Set when an install was blocked for want of a JRE; the prompt downloads
   *  it and calls back into the same install. */
  let javaMajor = $state<number | null>(null);
  let javaOpen = $state(false);
  /** Which server to start once the runtime lands; null means the prompt came
   *  from the create dialog instead. */
  let retryStart = $state<ServerSummary["id"] | null>(null);

  let editOpen = $state(false);
  let editing = $state<ServerSummary | null>(null);
  let editName = $state("");

  const visible = $derived(
    servers.filter((s) => {
      const okScope =
        scope === "all" || (scope === "running" ? isUp(s) : !isUp(s));
      const okQuery =
        !query.trim() || s.name.toLowerCase().includes(query.trim().toLowerCase());
      return okScope && okQuery;
    }),
  );
  const running = $derived(visible.filter(isUp));
  const stopped = $derived(visible.filter((s) => !isUp(s)));

  /**
   * Uptime as of now.
   *
   * `listServers` reports it as of the fetch, and refetching once a second to
   * animate a clock would be a filesystem walk per tick. `ticks` advances on a
   * local interval instead, and every refetch resnaps to the core's value, so
   * the display can drift by at most a second.
   */
  function uptimeOf(server: ServerSummary) {
    return (server.uptimeSecs ?? 0) + ticks;
  }

  function versionLine(s: ServerSummary) {
    if (!s.mcVersion && !s.forgeVersion) return "尚未安裝 Forge";
    return [s.mcVersion, s.forgeVersion && `Forge ${s.forgeVersion}`]
      .filter(Boolean)
      .join(" · ");
  }

  async function run<T>(action: () => Promise<T>) {
    try {
      await action();
      error = null;
    } catch (e) {
      error = errorMessage(e);
    }
  }

  const refresh = () =>
    run(async () => {
      servers = await listServers();
      ticks = 0;
      loadCrashes();
    });

  /** One diagnosis per crashed server, asked for once. Entries for servers
   *  that are no longer crashed are dropped — the card has moved on. */
  async function loadCrashes() {
    const crashed = servers.filter((s) => s.state.kind === "crashed");
    const next: Record<string, string> = {};
    for (const server of crashed) {
      next[server.id] =
        crashes[server.id] ?? (await crashReport(server.id))?.headline ?? "";
    }
    crashes = next;
  }

  /** `forge-1.20.1-47.2.0-installer.jar` → `Forge 1.20.1-47.2.0`. */
  function prettyInstaller(name: string) {
    const m = name.match(/^forge-(.+)-installer\.jar$/i);
    return m ? `Forge ${m[1]}` : name;
  }

  /**
   * Reads the downloads folder each time the dialog opens, so a jar fetched on
   * the download page a moment ago is already here.
   */
  async function openCreate() {
    menuOpen = false;
    addOpen = true;
    phase = null;
    lastLog = null;
    try {
      installers = await listDownloads();
      if (!installers.some((f) => f.path === installer)) {
        installer = installers[0]?.path ?? "";
      }
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function createAndInstall() {
    if (!newName.trim() || !installer || phase) return;
    phase = "執行安裝程式…";
    lastLog = null;
    try {
      await createServerFromInstaller(newName.trim(), installer);
      addOpen = false;
      newName = "";
      error = null;
    } catch (e) {
      // A missing JRE is not a failure to report, it is a thing to offer to
      // fix — the installer is itself a jar and cannot run without one.
      if (isCoreError(e) && e.kind === "javaMissing") {
        javaMajor = e.major;
        javaOpen = true;
        retryStart = null;
      } else {
        // Stays open: the message and the installer output are both here, and
        // closing would throw away the only explanation of what failed.
        error = errorMessage(e);
      }
    } finally {
      phase = null;
    }
  }

  /** Start, offering the JRE download when the machine has no matching Java. */
  async function start(id: ServerSummary["id"]) {
    try {
      await startServer(id);
      error = null;
    } catch (e) {
      if (isCoreError(e) && e.kind === "javaMissing") {
        javaMajor = e.major;
        javaOpen = true;
        retryStart = id;
      } else {
        error = errorMessage(e);
      }
    }
  }

  const importExisting = () =>
    run(async () => {
      menuOpen = false;
      const picked = await open({ directory: true, title: "選擇伺服器資料夾" });
      if (typeof picked === "string") await importServer(picked);
    });

  function openEdit(server: ServerSummary) {
    editing = server;
    editName = server.name;
    editOpen = true;
  }

  const saveName = () =>
    editing &&
    editName.trim() &&
    run(async () => {
      const config = await getServerConfig(editing!.id);
      await saveServerConfig(editing!.id, { ...config, name: editName.trim() });
      editOpen = false;
    });

  onMount(() => {
    refresh();
    // A jar dropped while the new-server dialog is open picks itself.
    const dropped = getCurrentWebview().onDragDropEvent((event) => {
      if (!addOpen || event.payload.type !== "drop") return;
      const jar = event.payload.paths.find((p) => p.toLowerCase().endsWith(".jar"));
      if (!jar) return;
      droppedInstaller = { path: jar, name: jar.split(/[\\/]/).pop() ?? jar, bytes: 0 };
      installer = jar;
    });

    const unlisten = onCoreEvent((event) => {
      // The event already carries the new state, so apply it directly rather
      // than waiting on the refetch. The refetch still runs — it brings the
      // player count and uptime with it — but a card must not sit on a stale
      // state if that call is slow, or fails, or lands out of order.
      if (event.type === "serverState") {
        servers = servers.map((s) =>
          s.id === event.id ? { ...s, state: event.state, light: event.light } : s,
        );
      }
      if (event.type === "serversChanged" || event.type === "serverState") refresh();

      // The installer's own output is the only progress it reports, so the
      // dialog shows the latest line rather than inventing a percentage.
      if (phase && event.type === "serverLog") {
        const line = event.lines.at(-1);
        if (line) lastLog = line.text;
      }
    });
    // Only the clock; player counts and states arrive as events.
    const tick = setInterval(() => (ticks += 1), 1000);

    return () => {
      clearInterval(tick);
      void unlisten.then((fn) => fn());
      void dropped.then((fn) => fn());
    };
  });
</script>

<svelte:window onclick={() => (menuOpen = false)} />

<section class="page">
  <header>
    <h1>Forge Server Manager</h1>

    <!-- Split button: the label creates, the caret opens the menu. The divider
         is what tells you the caret is a separate hit target. -->
    <div class="split">
      <button class="split-main" onclick={openCreate}>新增伺服器</button>
      <button
        class="split-caret"
        aria-label="更多新增方式"
        aria-expanded={menuOpen}
        onclick={(e) => (e.stopPropagation(), (menuOpen = !menuOpen))}
      >
        <Icon name="chevron-down" size={16} strokeWidth={2} />
      </button>

      {#if menuOpen}
        <div class="menu">
          <button class="menu-item default" onclick={openCreate}>
            <span class="mi-icon"><Icon name="plus" size={17} /></span>
            <span>
              <strong>建立新伺服器</strong>
              <small>選擇 Forge 版本並安裝</small>
            </span>
          </button>
          <button class="menu-item" onclick={importExisting}>
            <span class="mi-icon"><Icon name="folder-open" size={17} /></span>
            <span>
              <strong>匯入現有資料夾…</strong>
              <small>已經裝好 Forge 的伺服器</small>
            </span>
          </button>
        </div>
      {/if}
    </div>
  </header>

  <div class="toolbar">
    <label class="search">
      <Icon name="search" size={15} />
      <input bind:value={query} placeholder="Search server" />
    </label>

    <div class="segmented" role="group" aria-label="篩選">
      <button class:active={scope === "all"} onclick={() => (scope = "all")}>全部</button>
      <button class:active={scope === "running"} onclick={() => (scope = "running")}>執行中</button>
      <button class:active={scope === "stopped"} onclick={() => (scope = "stopped")}>已停止</button>
    </div>

    <span class="count tabular">
      {servers.length} 座伺服器 · {servers.filter(isUp).length} 座執行中
    </span>
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if servers.length === 0}
    <p class="empty">還沒有伺服器。用右上角的「新增伺服器」建立一座，或匯入現有資料夾。</p>
  {:else if visible.length === 0}
    <p class="empty">沒有符合的伺服器。</p>
  {/if}

  {#each [{ title: "執行中", list: running }, { title: "已停止", list: stopped }] as group (group.title)}
    {#if group.list.length}
      <section class="group">
        <div class="group-head">
          <h2>{group.title}</h2>
          <span class="n tabular">{group.list.length}</span>
          <span class="line"></span>
        </div>

        <ul class="list">
          {#each group.list as server (server.id)}
            {@const action = primaryAction(server.state)}
            <!-- The row is the link to the server's page; the buttons inside
                 stop the click from reaching it. -->
            <li>
              <div
                class="row"
                role="button"
                tabindex="0"
                onclick={() => openServer(server.id)}
                onkeydown={(e) => e.key === "Enter" && openServer(server.id)}
              >
                <span class="dot" data-light={server.light}></span>

                <span class="ident">
                  <span class="name" title={server.name}>{server.name}</span>
                  <span class="meta">
                    {STATE_LABEL[server.state.kind]}
                    <span class="sep">·</span>{versionLine(server)}{server.imported
                      ? " · 已匯入"
                      : ""}
                  </span>
                  <!-- Only ever one of these: a running server names who is on
                       it, a crashed one says why it stopped. -->
                  {#if server.players?.length}
                    <span class="who">{server.players.join("、")}</span>
                  {:else if crashes[server.id]}
                    <span class="why">{crashes[server.id]}</span>
                  {/if}
                </span>

                <!-- A running server shows what it is doing; a stopped one has
                     nothing to report, so it shows what it is configured as.
                     Both sets are right-aligned on the same column so the eye
                     can run down them. -->
                <span class="facts tabular">
                  {#if server.uptimeSecs !== null}
                    {formatUptime(uptimeOf(server))}<span class="gap"></span>{server.players
                      ?.length ?? 0} / {server.maxPlayers ?? "—"} 人<span class="gap"></span>{formatMemory(
                      server.memoryMb,
                    )}
                  {:else}
                    :{server.port ?? "—"}<span class="gap"></span>{server.maxPlayers ?? "—"} 人<span
                      class="gap"
                    ></span>{formatMemory(server.memoryMb)}
                  {/if}
                </span>

                <button
                  class="act"
                  class:primary={action.primary}
                  disabled={action.disabled}
                  onclick={(e) => (
                    e.stopPropagation(),
                    action.run === "start"
                      ? start(server.id)
                      : action.run === "stop" && run(() => stopServer(server.id))
                  )}
                >
                  {action.label}
                </button>
                <button
                  class="act more"
                  onclick={(e) => (e.stopPropagation(), openEdit(server))}
                  aria-label="更多選項"
                >
                  <Icon name="ellipsis" size={15} />
                </button>
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {/each}
</section>

{#if javaMajor !== null}
  <JavaPrompt
    bind:open={javaOpen}
    major={javaMajor}
    onready={() => (retryStart ? start(retryStart) : createAndInstall())}
  />
{/if}

<Modal bind:open={addOpen} title="新增伺服器" width={480}>
  <div class="form">
    <label class="field">
      <span>Server name</span>
      <input
        bind:value={newName}
        placeholder="my server"
        disabled={!!phase}
        onkeydown={(e) => e.key === "Enter" && createAndInstall()}
      />
    </label>

    {#if installers.length === 0 && !droppedInstaller}
      <div class="empty-box">
        <p>下載資料夾裡還沒有 Forge 安裝檔。</p>
        <p class="drop-hint">也可以直接把安裝檔拖進這個視窗。</p>
        <Button onclick={() => ((addOpen = false), goToDownloads())}>前往下載頁</Button>
      </div>
    {:else}
      <label class="field">
        <span>Forge 安裝檔</span>
        <div class="select">
          <select bind:value={installer} disabled={!!phase}>
            {#each installers as file (file.path)}
              <option value={file.path}>{prettyInstaller(file.name)}</option>
            {/each}
            <!-- A jar dropped onto the window is not in the downloads folder,
                 so it is listed separately rather than pretending to be. -->
            {#if droppedInstaller}
              <option value={droppedInstaller.path}>
                {prettyInstaller(droppedInstaller.name)}（拖入）
              </option>
            {/if}
          </select>
          <Icon name="chevron-down" size={14} />
        </div>
      </label>
    {/if}

    {#if phase}
      <div class="progress">
        <div class="progress-line"><span>{phase}</span></div>
        <!-- The Forge installer reports no percentage, so the bar says
             "working" rather than inventing one. -->
        <div class="bar indeterminate"><div class="fill"></div></div>
        {#if lastLog}
          <p class="log">{lastLog}</p>
        {/if}
      </div>
    {:else if installers.length || droppedInstaller}
      <p class="hint">
        安裝 Forge 需要機器上有對應版本的 Java。建立即表示你同意
        <a href="https://aka.ms/MinecraftEULA" target="_blank" rel="noreferrer">Minecraft EULA</a>。
      </p>
    {/if}
  </div>

  {#snippet footer()}
    <Button onclick={() => (addOpen = false)} disabled={!!phase}>
      {phase ? "安裝中…" : "取消"}
    </Button>
    <Button
      variant="primary"
      disabled={!newName.trim() || !installer || !!phase}
      onclick={createAndInstall}
    >
      建立並安裝
    </Button>
  {/snippet}
</Modal>

<Modal bind:open={editOpen} title="Rename" width={460}>
  <label class="field">
    <span>Server name</span>
    <input bind:value={editName} onkeydown={(e) => e.key === "Enter" && saveName()} />
  </label>
  <p class="hint">
    {#if editing?.imported}
      這是匯入的伺服器，檔案在你自己的資料夾裡。改名只會影響這裡的顯示。
    {:else}
      資料夾名稱不會跟著改。名稱只是顯示用，改名不會影響正在執行的伺服器。
    {/if}
  </p>

  {#snippet footer()}
    <Button onclick={() => (editOpen = false)}>取消</Button>
    <Button variant="primary" disabled={!editName.trim()} onclick={saveName}>儲存</Button>
  {/snippet}
</Modal>

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
    align-items: center;
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

  /* ── split button ────────────────────────────────────── */

  .split {
    position: relative;
    display: flex;
    flex: none;
  }
  .split > button {
    height: 42px;
    border: 0;
    background: var(--accent);
    color: var(--accent-fg);
    font-weight: 600;
    transition: filter 120ms var(--ease);
  }
  .split > button:hover {
    filter: brightness(1.1);
  }
  .split-main {
    padding: 0 16px 0 18px;
    border-radius: var(--radius-button) 0 0 var(--radius-button);
  }
  .split-caret {
    position: relative;
    width: 38px;
    display: grid;
    place-items: center;
    border-radius: 0 var(--radius-button) var(--radius-button) 0;
  }
  .split-caret::before {
    content: "";
    position: absolute;
    left: 0;
    top: 10px;
    bottom: 10px;
    width: 1px;
    background: color-mix(in srgb, var(--accent-fg) 34%, transparent);
  }

  .menu {
    position: absolute;
    top: 50px;
    right: 0;
    z-index: 20;
    width: 296px;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-modal);
    animation: pop 140ms var(--ease);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(-6px);
    }
  }

  .menu-item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--gap-field);
    padding: 10px 12px;
    border: 0;
    border-radius: var(--radius-button);
    background: transparent;
    text-align: left;
  }
  .menu-item:hover {
    background: var(--wash);
  }
  .menu-item.default {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .mi-icon {
    display: flex;
    flex: none;
    color: var(--muted);
  }
  .menu-item.default .mi-icon {
    color: var(--accent);
  }
  .menu-item strong {
    display: block;
    font-size: 13px;
    font-weight: 600;
  }
  .menu-item small {
    display: block;
    margin-top: 2px;
    font-size: var(--font-tiny);
    color: var(--muted);
  }

  /* ── toolbar ─────────────────────────────────────────── */

  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--gap-field);
    flex-wrap: wrap;
  }

  .search {
    flex: 1 1 260px;
    max-width: 320px;
    height: 38px;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 0 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    background: var(--surface-veil);
    color: var(--faint);
    transition: border-color 130ms var(--ease);
  }
  .search:focus-within {
    border-color: var(--accent);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--fg);
    font: inherit;
    outline: none;
    user-select: text;
  }
  .search input::placeholder {
    color: var(--faint);
  }

  .segmented {
    display: flex;
    gap: 3px;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    background: var(--surface-veil);
  }
  .segmented button {
    height: 30px;
    padding: 0 14px;
    border: 0;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--muted);
    font-size: var(--font-small);
    transition: background-color 120ms var(--ease), color 120ms var(--ease);
  }
  .segmented button:hover {
    color: var(--fg);
  }
  .segmented button.active {
    background: var(--wash-2);
    color: var(--fg);
    font-weight: 600;
  }

  .count {
    margin-left: auto;
    font-size: var(--font-small);
    color: var(--muted);
  }

  .error {
    margin: 0;
    color: var(--error);
    font-size: var(--font-small);
  }
  /* One line under the name: who is on, or why it stopped. Both are the
     card's own news, so they sit with the name rather than in the aligned
     facts column, which is for numbers you scan down. */
  .who,
  .why {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--font-small);
  }

  .who {
    color: var(--muted);
  }

  .why {
    color: var(--error);
  }

  .empty {
    margin: 0;
    color: var(--muted);
  }

  /* ── groups + cards ──────────────────────────────────── */

  .group {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
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
  .group-head .line {
    flex: 1;
    height: 1px;
    background: var(--border);
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* One hairline between rows and none around the group: the boundary that
     matters is between two servers, not around the set of them. */
  .list li + li .row {
    border-top: 1px solid var(--border);
  }

  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 15px 4px;
    background: none;
    border: 0;
    text-align: left;
    transition: background-color 120ms var(--ease);
  }

  .row:hover {
    /* A wash the width of the row, rather than a lifted card: the row is a
       target, not an object. */
    background: var(--wash);
  }

  .row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--offline);
  }
  .dot[data-light="online"] {
    background: var(--online);
  }
  .dot[data-light="starting"] {
    background: var(--starting);
  }
  .dot[data-light="error"] {
    background: var(--error);
  }

  .ident {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .name {
    font-size: 17px;
    font-weight: 500;
    letter-spacing: -0.2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    color: var(--muted);
    font-size: var(--font-small);
  }

  .meta .sep {
    margin: 0 6px;
    color: var(--faint);
  }

  /* Fixed width so the numbers line up down the column even as they change. */
  .facts {
    width: 216px;
    flex: none;
    text-align: right;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .facts .gap {
    display: inline-block;
    width: 14px;
  }

  .act {
    height: var(--h-control-sm);
    flex: none;
    padding: 0 10px;
    display: flex;
    align-items: center;
    border: 0;
    border-radius: var(--radius-button);
    background: none;
    color: var(--muted);
    font-size: var(--font-small);
    transition: color 120ms var(--ease), background-color 120ms var(--ease);
  }

  .act:hover:not(:disabled) {
    background: var(--wash-2);
    color: var(--fg);
  }

  /* Installing and stopping are already going somewhere; the button says so
     and does nothing until they land. */
  .act:disabled {
    opacity: 0.5;
  }

  .act.primary {
    color: var(--accent);
  }

  .act.more {
    width: var(--h-control-sm);
    padding: 0;
    justify-content: center;
  }

  /* ── modal fields ────────────────────────────────────── */

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field span {
    font-size: var(--font-tiny);
    color: var(--muted);
  }
  .field input {
    height: 40px;
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    background: var(--bg);
    color: var(--fg);
    font: inherit;
    user-select: text;
  }
  .field input:focus {
    border-color: var(--accent);
    outline: none;
  }
  .field input:disabled {
    opacity: 0.5;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .empty-box {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--gap-field);
    padding: 16px;
    border: 1px dashed var(--border);
    border-radius: var(--radius-button);
  }
  .drop-hint {
    color: var(--faint);
  }

  .empty-box p {
    margin: 0;
    font-size: var(--font-small);
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
    width: 100%;
    height: 40px;
    padding: 0 34px 0 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    background: var(--bg);
    color: var(--fg);
    font: inherit;
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

  .progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    background: var(--surface-2);
  }
  .progress-line {
    display: flex;
    justify-content: space-between;
    font-size: var(--font-small);
  }
  .bar {
    height: 6px;
    border-radius: var(--radius-pill);
    background: var(--wash);
    overflow: hidden;
  }
  .bar .fill {
    height: 100%;
    width: 0;
    border-radius: var(--radius-pill);
    background: var(--accent);
    transition: width 160ms linear;
  }
  /* The installer gives no percentage, so the bar says "working" instead of
     inventing one. */
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
  .log {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint a {
    color: var(--accent);
  }

  .hint {
    margin: 12px 0 0;
    font-size: var(--font-tiny);
    color: var(--muted);
    line-height: 1.6;
  }
</style>
