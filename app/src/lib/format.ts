/** Shared number formatting, so a size reads the same everywhere it appears. */

const UNITS = ["B", "KB", "MB", "GB", "TB"];

/**
 * Human-readable byte count.
 *
 * One decimal below 100 and none above, so the string stays a stable width as
 * a download counts up and the row does not jitter.
 */
export function formatBytes(n: number): string {
  let value = Math.max(0, n);
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}

/** `02:14:31`. Fixed width, so a counting clock does not reflow its row. */
export function formatUptime(secs: number): string {
  const parts = [Math.floor(secs / 3600), Math.floor((secs % 3600) / 60), secs % 60];
  return parts.map((n) => String(n).padStart(2, "0")).join(":");
}

/** `4 GB`, `512 MB`. */
export function formatMemory(mb: number): string {
  if (mb < 1024) return `${mb} MB`;
  const gb = mb / 1024;
  return `${Number.isInteger(gb) ? gb : gb.toFixed(1)} GB`;
}

export const formatSpeed = (bytesPerSec: number) => `${formatBytes(bytesPerSec)}/s`;
