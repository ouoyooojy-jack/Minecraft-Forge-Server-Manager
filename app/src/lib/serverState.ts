/**
 * How a server's lifecycle state reads on screen.
 *
 * Both the card grid and the detail page draw the same control, so the mapping
 * lives here once. A button that says 啟動 while the server is already booting
 * is the kind of thing that goes wrong when two files each own half the rule.
 */

import type { ServerState } from "./types";

export const STATE_LABEL: Record<ServerState["kind"], string> = {
  stopped: "已停止",
  installing: "安裝中",
  starting: "啟動中",
  online: "執行中",
  stopping: "停止中",
  crashed: "已當機",
};

/** Anything with a live process behind it — up, or on its way either way. */
export const isActive = (state: ServerState) =>
  state.kind !== "stopped" && state.kind !== "crashed";

export interface PrimaryAction {
  /** Which command the button runs. `null` means it runs nothing. */
  run: "start" | "stop" | null;
  label: string;
  /** The accent fill is for the action that begins something. */
  primary: boolean;
  disabled: boolean;
  icon: "play" | "square";
}

/**
 * The one button a server always has.
 *
 * `starting` still offers 停止: booting a modded server takes minutes, and
 * being unable to call one off until it finishes is worse than a stop that has
 * to wait for the JVM to reach a point where it can honour it.
 *
 * `installing` and `stopping` offer nothing. Both are already going somewhere,
 * and the only honest thing a button can do is say so.
 */
export function primaryAction(state: ServerState): PrimaryAction {
  switch (state.kind) {
    case "online":
      return { run: "stop", label: "停止", primary: false, disabled: false, icon: "square" };
    case "starting":
      return { run: "stop", label: "停止", primary: false, disabled: false, icon: "square" };
    case "stopping":
      return { run: null, label: "停止中…", primary: false, disabled: true, icon: "square" };
    case "installing":
      return { run: null, label: "安裝中…", primary: false, disabled: true, icon: "square" };
    case "crashed":
      return { run: "start", label: "重新啟動", primary: true, disabled: false, icon: "play" };
    case "stopped":
      return { run: "start", label: "啟動", primary: true, disabled: false, icon: "play" };
  }
}
