/**
 * The little physics the radar runs: circles you can throw, that bump into
 * each other and into the device in the middle.
 *
 * Everything here is pure and frame-rate independent — positions in CSS
 * pixels, velocities in pixels per second, `dt` in seconds — so the whole
 * thing can be stepped in a test without a window.
 */

/** A circle that moves. */
export interface Body {
  x: number;
  y: number;
  vx: number;
  vy: number;
  /** Collision radius, which is a little larger than the drawn circle. */
  r: number;
  /** Held by the pointer: it moves where the pointer says and nothing pushes it. */
  held?: boolean;
}

/** A circle that does not move, like the device in the middle. */
export interface Fixed {
  x: number;
  y: number;
  r: number;
}

/** What a collision with the centre hands back, so the centre can wobble. */
export interface Impulse {
  x: number;
  y: number;
}

/**
 * Speed left after a second of drifting.
 *
 * Small on purpose: a circle comes to rest within about half a second of
 * being let go, after 30 to 130px depending on how hard it was thrown. At
 * 0.08 it kept gliding, which made the radar feel like ice rather than a
 * desk.
 */
const DAMPING_PER_SECOND = 0.001;
/** How much speed survives a bounce. Walls eat more than circles do. */
const WALL_RESTITUTION = 0.6;
const BODY_RESTITUTION = 0.8;
/** Below this a circle is treated as parked, which is what lets the loop idle. */
export const REST_SPEED = 12;
/** A throw faster than this would tunnel through things in one frame. */
const MAX_SPEED = 2600;
/** Longest step taken at once; a slow frame is cut into several of these. */
export const MAX_STEP = 1 / 60;

function clampSpeed(body: Body): void {
  const speed = Math.hypot(body.vx, body.vy);
  if (speed > MAX_SPEED) {
    body.vx = (body.vx / speed) * MAX_SPEED;
    body.vy = (body.vy / speed) * MAX_SPEED;
  }
}

/** Moves one body and slows it down. A held body is driven by the pointer. */
export function integrate(body: Body, dt: number): void {
  if (body.held) return;
  clampSpeed(body);
  body.x += body.vx * dt;
  body.y += body.vy * dt;
  const damping = Math.pow(DAMPING_PER_SECOND, dt);
  body.vx *= damping;
  body.vy *= damping;
  if (Math.hypot(body.vx, body.vy) < REST_SPEED) {
    body.vx = 0;
    body.vy = 0;
  }
}

/** Keeps a body inside a square window, bouncing off the edges. */
export function bounceOffWalls(body: Body, size: number): void {
  const min = body.r;
  const max = size - body.r;
  if (body.x < min) {
    body.x = min;
    if (!body.held) body.vx = Math.abs(body.vx) * WALL_RESTITUTION;
  } else if (body.x > max) {
    body.x = max;
    if (!body.held) body.vx = -Math.abs(body.vx) * WALL_RESTITUTION;
  }
  if (body.y < min) {
    body.y = min;
    if (!body.held) body.vy = Math.abs(body.vy) * WALL_RESTITUTION;
  } else if (body.y > max) {
    body.y = max;
    if (!body.held) body.vy = -Math.abs(body.vy) * WALL_RESTITUTION;
  }
}

/**
 * Separates two overlapping circles and swaps the speed along the line
 * between them. Equal masses, except that a held circle is immovable: the
 * pointer wins every argument.
 *
 * Returns whether they were touching at all.
 */
export function collide(a: Body, b: Body): boolean {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const distance = Math.hypot(dx, dy);
  const overlap = a.r + b.r - distance;
  if (overlap <= 0) return false;

  // Two circles exactly on top of each other have no line to push along, so
  // pick one rather than dividing by zero.
  const nx = distance > 0.001 ? dx / distance : 1;
  const ny = distance > 0.001 ? dy / distance : 0;

  const aFixed = a.held === true;
  const bFixed = b.held === true;
  if (aFixed && bFixed) return true;

  // Push them apart first, so the next frame does not see the same overlap.
  const aShare = aFixed ? 0 : bFixed ? 1 : 0.5;
  const bShare = 1 - aShare;
  a.x -= nx * overlap * aShare;
  a.y -= ny * overlap * aShare;
  b.x += nx * overlap * bShare;
  b.y += ny * overlap * bShare;

  const approach = (b.vx - a.vx) * nx + (b.vy - a.vy) * ny;
  if (approach > 0) return true; // already moving apart

  // Equal masses: each takes half. Against a held circle, one takes it all.
  const impulse = -(1 + BODY_RESTITUTION) * approach * (aFixed || bFixed ? 1 : 0.5);
  if (!aFixed) {
    a.vx -= impulse * nx;
    a.vy -= impulse * ny;
  }
  if (!bFixed) {
    b.vx += impulse * nx;
    b.vy += impulse * ny;
  }
  return true;
}

