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
  type ForgeVersionGroup,
  type LogLine,
  type ServerConfig,
  type ModFile,
  type AgentSettings,
  type RemoteHost,
  type RemoteServer,
  type PropertyField,
  type Backup,
  type CrashInfo,
  type PlayitStatus,
  type PlayerAccess,
  type AccessList,
  type ServerFile,
  type ServerId,
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
 * Create a server for one Forge version, downloading its installer first if it
 * is not on disk yet. Download progress and installer output arrive as events.
 */
export const createServer = (name: string, version: string) =>
  invoke<ServerId>("create_server", { name, version });

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

/** Every key in the file, plus the ones Minecraft writes by default. */
export const getProperties = (id: ServerId) =>
  invoke<PropertyField[]>("get_properties", { id });

/**
 * Write back only what changed, key by key — comments and keys this app has
 * never heard of survive. Refused while the server is running.
 */
export const saveProperties = (id: ServerId, changes: Record<string, string>) =>
  invoke<void>("save_properties", { id, changes });

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

/** Who is connected. Empty unless the server is fully up. */
export const listPlayers = (id: ServerId) =>
  invoke<string[]>("list_players", { id });

/**
 * Why the server stopped, or `null` if it did not crash.
 *
 * Pulled on demand rather than pushed with the state change: it reads the
 * crash report off disk, and a server that crashes with nobody watching should
 * not pay for a diagnosis nobody sees.
 */
export const crashReport = (id: ServerId) =>
  invoke<CrashInfo | null>("crash_report", { id });

/** Open the newest crash report in the default text editor. */
export const openCrashReport = (id: ServerId) =>
  invoke<void>("open_crash_report", { id });

/** The whitelist and the operators, from the server's own files. */
export const playerAccess = (id: ServerId) =>
  invoke<PlayerAccess>("player_access", { id });

/** Add or remove a name. Goes through the console, so only while `online`. */
export const setPlayerAccess = (
  id: ServerId,
  list: AccessList,
  name: string,
  allowed: boolean,
) => invoke<void>("set_player_access", { id, list, name, allowed });

/** Whitelist on or off on a running server. */
export const setWhitelist = (id: ServerId, on: boolean) =>
  invoke<void>("set_whitelist", { id, on });

// ── console ─────────────────────────────────────────────────

/** Only the lines newer than `afterSeq`. Sequences start at 1, so pass 0 for
 *  everything the buffer still holds. */
export const consoleSince = (id: ServerId, afterSeq: number) =>
  invoke<LogLine[]>("console_since", { id, afterSeq });

// ── process control ─────────────────────────────────────────

/** Launch the server. Rejects unless the EULA has been accepted. */
export const startServer = (id: ServerId) => invoke<void>("start_server", { id });

/**
 * Graceful shutdown — the world saves. A force-kill follows automatically only
 * if the server ignores `stop` past the grace period.
 */
export const stopServer = (id: ServerId) => invoke<void>("stop_server", { id });

/** End the process now. The world is not saved. */
export const killServer = (id: ServerId) => invoke<void>("kill_server", { id });

/** Call off an automatic restart that is still counting down. */
export const cancelRestart = (id: ServerId) =>
  invoke<void>("cancel_restart", { id });

/** Type a line into the server console. Only valid while `online`. */
export const sendConsoleCommand = (id: ServerId, command: string) =>
  invoke<void>("send_console_command", { id, command });

// ── mods ────────────────────────────────────────────────────

/** Jars in the server's `mods/`, alphabetical. Empty for a folder with none. */
export const listMods = (id: ServerId) => invoke<ModFile[]>("list_mods", { id });

/**
 * Copy jars into `mods/`, replacing any of the same name — which is what
 * updating a mod looks like. Refused while the server is running.
 */
export const addMods = (id: ServerId, paths: string[]) =>
  invoke<void>("add_mods", { id, paths });

export const deleteMod = (id: ServerId, name: string) =>
  invoke<void>("delete_mod", { id, name });

/** Take a mod out of the load order without losing the file — which is what
 *  bisecting a mod conflict needs. Refused while the server is running. */
export const setModEnabled = (id: ServerId, name: string, enabled: boolean) =>
  invoke<void>("set_mod_enabled", { id, name, enabled });

// ── backups ─────────────────────────────────────────────────

/** Saved worlds for this server, newest first. */
export const listBackups = (id: ServerId) =>
  invoke<Backup[]>("list_backups", { id });

/**
 * Zip the world. Resolves with the file name written.
 *
 * Refused while the server is running: Minecraft writes region files on its
 * own schedule, so a copy taken now would capture a save in progress.
 */
export const createBackup = (id: ServerId) =>
  invoke<string>("create_backup", { id });

/** Replace the world with a backup. The world being replaced is saved first. */
export const restoreBackup = (id: ServerId, name: string) =>
  invoke<void>("restore_backup", { id, name });

export const deleteBackup = (id: ServerId, name: string) =>
  invoke<void>("delete_backup", { id, name });

/**
 * Bundle the log, the crash report, the mod list and the config into one zip,
 * and select it in Explorer. Resolves with its path. Nothing is uploaded — the
 * user decides who sees it.
 */
