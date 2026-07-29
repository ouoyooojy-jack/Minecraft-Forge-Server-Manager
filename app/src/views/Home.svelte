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

  import Button from "../lib/Button.svelte";
  import Icon from "../lib/Icon.svelte";
  import JavaPrompt from "../lib/JavaPrompt.svelte";
  import Modal from "../lib/Modal.svelte";
  import { formatMemory, formatUptime } from "../lib/format";
  import {
    createServerFromInstaller,
    deleteServer,
    getServerConfig,
    importServer,
    listDownloads,
    listServers,
    onCoreEvent,
    saveServerConfig,
    startServer,
    stopServer,
  } from "../lib/api";
  import {
    errorMessage,
    isCoreError,
    type DownloadedFile,
    type ServerState,
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

  const STATE_LABEL: Record<ServerState["kind"], string> = {
    stopped: "已停止",
    installing: "安裝中",
    starting: "啟動中",
    online: "執行中",
    stopping: "停止中",
    crashed: "已當機",
  };

  /** Up, or on its way there — the grouping the header counts. */
  const isUp = (s: ServerSummary) =>
    s.state.kind !== "stopped" && s.state.kind !== "crashed";

  let servers = $state<ServerSummary[]>([]);
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
  let confirmingDelete = $state(false);

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
    });

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
    confirmingDelete = false;
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

  const remove = () =>
    editing &&
    run(async () => {
      await deleteServer(editing!.id);
      editOpen = false;
    }).finally(() => (confirmingDelete = false));

  onMount(() => {
    refresh();
    const unlisten = onCoreEvent((event) => {
      // Both change what a card shows. Refetching keeps the UI from holding a
      // second, half-updated copy of core state.
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
      <input bind:value={query} placeholder="搜尋伺服器" />
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

        <div class="grid">
          {#each group.list as server (server.id)}
            <!-- The whole card is the link to the server's page; the action
                 buttons inside stop the click from reaching it. -->
            <div
              class="card"
              role="button"
              tabindex="0"
              onclick={() => openServer(server.id)}
              onkeydown={(e) => e.key === "Enter" && openServer(server.id)}
            >
              <h3>
                <span class="dot" data-light={server.light}></span>
                <span class="label" title={server.name}>{server.name}</span>
              </h3>

              <p class="meta">
                <span class="state" data-light={server.light}>
                  {STATE_LABEL[server.state.kind]}
                </span>
                · {versionLine(server)}{server.imported ? " · 已匯入" : ""}
              </p>

              <!-- A running server shows what it is doing; a stopped one has
                   nothing to report, so it shows what it is configured as. -->
              <div class="facts tabular">
                {#if server.uptimeSecs !== null}
                  <span>{formatUptime(uptimeOf(server))}</span>
                  <span>{server.playersOnline ?? 0} / {server.maxPlayers ?? "—"} 人</span>
                  <span>{formatMemory(server.memoryMb)}</span>
                {:else}
                  <span class:dim={server.port === null}>:{server.port ?? "—"}</span>
                  <span>{formatMemory(server.memoryMb)}</span>
                  <span class:dim={server.maxPlayers === null}>{server.maxPlayers ?? "—"} 人</span>
                {/if}
              </div>

              <div class="actions">
                {#if isUp(server)}
                  <button
                    class="act"
                    onclick={(e) => (e.stopPropagation(), run(() => stopServer(server.id)))}
                  >
                    <Icon name="square" size={13} />
                    停止
                  </button>
                {:else}
                  <button
                    class="act primary"
                    onclick={(e) => (e.stopPropagation(), start(server.id))}
                  >
                    <Icon name="play" size={13} />
                    啟動
                  </button>
                {/if}
                <button
                  class="act icon"
                  onclick={(e) => (e.stopPropagation(), openEdit(server))}
                  aria-label="更多選項"
                >
                  <Icon name="ellipsis" size={16} />
                </button>
              </div>
            </div>
          {/each}
        </div>
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
      <span>伺服器名稱</span>
      <input
        bind:value={newName}
        placeholder="my server"
        disabled={!!phase}
        onkeydown={(e) => e.key === "Enter" && createAndInstall()}
      />
    </label>

    {#if installers.length === 0}
      <div class="empty-box">
        <p>下載資料夾裡還沒有 Forge 安裝檔。</p>
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
    {:else if installers.length}
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

<Modal bind:open={editOpen} title="伺服器選項" width={460}>
  <label class="field">
    <span>伺服器名稱</span>
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
    {#if confirmingDelete}
      <span class="confirm">
        {editing?.imported
          ? "確定移除？只會解除連結，你的資料夾不會被刪除。"
          : "確定刪除？資料夾與世界存檔都會消失。"}
      </span>
      <Button onclick={() => (confirmingDelete = false)}>取消</Button>
      <Button danger onclick={remove}>{editing?.imported ? "確定移除" : "確定刪除"}</Button>
    {:else}
      <Button danger onclick={() => (confirmingDelete = true)}>
        {editing?.imported ? "移除" : "刪除"}
      </Button>
      <span class="grow"></span>
      <Button onclick={() => (editOpen = false)}>取消</Button>
      <Button variant="primary" disabled={!editName.trim()} onclick={saveName}>儲存</Button>
    {/if}
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
  h1 {
    margin: 0;
    font-size: var(--font-hero);
    font-weight: 600;
    letter-spacing: -0.4px;
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
    box-shadow: var(--shadow-hi);
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

  /* `1fr` tracks so the row packs as many columns as fit, with the cap on the
     card. Capping the track instead makes auto-fill count columns at the
     maximum width and leave a dead strip on the right. */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(264px, 1fr));
    gap: 18px;
  }

  .card {
    display: flex;
    flex-direction: column;
    min-height: 170px;
    max-width: 320px;
    padding: 16px 18px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface-veil);
    box-shadow: var(--shadow), var(--edge-highlight);
    transition: transform 170ms var(--ease), box-shadow 170ms var(--ease),
      border-color 170ms var(--ease);
  }
  .card:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .card:hover {
    transform: translateY(-2px);
    box-shadow: var(--shadow-hi), var(--edge-highlight);
    border-color: color-mix(in srgb, var(--muted) 45%, transparent);
  }

  .card h3 {
    display: flex;
    align-items: center;
    gap: 9px;
    margin: 0;
    font-size: 19px;
    font-weight: 600;
    letter-spacing: -0.2px;
    overflow: hidden;
  }
  .card h3 .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--offline);
  }
  .dot[data-light="online"] {
    background: var(--online);
  }
  .dot[data-light="error"] {
    background: var(--error);
  }
  /* Starting is the only transient state, so it is the only thing that moves. */
  .dot[data-light="starting"] {
    background: var(--starting);
    animation: pulse 1.6s var(--ease) infinite;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  /* The state is still a word, not just a colour — it shares the meta line
     rather than owning a pill and a row of its own. */
  .meta {
    margin: 5px 0 0;
    font-size: var(--font-small);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta .state {
    font-weight: 600;
    color: var(--offline);
  }
  .meta .state[data-light="online"] {
    color: var(--online);
  }
  .meta .state[data-light="starting"] {
    color: var(--starting);
  }
  .meta .state[data-light="error"] {
    color: var(--error);
  }

  /* Facts, not a dashboard. Unlabelled — ":25565", "4 GB" and "20 人" each say
     what they are. */
  .facts {
    margin: 14px 0 0;
    display: flex;
    align-items: baseline;
    gap: 12px;
    font-family: var(--font-mono);
    font-size: var(--font-small);
    color: var(--muted);
  }
  .facts span + span::before {
    content: "·";
    margin-right: 12px;
    color: var(--faint);
  }
  .facts .dim {
    color: var(--faint);
  }

  .actions {
    margin-top: auto;
    padding-top: 14px;
    display: flex;
    gap: 8px;
  }
  .act {
    flex: 1;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    border: 0;
    border-radius: var(--radius-button);
    background: var(--surface-2);
    color: var(--fg);
    font-size: var(--font-small);
    font-weight: 600;
    transition: background-color 120ms var(--ease);
  }
  .act:hover {
    background: var(--wash-2);
  }
  .act.primary {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
  }
  .act.primary:hover {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }
  .act.icon {
    flex: none;
    width: 40px;
    color: var(--muted);
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
  .confirm {
    flex: 1;
    align-self: center;
    font-size: var(--font-small);
    color: var(--error);
  }
  .grow {
    flex: 1;
  }
</style>
