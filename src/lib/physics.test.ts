import { describe, expect, it } from "vitest";
import {
  atRest,
  bounceOffWalls,
  collide,
  collideWithFixed,
  integrate,
  newWobble,
  step,
  stepWobble,
  type Body,
} from "./physics";

function body(partial: Partial<Body> = {}): Body {
  return { x: 100, y: 100, vx: 0, vy: 0, r: 38, ...partial };
}

describe("integrate", () => {
  it("moves a body along its velocity", () => {
    const moving = body({ vx: 120, vy: -60 });
    integrate(moving, 0.5);
    expect(moving.x).toBeCloseTo(160, 0);
    expect(moving.y).toBeCloseTo(70, 0);
  });

  it("slows a body down until it parks", () => {
    const thrown = body({ vx: 900 });
    for (let frame = 0; frame < 300; frame++) integrate(thrown, 1 / 60);
    expect(thrown.vx).toBe(0);
  });

  it("lands close to where a throw let go of it", () => {
    // 200px/s is about what a brisk flick hands over once the throw is
    // scaled down, and it should stop within half a second and half a
    // circle's travel, not glide on across the radar.
    const thrown = body({ vx: 200 });
    let seconds = 0;
    while (thrown.vx !== 0 && seconds < 5) {
      integrate(thrown, 1 / 60);
      seconds += 1 / 60;
    }
    expect(seconds).toBeLessThan(0.5);
    expect(thrown.x - 100).toBeLessThan(40);
  });

  it("leaves a held body where the pointer put it", () => {
    const held = body({ vx: 900, held: true });
    integrate(held, 1);
    expect(held.x).toBe(100);
    expect(held.vx).toBe(900);
  });
});

describe("bounceOffWalls", () => {
  it("turns a body around at the edge", () => {
    const escaping = body({ x: -20, vx: -300 });
    bounceOffWalls(escaping, 480);
    expect(escaping.x).toBe(escaping.r);
    expect(escaping.vx).toBeGreaterThan(0);
  });

  it("loses speed on the way back", () => {
    const escaping = body({ y: 600, vy: 400 });
    bounceOffWalls(escaping, 480);
    expect(escaping.y).toBe(480 - escaping.r);
    expect(Math.abs(escaping.vy)).toBeLessThan(400);
  });

  it("does not launch a held body", () => {
    const held = body({ x: -20, vx: -300, held: true });
    bounceOffWalls(held, 480);
    expect(held.x).toBe(held.r);
    expect(held.vx).toBe(-300);
  });
});

describe("collide", () => {
  it("ignores circles that do not touch", () => {
    expect(collide(body({ x: 0 }), body({ x: 500 }))).toBe(false);
  });

  it("pushes overlapping circles apart", () => {
    const left = body({ x: 100 });
    const right = body({ x: 140 });
    expect(collide(left, right)).toBe(true);
    expect(right.x - left.x).toBeCloseTo(left.r + right.r, 5);
  });

  it("passes the speed along", () => {
    const moving = body({ x: 100, vx: 400 });
    const still = body({ x: 170 });
    collide(moving, still);
    expect(still.vx).toBeGreaterThan(0);
    expect(moving.vx).toBeLessThan(400);
  });

  it("never moves a held circle", () => {
    const held = body({ x: 100, held: true });
    const hit = body({ x: 140 });
    collide(held, hit);
    expect(held.x).toBe(100);
    expect(held.vx).toBe(0);
    expect(hit.x).toBeCloseTo(100 + held.r + hit.r, 5);
  });

  it("leaves circles that are already parting alone", () => {
    const left = body({ x: 100, vx: -200 });
    const right = body({ x: 140, vx: 200 });
    collide(left, right);
    expect(left.vx).toBe(-200);
    expect(right.vx).toBe(200);
  });
});

describe("collideWithFixed", () => {
  const centre = { x: 240, y: 240, r: 48 };

  it("says nothing when the body is clear of it", () => {
    expect(collideWithFixed(body({ x: 0, y: 0 }), centre)).toBeNull();
  });

  it("pushes the body out and reports the kick", () => {
    const hitting = body({ x: 250, y: 240, vx: -300 });
    const push = collideWithFixed(hitting, centre);
    expect(push).not.toBeNull();
    expect(hitting.x).toBeGreaterThan(centre.x + centre.r);
    expect(hitting.vx).toBeGreaterThan(0);
    // The centre is pushed the way the body was going.
    expect(push!.x).toBeLessThan(0);
  });
});

describe("step", () => {
  it("keeps everything inside the window", () => {
    const bodies = [body({ x: 10, y: 10, vx: -900, vy: -900 }), body({ x: 470, y: 470, vx: 900 })];
    for (let frame = 0; frame < 120; frame++) step(bodies, { x: 240, y: 240, r: 48 }, 480, 1 / 60);
    for (const one of bodies) {
      expect(one.x).toBeGreaterThanOrEqual(one.r - 0.001);
      expect(one.x).toBeLessThanOrEqual(480 - one.r + 0.001);
      expect(one.y).toBeGreaterThanOrEqual(one.r - 0.001);
      expect(one.y).toBeLessThanOrEqual(480 - one.r + 0.001);
    }
  });

  it("comes to a stop on its own", () => {
    const bodies = [body({ x: 120, y: 120, vx: 500, vy: 300 })];
    const wobble = newWobble();
    for (let frame = 0; frame < 600; frame++) {
      const push = step(bodies, { x: 240, y: 240, r: 48 }, 480, 1 / 60);
      stepWobble(wobble, push, 1 / 60);
    }
    expect(atRest(bodies, wobble)).toBe(true);
  });
});

describe("stepWobble", () => {
  it("springs back to the middle", () => {
    const wobble = newWobble();
    stepWobble(wobble, { x: 300, y: 0 }, 1 / 60);
    expect(Math.abs(wobble.x)).toBeGreaterThan(0);
    for (let frame = 0; frame < 300; frame++) stepWobble(wobble, { x: 0, y: 0 }, 1 / 60);
    expect(wobble.x).toBe(0);
    expect(wobble.y).toBe(0);
  });

  it("never wanders far from the middle", () => {
    const wobble = newWobble();
    for (let frame = 0; frame < 60; frame++) {
      stepWobble(wobble, { x: 4000, y: 4000 }, 1 / 60);
      expect(Math.hypot(wobble.x, wobble.y)).toBeLessThanOrEqual(7.001);
    }
  });
});