/**
 * Bounces a body off something that never moves, and reports the push the
 * fixed thing would have taken. The centre circle uses that to wobble.
 */
export function collideWithFixed(body: Body, fixed: Fixed): Impulse | null {
  const dx = body.x - fixed.x;
  const dy = body.y - fixed.y;
  const distance = Math.hypot(dx, dy);
  const overlap = body.r + fixed.r - distance;
  if (overlap <= 0) return null;

  const nx = distance > 0.001 ? dx / distance : 0;
  const ny = distance > 0.001 ? dy / distance : -1;
  body.x += nx * overlap;
  body.y += ny * overlap;

  const approach = body.vx * nx + body.vy * ny;
  if (approach >= 0 || body.held) {
    // A held circle is shoved by the pointer rather than by physics, so the
    // centre still feels it: the overlap is the only measure of how hard.
    return { x: -nx * overlap * 8, y: -ny * overlap * 8 };
  }
  const impulse = -(1 + BODY_RESTITUTION) * approach;
  body.vx += impulse * nx;
  body.vy += impulse * ny;
  return { x: -nx * impulse, y: -ny * impulse };
}

/**
 * One step of everything: move, hit the walls, hit each other, hit the middle.
 *
 * Returns the total push the centre took, for the wobble.
 */
export function step(bodies: Body[], centre: Fixed, size: number, dt: number): Impulse {
  const total: Impulse = { x: 0, y: 0 };
  for (const body of bodies) {
    integrate(body, dt);
    bounceOffWalls(body, size);
    const push = collideWithFixed(body, centre);
    if (push) {
      total.x += push.x;
      total.y += push.y;
    }
  }
  for (let i = 0; i < bodies.length; i++) {
    for (let j = i + 1; j < bodies.length; j++) {
      collide(bodies[i], bodies[j]);
    }
  }
  return total;
}

/** True when nothing is moving and the loop can stop until something happens. */
export function atRest(bodies: Body[], wobble: Wobble): boolean {
  if (bodies.some((body) => body.held || body.vx !== 0 || body.vy !== 0)) return false;
  return (
    Math.hypot(wobble.x, wobble.y) < 0.1 && Math.hypot(wobble.vx, wobble.vy) < REST_SPEED
  );
}

/** The centre's offset from the middle, and how fast it is heading back. */
export interface Wobble {
  x: number;
  y: number;
  vx: number;
  vy: number;
}

/** Stiff enough to snap back in a few hundred milliseconds. */
const WOBBLE_STIFFNESS = 260;
const WOBBLE_DAMPING = 14;
/** The centre is nudged, not moved: this is as far as it ever goes. */
const WOBBLE_LIMIT = 7;

export function newWobble(): Wobble {
  return { x: 0, y: 0, vx: 0, vy: 0 };
}

/** A spring back to the middle, with whatever push it just took. */
export function stepWobble(wobble: Wobble, push: Impulse, dt: number): void {
  wobble.vx += push.x;
  wobble.vy += push.y;
  wobble.vx += (-WOBBLE_STIFFNESS * wobble.x - WOBBLE_DAMPING * wobble.vx) * dt;
  wobble.vy += (-WOBBLE_STIFFNESS * wobble.y - WOBBLE_DAMPING * wobble.vy) * dt;
  wobble.x += wobble.vx * dt;
  wobble.y += wobble.vy * dt;

  const distance = Math.hypot(wobble.x, wobble.y);
  if (distance > WOBBLE_LIMIT) {
    wobble.x = (wobble.x / distance) * WOBBLE_LIMIT;
    wobble.y = (wobble.y / distance) * WOBBLE_LIMIT;
  }
  if (distance < 0.1 && Math.hypot(wobble.vx, wobble.vy) < REST_SPEED) {
    wobble.x = 0;
    wobble.y = 0;
    wobble.vx = 0;
    wobble.vy = 0;
  }
}
