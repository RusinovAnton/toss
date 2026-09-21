import { describe, expect, it } from "vitest";
import { MENU_WIDTH, menuPosition } from "./menu";

const SIZE = 480;
const menu = { width: MENU_WIDTH, height: 200 };

describe("menuPosition", () => {
  it("centres the menu under the circle when it fits", () => {
    expect(menuPosition({ x: 240, y: 160 }, menu, SIZE)).toEqual({
      left: 240 - MENU_WIDTH / 2,
      top: 168,
    });
  });

  it("keeps a menu off the left edge", () => {
    expect(menuPosition({ x: 40, y: 160 }, menu, SIZE).left).toBe(8);
  });

  it("keeps a menu off the right edge", () => {
    expect(menuPosition({ x: 450, y: 160 }, menu, SIZE).left).toBe(SIZE - 8 - MENU_WIDTH);
  });

  it("opens above a circle near the bottom", () => {
    expect(menuPosition({ x: 240, y: 400 }, menu, SIZE).top).toBe(400 - 8 - 200);
  });

  it("pins the menu inside when neither side has room", () => {
    const tall = { width: MENU_WIDTH, height: 300 };
    expect(menuPosition({ x: 240, y: 260 }, tall, SIZE).top).toBe(SIZE - 8 - 300);
  });

  it("keeps the top of a menu taller than the window visible", () => {
    const huge = { width: MENU_WIDTH, height: 600 };
    expect(menuPosition({ x: 240, y: 240 }, huge, SIZE).top).toBe(8);
  });
});
