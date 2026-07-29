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
  /** Live process facts — `null` unless the server is running. */
  playersOnline: number | null;
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
}

export type Gamemode = "survival" | "creative" | "adventure" | "spectator";
export type Difficulty = "peaceful" | "easy" | "normal" | "hard";

/**
 * The files the raw editor may open. The core maps these to paths, so the UI
 * can never name a file outside the server folder.
 */
export type ServerFile = "properties" | "jvmArgs";

/** The subset of server.properties the settings modal edits. */
export interface ServerProperties {
  motd: string;
  port: number;
  maxPlayers: number;
  gamemode: Gamemode;
  difficulty: Difficulty;
  pvp: boolean;
  onlineMode: boolean;
  viewDistance: number;
  whiteList: boolean;
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
  | { kind: "jre"; major: number };

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
