import { describe, it, expect } from "vitest";
import { positionContextMenu } from "./rowContextMenuLogic";

const viewport = { width: 1000, height: 800 };
const menuSize = { width: 200, height: 300 };
const submenuSize = { width: 176, height: 80 };

describe("positionContextMenu", () => {
  it("keeps the menu at the anchor when it fits", () => {
    const layout = positionContextMenu({
      anchor: { x: 100, y: 100 },
      menuSize,
      submenuSize,
      viewport,
    });
    expect(layout).toEqual({ left: 100, top: 100, submenuSide: "right" });
  });

  it("flips left/up when the menu would overflow the viewport", () => {
    const layout = positionContextMenu({
      anchor: { x: 950, y: 780 },
      menuSize,
      submenuSize,
      viewport,
    });
    expect(layout.left).toBeLessThan(950);
    expect(layout.top).toBeLessThan(780);
    expect(layout.left).toBeGreaterThanOrEqual(0);
    expect(layout.top).toBeGreaterThanOrEqual(0);
  });

  it("opens the submenu to the left when there is no room on the right", () => {
    const layout = positionContextMenu({
      anchor: { x: 790, y: 100 },
      menuSize,
      submenuSize,
      viewport,
    });
    expect(layout.submenuSide).toBe("left");
  });
});
