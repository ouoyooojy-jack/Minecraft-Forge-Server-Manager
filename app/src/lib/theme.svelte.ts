/**
 * Active colour scheme.
 *
 * Applied by stamping `data-theme` on the document root, which is what
 * `app.css` keys its palettes off. Nothing else in the app reads or writes the
 * theme directly.
 */

export type ThemeName = "dark" | "light" | "claude";

export const THEMES: { name: ThemeName; label: string; desc: string }[] = [
  { name: "dark", label: "深色", desc: "中性近黑" },
  { name: "light", label: "淺色", desc: "中性冷白" },
  { name: "claude", label: "Claude", desc: "暖紙白 · 赤陶橘" },
];

const KEY = "theme";
const KEY_LAST_LIGHT = "theme.lastLight";

function read<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const saved = localStorage.getItem(key);
  return allowed.includes(saved as T) ? (saved as T) : fallback;
}

const NAMES = THEMES.map((t) => t.name);
const LIGHT_NAMES: ThemeName[] = ["light", "claude"];

class ThemeState {
  /** The palette in use. */
  name = $state<ThemeName>(read(KEY, NAMES, "claude"));

  /**
   * Which light palette the title-bar toggle returns to.
   *
   * Without remembering it, going dark and back would silently discard a
   * "claude" choice made in Settings — the toggle would have to guess, and it
   * would guess wrong half the time.
   */
  lastLight = $state<ThemeName>(read(KEY_LAST_LIGHT, LIGHT_NAMES, "claude"));

  constructor() {
    $effect.root(() => {
      $effect(() => {
        document.documentElement.dataset.theme = this.name;
        localStorage.setItem(KEY, this.name);
        localStorage.setItem(KEY_LAST_LIGHT, this.lastLight);
      });
    });
  }

  get isDark() {
    return this.name === "dark";
  }

  set(name: ThemeName) {
    this.name = name;
    if (name !== "dark") this.lastLight = name;
  }

  /** Title-bar behaviour: light ↔ dark only, never picking a light palette. */
  toggleMode() {
    this.set(this.isDark ? this.lastLight : "dark");
  }
}

export const theme = new ThemeState();
