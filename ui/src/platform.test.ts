import { describe, expect, it } from "vitest";
import { keys } from "./platform";

describe("keys", () => {
  it("leaves Mac labels alone on macOS", () => {
    expect(keys("⇧⌘S · ⌥-drag", true)).toBe("⇧⌘S · ⌥-drag");
  });

  it("spells modifiers out elsewhere, Ctrl before Alt before Shift", () => {
    expect(keys("⌘E", false)).toBe("Ctrl+E");
    expect(keys("⇧⌘S", false)).toBe("Ctrl+Shift+S");
    expect(keys("⌘⇧E split", false)).toBe("Ctrl+Shift+E split");
    expect(keys("⇧↑", false)).toBe("Shift+↑");
  });

  it("drops the plus where the modifier stands alone", () => {
    expect(keys("⌥-drag a note: duplicate", false)).toBe("Alt-drag a note: duplicate");
    expect(keys("⌥-wheel transpose (⌥⇧ octave)", false)).toBe("Alt-wheel transpose (Alt+Shift octave)");
    expect(keys("⇧/⌘-drag: marquee", false)).toBe("Shift/Ctrl-drag: marquee");
    expect(keys("drag: move (⇧ one axis)", false)).toBe("drag: move (Shift one axis)");
    expect(keys("⌥: off the grid", false)).toBe("Alt: off the grid");
    expect(keys("ends with ⇧", false)).toBe("ends with Shift");
  });

  it("names the delete key", () => {
    expect(keys("⌫ delete", false)).toBe("Backspace delete");
  });
});
