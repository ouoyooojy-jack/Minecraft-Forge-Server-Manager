/**
 * Mirror of `src-tauri/src/types.rs`. Change both together.
 *
 * Hand-written on purpose: one contract file per side is small enough that
 * drift shows up in review. If these two ever get big enough that they don't,
 * swap in `tauri-specta` codegen and delete this file.
 */

// ── identity ────────────────────────────────────────────────

/** Folder name under `servers/`. Stable — a rename never changes it. */
export type ServerId = string;

export type DownloadId = number;

// ── server lifecycle ────────────────────────────────────────

export type ServerState =
  | { kind: "stopped" }
  | { kind: "installing" }
  | { kind: "starting" }
  | { kind: "online" }
  | { kind: "stopping" }
  | { kind: "crashed"; code: number | null };

/** The four legend colours on the home screen. Derived in Rust. */
export type StatusLight = "online" | "offline" | "starting" | "error";

/**
 * Everything a home-screen card draws — one call for the whole grid rather
 * than a call plus a properties read per server.
 */
export interface ServerSummary {
  id: ServerId;
  name: string;
  state: ServerState;
  light: StatusLight;
  mcVersion: string | null;
  forgeVersion: string | null;
  memoryMb: number;
  /** `null` before the server has ever run — show a dash, not a default. */
  port: number | null;
  maxPlayers: number | null;
  /** Adopted from elsewhere on disk; deleting only unlinks it. */
  imported: boolean;
  /**
   * Who is connected — `null` unless the server is running.
   *
   * The names rather than a count, so the card and the server page's roster
   * cannot disagree about how many there are.
   */
  players: string[] | null;
  uptimeSecs: number | null;
}

// ── per-server configuration ────────────────────────────────

export interface ServerConfig {
  name: string;
  memoryMb: number;
  /** `null` means "use the JRE this app manages for the required major". */
  javaPath: string | null;
  jvmArgs: string[];
  mcVersion: string | null;
  forgeVersion: string | null;
  /** Set when the server lives outside the managed folder. */
  externalPath: string | null;
  /**
   * Start again by itself after a crash.
   *
   * Never blind: the core skips the restart for faults a restart cannot fix,
   * and gives up after a few tries rather than looping all night.
   */
  restartOnCrash: boolean;
}

/**
 * The files the raw editor may open. The core maps these to paths, so the UI
 * can never name a file outside the server folder.
 */
export type ServerFile = "properties" | "jvmArgs" | "runScript";

/**
 * A machine reachable over SSH — a friend's PC on the VPN, in practice.
 *
 * Carries no credentials: authentication is the system `ssh` client's job,
 * using the keys and `known_hosts` already on this machine.
 */
export interface RemoteHost {
  /** Empty when creating; the core assigns one on save. */
  id: string;
  label: string;
  host: string;
  transport: Transport;
  /** Pairing code shown by the other machine's app. `app` transport only. */
  token: string | null;
  /** `ssh` transport only. */
  user: string;
  port: number;
  /** `null` lets ssh pick from its own defaults. */
  keyPath: string | null;
  /** The server folder on that machine. `ssh` transport only. */
  dir: string;
}

/**
 * How this app reaches a remote machine.
 *
 * `app` — the other side runs this app with remote access on. Nothing to
 * install, a pairing code instead of a key.
 * `ssh` — the system ssh client, for a machine that does not run this app.
 */
export type Transport = "app" | "ssh";

/** One server on a remote machine. */
export interface RemoteServer {
  id: string;
  name: string;
}

/** This machine's own listener. Off until switched on. */
export interface AgentSettings {
  enabled: boolean;
  port: number;
  /** The pairing code to read out to whoever should have access. */
  token: string;
}

// ── server.properties ───────────────────────────────────────

/** Which section of the settings tab a key appears under. */
export type PropertyGroup = "connection" | "gameplay" | "world" | "advanced";

