// Shortcut labels for the OS the app runs on. Help text is written once with the Mac glyphs
// (⌘ ⌥ ⇧); on Windows and Linux `keys()` spells them out, in the usual Ctrl, Alt, Shift order.

/** True on macOS, where the glyphs are what people expect to read. */
export const IS_MAC =
  typeof navigator !== "undefined" && /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent || "");

const NAMES: Record<string, string> = { "⌘": "Ctrl", "⌥": "Alt", "⇧": "Shift" };
const ORDER = ["Ctrl", "Alt", "Shift"];

/**
 * Rewrite a label written with Mac glyphs for the current OS.
 *
 * `⇧⌘S` → `Ctrl+Shift+S`, `⌥-drag` → `Alt-drag`, `(⌥⇧ octave)` → `(Alt+Shift octave)`,
 * `⇧/⌘-drag` → `Shift/Ctrl-drag`. A run of modifiers joins the key after it with `+`; before a
 * space, a dash or punctuation it stands alone. On macOS the text comes back unchanged.
 */
export function keys(text: string, mac = IS_MAC): string {
  if (mac) return text;
  return text
    .replace(/[⌘⌥⇧]+/g, (run, offset: number, all: string) => {
      const names = [...run].map((g) => NAMES[g]).sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
      const next = all[offset + run.length];
      const joinsKey = next !== undefined && !/[\s\-:),/·]/.test(next);
      return names.join("+") + (joinsKey ? "+" : "");
    })
    .replace(/⌫/g, "Backspace");
}
