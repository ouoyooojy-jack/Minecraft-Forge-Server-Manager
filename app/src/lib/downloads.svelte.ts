import { onCoreEvent } from "./api";
import type { DownloadProgress } from "./types";

/**
 * Every transfer the core is running or just finished, keyed by id.
 *
 * Kept here rather than in a page: a download goes on when you leave the page
 * that started it, and coming back has to show it — and still block a second
 * one, which the core would refuse.
 */
export const transfers = $state<Record<number, DownloadProgress>>({});

void onCoreEvent((event) => {
  if (event.type !== "download") return;
  transfers[event.id] = event;
  if (event.state.kind !== "running") {
    // A finished transfer stays on screen briefly so the outcome is readable,
    // then clears itself. Failures linger longer — the message is the point.
    const linger = event.state.kind === "failed" ? 8000 : 2500;
    setTimeout(() => delete transfers[event.id], linger);
  }
});
