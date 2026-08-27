/**
 * Active colour scheme.
 *
 * Applied by stamping `data-theme` on the document root, which is what
 * `app.css` keys its palettes off. Nothing else in the app reads or writes the
 * theme directly.
 *
 * Two things are stored, not one: what the user *chose* and what that resolves
 * to. "跟隨系統" is a choice whose answer changes while the app is open, so the
 * chosen value has to survive as `system` rather than being flattened into the
 * palette it happened to mean at startup.
 */

export type ThemeName = "dark" | "light" | "claude";
/** What the user picked. `system` follows the OS setting, live. */
export type ThemeChoice = ThemeName | "system";

export const THEMES: { name: ThemeChoice; label: string; icon: ThemeIcon }[] = [
  { name: "claude", label: "紙色", icon: "sun" },
  { name: "light", label: "淺色", icon: "sun" },
  { name: "dark", label: "深色", icon: "moon" },
  { name: "system", label: "跟隨系統", icon: "contrast" },
];

type ThemeIcon = "sun" | "moon" | "contrast";

const KEY = "theme";
const KEY_LAST_LIGHT = "theme.lastLight";

const CHOICES = THEMES.map((t) => t.name);
const LIGHT_NAMES: ThemeName[] = ["light", "claude"];

function read<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const saved = localStorage.getItem(key);
  return allowed.includes(saved as T) ? (saved as T) : fallback;
}

/** The OS preference. Windows exposes it; a browser preview may not. */
const systemDark = window.matchMedia?.("(prefers-color-scheme: dark)");

class ThemeState {
  /** What the user picked, which may be `system`. */
  choice = $state<ThemeChoice>(read(KEY, CHOICES, "claude"));

  /**
   * Which light palette a switch back from dark returns to.
   *
   * Without remembering it, going dark and back would silently discard a
   * "紙色" choice — the toggle would have to guess, and it would guess wrong
   * half the time.
   */
  lastLight = $state<ThemeName>(read(KEY_LAST_LIGHT, LIGHT_NAMES, "claude"));

  /** Tracks the OS while `choice` is `system`; ignored otherwise. */
  private osDark = $state(systemDark?.matches ?? false);

  constructor() {
    systemDark?.addEventListener("change", (e) => (this.osDark = e.matches));

    $effect.root(() => {
      $effect(() => {
        document.documentElement.dataset.theme = this.name;
        localStorage.setItem(KEY, this.choice);
        localStorage.setItem(KEY_LAST_LIGHT, this.lastLight);
      });
    });
  }

  /** The palette actually in use — `system` resolved. */
  get name(): ThemeName {
    if (this.choice !== "system") return this.choice;
    return this.osDark ? "dark" : this.lastLight;
  }

  get isDark() {
    return this.name === "dark";
  }

  set(choice: ThemeChoice) {
    this.choice = choice;
    if (choice !== "dark" && choice !== "system") this.lastLight = choice;
  }
}

export const theme = new ThemeState();