export interface EnumOption {
  value: string;
  label: string;
}

interface PropertyBase {
  key: string;
  /** Chinese label, or the raw key for one this app has no row for. */
  label: string;
  group: PropertyGroup;
  /**
   * Current value as it sits in the file. A string for every kind, including
   * the numeric ones — the file holds text, and a round trip through `number`
   * would eventually write `20.0` where Minecraft wants `20`.
   */
  value: string;
  hint: string;
  /** False for a key the core has no row for: free text, shown last. */
  known: boolean;
}

/**
 * One editable line of server.properties, with the control that edits it.
 *
 * Rust flattens `PropertyKind` into the field, so `kind` sits alongside `key`
 * rather than nested — spelled out as four members here so that switching on
 * `kind` narrows to the extra fields each control needs.
 */
export type PropertyField =
  | (PropertyBase & { kind: "bool" })
  | (PropertyBase & { kind: "enum"; options: EnumOption[] })
  | (PropertyBase & { kind: "int"; min: number | null; max: number | null })
  | (PropertyBase & { kind: "text" });

// ── why a server stopped ────────────────────────────────────

/**
 * The next step to offer, not a message to print.
 *
 * Each variant names somewhere in this app, so the UI can render a button that
 * goes there rather than telling the user to go looking.
 */
export type Fix =
  | { kind: "memory" }
  | { kind: "port" }
  | { kind: "java"; major: number }
  | { kind: "mods" }
  | { kind: "eula" };

export interface CrashInfo {
  /** One Chinese sentence: what went wrong. */
  headline: string;
  /** The lines it was read off. Display verbatim; never parse this. */
  detail: string;
  fix: Fix | null;
  /** The crash report on disk, when Minecraft wrote one. */
  path: string | null;
}

// ── console ─────────────────────────────────────────────────

export type LogStream = "stdout" | "stderr" | "system";

export interface LogLine {
  /**
   * Monotonic per server, never reused, starting at 1. A gap means lines were
   * evicted from the ring buffer — pass 0 to `consoleSince` to ask for all of it.
   */
  seq: number;
  stream: LogStream;
  text: string;
}

// ── downloads ───────────────────────────────────────────────

export type DownloadKind =
  | { kind: "forgeInstaller"; version: string }
  | { kind: "jre"; major: number }
  | { kind: "playitAgent" };

// ── playit.gg ───────────────────────────────────────────────

/** One tunnel, reduced to what a person reads out loud to a friend. */
export interface PlayitTunnel {
  id: string;
  /** Named after the server it points at — that is how the two are matched. */
  name: string;
  /** What a player types. Already carries a port when there is one to type. */
  address: string;
  /** Set when playit has switched the tunnel off, with its reason. */
  disabledReason: string | null;
}

export interface PlayitStatus {
  /** The agent binary is on disk. */
  installed: boolean;
  /** A key is stored, so this machine is attached to the user's playit account. */
  linked: boolean;
  /** The agent process is up. Tunnels only carry traffic while it is. */
  running: boolean;
  /** Starting a server brings its public address up with it. */
  auto: boolean;
  tunnels: PlayitTunnel[];
  /** Names of tunnels playit is still allocating an address for. */
  pending: string[];
  /** Messages from playit, shown verbatim. */
  notices: string[];
  /** Set when playit could not be reached; the rest is local knowledge only. */
  offline: string | null;
}

export type DownloadState =
  | { kind: "running" }
  | { kind: "done"; path: string }
  | { kind: "cancelled" }
  | { kind: "failed"; message: string };

export interface DownloadProgress {
  id: DownloadId;
  what: DownloadKind;
  received: number;
  /** `null` when the server sent no Content-Length — show an indeterminate bar. */
  total: number | null;
  bytesPerSec: number;
  state: DownloadState;
}

/** `null` when the total is unknown; the bar should be indeterminate, not 0%. */
export function downloadPercent(p: DownloadProgress): number | null {
  return p.total && p.total > 0 ? (p.received / p.total) * 100 : null;
}

