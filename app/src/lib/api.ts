/**
 * Typed wrapper over the Tauri IPC surface.
 *
 * Nothing else in the UI calls `invoke` or `listen` directly — every command
 * and the event channel are named exactly once, here, so a rename in Rust
 * breaks one file instead of a dozen components.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import {
  EVENT_CHANNEL,
  type CoreEvent,
  type DownloadedFile,
  type DownloadId,
  type DownloadKind,
  type ForgeVersionGroup,
  type LogLine,
  type ServerConfig,
  type ServerFile,
  type ServerId,
  type ServerProperties,
  type ServerSummary,
} from "./types";

// ── servers ─────────────────────────────────────────────────

export const listServers = () => invoke<ServerSummary[]>("list_servers");

/**
 * Create a server and install Forge into it from a jar already on disk,
 * resolving when it is ready to start. Installer output arrives as
 * `CoreEvent` of type `serverLog` while it runs.
 *
 * Accepts the Minecraft EULA on the user's behalf; the dialog says so.
 */
export const createServerFromInstaller = (name: string, installer: string) =>
  invoke<ServerId>("create_server_from_installer", { name, installer });

/**
 * Adopt an installation already on disk. Nothing is copied or moved, and
 * deleting the server later only unlinks it.
 */
export const importServer = (path: string) =>
  invoke<ServerId>("import_server", { path });

export const deleteServer = (id: ServerId) =>
  invoke<void>("delete_server", { id });

export const getServerConfig = (id: ServerId) =>
  invoke<ServerConfig>("get_server_config", { id });

export const saveServerConfig = (id: ServerId, config: ServerConfig) =>
  invoke<void>("save_server_config", { id, config });

// ── server.properties + EULA ────────────────────────────────

export const getServerProperties = (id: ServerId) =>
  invoke<ServerProperties>("get_server_properties", { id });

export const saveServerProperties = (
  id: ServerId,
  properties: ServerProperties,
) => invoke<void>("save_server_properties", { id, properties });

/** One editable file, verbatim. A file that does not exist yet reads as "". */
export const readServerFile = (id: ServerId, file: ServerFile) =>
  invoke<string>("read_server_file", { id, file });

/** Write it back verbatim. Rejected while the server is running, because
 *  Minecraft rewrites `server.properties` on shutdown. */
export const writeServerFile = (id: ServerId, file: ServerFile, text: string) =>
  invoke<void>("write_server_file", { id, file, text });

/** Reveal the server folder in Explorer. */
export const openServerFolder = (id: ServerId) =>
  invoke<void>("open_server_folder", { id });

export const getEula = (id: ServerId) => invoke<boolean>("get_eula", { id });

export const setEula = (id: ServerId, accepted: boolean) =>
  invoke<void>("set_eula", { id, accepted });

// ── console ─────────────────────────────────────────────────

/** Only the lines newer than `afterSeq`. Sequences start at 1, so pass 0 for
 *  everything the buffer still holds. */
export const consoleSince = (id: ServerId, afterSeq: number) =>
  invoke<LogLine[]>("console_since", { id, afterSeq });

/** Oldest sequence still in the ring buffer, or `null` when empty. */
export const consoleOldestSeq = (id: ServerId) =>
  invoke<number | null>("console_oldest_seq", { id });

// ── process control ─────────────────────────────────────────

/** Launch the server. Rejects unless the EULA has been accepted. */
export const startServer = (id: ServerId) => invoke<void>("start_server", { id });

/**
 * Graceful shutdown — the world saves. A force-kill follows automatically only
 * if the server ignores `stop` past the grace period.
 */
export const stopServer = (id: ServerId) => invoke<void>("stop_server", { id });

/**
 * Immediate termination, skipping the save. Only for a server that has stopped
 * responding — warn before calling.
 */
export const killServer = (id: ServerId) => invoke<void>("kill_server", { id });

/** Type a line into the server console. Only valid while `online`. */
export const sendConsoleCommand = (id: ServerId, command: string) =>
  invoke<void>("send_console_command", { id, command });

/**
 * Run a Forge installer into the server folder. Does not accept the EULA —
 * call `setEula` for that, driven by the checkbox the user ticked.
 */
export const installForge = (id: ServerId, installer: string) =>
  invoke<void>("install_forge", { id, installer });

// ── java ────────────────────────────────────────────────────

/**
 * Fetch and unpack the JRE for `major`. Nothing is downloaded until something
 * actually needs it, so this is called from the prompt a failed launch raises.
 * Progress arrives as a `download` event with a `jre` kind.
 */
export const installJava = (major: number) =>
  invoke<void>("install_java", { major });

// ── downloads ───────────────────────────────────────────────

/**
 * Begin a download; resolves with its id as soon as it starts, not when it
 * finishes. Watch `CoreEvent` of type `download` for progress and the outcome.
 *
 * `filename` is a bare name — any directory part is stripped by the core.
 */
export const startDownload = (
  what: DownloadKind,
  url: string,
  filename: string,
) => invoke<DownloadId>("start_download", { what, url, filename });

/** Ask a running download to stop. Unknown ids are ignored. */
export const cancelDownload = (id: DownloadId) =>
  invoke<void>("cancel_download", { id });

/** Installer jars already on disk, newest first. */
export const listDownloads = () => invoke<DownloadedFile[]>("list_downloads");

export const deleteDownload = (path: string) =>
  invoke<void>("delete_download", { path });

// ── forge ───────────────────────────────────────────────────

/**
 * Published Forge builds, grouped by Minecraft line, newest first. Cached in
 * the core for the life of the process — pass `refresh` to force a re-fetch.
 */
export const listForgeVersions = (refresh = false) =>
  invoke<ForgeVersionGroup[]>("list_forge_versions", { refresh });

/**
 * Fetch one Forge installer. The URL and filename are derived in the core, so
 * a version string can only ever produce the file Forge itself publishes.
 */
export const downloadForge = (version: string) =>
  invoke<DownloadId>("download_forge", { version });

// ── events ──────────────────────────────────────────────────

/**
 * Subscribe to everything the core pushes. One channel, one listener —
 * remember to call the returned unlisten on teardown.
 */
export function onCoreEvent(
  handler: (event: CoreEvent) => void,
): Promise<UnlistenFn> {
  return listen<CoreEvent>(EVENT_CHANNEL, (e) => handler(e.payload));
}
