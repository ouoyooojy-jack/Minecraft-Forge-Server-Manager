/**
 * Whether the page on screen holds edits that leaving would throw away.
 *
 * Navigation lives in the shell and the edits live in the page, so the flag
 * sits between them: the page writes it, the shell reads it before switching.
 */
export const unsaved = $state({ dirty: false });
