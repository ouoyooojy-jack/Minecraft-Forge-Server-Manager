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
  import { transfers } from "../lib/downloads.svelte";
  import { formatBytes, formatMemory, formatUptime } from "../lib/format";
  import {
    crashReport,
    createServer,
    createServerFromInstaller,
    deleteServer,
    getServerConfig,
    importServer,
    listDownloads,
    listForgeVersions,
    listServers,
    onCoreEvent,
    openServerFolder,
    saveServerConfig,
    startServer,
    stopServer,
  } from "../lib/api";
  import { isActive, primaryAction, STATE_LABEL } from "../lib/serverState";
  import {
    downloadPercent,
    errorMessage,
    isCoreError,
    type DownloadedFile,
    type ForgeVersionGroup,
    type ServerSummary,
  } from "../lib/types";

  let {
    openServer,
  }: {
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
  /** Which row's "…" menu is open. */
  let rowMenu = $state<ServerSummary["id"] | null>(null);

  // ── create-and-install ──────────────────────────────────
  // One dialog: a name and a Forge version. The core fetches the installer
  // when it is not on disk yet, so nobody has to visit the downloads page
  // first.
  let addOpen = $state(false);
  let newName = $state("");
  let groups = $state<ForgeVersionGroup[]>([]);
  let major = $state("");
  let version = $state("");
  let versionError = $state<string | null>(null);
  /** Installer file names already downloaded — those builds skip the fetch. */
  let onDisk = $state<string[]>([]);
  let createError = $state<string | null>(null);

  /**
   * An installer jar dropped straight onto the dialog. It is used instead of
   * the version picker while it lasts.
   */
  let droppedInstaller = $state<DownloadedFile | null>(null);
  /** Non-null while an install is running; the text describes the step. */
  let phase = $state<string | null>(null);
  let lastLog = $state<string | null>(null);

  const versions = $derived(groups.find((g) => g.mcMajor === major)?.versions ?? []);
  const installerName = (v: string) => `forge-${v}-installer.jar`;
  /** The chosen build's installer, while the core is fetching it. */
  const fetching = $derived(
    Object.values(transfers).find(
      (t) =>
        t.what.kind === "forgeInstaller" &&
        t.what.version === version &&
        t.state.kind === "running",
    ),
  );

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

  let deleteOpen = $state(false);
  let deleting = $state<ServerSummary | null>(null);
  /** Whether it has a playit address, which goes with it — the dialog says so. */
  let deletingTunnel = $state(false);

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

  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  /** Events come in bursts — one start is several state changes — and each
   *  refetch is a walk over every server folder. */
  function refreshSoon() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(refresh, 150);
  }

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
   * Reads the version list and the downloads folder each time the dialog
   * opens, so a jar fetched on the download page a moment ago counts.
   */
  async function openCreate() {
    menuOpen = false;
    addOpen = true;
    phase = null;
    lastLog = null;
    createError = null;
    droppedInstaller = null;
    versionError = null;
    try {
      const [list, files] = await Promise.all([listForgeVersions(), listDownloads()]);
      groups = list;
      onDisk = files.map((f) => f.name);
      if (!list.some((g) => g.versions.includes(version))) {
        // A build already downloaded is most likely the one meant; otherwise
        // the newest.
        const local = files
          .map((f) => f.name.match(/^forge-(.+)-installer\.jar$/i)?.[1])
          .find((v) => v && list.some((g) => g.versions.includes(v)));
        pickVersion(local ?? list[0]?.versions[0] ?? "");
      }
    } catch (e) {
      versionError = errorMessage(e);
    }
  }

  function pickVersion(v: string) {
    version = v;
    major = groups.find((g) => g.versions.includes(v))?.mcMajor ?? "";
  }

  function pickMajor(value: string) {
    major = value;
    // The old build belongs to the old line; take the newest of the new one.
    version = groups.find((g) => g.mcMajor === value)?.versions[0] ?? "";
  }

  async function createAndInstall() {
    const jar = droppedInstaller?.path;
    if (!newName.trim() || (!jar && !version) || phase) return;
    phase = "執行安裝程式…";
    lastLog = null;
    createError = null;
    try {
      await (jar
        ? createServerFromInstaller(newName.trim(), jar)
        : createServer(newName.trim(), version));
      addOpen = false;
      newName = "";
      droppedInstaller = null;
      error = null;
    } catch (e) {
      // A missing JRE is not a failure to report, it is a thing to offer to
      // fix — the installer is itself a jar and cannot run without one.
      if (isCoreError(e) && e.kind === "javaMissing") {
        javaMajor = e.major;
        javaOpen = true;
        retryStart = null;
      } else {
        // Stays open, with the message inside it: closing would throw away
        // the only explanation of what failed.
        createError = errorMessage(e);
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
    rowMenu = null;
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

  async function askDelete(server: ServerSummary) {
    rowMenu = null;
    deleting = server;
    deletingTunnel = false;
    deleteOpen = true;
    try {
      deletingTunnel = !!(await getServerConfig(server.id)).tunnelId;
    } catch {
      // Unreadable config: nothing to say about a tunnel.
    }
  }

  function confirmDelete() {
    const id = deleting?.id;
    deleteOpen = false;
    if (id) run(() => deleteServer(id));
  }

  onMount(() => {
    refresh();
    // A jar dropped while the new-server dialog is open picks itself.
    const dropped = getCurrentWebview().onDragDropEvent((event) => {
      if (!addOpen || event.payload.type !== "drop") return;
      const jar = event.payload.paths.find((p) => p.toLowerCase().endsWith(".jar"));
      if (!jar || phase) return;
      droppedInstaller = { path: jar, name: jar.split(/[\\/]/).pop() ?? jar, bytes: 0 };
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
      if (event.type === "serversChanged" || event.type === "serverState") refreshSoon();

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
      clearTimeout(refreshTimer);
      void unlisten.then((fn) => fn());
      void dropped.then((fn) => fn());
    };
  });
</script>

<svelte:window onclick={() => ((menuOpen = false), (rowMenu = null))} />

<section class="page">
  <header>
    <h1>我的伺服器</h1>

    <!-- Split button: the label creates, the caret opens the menu. The divider
         is what tells you the caret is a separate hit target. -->
    <div class="split">
      <button class="split-main" onclick={openCreate}>新增伺服器</button>
      <button
        class="split-caret"
        aria-label="更多新增方式"
        aria-expanded={menuOpen}
        onclick={(e) => (e.stopPropagation(), (rowMenu = null), (menuOpen = !menuOpen))}
      >
        <Icon name="chevron-down" size={16} strokeWidth={2} />
      </button>

      {#if menuOpen}
        <div class="menu">
          <button class="menu-item default" onclick={openCreate}>
            <span class="mi-icon"><Icon name="plus" size={17} /></span>
            <span>
              <strong>建立新伺服器</strong>
              <small>選版本，自動下載並安裝 Forge</small>
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
                onkeydown={(e) =>
                  // Only the row itself: Enter on a button inside it is that
                  // button's click, not "open this server" as well.
                  e.key === "Enter" && e.target === e.currentTarget && openServer(server.id)}
              >
                <span class="dot" data-light={server.light}></span>

                <span class="ident">
                  <span class="name" title={server.name}>{server.name}</span>
                  <span class="meta">
                    {STATE_LABEL[server.state.kind]}{#if server.restartPending}（即將自動重啟）{/if}
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
                <span class="more-wrap">
                  <button
                    class="act more"
                    onclick={(e) => (
                      e.stopPropagation(),
                      (menuOpen = false),
                      (rowMenu = rowMenu === server.id ? null : server.id)
                    )}
                    aria-label="更多選項"
                    aria-expanded={rowMenu === server.id}
                  >
                    <Icon name="ellipsis" size={15} />
                  </button>
                  {#if rowMenu === server.id}
                    <!-- Clicks inside stay inside: the row behind would open
                         the server, the window would close the menu. -->
                    <!-- svelte-ignore a11y_click_events_have_key_events -->
                    <div
                      class="menu row-menu"
                      role="menu"
                      tabindex="-1"
                      onclick={(e) => e.stopPropagation()}
                    >
                      <button class="menu-item" role="menuitem" onclick={() => openEdit(server)}>
                        <span class="mi-icon"><Icon name="pencil-line" size={16} /></span>
                        <span><strong>重新命名</strong></span>
                      </button>
                      <button
                        class="menu-item"
                        role="menuitem"
                        onclick={() => ((rowMenu = null), run(() => openServerFolder(server.id)))}
                      >
                        <span class="mi-icon"><Icon name="folder-open" size={16} /></span>
                        <span><strong>開啟資料夾</strong></span>
                      </button>
                      <button
                        class="menu-item danger"
                        role="menuitem"
                        disabled={isUp(server)}
                        title={isUp(server) ? "請先停止伺服器" : undefined}
                        onclick={() => askDelete(server)}
                      >
                        <span class="mi-icon"><Icon name="trash" size={16} /></span>
                        <span>
                          <strong>刪除</strong>
                          {#if isUp(server)}<small>請先停止伺服器</small>{/if}
                        </span>
                      </button>
                    </div>
                  {/if}
                </span>
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

<Modal bind:open={addOpen} title="新增伺服器" width={480} dismissible={!phase}>
  <div class="form">
    <label class="field">
      <span>伺服器名稱</span>
      <input
        bind:value={newName}
        placeholder="例如：生存服"
        disabled={!!phase}
        onkeydown={(e) => e.key === "Enter" && createAndInstall()}
      />
    </label>

    {#if droppedInstaller}
      <div class="dropped">
        <span>使用拖入的安裝檔 <strong>{prettyInstaller(droppedInstaller.name)}</strong></span>
        <button class="link" disabled={!!phase} onclick={() => (droppedInstaller = null)}>
          改用版本清單
        </button>
      </div>
    {:else if versionError}
      <div class="empty-box">
        <p>{versionError}</p>
        <p class="drop-hint">也可以直接把 Forge 安裝檔拖進這個視窗。</p>
        <Button onclick={openCreate}>重試</Button>
      </div>
    {:else if !groups.length}
      <p class="hint">載入版本清單中…</p>
    {:else}
      <div class="pair">
        <label class="field">
          <span>Minecraft 版本</span>
          <div class="select">
            <select
              value={major}
              disabled={!!phase}
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
          <span>Forge 版本</span>
          <div class="select">
            <select bind:value={version} disabled={!!phase || !versions.length}>
              {#each versions as v (v)}
                <option value={v}>{v}{onDisk.includes(installerName(v)) ? "（已下載）" : ""}</option>
              {/each}
            </select>
            <Icon name="chevron-down" size={14} />
          </div>
        </label>
      </div>
    {/if}

    {#if phase}
      <div class="progress">
        {#if fetching}
          {@const percent = downloadPercent(fetching)}
          <div class="progress-line">
            <span>下載 Forge 安裝檔…</span>
            <span class="tabular">
              {percent === null ? formatBytes(fetching.received) : `${percent.toFixed(0)}%`}
            </span>
          </div>
          <div class="bar" class:indeterminate={percent === null}>
            <div class="fill" style={percent !== null ? `width:${percent}%` : undefined}></div>
          </div>
        {:else}
          <div class="progress-line"><span>{phase}</span></div>
          <!-- The Forge installer reports no percentage, so the bar says
               "working" rather than inventing one. -->
          <div class="bar indeterminate"><div class="fill"></div></div>
          {#if lastLog}
            <p class="log">{lastLog}</p>
          {/if}
        {/if}
      </div>
    {:else}
      <p class="hint">
        {#if !droppedInstaller && version && !onDisk.includes(installerName(version))}
          會先自動下載這個版本的安裝檔。
        {/if}
        需要對應版本的 Java，沒有的話會問你要不要下載。建立即表示你同意
        <a href="https://aka.ms/MinecraftEULA" target="_blank" rel="noreferrer">Minecraft EULA</a>。
      </p>
    {/if}

    {#if createError}
      <p class="error" role="alert">{createError}</p>
    {/if}
  </div>

  {#snippet footer()}
    <Button onclick={() => (addOpen = false)} disabled={!!phase}>
      {phase ? "安裝中…" : "取消"}
    </Button>
    <Button
      variant="primary"
      disabled={!newName.trim() || (!droppedInstaller && !version) || !!phase}
      onclick={createAndInstall}
    >
      建立並安裝
    </Button>
  {/snippet}
</Modal>

<Modal bind:open={editOpen} title="重新命名" width={460}>
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
    <Button onclick={() => (editOpen = false)}>取消</Button>
    <Button variant="primary" disabled={!editName.trim()} onclick={saveName}>儲存</Button>
  {/snippet}
</Modal>

<Modal bind:open={deleteOpen} title="刪除「{deleting?.name ?? ''}」？" width={460}>
  <p class="hint first">
    {#if deleting?.imported}
      只會從清單移除，資料夾與世界檔案留在原處；在這裡做的備份會一起刪除。
    {:else}
      伺服器資料夾會整個刪除，包括世界、模組和備份，無法復原。
    {/if}
    {#if deletingTunnel}
      它在 playit.gg 的公開位址也會一起刪除，朋友之後要改用新位址。
    {/if}
  </p>

  {#snippet footer()}
    <Button onclick={() => (deleteOpen = false)}>取消</Button>
    <Button danger onclick={confirmDelete}>確定刪除</Button>
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

  .more-wrap {
    position: relative;
    flex: none;
  }
  .menu.row-menu {
    top: calc(100% + 6px);
    width: 200px;
  }
  .menu-item:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .menu-item:disabled:hover {
    background: transparent;
  }
  .menu-item.danger:not(:disabled) strong,
  .menu-item.danger:not(:disabled) .mi-icon {
    color: var(--error);
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
  .form .hint {
    margin: 0;
  }
  .hint.first {
    margin-top: 0;
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1.4fr;
    gap: var(--gap-field);
  }
  .dropped {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--gap-field);
    padding: 12px 14px;
    border: 1px dashed var(--border);
    border-radius: var(--radius-button);
    font-size: var(--font-small);
    color: var(--muted);
  }
  .dropped strong {
    color: var(--fg);
    font-weight: 600;
  }
  .link {
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font-size: var(--font-small);
  }
  .link:disabled {
    opacity: 0.5;
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
