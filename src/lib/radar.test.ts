import { describe, expect, it } from "vitest";
import {
  CENTER_DIAMETER,
  DEVICE_DIAMETER,
  MAX_PER_ORBIT,
  hitTest,
  placeDevices,
  pulseScale,
} from "./radar";

const SIZE = 480;

describe("placeDevices", () => {
  it("places nothing when there is nothing to place", () => {
    expect(placeDevices(0, SIZE)).toEqual([]);
  });

  it("puts a single device at twelve o'clock", () => {
    const [only] = placeDevices(1, SIZE);
    expect(only.x).toBeCloseTo(SIZE / 2);
    expect(only.y).toBeLessThan(SIZE / 2);
    expect(only.orbit).toBe(0);
  });

  it("goes clockwise from the top", () => {
    const [top, right, bottom, left] = placeDevices(4, SIZE);
    expect(top.y).toBeLessThan(SIZE / 2);
    expect(right.x).toBeGreaterThan(SIZE / 2);
    expect(bottom.y).toBeGreaterThan(SIZE / 2);
    expect(left.x).toBeLessThan(SIZE / 2);
  });

  it("spaces devices evenly on the orbit", () => {
    const placements = placeDevices(6, SIZE);
    const distances = placements.map((p, index) => {
      const next = placements[(index + 1) % placements.length];
      return Math.hypot(next.x - p.x, next.y - p.y);
    });
    for (const distance of distances) {
      expect(distance).toBeCloseTo(distances[0], 5);
    }
  });

  it("moves the overflow to a second orbit", () => {
    const placements = placeDevices(MAX_PER_ORBIT + 3, SIZE);
    expect(placements.filter((p) => p.orbit === 0)).toHaveLength(MAX_PER_ORBIT);
    expect(placements.filter((p) => p.orbit === 1)).toHaveLength(3);

    const center = SIZE / 2;
    const radius = (p: { x: number; y: number }) =>
      Math.hypot(p.x - center, p.y - center);
    const inner = radius(placements[0]);
    const outer = radius(placements[MAX_PER_ORBIT]);
    expect(outer).toBeGreaterThan(inner);
  });

  it("keeps every circle inside the window", () => {
    for (const size of [360, 480, 700]) {
      for (const placement of placeDevices(12, size)) {
        expect(placement.x).toBeGreaterThanOrEqual(DEVICE_DIAMETER / 2);
        expect(placement.x).toBeLessThanOrEqual(size - DEVICE_DIAMETER / 2);
        expect(placement.y).toBeGreaterThanOrEqual(DEVICE_DIAMETER / 2);
        expect(placement.y).toBeLessThanOrEqual(size - DEVICE_DIAMETER / 2);
      }
    }
  });

  it("never overlaps the centre circle", () => {
    const center = 360 / 2;
    for (const placement of placeDevices(8, 360)) {
      const distance = Math.hypot(placement.x - center, placement.y - center);
      expect(distance).toBeGreaterThanOrEqual(
        CENTER_DIAMETER / 2 + DEVICE_DIAMETER / 2 - 0.001,
      );
    }
  });
});

describe("pulseScale", () => {
  it("grows the rings from the centre circle to the window edge", () => {
    expect(pulseScale(480)).toBeCloseTo(5);
    expect(pulseScale(360)).toBeCloseTo(3.75);
  });
});

describe("hitTest", () => {
  const placements = placeDevices(3, SIZE);

  it("finds the circle under the pointer", () => {
    expect(hitTest(placements, placements[1].x, placements[1].y)).toBe(1);
  });

  it("is forgiving near the edge of a circle", () => {
    const near = placements[0].x + DEVICE_DIAMETER / 2 + 4;
    expect(hitTest(placements, near, placements[0].y)).toBe(0);
  });

  it("returns -1 for empty space", () => {
    expect(hitTest(placements, SIZE / 2, SIZE / 2)).toBe(-1);
  });
});
