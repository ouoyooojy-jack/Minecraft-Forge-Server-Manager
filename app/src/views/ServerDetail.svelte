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
  import { getCurrentWebview } from "@tauri-apps/api/webview";

  import Button from "../lib/Button.svelte";
  import Icon from "../lib/Icon.svelte";
  import JavaPrompt from "../lib/JavaPrompt.svelte";
  import Modal from "../lib/Modal.svelte";
  import { formatBytes, formatMemory, formatUptime } from "../lib/format";
  import { isActive, primaryAction, STATE_LABEL } from "../lib/serverState";
  import { unsaved } from "../lib/unsaved.svelte";
  import {
    addMods,
    cancelRestart,
    killServer,
    openCrashReport,
    playerAccess,
    playitDeleteTunnel,
    playitStart,
    setPlayerAccess,
    setWhitelist,
    deleteMod,
    deleteServer,
    consoleSince,
    createBackup,
    crashReport,
    deleteBackup,
    exportDiagnostics,
    listBackups,
    restoreBackup,
    setModEnabled,
    getServerConfig,
    listPlayers,
    getProperties,
    listMods,
    listServers,
    onCoreEvent,
    openServerFolder,
    playitCreateTunnel,
    playitStatus,
    readServerFile,
    saveProperties,
    saveServerConfig,
    sendConsoleCommand,
    startServer,
    stopServer,
    writeServerFile,
  } from "../lib/api";
  import {
    errorMessage,
    isCoreError,
    type Backup,
    type CrashInfo,
    type Fix,
    type LogLine,
    type AccessList,
    type ModFile,
    type PlayerAccess,
    type PlayitStatus,
    type PropertyField,
    type PropertyGroup,
    type ServerConfig,
    type ServerFile,
    type ServerId,
    type ServerSummary,
  } from "../lib/types";

  let { id, back }: { id: ServerId; back: () => void } = $props();

  /** Left rail of the settings tab. The four groups come from the core's
   *  table; the last two are this app's own, not part of the file. */
  type Section = "general" | PropertyGroup | "players" | "mods" | "backups" | "files";

  const SECTIONS: [Section, string][] = [
    ["general", "一般"],
    ["connection", "連線"],
    ["gameplay", "玩法"],
    ["world", "世界"],
    ["advanced", "進階"],
    ["players", "玩家"],
    ["mods", "模組"],
    ["backups", "備份"],
    ["files", "原始檔"],
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

  /** Free-text filter over the console, case-insensitive. */
  let find = $state("");
  /** Hide everything below a warning. The severity is Minecraft's own tag,
   *  read by `level` — this is a filter over that, not a second guess. */
  let warnOnly = $state(false);

  const visibleLines = $derived(
    lines.filter((l) => {
      if (l.seq < hideBefore) return false;
      if (warnOnly && level(l) === "info") return false;
      return find === "" || l.text.toLowerCase().includes(find.toLowerCase());
    }),
  );

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
  let name = $state("");
  let section = $state<Section>("general");

  /** Memory, in MB, as typed. Not a server.properties key — it lives in
   *  `user_jvm_args.txt` as `-Xmx`, which the core keeps in sync with the
   *  config. Kept as a string so a half-typed number is not clamped mid-edit. */
  let memory = $state("");

  /**
   * The app's own per-server switches, as the form holds them.
   *
   * They save with 儲存 like every other row. They used to save on click, and
   * that save reloaded the form — throwing away any server.properties edit
   * sitting unsaved next to them, with nothing on screen to say so.
   */
  let restartOnCrash = $state(false);
  let backupOnStop = $state(false);
  /** Hours as typed; "0" is off. */
  let restartHours = $state("0");

  /** Every server, to catch two of them claiming the same port. The check has
   *  to look outside this page, so the whole list is kept rather than one. */
  let all = $state<ServerSummary[]>([]);

  /** Every key in the file, in the core's order. Never mutated in place — an
   *  edit goes in `edits`, so what is on disk stays visible for comparison. */
  let fields = $state<PropertyField[]>([]);

  /**
   * Pending changes, keyed exactly as the file keys them.
   *
   * Only what actually differs from disk lives here: `set` drops a key once
   * it is typed back to its original value, which keeps the save a true diff
   * and means "還原" has nothing to undo when nothing was changed.
   */
  let edits = $state<Record<string, string>>({});

  const shown = $derived(fields.filter((f) => f.group === section));

  /** How many rows differ from disk — what the save bar counts. */
  const pending = $derived(
    Object.keys(edits).length +
      [
        name.trim() !== "" && name.trim() !== config?.name,
        memory !== "" && Number(memory) !== config?.memoryMb,
        config !== null && restartOnCrash !== config.restartOnCrash,
        config !== null && backupOnStop !== config.backupOnStop,
        restartHours !== "" && Number(restartHours) !== config?.restartEveryHours,
      ].filter(Boolean).length,
  );
  const dirty = $derived(pending > 0);

  // The shell asks before navigating away from unsaved edits.
  $effect(() => {
    unsaved.dirty = dirty;
    return () => (unsaved.dirty = false);
  });

  /** Below 512 MB no Forge server starts at all; above 64 GB the number is a
   *  typo, and an -Xmx larger than the machine has makes the JVM refuse to
   *  launch with a message nobody reads. */
  const memoryBad = $derived(
    memory !== "" && !(/^\d+$/.test(memory) && +memory >= 512 && +memory <= 65536),
  );

  /** The port as it would be saved, and who else already answers on it. */
  const portClash = $derived.by(() => {
    const field = fields.find((f) => f.key === "server-port");
    if (!field) return null;
    const port = Number(edits["server-port"] ?? field.value);
    return all.find((s) => s.id !== id && s.port === port)?.name ?? null;
  });

  /** Numeric fields are typed as free text, so an empty or non-numeric box has
   *  to block the save — `max-players=` would leave the server unstartable. */
  const hoursBad = $derived(!(/^\d+$/.test(restartHours) && +restartHours <= 168));

  const invalid = $derived(
    memoryBad ||
      hoursBad ||
      fields.some(
        (f) => f.kind === "int" && f.key in edits && !/^-?\d+$/.test(edits[f.key]),
      ),
  );

  /**
   * The playit.gg tunnel for this server, for the 連線 section.
   *
   * By the id the core stored when it made the tunnel. The name is only a
   * fallback for a tunnel made before ids were stored — names drift when a
   * server is renamed, ids do not.
   */
  let playit = $state<PlayitStatus | null>(null);
  let tunnelBusy = $state(false);
  let confirmingUntunnel = $state(false);
  const tunnel = $derived(
    playit?.tunnels.find((t) => config?.tunnelId && t.id === config.tunnelId) ??
      playit?.tunnels.find((t) => t.name === config?.name) ??
      null,
  );
  /** Created, but playit has not allocated the hostname yet. */
  const tunnelPending = $derived(playit?.pending.includes(config?.name ?? "") ?? false);
  /** The tunnel forwards somewhere this server no longer listens. */
  const tunnelStale = $derived(
    tunnel?.localPort != null && server?.port != null && tunnel.localPort !== server.port,
  );

  /** Flips for a moment after a copy, so a silent clipboard write has a reply. */
  let copied = $state(false);

  function copyInvite() {
    const address = tunnel?.address;
    if (!address) return;
    navigator.clipboard.writeText(address);
    copied = true;
    setTimeout(() => (copied = false), 1600);
  }

  /**
   * Everything a friend needs to join, as one paste: where, which version, and
   * which mods to install. The mod list is the part people forget, and a
   * modded server refuses a client that is missing one.
   */
  let invited = $state(false);

  function copyInvitation() {
    if (!tunnel?.address || !server) return;
    const enabled = mods.filter((m) => m.enabled);
    const forge = server.forgeVersion ? ` + Forge ${server.forgeVersion}` : "";
    const text = [
      `伺服器：${server.name}`,
      `位址：${tunnel.address}`,
      `版本：Minecraft ${server.mcVersion ?? "?"}${forge}`,
      enabled.length
        ? `模組（${enabled.length} 個，要裝一樣的）：\n` +
          enabled
            .map((m) => `- ${m.displayName ?? m.name}${m.version ? ` ${m.version}` : ""}`)
            .join("\n")
        : "沒有裝模組。",
    ].join("\n");
    navigator.clipboard.writeText(text);
    invited = true;
    setTimeout(() => (invited = false), 1600);
  }

  const refreshPlayit = () =>
    playitStatus()
      .then((s) => (playit = s))
      // Not surfaced: the section still has to draw its own rows, and the
      // address row already says what to do when there is no tunnel.
      .catch(() => (playit = null));

  const valueOf = (f: PropertyField) => edits[f.key] ?? f.value;

  function set(f: PropertyField, value: string) {
    const next = { ...edits };
    if (value === f.value) delete next[f.key];
    else next[f.key] = value;
    edits = next;
  }
  let saving = $state(false);
  let saved = $state(false);

  let fileOpen = $state(false);
  let fileWhich = $state<ServerFile>("properties");
  let fileText = $state("");
  let fileBusy = $state(false);

  let mods = $state<ModFile[]>([]);
  let modsBusy = $state(false);

  let backups = $state<Backup[]>([]);
  /** Set while a zip is being written or unpacked. Both can take a minute on
   *  a large world, and both must not be startable twice. */
  let backupBusy = $state(false);
  let restoring = $state<string | null>(null);

  const loadBackups = () =>
    run(async () => {
      backups = await listBackups(id);
    });

  async function backupNow() {
    backupBusy = true;
    await run(async () => {
      await createBackup(id);
      backups = await listBackups(id);
    });
    backupBusy = false;
  }

  async function restore(name: string) {
    backupBusy = true;
    restoring = null;
    await run(async () => {
      await restoreBackup(id, name);
      backups = await listBackups(id);
    });
    backupBusy = false;
  }

  let deletingBackup = $state<string | null>(null);

  const removeBackup = (name: string) =>
    run(async () => {
      deletingBackup = null;
      await deleteBackup(id, name);
      backups = await listBackups(id);
    });

  /** Which kind of copy it is, from the prefix the core gave its name. */
  const backupKind = (name: string) =>
    name.startsWith("stop-")
      ? "關閉時自動備份"
      : name.startsWith("before-restore-")
        ? "還原前自動備份"
        : null;

  /**
   * Files hovering over the window, while the mods pane is the one showing.
   *
   * The drag events are the webview's, not this element's — the OS hands the
   * drop to the window, so there is no DOM target to hang it on. Which means
   * this has to gate on the pane itself being open, or a jar dropped while
   * the console is showing would silently install.
   */
  let dropping = $state(false);

  const modsPaneOpen = $derived(tab === "settings" && section === "mods");

  async function dropMods(paths: string[]) {
    const jars = paths.filter((p) => p.toLowerCase().endsWith(".jar"));
    if (!jars.length || isUp) return;
    modsBusy = true;
    await run(async () => {
      await addMods(id, jars);
      mods = await listMods(id);
    });
    modsBusy = false;
  }

  const toggleMod = (mod: ModFile) =>
    run(async () => {
      await setModEnabled(id, mod.name, !mod.enabled);
      mods = await listMods(id);
    });

  /** Where the last bundle was written, so the message can say. */
  let exported = $state<string | null>(null);

  const exportBundle = () =>
    run(async () => {
      exported = await exportDiagnostics(id);
    });

  const dateOf = (secs: number) =>
    new Date(secs * 1000).toLocaleString("zh-TW", {
      dateStyle: "medium",
      timeStyle: "short",
    });

  let confirmingDelete = $state(false);

  /** Set while a restart is waiting for the process to actually exit. The
   *  start half fires from the state event rather than a timer, because a
   *  modded world can take a minute to save and any fixed wait would be wrong. */
  let restarting = $state(false);

  /**
   * Why the last run ended, when it ended badly. Cleared on every start: a
   * diagnosis left on screen while the server boots is describing the past.
   */
  let crash = $state<CrashInfo | null>(null);

  const loadCrash = () =>
    run(async () => {
      crash = await crashReport(id);
    });

  /**
   * The setting a diagnosis just sent us to, marked for a moment.
   *
   * Landing on the right tab is not the same as finding the right row: the
   * advanced section is forty rows long, and "we moved you here" has to say
   * where here is.
   */
  let highlight = $state<string | null>(null);

  function focusRow(key: string) {
    highlight = key;
    // After the section swap has rendered; the element does not exist yet.
    requestAnimationFrame(() =>
      document
        .getElementById(`row-${key}`)
        ?.scrollIntoView({ block: "center", behavior: "smooth" }),
    );
    setTimeout(() => (highlight = null), 2000);
  }

  /** Send the user to whatever fixes it. The core picked the destination; this
   *  only knows how to get there. */
  function applyFix(fix: Fix) {
    if (fix.kind === "java") {
      javaMajor = fix.major;
      javaOpen = true;
      return;
    }
    if (fix.kind === "eula") {
      // No form for the EULA — it is accepted at install time. Someone who got
      // here edited the file by hand, so hand them the file.
      run(() => openServerFolder(id));
      return;
    }
    tab = "settings";
    if (fix.kind === "memory") {
      section = "general";
      focusRow("memory");
    } else if (fix.kind === "port") {
      section = "connection";
      focusRow("server-port");
    } else {
      section = "mods";
    }
  }

  /**
   * Who is on the server right now.
   *
   * Kept from the core's `players` event, which carries the whole roster on
   * every change. Seeded once on mount for a page opened mid-session.
   */
  let players = $state<string[]>([]);

  const loadPlayers = () =>
    run(async () => {
      players = await listPlayers(id);
    });

  /** Every moderation action is a console command, so there is nothing to add
   *  to the core for this — the server already understands all four. */
  const moderate = (verb: string, who: string) =>
    run(() => sendConsoleCommand(id, `${verb} ${who}`));

  let banning = $state<string | null>(null);

  /** The whitelist and the operators. Read from the server's own files. */
  let access = $state<PlayerAccess | null>(null);
  let newWhitelist = $state("");
  let newOp = $state("");
  const NAME_RULE = /^[A-Za-z0-9_]{1,16}$/;

  const loadAccess = () =>
    run(async () => {
      access = await playerAccess(id);
    });

  /**
   * Through the server's console: Minecraft looks the name up and writes the
   * file itself. It writes it a moment after the command, so the list is read
   * again after that moment rather than straight away.
   */
  const changeAccess = (list: AccessList, who: string, allowed: boolean) =>
    run(async () => {
      await setPlayerAccess(id, list, who, allowed);
      if (list === "whitelist") newWhitelist = "";
      else newOp = "";
      setTimeout(loadAccess, 1200);
    });

  function addName(list: AccessList) {
    const who = (list === "whitelist" ? newWhitelist : newOp).trim();
    if (NAME_RULE.test(who)) changeAccess(list, who, true);
  }

  const toggleWhitelist = () =>
    run(async () => {
      await setWhitelist(id, !access?.whitelistOn);
      setTimeout(loadAccess, 1200);
    });

  /** What the button under a diagnosis should say. */
  const FIX_LABEL: Record<Fix["kind"], string> = {
    memory: "調高記憶體",
    port: "改連接埠",
    java: "下載對應的 Java",
    mods: "檢查模組",
    eula: "開啟伺服器資料夾",
  };

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

  /** Set while the second click that ends the process is being waited for. */
  let confirmingKill = $state(false);

  const kill = () =>
    run(async () => {
      confirmingKill = false;
      await killServer(id);
    });

  /**
   * Start, offering the download when the machine has no matching Java.
   *
   * Unsaved edits are saved first. Pressing 啟動 with a switch flipped and not
   * saved means "start it like this"; starting with the old value and saying
   * nothing is how a setting silently fails to take.
   */
  async function start() {
    if (dirty) {
      if (invalid) {
        tab = "settings";
        error = "有還沒儲存的設定格式不對，先改好或按「還原」再啟動。";
        return;
      }
      if (!(await saveSettings())) return;
    }
    try {
      await startServer(id);
      error = null;
      crash = null;
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

  // Read on the way into the 連線 section rather than on mount: a tunnel can be
  // started, stopped or deleted on playit's own site, so what matters is that
  // it is fresh when it is looked at.
  $effect(() => {
    if (section === "connection") void refreshPlayit();
  });

  /** Make the tunnel, or point the existing one back at this server's port. */
  async function createTunnel() {
    tunnelBusy = true;
    await run(async () => {
      await playitCreateTunnel(id);
      await refreshPlayit();
    });
    tunnelBusy = false;
  }

  async function startTunnel() {
    tunnelBusy = true;
    await run(async () => {
      await playitStart();
      await refreshPlayit();
    });
    tunnelBusy = false;
  }

  async function removeTunnel() {
    if (!tunnel) return;
    const which = tunnel.id;
    confirmingUntunnel = false;
    tunnelBusy = true;
    await run(async () => {
      await playitDeleteTunnel(which);
      await refreshPlayit();
    });
    tunnelBusy = false;
  }

  const refresh = () =>
    run(async () => {
      all = await listServers();
      const found = all.find((s) => s.id === id);
      // The server was deleted from under us — nothing left to show.
      if (!found) {
        unsaved.dirty = false;
        return back();
      }
      server = found;
      uptime = found.uptimeSecs;
    });

  const loadSettings = () =>
    run(async () => {
      config = await getServerConfig(id);
      fields = await getProperties(id);
      edits = {};
      name = config.name;
      memory = String(config.memoryMb);
      restartOnCrash = config.restartOnCrash;
      backupOnStop = config.backupOnStop;
      restartHours = String(config.restartEveryHours);
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

  /** Only server.properties edits are pending, and those wait for a stop. */
  const onlyPropsWhileUp = $derived(isUp && pending === Object.keys(edits).length);

  /** Resolves whether everything pending reached the disk. */
  async function saveSettings(): Promise<boolean> {
    if (!config || saving || invalid) return false;
    saving = true;
    try {
      // Everything but server.properties is config, so one write covers it —
      // and the core turns `memoryMb` back into the `-Xmx` line on disk.
      const next: ServerConfig = {
        ...config,
        name: name.trim() || config.name,
        memoryMb: memory === "" ? config.memoryMb : Number(memory),
        restartOnCrash,
        backupOnStop,
        restartEveryHours: Number(restartHours),
      };
      if (JSON.stringify(next) !== JSON.stringify(config)) {
        await saveServerConfig(id, next);
      }
      // Refused by the core while the server is running, and rightly so — it
      // would be overwritten on shutdown. Kept pending rather than dropped, so
      // they can be saved once it stops.
      const kept = isUp ? edits : {};
      if (!isUp && Object.keys(edits).length) await saveProperties(id, edits);
      await loadSettings();
      edits = kept;
      error = null;
      saved = true;
      setTimeout(() => (saved = false), 1600);
      return Object.keys(kept).length === 0;
    } catch (e) {
      error = errorMessage(e);
      return false;
    } finally {
      saving = false;
    }
  }

  const remove = () =>
    run(async () => {
      await deleteServer(id);
      unsaved.dirty = false;
      back();
    });

  const copyLog = () =>
    navigator.clipboard.writeText(visibleLines.map((l) => l.text).join("\n"));

  onMount(() => {
    // The roster is stitched together from join and leave lines, so it can
    // drift. `list` is the server's own authoritative answer and the reply
    // resets it — asked once, here, where a stale roster would be seen.
    refresh().then(() => {
      if (online) run(() => sendConsoleCommand(id, "list"));
    });
    // The copy button lives in the header, so the tunnel has to be known
    // before anyone opens the 連線 section.
    void refreshPlayit();
    loadBackups();
    loadPlayers();
    loadAccess();
    loadCrash();
    loadSettings();
    loadMods();
    reloadConsole();

    // Tauri hands drag-and-drop to the whole webview; `unlistenDrop` is kept
    // alongside the event subscription so both come off together.
    const dropped = getCurrentWebview().onDragDropEvent((event) => {
      if (!modsPaneOpen) {
        dropping = false;
        return;
      }
      if (event.payload.type === "over") dropping = true;
      else if (event.payload.type === "drop") {
        dropping = false;
        dropMods(event.payload.paths);
      } else dropping = false;
    });

    const unlisten = onCoreEvent((event) => {
      if (event.type === "serversChanged") refresh();
      if (event.type === "players" && event.id === id) players = event.names;
      if (event.type === "serverState" && event.id === id) {
        // Straight from the event; the refetch below only adds the live
        // counters, and the header must not lag behind on its own state.
        if (server) server = { ...server, state: event.state, light: event.light };
        refresh();
        // The crash report only exists once the process is gone, so this is
        // the earliest moment there is anything to read.
        if (event.state.kind === "crashed") loadCrash();
        if (event.state.kind !== "stopping") confirmingKill = false;
        // Minecraft rewrites server.properties as it shuts down, and the
        // whitelist command writes it too. Show what is on disk now — unless
        // there are edits, which a reload would throw away.
        if (event.state.kind === "stopped" || event.state.kind === "crashed") {
          if (!dirty) loadSettings();
          loadAccess();
          loadBackups();
        }
        // A start may have opened a public address on its way up. Re-read it
        // once the address has had a moment to be allocated, so the copy
        // button in the header appears without being gone looking for.
        if (event.state.kind === "starting") setTimeout(refreshPlayit, 4000);
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
      void dropped.then((fn) => fn());
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
      {#if server?.state.kind === "stopping"}
        <!-- A modded world can take minutes to save, and a hung one never
             finishes. The world loses whatever it had not written yet. -->
        <Button
          danger
          onclick={() => (confirmingKill ? kill() : (confirmingKill = true))}
          title="不等存檔，直接結束程序。最後幾分鐘的進度可能會遺失。"
        >
          {confirmingKill ? "確定強制結束" : "強制結束"}
        </Button>
      {/if}
      <!-- Restarting a server that has not finished booting would send `stop`
           to something not yet listening for it. -->
      <Button disabled={!online || restarting} onclick={restart}>
        <Icon name="rotate-cw" size={14} />
        {restarting ? "重啟中…" : "重啟"}
      </Button>
      {#if tunnel?.address}
        <Button onclick={copyInvite}>
          <Icon name={copied ? "check" : "copy"} size={14} />
          {copied ? "已複製" : "複製公開位址"}
        </Button>
      {/if}
      <Button onclick={() => run(() => openServerFolder(id))} aria-label="開啟資料夾">
        <Icon name="folder-open" size={16} />
      </Button>
    </div>
  </header>

  <div class="stats">
    <div class="stat">
      <span class="k">線上玩家</span>
      <span class="v tabular">{server?.players?.length ?? "—"}<small>/ {server?.maxPlayers ?? "—"}</small></span>
    </div>
    <div class="stat">
      <span class="k">已執行</span>
      <span class="v tabular">{uptime === null ? "—" : formatUptime(uptime)}</span>
    </div>
    <div class="stat">
      <span class="k">記憶體上限</span>
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
      伺服器設定
    </button>
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if server?.restartPending}
    <div class="pending-restart">
      <span>伺服器當機了，幾秒後會自動重新啟動。</span>
      <Button onclick={() => run(() => cancelRestart(id))}>取消自動重啟</Button>
    </div>
  {/if}

  {#if tab === "console"}
    <div class="console">
      {#if crash && server?.state.kind === "crashed"}
        <div class="crash">
          <p class="crash-head">{crash.headline}</p>
          <pre class="crash-detail">{crash.detail}</pre>
          <div class="crash-acts">
            {#if crash.fix}
              <Button variant="primary" onclick={() => crash?.fix && applyFix(crash.fix)}>
                {FIX_LABEL[crash.fix.kind]}
              </Button>
            {/if}
            {#if crash.path}
              <Button onclick={() => run(() => openCrashReport(id))}>開啟當機報告</Button>
            {/if}
            <Button onclick={exportBundle} title="壓成一個 zip，要問人時丟這個檔就好">
              匯出診斷檔
            </Button>
          </div>
        </div>
      {/if}

      {#if online}
        <div class="roster">
          <span class="roster-head">
            線上玩家
            <span class="tabular">{players.length}</span>
          </span>
          {#if players.length === 0}
            <span class="roster-empty">目前沒有人在線</span>
          {:else}
            <ul>
              {#each players as who (who)}
                <li>
                  <span class="who">{who}</span>
                  {#if banning === who}
                    <button onclick={() => (banning = null)}>取消</button>
                    <button
                      class="danger-act"
                      onclick={() => (moderate("ban", who), (banning = null))}
                    >
                      確定封鎖
                    </button>
                  {:else}
                    <button onclick={() => moderate("op", who)}>給 OP</button>
                    <button onclick={() => moderate("kick", who)}>踢出</button>
                    <button class="danger-act" onclick={() => (banning = who)}>封鎖</button>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}

      <div class="log-panel">
        <div class="log-bar">
          <span class="log-title">
            <Icon name="terminal" size={14} />
            主控台輸出
          </span>
          <span class="log-tools">
            <input
              class="find"
              bind:value={find}
              placeholder="搜尋"
              spellcheck="false"
            />
            <button class:on={warnOnly} onclick={() => (warnOnly = !warnOnly)}>
              只看警告以上
            </button>
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
      <nav class="sections" aria-label="設定分區">
        {#each SECTIONS as [key, label] (key)}
          <button
            class="section"
            class:active={section === key}
            aria-current={section === key ? "true" : undefined}
            onclick={() => (section = key)}
          >
            {label}
          </button>
        {/each}
      </nav>

      <div class="body">
        {#if section === "mods"}
          <div class="pane-head">
            <h2>模組</h2>
            <span class="count tabular">{mods.length}</span>
            <Button onclick={addModFiles} disabled={isUp || modsBusy}>
              <Icon name="plus" size={15} />
              新增模組
            </Button>
          </div>

          {#if isUp}
            <p class="note">伺服器執行中。模組只在啟動時載入，要停止後才能變更。</p>
          {:else}
            <p class="note" class:dropping>
              {dropping ? "放開就會加進來。" : "把 jar 檔拖進這個視窗也可以加入模組。"}
            </p>
          {/if}

          {#if mods.length === 0}
            <p class="note">
              還沒有模組。加進來的 jar 會複製到伺服器的 <code>mods/</code> 資料夾。
            </p>
          {:else}
            <ul class="mod-list">
              {#each mods as mod (mod.name)}
                <li class:off={!mod.enabled}>
                  <span class="mod-name" class:plain={mod.displayName} title={mod.name}>
                    {mod.displayName ?? mod.name}
                    {#if mod.version}<span class="mod-ver">{mod.version}</span>{/if}
                  </span>
                  <span class="mod-size tabular">{formatBytes(mod.bytes)}</span>
                  <button
                    class="mod-toggle"
                    class:on={mod.enabled}
                    onclick={() => toggleMod(mod)}
                    disabled={isUp}
                    title={mod.enabled ? "停用（保留檔案）" : "啟用"}
                  >
                    {mod.enabled ? "啟用中" : "已停用"}
                  </button>
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

        {:else if section === "players"}
          {#snippet accessList(list: AccessList, title: string, names: string[])}
            <div class="pane-head sub">
              <h3>{title}</h3>
              <span class="count tabular">{names.length}</span>
            </div>
            {#if names.length > 0}
              <ul class="mod-list">
                {#each names as who (who)}
                  <li>
                    <span class="mod-name plain">{who}</span>
                    <button
                      class="mod-remove"
                      disabled={!online}
                      onclick={() => changeAccess(list, who, false)}
                      aria-label="移除 {who}"
                    >
                      <Icon name="trash" size={15} />
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
            <div class="add-name">
              <input
                placeholder="玩家名稱（Minecraft 帳號名）"
                spellcheck="false"
                disabled={!online}
                value={list === "whitelist" ? newWhitelist : newOp}
                oninput={(e) =>
                  list === "whitelist"
                    ? (newWhitelist = e.currentTarget.value)
                    : (newOp = e.currentTarget.value)}
                onkeydown={(e) => e.key === "Enter" && addName(list)}
              />
              <Button
                disabled={!online ||
                  !NAME_RULE.test((list === "whitelist" ? newWhitelist : newOp).trim())}
                onclick={() => addName(list)}
              >
                加入
              </Button>
            </div>
          {/snippet}

          <div class="pane-head">
            <h2>玩家</h2>
          </div>
          <p class="note">
            {online
              ? "名單透過伺服器自己的指令修改，會立刻生效。"
              : "伺服器執行中才能修改名單：Minecraft 要自己查玩家帳號。下面是上次存下來的名單。"}
          </p>

          <div class="row">
            <span class="label">
              <span class="name">白名單</span>
              <code>white-list</code>
              <small>
                {access?.whitelistOn
                  ? "開啟中：只有名單上的人進得來。"
                  : "關閉中：任何知道位址的人都能進來。公開位址發出去之前建議打開。"}
              </small>
            </span>
            <span class="control">
              <button
                class="bool"
                class:on={access?.whitelistOn}
                disabled={!online}
                onclick={toggleWhitelist}
              >
                {access?.whitelistOn ? "開" : "關"}
              </button>
            </span>
          </div>

          {@render accessList("whitelist", "白名單", access?.whitelist ?? [])}
          {@render accessList("ops", "管理員（OP）", access?.ops ?? [])}
        {:else if section === "backups"}
          <div class="pane-head">
            <h2>備份</h2>
            <span class="count tabular">{backups.length}</span>
            <Button onclick={backupNow} disabled={isUp || backupBusy}>
              <Icon name="plus" size={15} />
              立即備份
            </Button>
          </div>

          {#if isUp}
            <p class="note">
              伺服器執行中。世界檔案正在被寫入，現在複製會拷到寫到一半的狀態——先停止伺服器。
            </p>
          {:else}
            <p class="note">
              把世界資料夾壓成 zip 存在這個 app 的資料夾裡，不會寫進伺服器目錄。
              手動備份保留最近 10 份、關閉時自動備份 5 份、還原前自動存的 3 份，各算各的。
            </p>
          {/if}

          {#if backupBusy}
            <p class="note">處理中……世界大的話要一兩分鐘。</p>
          {/if}

          {#if backups.length === 0}
            <p class="note">還沒有備份。</p>
          {:else}
            <ul class="mod-list">
              {#each backups as item (item.name)}
                  <li>
                  <span class="mod-name" title={item.name}>
                    {dateOf(item.createdSecs)}
                    {#if backupKind(item.name)}<span class="auto">{backupKind(item.name)}</span>{/if}
                  </span>
                  <span class="mod-size tabular">{formatBytes(item.bytes)}</span>
                  {#if restoring === item.name}
                    <button onclick={() => (restoring = null)}>取消</button>
                    <button class="mod-toggle on" onclick={() => restore(item.name)}>
                      確定覆蓋世界
                    </button>
                  {:else}
                    <button
                      class="mod-toggle"
                      onclick={() => (restoring = item.name)}
                      disabled={isUp || backupBusy}
                    >
                      還原
                    </button>
                  {/if}
                  {#if deletingBackup === item.name}
                    <button onclick={() => (deletingBackup = null)}>取消</button>
                    <button class="mod-toggle on" onclick={() => removeBackup(item.name)}>
                      確定刪除
                    </button>
                  {:else}
                    <button
                      class="mod-remove"
                      onclick={() => (deletingBackup = item.name)}
                      disabled={backupBusy}
                      aria-label="刪除備份"
                    >
                      <Icon name="trash" size={15} />
                    </button>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        {:else if section === "files"}
          <div class="pane-head">
            <h2>原始檔</h2>
          </div>
          <p class="note">直接編輯檔案內容。伺服器停止時才能存檔。</p>
          <button class="row file-row" onclick={exportBundle}>
            <span class="label">
              <span class="name">匯出診斷檔</span>
              <small>
                {exported
                  ? `已寫出：${exported}`
                  : "把主控台、Minecraft 的 log、當機報告、模組清單和設定壓成一個 zip，要問人時丟這個檔就好。不會上傳到任何地方。"}
              </small>
            </span>
            <span class="control"><Icon name="download" size={15} /></span>
          </button>

          {#each FILES as [which, filename, note] (which)}
            <button class="row file-row" onclick={() => openFile(which)}>
              <span class="label">
                <span class="name mono">{filename}</span>
                <small>{note}</small>
              </span>
              <span class="control"><Icon name="pencil-line" size={15} /></span>
            </button>
          {/each}
        {:else}
          {#if section === "general"}
            <label class="row">
              <span class="label">
                <span class="name">顯示名稱</span>
                <small>只是這個 app 裡的名字，不會動到資料夾。</small>
              </span>
              <span class="control">
                <input bind:value={name} placeholder={config?.name ?? ""} />
              </span>
            </label>

            <div class="row" class:changed={config !== null && restartOnCrash !== config.restartOnCrash}>
              <span class="label">
                <span class="name">當機後自動重啟</span>
                <small>
                  只在可能有救的時候重試：缺前置模組、Java 版本不對這類問題會直接放棄，
                  連續當機 3 次也會停手。決定會寫進主控台。
                </small>
              </span>
              <span class="control">
                <button class="bool" class:on={restartOnCrash} onclick={() => (restartOnCrash = !restartOnCrash)}>
                  {restartOnCrash ? "開" : "關"}
                </button>
              </span>
            </div>

            <div class="row" class:changed={config !== null && backupOnStop !== config.backupOnStop}>
              <span class="label">
                <span class="name">關閉時自動備份世界</span>
                <small>每次正常關閉後把世界壓成 zip，保留最近 5 份。世界大的話，關閉後要多等一下才能再啟動。</small>
              </span>
              <span class="control">
                <button class="bool" class:on={backupOnStop} onclick={() => (backupOnStop = !backupOnStop)}>
                  {backupOnStop ? "開" : "關"}
                </button>
              </span>
            </div>

            <label class="row" class:changed={Number(restartHours) !== config?.restartEveryHours}>
              <span class="label">
                <span class="name">定時重啟（小時）</span>
                <small>
                  {hoursBad
                    ? "請填 0 到 168 之間的整數。"
                    : +restartHours === 0
                      ? "0 是關閉。模組伺服器開久了容易越跑越卡，可以設每幾小時自動重啟一次。"
                      : `每連續執行 ${+restartHours} 小時重啟一次，重啟前 1 分鐘和 10 秒會在遊戲裡提醒。`}
                </small>
              </span>
              <span class="control">
                <input class="tabular" type="number" min="0" max="168" bind:value={restartHours} />
              </span>
            </label>

            <label
              class="row"
              id="row-memory"
              class:flash={highlight === "memory"}
              class:changed={memory !== "" && +memory !== config?.memoryMb}
            >
              <span class="label">
                <span class="name">記憶體上限</span>
                <code>-Xmx</code>
                <small>
                  {memoryBad
                    ? "請填 512 到 65536 之間的整數（MB）。"
                    : `${formatMemory(+memory || 0)}。裝了模組的伺服器建議 4096 以上，但不要超過這台電腦的實體記憶體。`}
                </small>
              </span>
              <span class="control">
                <input class="tabular" type="number" min="512" max="65536" bind:value={memory} />
              </span>
            </label>

            <div class="danger">
              <span class="label">
                <span class="name">刪除伺服器</span>
                <small>
                  {server?.imported
                    ? "只會從清單移除，資料夾與世界檔案留在原處；在這裡做的備份會一起刪除。"
                    : "伺服器資料夾會整個刪除，包括世界、模組和備份，無法復原。"}
                  {#if config?.tunnelId}它在 playit.gg 的公開位址也會一起刪除。{/if}
                </small>
              </span>
              {#if confirmingDelete}
                <span class="confirm">
                  <Button onclick={() => (confirmingDelete = false)}>取消</Button>
                  <Button danger onclick={remove}>確定刪除</Button>
                </span>
              {:else}
                <Button danger disabled={isUp} onclick={() => (confirmingDelete = true)}>
                  刪除
                </Button>
              {/if}
            </div>
          {/if}

          {#if section === "connection"}
            <div class="row">
              <span class="label">
                <span class="name">公開位址</span>
                <code>playit.gg</code>
                <small>
                  {#if !playit?.installed || !playit.linked}
                    要先到左側「設定」頁的「公開連線」下載代理程式並連結 playit.gg 帳號，只要做一次。
                  {:else if tunnel?.disabledReason}
                    playit.gg 停用了這個位址：{tunnel.disabledReason}
                  {:else if tunnelStale}
                    這個位址還指向連接埠 {tunnel?.localPort}，伺服器現在用 {server?.port}，朋友會連不進來。按「修正」改過去。
                  {:else if tunnel?.address}
                    朋友在「多人遊戲 → 直接連線」貼這個位址就能進來，不用設定路由器。
                  {:else if tunnel || tunnelPending}
                    playit.gg 還在配置位址，幾秒後按重新整理。
                  {:else}
                    建一個 playit.gg 位址，讓不在同一個網路的人也連得進來。{#if playit.auto}
                      「伺服器啟動時自動開公開連線」開著，直接啟動伺服器也會自動建。{/if}
                  {/if}
                </small>
              </span>
              <span class="control wide">
                {#if !playit?.installed || !playit.linked}
                  <!-- Nothing to press here; the setup lives in one place. -->
                {:else if tunnelStale}
                  <Button onclick={createTunnel} disabled={tunnelBusy}>
                    {tunnelBusy ? "修正中…" : "修正"}
                  </Button>
                {:else if tunnel?.address}
                  <code class="addr">{tunnel.address}</code>
                  <Button onclick={copyInvite}>{copied ? "已複製" : "複製"}</Button>
                {:else if tunnel || tunnelPending}
                  <Button onclick={refreshPlayit}>重新整理</Button>
                {:else}
                  <Button onclick={createTunnel} disabled={tunnelBusy}>
                    {tunnelBusy ? "建立中…" : "建立公開位址"}
                  </Button>
                {/if}
              </span>
            </div>

            {#if playit?.linked && tunnel}
              <div class="row">
                <span class="label">
                  <span class="name">隧道</span>
                  <small>
                    {playit.running
                      ? "執行中，位址現在連得進來。"
                      : "沒在跑：位址還在，但要啟動隧道才連得進來。"}
                  </small>
                </span>
                <span class="control">
                  {#if !playit.running}
                    <Button onclick={startTunnel} disabled={tunnelBusy}>啟動隧道</Button>
                  {/if}
                </span>
              </div>

              {#if tunnel.address}
                <div class="row">
                  <span class="label">
                    <span class="name">邀請訊息</span>
                    <small>位址、版本和要裝哪些模組，一次複製，貼給朋友就好。</small>
                  </span>
                  <span class="control">
                    <Button onclick={copyInvitation}>
                      <Icon name={invited ? "check" : "copy"} size={14} />
                      {invited ? "已複製" : "複製邀請"}
                    </Button>
                  </span>
                </div>
              {/if}

              <div class="row">
                <span class="label">
                  <span class="name">移除公開位址</span>
                  <small>從你的 playit.gg 帳號刪掉這個位址。之後再建會拿到不同的位址，要重新發給朋友。</small>
                </span>
                <span class="control">
                  {#if confirmingUntunnel}
                    <span class="confirm">
                      <Button onclick={() => (confirmingUntunnel = false)}>取消</Button>
                      <Button danger onclick={removeTunnel} disabled={tunnelBusy}>確定移除</Button>
                    </span>
                  {:else}
                    <Button danger onclick={() => (confirmingUntunnel = true)}>移除</Button>
                  {/if}
                </span>
              </div>
            {/if}
          {/if}

          {#each shown as field (field.key)}
            <label
              class="row"
              id="row-{field.key}"
              class:flash={highlight === field.key}
              class:changed={field.key in edits}
            >
              <span class="label">
                <span class="name">{field.label}</span>
                <code>{field.key}</code>
                {#if field.hint}<small>{field.hint}</small>{/if}
              </span>
              <span class="control">
                {#if field.kind === "bool"}
                  <button
                    class="bool"
                    class:on={valueOf(field) === "true"}
                    disabled={isUp}
                    onclick={() => set(field, valueOf(field) === "true" ? "false" : "true")}
                  >
                    {valueOf(field) === "true" ? "開" : "關"}
                  </button>
                {:else if field.kind === "enum"}
                  <select
                    disabled={isUp}
                    value={valueOf(field)}
                    onchange={(e) => set(field, e.currentTarget.value)}
                  >
                    {#each field.options as option (option.value)}
                      <option value={option.value}>{option.label}</option>
                    {/each}
                    <!-- A value the file holds that this app has no name for
                         still has to be selectable, or opening the page would
                         silently rewrite it to whichever option came first. -->
                    {#if !field.options.some((o) => o.value === valueOf(field))}
                      <option value={valueOf(field)}>{valueOf(field)}</option>
                    {/if}
                  </select>
                {:else if field.kind === "int"}
                  <input
                    class="tabular"
                    type="number"
                    min={field.min ?? undefined}
                    max={field.max ?? undefined}
                    disabled={isUp}
                    value={valueOf(field)}
                    oninput={(e) => set(field, e.currentTarget.value)}
                  />
                {:else}
                  <input
                    disabled={isUp}
                    value={valueOf(field)}
                    oninput={(e) => set(field, e.currentTarget.value)}
                  />
                {/if}
              </span>
            </label>

            {#if field.key === "server-port" && portClash}
              <p class="warn">「{portClash}」也用這個連接埠。兩座伺服器不能同時開在同一個埠。</p>
            {/if}
          {/each}

        {/if}

        <!-- Sticky, and in every section once something is pending: an edit
             made in 連線 must not look saved from 模組. -->
        {#if dirty || saved || !["players", "mods", "backups", "files"].includes(section)}
          <div class="save-row" class:pending={dirty}>
            <span class="hint">
              {#if saved && !dirty}
                已儲存。
              {:else if invalid}
                有欄位格式不對，先改好才能儲存。
              {:else if onlyPropsWhileUp}
                {pending} 項 server.properties 的變更要等伺服器停止後才能儲存。
              {:else if dirty}
                {pending} 項未儲存。{isUp ? "儲存後重新啟動才會生效。" : "按儲存，或直接按啟動（會先幫你儲存）。"}
              {:else if isUp}
                伺服器執行中，server.properties 要停止後才能修改。
              {:else}
                修改後按儲存，重新啟動伺服器才會生效。
              {/if}
            </span>
            <span class="save-buttons">
              <Button onclick={loadSettings} disabled={saving || !dirty}>還原</Button>
              <Button
                variant="primary"
                onclick={saveSettings}
                disabled={saving || invalid || !dirty || onlyPropsWhileUp}
              >
                <Icon name="check" size={14} />
                儲存
              </Button>
            </span>
          </div>
        {/if}
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

  /* A wordmark, not a heading. On a geometric face the authority comes from
     the drawing — even stroke, circular bowls — and a heavy weight destroys
     exactly that by thickening the monoline into a slab. Weight stays at 500
     and the letters are pulled together instead; that tightening is what makes
     a large geometric line read as set rather than merely enlarged. */
  h1 {
    font-family: var(--font-display);
    font-size: var(--font-hero);
    font-weight: 500;
    letter-spacing: -0.02em;
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

  /* ── why it stopped ────────────────────────────────── */

  /* The one place in this design that gets a tinted panel. It is not
     decoration: it appears only after a failure, and it has to read as a
     different kind of thing from the log it sits above. */
  .crash {
    padding: 14px 16px;
    background: color-mix(in srgb, var(--error) 8%, transparent);
    border-radius: var(--radius-card);
  }

  .crash-head {
    color: var(--fg);
    font-size: var(--font-body);
    font-weight: 600;
  }

  .crash-detail {
    margin-top: 8px;
    max-height: 132px;
    overflow: auto;
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
    line-height: 1.7;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .crash-acts {
    display: flex;
    gap: 10px;
    margin-top: 12px;
  }

  /* ── who is on ─────────────────────────────────────── */

  .roster {
    display: flex;
    align-items: baseline;
    gap: 20px;
    padding-bottom: 4px;
  }

  .roster-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    color: var(--muted);
    font-size: var(--font-small);
  }

  .roster-head .tabular {
    color: var(--fg);
  }

  .roster-empty {
    color: var(--faint);
    font-size: var(--font-small);
  }

  .roster ul {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
    list-style: none;
  }

  .roster li {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }

  .who {
    font-size: var(--font-body);
  }

  /* The actions stay quiet until the row is under the pointer: four verbs per
     player, always lit, would out-shout the names they act on. */
  .roster li button {
    color: var(--faint);
    font-size: var(--font-small);
    opacity: 0;
    transition: opacity 120ms var(--ease), color 120ms var(--ease);
  }

  .roster li:hover button,
  .roster li button:focus-visible {
    opacity: 1;
  }

  .roster li button:hover {
    color: var(--fg);
  }

  .roster li button.danger-act:hover {
    color: var(--error);
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

  /* Sized to the shortest useful query rather than to the space available:
     it sits in a toolbar, and a wide box there reads as the main event. */
  .find {
    width: 128px;
    height: 24px;
    padding: 0 8px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font: inherit;
    font-size: var(--font-small);
  }

  .find:focus {
    border-color: var(--accent);
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

  /* A rail of section names beside one column of rows. Nothing is boxed: the
     rows are separated by hairlines because a row genuinely ends there, and
     the rail is separated by distance alone. */
  .settings {
    flex: 1;
    display: flex;
    gap: 40px;
    align-items: flex-start;
    padding-bottom: 8px;
  }

  .sections {
    width: 84px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    /* Lines the first section name up with the first row's label rather than
       with the row's box, which is 8px taller. */
    padding-top: 10px;
  }

  .section {
    height: 30px;
    display: flex;
    align-items: center;
    padding: 0 10px;
    border-left: 2px solid transparent;
    margin-left: -2px;
    color: var(--muted);
    font-size: var(--font-body);
    text-align: left;
    transition: color 120ms var(--ease);
  }

  .section:hover {
    color: var(--fg);
  }

  .section.active {
    border-left-color: var(--accent);
    color: var(--fg);
    font-weight: 500;
  }

  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .pane-head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-bottom: 12px;
  }

  .pane-head h2 {
    font-size: var(--font-section);
    font-weight: 600;
  }

  .count {
    flex: 1;
    color: var(--faint);
    font-size: var(--font-small);
  }

  .note {
    padding: 6px 0 12px;
    color: var(--muted);
    font-size: var(--font-small);
    line-height: 1.7;
  }

  .note code {
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
  }

  /* ── one setting ───────────────────────────────────── */

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    min-height: 48px;
    padding: 8px 0;
    text-align: left;
  }

  .row + .row {
    border-top: 1px solid var(--border);
  }

  /* Two seconds of tint, then gone. The only animated thing in the app, and it
     exists because the alternative is the user hunting for the row we just
     promised to take them to. */
  .row.flash {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    transition: background-color 600ms var(--ease);
  }

  .label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .name {
    color: var(--fg);
    font-size: var(--font-body);
  }

  .name.mono {
    font-family: var(--font-mono);
    font-size: var(--font-small);
  }

  /* The raw key, under its Chinese name: the label is what you read, the key
     is what you search for when a wiki page names it. */
  .label code {
    color: var(--faint);
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
  }

  .label small {
    color: var(--muted);
    font-size: var(--font-small);
  }

  /* Unsaved rows mark themselves on the key, which is otherwise the quietest
     thing in the row — enough to find them again, not enough to shout. */
  .row.changed .label code {
    color: var(--accent);
  }

  .control {
    flex: none;
    width: 200px;
    display: flex;
    justify-content: flex-end;
    color: var(--muted);
  }

  /* A hostname and its copy button do not fit in a field's width, and the
     address is read rather than typed. */
  .control.wide {
    width: 320px;
    align-items: center;
    gap: 8px;
  }

  .control .addr {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: var(--font-small);
    color: var(--fg);
    user-select: text;
  }

  .control input,
  .control select {
    width: 100%;
    height: var(--h-input);
    padding: 0 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font: inherit;
    font-size: var(--font-body);
  }

  .control input:focus,
  .control select:focus {
    border-color: var(--accent);
  }

  .control input:disabled,
  .control select:disabled,
  .bool:disabled {
    color: var(--faint);
    cursor: not-allowed;
  }

  /* Spinners steal 20px from a field whose range is already stated. */
  .control input[type="number"]::-webkit-inner-spin-button {
    appearance: none;
  }

  .bool {
    min-width: 66px;
    height: var(--h-control-sm);
    padding: 0 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    color: var(--muted);
    font-size: var(--font-body);
    transition: color 120ms var(--ease), border-color 120ms var(--ease);
  }

  .bool.on {
    border-color: var(--fg);
    color: var(--fg);
  }

  .file-row {
    width: 100%;
  }

  .file-row:hover .name {
    color: var(--accent);
  }

  /* ── mods ──────────────────────────────────────────── */

  .mod-list {
    list-style: none;
  }

  .mod-list li {
    display: flex;
    align-items: center;
    gap: 14px;
    height: 40px;
  }

  .mod-list li + li {
    border-top: 1px solid var(--border);
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

  /* A mod's own name reads as words; only a bare file name is set in mono. */
  .mod-name.plain {
    font-family: var(--font-sans);
    font-size: var(--font-body);
  }

  .mod-ver {
    margin-left: 8px;
    color: var(--faint);
    font-family: var(--font-mono);
    font-size: var(--font-tiny);
  }

  .mod-size {
    color: var(--faint);
    font-size: var(--font-small);
  }

  .mod-remove {
    color: var(--faint);
  }

  /* A disabled mod is still listed, just clearly not loading. */
  .mod-list li.off .mod-name {
    color: var(--faint);
    text-decoration: line-through;
  }

  .note.dropping {
    color: var(--accent);
  }

  .mod-toggle {
    flex: none;
    height: var(--h-control-sm);
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-button);
    color: var(--muted);
    font-size: var(--font-small);
  }

  .mod-toggle.on {
    border-color: var(--fg);
    color: var(--fg);
  }

  .mod-toggle:disabled {
    opacity: 0.4;
  }

  .auto {
    margin-left: 8px;
    color: var(--faint);
    font-size: var(--font-tiny);
  }

  .mod-remove:hover:not(:disabled) {
    color: var(--error);
  }

  .mod-remove:disabled {
    opacity: 0.4;
  }

  /* ── delete ────────────────────────────────────────── */

  .danger {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    margin-top: 28px;
    padding-top: 18px;
    border-top: 1px solid var(--border);
  }

  .confirm {
    display: flex;
    gap: 10px;
  }

  /* ── save ──────────────────────────────────────────── */

  /* Sits under the row it is about, indented past nothing — it belongs to the
     row above, and a full-width line under a hairline reads as attached. */
  .warn {
    padding: 8px 0 10px;
    color: var(--starting);
    font-size: var(--font-small);
  }

  /* Pinned to the bottom of the page while it scrolls: the rows that need
     saving are forty lines above the button otherwise. */
  .save-row {
    position: sticky;
    bottom: -22px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    margin-top: 22px;
    padding: 12px 0 22px;
    background: var(--bg);
    border-top: 1px solid var(--border);
  }

  .save-row.pending .hint {
    color: var(--accent);
  }

  .pending-restart {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 14px;
    background: color-mix(in srgb, var(--starting) 12%, transparent);
    border-radius: var(--radius-input);
    font-size: var(--font-small);
  }

  .pane-head.sub {
    margin-top: 22px;
    padding-bottom: 6px;
  }

  .pane-head h3 {
    font-size: var(--font-body);
    font-weight: 600;
  }

  .add-name {
    display: flex;
    gap: 10px;
    margin-top: 8px;
  }

  .add-name input {
    flex: 1;
    height: var(--h-input);
    padding: 0 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    color: var(--fg);
    font: inherit;
    font-size: var(--font-body);
  }

  .add-name input:focus {
    border-color: var(--accent);
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