/** One jar in a server's `mods/` folder. */
export interface ModFile {
  /** File name — also the handle for deleting it. Always the enabled
   *  spelling, so toggling a mod does not change how it is addressed. */
  name: string;
  bytes: number;
  /** Forge loads `*.jar` only; a disabled mod is the same file renamed. */
  enabled: boolean;
}

/** One saved copy of a world. */
export interface Backup {
  /** File name, and the handle for restore and delete. */
  name: string;
  bytes: number;
  /** Unix seconds. */
  createdSecs: number;
  /** Taken automatically just before a restore, rather than asked for. */
  automatic: boolean;
}

/** An installer jar in the downloads folder. */
export interface DownloadedFile {
  path: string;
  name: string;
  bytes: number;
}

// ── forge versions ──────────────────────────────────────────

/**
 * Forge builds for one Minecraft line. Grouped because the flat list runs to
 * thousands of entries: the download page picks a line, then a build.
 */
export interface ForgeVersionGroup {
  /** Minecraft major line, e.g. `"1.20"`. */
  mcMajor: string;
  /** Full Forge versions in this line, newest first, e.g. `"1.20.1-47.2.0"`. */
  versions: string[];
}

// ── events ──────────────────────────────────────────────────

/** Single channel name the UI listens on. Matches `EVENT_CHANNEL` in Rust. */
export const EVENT_CHANNEL = "core";

export type CoreEvent =
  | { type: "serverState"; id: ServerId; state: ServerState; light: StatusLight }
  | { type: "serverLog"; id: ServerId; lines: LogLine[] }
  | { type: "serversChanged" }
  // The whole roster, not a delta: a page that opened mid-session would never
  // catch up from joins and leaves alone.
  | { type: "players"; id: ServerId; names: string[] }
  | { type: "agentActivity"; line: string }
  // Rust tags this newtype variant internally, so the progress fields sit
  // alongside `type` rather than nested under a key.
  | ({ type: "download" } & DownloadProgress)
  | { type: "error"; id: ServerId | null; error: CoreError };

// ── errors ──────────────────────────────────────────────────

/**
 * Split by recovery action, not by where it was thrown: `network` means retry,
 * `versionNotFound` means pick another version. Don't collapse these into a
 * string when rendering — the variant decides which button to offer.
 */
export type CoreError =
  | { kind: "network"; message: string }
  | { kind: "versionNotFound"; version: string }
  | { kind: "downloadAborted" }
  | { kind: "java"; message: string }
  // Recovery is a button, not a message: the UI offers to download this major.
  | { kind: "javaMissing"; major: number }
  | { kind: "install"; code: number | null; message: string }
  | { kind: "process"; message: string }
  | { kind: "precondition"; message: string }
  | { kind: "io"; message: string }
  | { kind: "config"; message: string };

/** True for a rejection that actually came from the core. */
export function isCoreError(e: unknown): e is CoreError {
  return typeof e === "object" && e !== null && "kind" in e;
}

/**
 * Human-readable fallback. Prefer switching on `kind` where an action exists.
 *
 * Takes `unknown` because not every rejection is a `CoreError` — a broken IPC
 * bridge or a mistyped command name throws a plain `TypeError`, and casting
 * that to `CoreError` produced `undefined` on screen instead of a clue.
 */
export function errorMessage(e: unknown): string {
  if (!isCoreError(e)) {
    return e instanceof Error ? e.message : String(e);
  }
  switch (e.kind) {
    case "downloadAborted":
      return "已取消";
    case "versionNotFound":
      return `找不到版本 ${e.version}`;
    case "javaMissing":
      return `需要 Java ${e.major}`;
    case "install":
      return `Forge 安裝失敗（結束碼 ${e.code ?? "?"}）：${e.message}`;
    default:
      return e.message;
  }
}
