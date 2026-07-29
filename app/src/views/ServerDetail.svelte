<!--
  One server, in full: console on one tab, settings on the other.

  A page rather than a modal. Everything here is something you sit with — you
  watch a log, you type commands, you edit a file — and a modal that you keep
  open for twenty minutes is just a page with a scrim behind it.

  The console owns the seq numbers it holds. Events append; a gap in the
  sequence means the ring buffer evicted lines, and the whole buffer is
  refetched rather than showing a silently discontinuous log.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";

  import Button from "../lib/Button.svelte";
  import Icon from "../lib/Icon.svelte";
  import JavaPrompt from "../lib/JavaPrompt.svelte";
  import Modal from "../lib/Modal.svelte";
  import { formatBytes, formatMemory, formatUptime } from "../lib/format";
  import { isActive, primaryAction, STATE_LABEL } from "../lib/serverState";
  import {
    addMods,
    deleteMod,
    deleteServer,
    consoleSince,
    getServerConfig,
    getServerProperties,
    listMods,
    listServers,
    onCoreEvent,
    openServerFolder,
    readServerFile,
    saveServerConfig,
    saveServerProperties,
    sendConsoleCommand,
    startServer,
    stopServer,
    writeServerFile,
  } from "../lib/api";
  import {
    errorMessage,
    isCoreError,
    type Difficulty,
    type Gamemode,
    type LogLine,
    type ModFile,
    type ServerConfig,
    type ServerFile,
    type ServerId,
    type ServerProperties,
    type ServerSummary,
  } from "../lib/types";

  let { id, back }: { id: ServerId; back: () => void } = $props();

  const GAMEMODES: [Gamemode, string][] = [
    ["survival", "生存"],
    ["creative", "創造"],
    ["adventure", "冒險"],
    ["spectator", "旁觀"],
  ];
  const DIFFICULTIES: [Difficulty, string][] = [
    ["peaceful", "和平"],
    ["easy", "簡單"],
    ["normal", "普通"],
    ["hard", "困難"],
  ];

  const FILES: [ServerFile, string, string][] = [
    ["properties", "server.properties", "Minecraft 的完整設定檔"],
    ["jvmArgs", "user_jvm_args.txt", "JVM 參數，含 -Xmx 記憶體上限"],
    ["runScript", "run.bat", "啟動腳本；這個 app 從裡面讀啟動參數"],
  ];

  let server = $state<ServerSummary | null>(null);
  let error = $state<string | null>(null);
  let tab = $state<"console" | "settings">("console");

  // ── console ─────────────────────────────────────────────
  /** Matches the core's ring buffer. Events append forever otherwise, and a
   *  server left running overnight would grow this without bound. */
  const MAX_LINES = 5000;

  let lines = $state<LogLine[]>([]);
  /** Lines below this seq are hidden — a local "clear", since the core's ring
   *  buffer is shared with anything else reading the same server. */
  let hideBefore = $state(0);
  let command = $state("");
  let autoscroll = $state(true);
  let logEl = $state<HTMLDivElement>();

  const visibleLines = $derived(lines.filter((l) => l.seq >= hideBefore));

  const online = $derived(server?.state.kind === "online");
  const isUp = $derived(server !== null && isActive(server.state));
  /** What the one lifecycle button says and does right now. A server we have
   *  not loaded yet reads as stopped, so the button is inert rather than absent
   *  — the header must not change height when the fetch lands. */
  const action = $derived(primaryAction(server?.state ?? { kind: "stopped" }));

  /** Seconds, seeded from the core on every refresh and ticked locally in
   *  between so the clock counts smoothly without polling once a second. */
  let uptime = $state<number | null>(null);

  /** Colour class for one log line. Minecraft tags its own severity, so this
   *  reads the tag rather than guessing from the text. */
  function level(line: LogLine): "error" | "warn" | "ready" | "info" {
    if (line.stream === "stderr" || line.text.includes("/ERROR")) return "error";
    if (line.text.includes("/WARN")) return "warn";
    if (line.text.includes("Done (") && line.text.includes("For help")) return "ready";
    return "info";
  }

  // ── settings tab ────────────────────────────────────────
  let config = $state<ServerConfig | null>(null);
  let properties = $state<ServerProperties | null>(null);
  let name = $state("");
  let saving = $state(false);
  let saved = $state(false);

  let fileOpen = $state(false);
  let fileWhich = $state<ServerFile>("properties");
  let fileText = $state("");
  let fileBusy = $state(false);

  let mods = $state<ModFile[]>([]);
  let modsBusy = $state(false);

  let confirmingDelete = $state(false);

  /** Set while a restart is waiting for the process to actually exit. The
   *  start half fires from the state event rather than a timer, because a
   *  modded world can take a minute to save and any fixed wait would be wrong. */
  let restarting = $state(false);

  /** Set when a start was blocked for want of a JRE. */
  let javaMajor = $state<number | null>(null);
  let javaOpen = $state(false);

  /** Stop, then start again once the process has actually exited. */
  async function restart() {
    restarting = true;
    try {
      await stopServer(id);
    } catch (e) {
      // The stop never landed, so no state event is coming to finish this.
      restarting = false;
      error = errorMessage(e);
    }
  }

  /** Start, offering the download when the machine has no matching Java. */
  async function start() {
    try {
      await startServer(id);
      error = null;
    } catch (e) {
      if (isCoreError(e) && e.kind === "javaMissing") {
        javaMajor = e.major;
        javaOpen = true;
      } else {
        error = errorMessage(e);
      }
    }
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
      const found = (await listServers()).find((s) => s.id === id);
      // The server was deleted from under us — nothing left to show.
      if (!found) return back();
      server = found;
      uptime = found.uptimeSecs;
    });

  const loadSettings = () =>
    run(async () => {
      config = await getServerConfig(id);
      properties = await getServerProperties(id);
      name = config.name;
    });

  const loadMods = () =>
    run(async () => {
      mods = await listMods(id);
    });

  /** Pick jars and copy them in. Multi-select: a modpack is never one file. */
  async function addModFiles() {
    const picked = await open({
      multiple: true,
      filters: [{ name: "Forge 模組", extensions: ["jar"] }],
      title: "選擇模組",
    });
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (!paths.length) return;

    modsBusy = true;
    await run(async () => {
      await addMods(id, paths);
      mods = await listMods(id);
    });
    modsBusy = false;
  }

  const removeMod = (name: string) =>
    run(async () => {
      await deleteMod(id, name);
      mods = await listMods(id);
    });

  /** Full refetch — used on mount and whenever a seq gap appears. */
  const reloadConsole = () =>
    run(async () => {
      lines = await consoleSince(id, 0);
    });

  function send() {
    const text = command.trim();
    if (!text || !online) return;
    command = "";
    run(() => sendConsoleCommand(id, text));
  }

  async function openFile(which: ServerFile) {
    fileWhich = which;
    fileBusy = true;
    fileOpen = true;
    await run(async () => {
      fileText = await readServerFile(id, which);
    });
    fileBusy = false;
  }

  async function saveFile() {
    fileBusy = true;
    try {
      await writeServerFile(id, fileWhich, fileText);
      fileOpen = false;
      error = null;
      // -Xmx may have moved memoryMb, and the properties file may have moved
      // anything; the form must not keep showing what is no longer on disk.
      await loadSettings();
      await refresh();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      fileBusy = false;
    }
  }

  async function saveSettings() {
    if (!config || !properties || saving) return;
    saving = true;
    try {
      if (name.trim() && name.trim() !== config.name) {
        await saveServerConfig(id, { ...config, name: name.trim() });
      }
      // Refused by the core while the server is running, and rightly so — it
      // would be overwritten on shutdown. The fields are disabled to match.
      if (!isUp) await saveServerProperties(id, properties);
      await loadSettings();
      error = null;
      saved = true;
      setTimeout(() => (saved = false), 1600);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  const remove = () =>
    run(async () => {
      await deleteServer(id);
      back();
    });

  const copyLog = () =>
    navigator.clipboard.writeText(visibleLines.map((l) => l.text).join("\n"));

  onMount(() => {
    refresh();
    loadSettings();
    loadMods();
    reloadConsole();

    const unlisten = onCoreEvent((event) => {
      if (event.type === "serversChanged") refresh();
      if (event.type === "serverState" && event.id === id) {
        // Straight from the event; the refetch below only adds the live
        // counters, and the header must not lag behind on its own state.
        if (server) server = { ...server, state: event.state, light: event.light };
        refresh();
        // Only a clean stop leads back to a start. A crash means the restart
        // already failed, and relaunching would loop on the same fault.
        if (restarting && (event.state.kind === "stopped" || event.state.kind === "crashed")) {
          const wasClean = event.state.kind === "stopped";
          restarting = false;
          if (wasClean) start();
        }
      }

      if (event.type === "serverLog" && event.id === id) {
        const first = event.lines[0];
        if (!first) return;
        const last = lines.at(-1);
        // A gap means the ring buffer dropped lines between what we hold and
        // what just arrived. Appending anyway would show a log that silently
        // skips; refetching shows exactly what the core still has.
        if (last && first.seq > last.seq + 1) return void reloadConsole();
        const merged = [...lines, ...event.lines.filter((l) => !last || l.seq > last.seq)];
        lines = merged.length > MAX_LINES ? merged.slice(-MAX_LINES) : merged;
      }
    });

    const tick = setInterval(() => {
      if (uptime !== null) uptime += 1;
    }, 1000);

    return () => {
      clearInterval(tick);
      void unlisten.then((fn) => fn());
    };
  });

  // Sticks to the bottom as lines arrive, unless the user turned that off to
  // read something further up.
  $effect(() => {
    void visibleLines.length;
    if (autoscroll && logEl) logEl.scrollTop = logEl.scrollHeight;
  });
</script>

<section class="page">
  <button class="back" onclick={back}>
    <Icon name="chevron-left" size={15} />
    所有伺服器
  </button>

  <header>
    <div class="ident">
      <div class="title">
        <h1>{server?.name ?? id}</h1>
        {#if server}
          <span class="pill" data-light={server.light}>
            <span class="dot"></span>
            {STATE_LABEL[server.state.kind]}
          </span>
        {/if}
      </div>
      <p class="meta">
        {#if server?.mcVersion}Minecraft {server.mcVersion}{:else}尚未安裝 Forge{/if}
        {#if server?.forgeVersion}<span class="sep">·</span>Forge {server.forgeVersion}{/if}
        <span class="sep">·</span>
        <code>{id}</code>
        {#if server?.imported}<span class="sep">·</span>已匯入{/if}
      </p>
    </div>

    <div class="actions">
      <Button
        variant={action.primary ? "primary" : "secondary"}
        danger={action.run === "stop"}
        disabled={action.disabled || restarting}
        onclick={() => (action.run === "start" ? start() : run(() => stopServer(id)))}
      >
        <Icon name={action.icon} size={14} />
        {action.label}
      </Button>
      <!-- Restarting a server that has not finished booting would send `stop`
           to something not yet listening for it. -->
      <Button disabled={!online || restarting} onclick={restart}>
        <Icon name="rotate-cw" size={14} />
        {restarting ? "重啟中…" : "重啟"}
      </Button>
      <Button onclick={() => run(() => openServerFolder(id))} aria-label="開啟資料夾">
        <Icon name="folder-open" size={16} />
      </Button>
    </div>
  </header>

  <div class="stats">
    <div class="stat">
      <span class="k">線上玩家</span>
      <span class="v tabular">{server?.playersOnline ?? "—"}<small>/ {server?.maxPlayers ?? "—"}</small></span>
    </div>
    <div class="stat">
      <span class="k">運行時間</span>
      <span class="v tabular">{uptime === null ? "—" : formatUptime(uptime)}</span>
    </div>
    <div class="stat">
      <span class="k">記憶體</span>
      <span class="v tabular">{server ? formatMemory(server.memoryMb) : "—"}</span>
    </div>
    <div class="stat">
      <span class="k">連接埠</span>
      <span class="v tabular">{server?.port ?? "—"}</span>
    </div>
  </div>

  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={tab === "console"} onclick={() => (tab = "console")}>
      主控台
    </button>
    <button role="tab" aria-selected={tab === "settings"} onclick={() => (tab = "settings")}>
      設定
    </button>
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if tab === "console"}
    <div class="console">
      <div class="log-panel">
        <div class="log-bar">
          <span class="log-title">
            <Icon name="terminal" size={14} />
            主控台輸出
          </span>
          <span class="log-tools">
            <button class:on={autoscroll} onclick={() => (autoscroll = !autoscroll)}>
              自動捲動
            </button>
            <button onclick={copyLog} aria-label="複製全部"><Icon name="copy" size={15} /></button>
            <button
              onclick={() => (hideBefore = (lines.at(-1)?.seq ?? 0) + 1)}
              aria-label="清除畫面"
            >
              <Icon name="trash" size={15} />
            </button>
          </span>
        </div>

        <div class="log" bind:this={logEl}>
          {#each visibleLines as line (line.seq)}
            <p class="line" data-level={level(line)}>{line.text}</p>
          {:else}
            <p class="log-empty">沒有輸出。啟動伺服器後這裡會即時顯示。</p>
          {/each}
        </div>
      </div>

      <div class="cmd" class:disabled={!online}>
        <span class="caret">/</span>
        <input
          bind:value={command}
          disabled={!online}
          placeholder={online ? "輸入指令後按 Enter 送出" : "伺服器未執行"}
          onkeydown={(e) => e.key === "Enter" && send()}
        />
        <button onclick={send} disabled={!online || !command.trim()} aria-label="送出">
          <Icon name="corner-down-left" size={15} />
        </button>
      </div>
    </div>
  {:else}
    <div class="settings">
      <div class="cols">
        <section class="card">
          <h2>伺服器</h2>

          <label class="field">
            <span>顯示名稱</span>
            <input bind:value={name} placeholder={config?.name ?? ""} />
          </label>

          <label class="field">
            <span>MOTD</span>
            <input
              disabled={isUp}
              value={properties?.motd ?? ""}
              oninput={(e) => properties && (properties.motd = e.currentTarget.value)}
            />
          </label>

          <p class="files-hint">直接編輯原始檔（伺服器停止時才能存檔）</p>
          {#each FILES as [which, filename, note] (which)}
            <button class="file-row" onclick={() => openFile(which)}>
              <span>
                <strong>{filename}</strong>
                <small>{note}</small>
              </span>
              <Icon name="pencil-line" size={15} />
            </button>
          {/each}
        </section>

        <section class="card">
          <h2>遊戲</h2>

          <div class="pair">
            <label class="field">
              <span>遊戲模式</span>
              <select
                disabled={isUp}
                value={properties?.gamemode}
                onchange={(e) => properties && (properties.gamemode = e.currentTarget.value as Gamemode)}
              >
                {#each GAMEMODES as [value, label] (value)}
                  <option {value}>{label}</option>
                {/each}
              </select>
            </label>
            <label class="field">
              <span>難度</span>
              <select
                disabled={isUp}
                value={properties?.difficulty}
                onchange={(e) => properties && (properties.difficulty = e.currentTarget.value as Difficulty)}
              >
                {#each DIFFICULTIES as [value, label] (value)}
                  <option {value}>{label}</option>
                {/each}
              </select>
            </label>
          </div>

          <div class="toggles">
            <label class="toggle">
              <span><strong>PVP</strong></span>
              <input
                type="checkbox"
                disabled={isUp}
                checked={properties?.pvp ?? true}
                onchange={(e) => properties && (properties.pvp = e.currentTarget.checked)}
              />
            </label>
            <label class="toggle">
              <span>
                <strong>正版驗證</strong>
                <small>關閉後非正版帳號可加入</small>
              </span>
              <input
                type="checkbox"
                disabled={isUp}
                checked={properties?.onlineMode ?? true}
                onchange={(e) => properties && (properties.onlineMode = e.currentTarget.checked)}
              />
            </label>
            <label class="toggle">
              <span><strong>白名單</strong></span>
              <input
                type="checkbox"
                disabled={isUp}
                checked={properties?.whiteList ?? false}
                onchange={(e) => properties && (properties.whiteList = e.currentTarget.checked)}
              />
            </label>
          </div>
        </section>
      </div>

      <section class="card mods">
        <div class="mods-head">
          <h2>模組</h2>
          <span class="mods-count tabular">{mods.length}</span>
          <span class="mods-line"></span>
          <Button onclick={addModFiles} disabled={isUp || modsBusy}>
            <Icon name="plus" size={15} />
            新增模組
          </Button>
        </div>

        {#if isUp}
          <p class="mods-note">伺服器執行中。模組只在啟動時載入，要停止後才能變更。</p>
        {/if}

        {#if mods.length === 0}
          <p class="mods-empty">
            還沒有模組。加進來的 jar 會複製到伺服器的 <code>mods/</code> 資料夾。
          </p>
        {:else}
          <ul class="mod-list">
            {#each mods as mod (mod.name)}
              <li>
                <span class="mod-name" title={mod.name}>{mod.name}</span>
                <span class="mod-size tabular">{formatBytes(mod.bytes)}</span>
                <button
                  class="mod-remove"
                  onclick={() => removeMod(mod.name)}
                  disabled={isUp}
                  aria-label="移除 {mod.name}"
                >
                  <Icon name="trash" size={15} />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <section class="danger">
        <span>
          <strong>刪除伺服器</strong>
          <small>
            {server?.imported
              ? "只會從清單移除，資料夾與世界檔案留在原處。"
              : "移除這個伺服器與它的世界檔案，無法復原。"}
          </small>
        </span>
        {#if confirmingDelete}
          <span class="confirm">
            <Button onclick={() => (confirmingDelete = false)}>取消</Button>
            <Button danger onclick={remove}>確定刪除</Button>
          </span>
        {:else}
          <Button danger disabled={isUp} onclick={() => (confirmingDelete = true)}>
            <Icon name="trash" size={14} />
            刪除
          </Button>
        {/if}
      </section>

      <div class="save-row">
        <span class="hint">
          {#if saved}已儲存。{/if}
          {#if isUp}
            伺服器執行中，關閉時會覆寫 server.properties。請先停止再修改。
          {:else}
            連接埠與遊戲模式需要重新啟動伺服器才會生效。
          {/if}
        </span>
        <span class="save-buttons">
          <Button onclick={loadSettings} disabled={saving}>還原</Button>
          <Button variant="primary" onclick={saveSettings} disabled={saving || !properties}>
            <Icon name="check" size={14} />
            儲存
          </Button>
        </span>
      </div>
    </div>
  {/if}
</section>

{#if javaMajor !== null}
  <JavaPrompt bind:open={javaOpen} major={javaMajor} onready={start} />
{/if}

<Modal
  bind:open={fileOpen}
  title={FILES.find(([f]) => f === fileWhich)?.[1] ?? ""}
  width={640}
>
  {#if isUp}
    <p class="modal-warn">伺服器執行中。關閉伺服器時會覆寫這個檔案，現在存檔不會生效。</p>
  {/if}
  <textarea class="editor" bind:value={fileText} spellcheck="false" disabled={fileBusy}></textarea>

  {#snippet footer()}
    <Button onclick={() => (fileOpen = false)}>取消</Button>
    <Button variant="primary" onclick={saveFile} disabled={fileBusy || isUp}>儲存</Button>
  {/snippet}
</Modal>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 18px 28px 22px;
    overflow: auto;
  }

  /* The UA gives a bare <button> a grey box and a border. Every button in this
     page draws its own surface, so clear it once here. */
  .page button {
    background: none;
    border: 0;
    padding: 0;
  }

  .back {
    display: flex;
    align-items: center;
    gap: 4px;
    align-self: flex-start;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .back:hover {
    color: var(--fg);
  }

  header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 20px;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  h1 {
    font-size: var(--font-hero);
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .pill {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 26px;
    padding: 0 11px;
    border-radius: var(--radius-pill);
    background: var(--wash);
    color: var(--muted);
    font-size: var(--font-small);
    font-weight: 500;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
  }

  .pill[data-light="online"] {
    color: var(--online);
    background: color-mix(in srgb, var(--online) 12%, transparent);
  }

  .pill[data-light="starting"] {
    color: var(--starting);
    background: color-mix(in srgb, var(--starting) 14%, transparent);
  }

  .pill[data-light="error"] {
    color: var(--error);
    background: color-mix(in srgb, var(--error) 12%, transparent);
  }

  .meta {
    margin-top: 7px;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .meta code {
    font-family: var(--font-mono);
    color: var(--faint);
  }

  .sep {
    margin: 0 6px;
    color: var(--faint);
  }

  .actions {
    display: flex;
    gap: 10px;
    flex: none;
  }

  /* ── stats ─────────────────────────────────────────── */

  .stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 18px;
  }

  .stat {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 13px 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
  }

  .k {
    color: var(--muted);
    font-size: var(--font-tiny);
  }

  .v {
    font-size: 22px;
    font-weight: 600;
  }

  .v small {
    margin-left: 5px;
    color: var(--faint);
    font-size: var(--font-small);
    font-weight: 400;
  }

  /* ── tabs ──────────────────────────────────────────── */

  .tabs {
    display: flex;
    gap: 26px;
    border-bottom: 1px solid var(--border);
  }

  .tabs button {
    padding: 0 2px 10px;
    color: var(--muted);
    font-size: var(--font-section);
    /* Reserves the underline's height on the inactive tab too, so switching
       tabs does not shift the panel below by two pixels. */
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    transition: color 140ms var(--ease), border-color 140ms var(--ease);
  }

  .tabs button:hover {
    color: var(--fg);
  }

  .tabs button[aria-selected="true"] {
    color: var(--fg);
    font-weight: 600;
    border-bottom-color: var(--accent);
  }

  /* ── console ───────────────────────────────────────── */

  .console {
    flex: 1;
    min-height: 260px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .log-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    overflow: hidden;
  }

  .log-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 42px;
    padding: 0 16px;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--font-small);
  }

  .log-title,
  .log-tools {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .log-tools button {
    display: flex;
    align-items: center;
    color: var(--muted);
  }

  .log-tools button:hover {
    color: var(--fg);
  }

  .log-tools button.on {
    padding: 4px 10px;
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
  }

  .log {
    flex: 1;
    overflow: auto;
    padding: 12px 16px;
    font-family: var(--font-mono);
    font-size: var(--font-small);
    line-height: 1.75;
  }

  .line {
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--muted);
  }

  .line[data-level="info"] {
    color: var(--fg);
  }

  .line[data-level="warn"] {
    color: var(--starting);
  }

  .line[data-level="error"] {
    color: var(--error);
  }

  .line[data-level="ready"] {
    color: var(--online);
  }

  .log-empty {
    color: var(--faint);
    font-family: var(--font-sans);
  }

  .cmd {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 44px;
    padding: 0 8px 0 14px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
  }

  .cmd:focus-within {
    border-color: var(--accent);
  }

  .cmd.disabled {
    opacity: 0.6;
  }

  .caret {
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--accent);
  }

  .cmd input {
    flex: 1;
    min-width: 0;
    /* The row itself is the input's frame — the field inside it draws nothing. */
    border: 0;
    background: transparent;
    outline: none;
    font-family: var(--font-mono);
    font-size: var(--font-body);
    color: var(--fg);
    user-select: text;
  }

  .cmd > button {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-fg);
  }

  .cmd > button:disabled {
    background: var(--wash-2);
    color: var(--faint);
  }

  /* ── settings ──────────────────────────────────────── */

  .settings {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding-bottom: 4px;
  }

  .cols {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 18px;
    align-items: start;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
  }

  .card h2 {
    font-size: var(--font-section);
    font-weight: 600;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field > span {
    color: var(--muted);
    font-size: var(--font-small);
  }

  .field input,
  .field select {
    height: 38px;
    padding: 0 12px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    color: var(--fg);
    font-size: var(--font-body);
  }

  .field input,
  .editor {
    user-select: text;
  }

  .field input:disabled,
  .field select:disabled,
  .toggle input:disabled {
    opacity: 0.5;
  }

  .field input:focus,
  .field select:focus {
    border-color: var(--accent);
  }

  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }

  .files-hint {
    margin-top: 2px;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .file-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 9px 12px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    color: var(--muted);
    text-align: left;
  }

  .file-row:hover {
    border-color: var(--accent);
    color: var(--fg);
  }

  .file-row strong,
  .toggle strong,
  .danger strong {
    display: block;
    color: var(--fg);
    font-size: var(--font-body);
    font-weight: 500;
  }

  .file-row strong {
    font-family: var(--font-mono);
    font-size: var(--font-small);
  }

  .file-row small,
  .toggle small,
  .danger small {
    color: var(--muted);
    font-size: var(--font-tiny);
  }

  .toggles {
    display: flex;
    flex-direction: column;
  }

  .toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 44px;
    padding: 6px 0;
    border-top: 1px solid var(--border);
  }

  .toggle:first-child {
    border-top: none;
  }

  /* A checkbox restyled into a switch: the native control keeps the label
     association, keyboard behaviour, and focus ring for free. */
  .toggle input {
    appearance: none;
    flex: none;
    width: 39px;
    height: 22px;
    border-radius: var(--radius-pill);
    background: var(--wash-2);
    transition: background-color 140ms var(--ease);
  }

  .toggle input::after {
    content: "";
    display: block;
    width: 16px;
    height: 16px;
    margin: 3px;
    border-radius: 50%;
    background: var(--surface);
    box-shadow: var(--shadow);
    transition: transform 140ms var(--ease);
  }

  .toggle input:checked {
    background: var(--accent);
  }

  .toggle input:checked::after {
    transform: translateX(17px);
  }

  /* ── mods ──────────────────────────────────────────── */

  .mods-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .mods-count {
    color: var(--muted);
    font-size: var(--font-small);
  }

  /* Pushes the button to the right and draws the rule the header sits on. */
  .mods-line {
    flex: 1;
    height: 1px;
    background: var(--border);
  }

  .mods-note,
  .mods-empty {
    color: var(--muted);
    font-size: var(--font-small);
    line-height: 1.6;
  }

  .mods-empty code {
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
  }

  .mod-list {
    display: flex;
    flex-direction: column;
    list-style: none;
    /* A long mod list must not push the save row off the page. */
    max-height: 260px;
    overflow: auto;
  }

  .mod-list li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 40px;
    padding: 4px 0;
    border-top: 1px solid var(--border);
  }

  .mod-list li:first-child {
    border-top: none;
  }

  .mod-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: var(--font-small);
  }

  .mod-size {
    color: var(--faint);
    font-size: var(--font-small);
  }

  .mod-remove {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 8px;
    color: var(--muted);
  }

  .mod-remove:hover:not(:disabled) {
    background: color-mix(in srgb, var(--error) 12%, transparent);
    color: var(--error);
  }

  .mod-remove:disabled {
    opacity: 0.4;
  }

  .danger {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 14px 18px;
    background: color-mix(in srgb, var(--error) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--error) 22%, transparent);
    border-radius: var(--radius-card);
  }

  .confirm {
    display: flex;
    gap: 10px;
    flex: none;
  }

  .save-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .hint {
    color: var(--muted);
    font-size: var(--font-small);
  }

  .save-buttons {
    display: flex;
    gap: 10px;
  }

  /* ── file editor ───────────────────────────────────── */

  .modal-warn {
    margin-bottom: 12px;
    padding: 10px 12px;
    background: color-mix(in srgb, var(--starting) 12%, transparent);
    border-radius: var(--radius-input);
    color: var(--fg);
    font-size: var(--font-small);
  }

  .editor {
    width: 100%;
    height: 380px;
    padding: 12px 14px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: var(--font-small);
    line-height: 1.7;
    resize: none;
    white-space: pre;
  }

  .editor:focus {
    border-color: var(--accent);
  }

  .error {
    padding: 10px 14px;
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border-radius: var(--radius-input);
    color: var(--error);
    font-size: var(--font-small);
  }
</style>
