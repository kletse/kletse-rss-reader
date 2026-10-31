import { describe, expect, it } from "vitest";
import { fontFamily } from "./theme";

describe("fontFamily", () => {
  it("leaves CSS generic families unquoted", () => {
    expect(fontFamily("ui-monospace")).toBe("ui-monospace");
    expect(fontFamily("monospace")).toBe("monospace");
  });
  it("quotes real font names", () => {
    expect(fontFamily("JetBrainsMono Nerd Font")).toBe('"JetBrainsMono Nerd Font"');
    expect(fontFamily("Iosevka")).toBe('"Iosevka"');
  });
});