export const exportDiagnostics = (id: ServerId) =>
  invoke<string>("export_diagnostics", { id });

// ── remote hosts ────────────────────────────────────────────

export const listRemotes = () => invoke<RemoteHost[]>("list_remotes");

/** Add or update. An empty id means new; the stored entry comes back. */
export const saveRemote = (host: RemoteHost) =>
  invoke<RemoteHost>("save_remote", { host });

export const deleteRemote = (id: string) =>
  invoke<void>("delete_remote", { id });

/** Connect and look at the folder. Resolves with what it found there. */
export const testRemote = (id: string) => invoke<string>("test_remote", { id });

/** The host key fingerprint, to read before trusting it. */
export const remoteFingerprint = (id: string) =>
  invoke<string>("remote_fingerprint", { id });

/** Record the host key in `known_hosts`. Only after the user has seen it. */
export const trustRemote = (id: string) => invoke<void>("trust_remote", { id });

/** The servers on that machine. `app` transport only. */
export const remoteServers = (id: string) =>
  invoke<RemoteServer[]>("remote_servers", { id });

/** `server` names which server on the far side — required over `app`,
 *  ignored over `ssh`, where the host entry points at one folder already. */
export const readRemoteFile = (
  id: string,
  server: string | null,
  file: ServerFile,
) => invoke<string>("read_remote_file", { id, server, file });

export const writeRemoteFile = (
  id: string,
  server: string | null,
  file: ServerFile,
  text: string,
) => invoke<void>("write_remote_file", { id, server, file, text });

// ── this machine's listener ─────────────────────────────────

export const getAgentSettings = () =>
  invoke<AgentSettings>("get_agent_settings");

/** Switch it on or off, or move the port. Binds immediately, so a port already
 *  in use rejects here instead of failing silently later. */
export const setAgentSettings = (enabled: boolean, port: number) =>
  invoke<AgentSettings>("set_agent_settings", { enabled, port });

/** New pairing code. Everyone holding the old one loses access. */
export const regenerateAgentToken = () =>
  invoke<AgentSettings>("regenerate_agent_token");

// ── java ────────────────────────────────────────────────────

/**
 * Fetch and unpack the JRE for `major`. Nothing is downloaded until something
 * actually needs it, so this is called from the prompt a failed launch raises.
 * Progress arrives as a `download` event with a `jre` kind.
 */
export const installJava = (major: number) =>
  invoke<void>("install_java", { major });

// ── downloads ───────────────────────────────────────────────

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

// ── feedback ────────────────────────────────────────────────

/**
 * Open a GitHub issue in the browser, carrying what the user typed.
 *
 * The destination is built in the core, so the UI supplies words and never a
 * URL. Nothing is sent from here — the user reads, edits and submits the page
 * themselves, and no log leaves the machine unless they attach one.
 */
export const reportIssue = (title: string, body: string) =>
  invoke<void>("report_issue", { title, body });

// ── playit.gg tunnels ───────────────────────────────────────

/**
 * Everything the tunnel UI draws: whether the agent is installed, whether this
 * machine is linked to the user's playit account, whether it is running, and
 * every tunnel on that account.
 *
 * `offline` being set means playit could not be reached — the local half of the
 * answer is still true, so the UI keeps drawing rather than going blank.
 */
export const playitStatus = () => invoke<PlayitStatus>("playit_status");

/**
 * Fetch the agent from playit's own GitHub release.
 *
 * Not bundled in the installer: playit's terms forbid offering their service as
 * part of a bundled product, and their own guidance is to run the program only
 * when it came from an official source. Progress arrives on the normal download
 * events.
 */
export const playitInstall = () => invoke<void>("playit_install");

/**
 * Start linking: opens playit's claim page in the user's default browser and
 * returns `[code, url]`. Pass the code to `playitFinishClaim` to wait for the
 * approval; the URL is only for showing, in case no browser came to the front.
 *
 * The user signs into playit as themselves; this app never holds an account and
 * never sees their password.
 */
export const playitClaimUrl = () => invoke<[string, string]>("playit_claim_url");

/** Wait for the user to approve in the browser, then store the key. */
export const playitFinishClaim = (code: string) =>
  invoke<void>("playit_finish_claim", { code });

/** Stop waiting for an approval the user is not going to give. */
export const playitCancelClaim = () => invoke<void>("playit_cancel_claim");

/** Bring the tunnel up. Its log arrives as `agentActivity` events. */
export const playitStart = () => invoke<void>("playit_start");

export const playitStop = () => invoke<void>("playit_stop");

/**
 * Make sure one server has a tunnel pointing at its current port — reusing
 * the one it already has. playit allocates asynchronously, so the address
 * appears in a later `playitStatus` rather than in the return value.
 */
export const playitCreateTunnel = (id: ServerId) =>
  invoke<void>("playit_create_tunnel", { id });

export const playitDeleteTunnel = (id: string) =>
  invoke<void>("playit_delete_tunnel", { id });

/** Switch "public address follows the server" on or off. */
export const playitSetAuto = (on: boolean) => invoke<void>("playit_set_auto", { on });

/**
 * Forget this machine's key. The agent stays on the user's playit account until
 * they remove it there.
 */
export const playitUnlink = () => invoke<void>("playit_unlink");

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
