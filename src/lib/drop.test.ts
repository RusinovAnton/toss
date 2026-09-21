import { describe, expect, it } from "vitest";
import { dropPoint } from "./drop";

const MAC =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
const WINDOWS =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";
const LINUX =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

describe("dropPoint", () => {
  it("leaves a macOS point alone on a Retina screen", () => {
    expect(dropPoint({ x: 240, y: 80 }, MAC, 2)).toEqual({ x: 240, y: 80 });
  });

  it("leaves a Linux point alone", () => {
    expect(dropPoint({ x: 100, y: 200 }, LINUX, 2)).toEqual({ x: 100, y: 200 });
  });

  it("scales a Windows point down to CSS pixels", () => {
    expect(dropPoint({ x: 360, y: 120 }, WINDOWS, 1.5)).toEqual({ x: 240, y: 80 });
  });

  it("treats a missing ratio as 1", () => {
    expect(dropPoint({ x: 240, y: 80 }, WINDOWS, 0)).toEqual({ x: 240, y: 80 });
  });
});
