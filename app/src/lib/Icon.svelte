<!--
  Inline icon set.

  Drawn here rather than pulled from an icon package: the CSP blocks external
  hosts, and a whole library to render a dozen 24px glyphs is weight that never
  comes back off. Geometry follows Lucide, which is what the .pen files name
  their icons after (`terminal`, `pencil-line`, `settings-2`).

  Everything is stroke-based and inherits `currentColor`, so an icon takes the
  colour of whatever it sits in and needs no per-use fill.
-->
<script lang="ts" module>
  const PATHS = {
    menu: '<path d="M4 6h16M4 12h16M4 18h16"/>',
    home: '<path d="M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1z"/>',
    download:
      '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/>',
    "settings-2":
      '<path d="M20 7h-9M14 17H5"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/>',
    sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
    moon: '<path d="M12 3a6.4 6.4 0 0 0 9 9 9 9 0 1 1-9-9z"/>',
    minimize: '<path d="M5 12h14"/>',
    maximize: '<rect x="5" y="5" width="14" height="14" rx="2"/>',
    close: '<path d="M18 6 6 18M6 6l12 12"/>',
    plus: '<path d="M5 12h14M12 5v14"/>',
    terminal: '<path d="m4 17 6-6-6-6M12 19h8"/>',
    "pencil-line":
      '<path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"/>',
    trash:
      '<path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2M19 6l-1 14a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1L5 6"/>',
    "chevron-down": '<path d="m6 9 6 6 6-6"/>',
    "chevron-left": '<path d="m15 6-6 6 6 6"/>',
    "rotate-cw": '<path d="M21 12a9 9 0 1 1-2.6-6.4L21 8M21 3v5h-5"/>',
    "corner-down-left": '<path d="M9 10 4 15l5 5"/><path d="M20 4v7a4 4 0 0 1-4 4H4"/>',
    copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1"/>',
    refresh:
      '<path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6"/>',
    play: '<path d="m6 4 14 8-14 8z" fill="currentColor" stroke="none"/>',
    square: '<rect x="6" y="6" width="12" height="12" rx="1.5"/>',
    search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
    ellipsis:
      '<circle cx="5" cy="12" r="1.6" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none"/><circle cx="19" cy="12" r="1.6" fill="currentColor" stroke="none"/>',
    check: '<path d="m5 13 4 4L19 7"/>',
    "folder-open":
      '<path d="M3 7a1 1 0 0 1 1-1h5l2 2h8a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"/>',
  } as const;

  export type IconName = keyof typeof PATHS;
  export const iconNames = Object.keys(PATHS) as IconName[];

  export function iconMarkup(name: IconName): string {
    return PATHS[name];
  }
</script>

<script lang="ts">
  let {
    name,
    size = 20,
    strokeWidth = 1.75,
  }: { name: IconName; size?: number; strokeWidth?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <!-- eslint-disable-next-line svelte/no-at-html-tags -->
  {@html iconMarkup(name)}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>
