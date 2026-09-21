import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  atRest,
  MAX_STEP,
  newWobble,
  step,
  stepWobble,
  type Body,
  type Wobble,
} from "../lib/physics";
import { CENTER_DIAMETER, DEVICE_DIAMETER, placeDevices } from "../lib/radar";
import type { Device } from "../lib/tauri";

/** A little wider than the circle, so circles never look squeezed together. */
const BODY_RADIUS = DEVICE_DIAMETER / 2 + 2;
const CENTRE_RADIUS = CENTER_DIAMETER / 2 + 4;
/** Past this a press counts as a drag rather than a click. */
const DRAG_SLOP = 5;
/** A throw takes its speed from the last few moves, not the whole drag. */
const VELOCITY_MEMORY = 0.06;
/**
 * How much of the pointer's speed a circle keeps once it is let go.
 *
 * At full speed a flick sent a circle across the window and back, which made
 * the radar hard to aim at. A third of it still reads as a throw and settles
 * in a fraction of the distance.
 */
const THROW_SCALE = 1 / 3;

export interface Point {
  x: number;
  y: number;
}

/**
 * Runs the radar's physics and hands back where to draw everything.
 *
 * Devices spawn on their orbit and stay there until someone throws them. The
 * loop stops once nothing is moving, so a still radar costs nothing, and any
 * new device or grab wakes it again.
 */
export function useRadarPhysics(devices: Device[], size: number) {
  const bodiesRef = useRef(new Map<string, Body>());
  const wobbleRef = useRef<Wobble>(newWobble());
  const frameRef = useRef<number | null>(null);
  const lastFrameRef = useRef(0);
  const sizeRef = useRef(size);
  sizeRef.current = size;
  /** Live positions for code that runs outside React, like the file drop. */
  const positionsRef = useRef<Point[]>([]);
  const orderRef = useRef<string[]>([]);
  const draggedRef = useRef(false);

  const [positions, setPositions] = useState<Point[]>([]);
  const [wobble, setWobble] = useState<Point>({ x: 0, y: 0 });

  const publish = useCallback(() => {
    const next = orderRef.current.map((id) => {
      const body = bodiesRef.current.get(id);
      return { x: body?.x ?? 0, y: body?.y ?? 0 };
    });
    positionsRef.current = next;
    setPositions(next);
    setWobble({ x: wobbleRef.current.x, y: wobbleRef.current.y });
  }, []);

  const run = useCallback(
    (now: number) => {
      const box = sizeRef.current;
      const centre = { x: box / 2, y: box / 2, r: CENTRE_RADIUS };
      const bodies = [...bodiesRef.current.values()];
      // A tab that was in the background hands back a huge delta; simulating
      // all of it would fire everything into the walls.
      let remaining = Math.min((now - lastFrameRef.current) / 1000, 0.1);
      lastFrameRef.current = now;
      while (remaining > 0) {
        const slice = Math.min(remaining, MAX_STEP);
        const push = step(bodies, centre, box, slice);
        stepWobble(wobbleRef.current, push, slice);
        remaining -= slice;
      }
      publish();
      frameRef.current = atRest(bodies, wobbleRef.current)
        ? null
        : requestAnimationFrame(run);
    },
    [publish],
  );

  const wake = useCallback(() => {
    if (frameRef.current !== null) return;
    lastFrameRef.current = performance.now();
    frameRef.current = requestAnimationFrame(run);
  }, [run]);

  // Devices appearing or leaving, and the window changing size. A device that
  // is already on screen keeps where it is; only new ones take an orbit slot.
  const fingerprints = useMemo(
    () => devices.map((device) => device.fingerprint).join(","),
    [devices],
  );
  // Layout rather than effect: a new circle should be painted on its orbit,
  // never for one frame in the middle of the window.
  useLayoutEffect(() => {
    const home = placeDevices(devices.length, size);
    const next = new Map<string, Body>();
    devices.forEach((device, index) => {
      const existing = bodiesRef.current.get(device.fingerprint);
      next.set(
        device.fingerprint,
        existing ?? {
          x: home[index]?.x ?? size / 2,
          y: home[index]?.y ?? size / 2,
          vx: 0,
          vy: 0,
          r: BODY_RADIUS,
        },
      );
    });
    bodiesRef.current = next;
    orderRef.current = devices.map((device) => device.fingerprint);
    publish();
    wake();
  }, [fingerprints, devices, size, publish, wake]);

  useEffect(
    () => () => {
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    },
    [],
  );

  /**
   * Starts a drag. The circle follows the pointer exactly, remembers how fast
   * it was moving, and keeps that speed when it is let go.
   */
  const grab = useCallback(
    (fingerprint: string, event: React.PointerEvent<HTMLElement>) => {
      const body = bodiesRef.current.get(fingerprint);
      if (!body || event.button !== 0) return;
      const surface = event.currentTarget.parentElement ?? event.currentTarget;
      const box = surface.getBoundingClientRect();
      const grabX = event.clientX - box.left - body.x;
      const grabY = event.clientY - box.top - body.y;

      body.held = true;
      body.vx = 0;
      body.vy = 0;
      draggedRef.current = false;
      let lastX = event.clientX;
      let lastY = event.clientY;
      let lastTime = performance.now();
      let travelled = 0;

      const move = (moved: PointerEvent) => {
        const now = performance.now();
        const dt = Math.max((now - lastTime) / 1000, 0.001);
        body.x = moved.clientX - box.left - grabX;
        body.y = moved.clientY - box.top - grabY;
        // Blend so one stuttering frame does not decide the throw.
        const blend = Math.min(dt / VELOCITY_MEMORY, 1);
        body.vx += ((moved.clientX - lastX) / dt - body.vx) * blend;
        body.vy += ((moved.clientY - lastY) / dt - body.vy) * blend;
        travelled += Math.hypot(moved.clientX - lastX, moved.clientY - lastY);
        if (travelled > DRAG_SLOP) draggedRef.current = true;
        lastX = moved.clientX;
        lastY = moved.clientY;
        lastTime = now;
        wake();
      };

      const release = (ended: PointerEvent) => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", release);
        window.removeEventListener("pointercancel", release);
        body.held = false;
        // A pointer that rested before letting go should not fling anything.
        if (performance.now() - lastTime > 120) {
          body.vx = 0;
          body.vy = 0;
        } else {
          body.vx *= THROW_SCALE;
          body.vy *= THROW_SCALE;
        }
        if (ended.type === "pointercancel") draggedRef.current = false;
        wake();
      };

      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", release);
      window.addEventListener("pointercancel", release);
      wake();
    },
    [wake],
  );

  /** Whether the press that just ended moved far enough to be a drag. */
  const wasDragged = useCallback(() => draggedRef.current, []);

  return { positions, positionsRef, wobble, grab, wasDragged };
}
