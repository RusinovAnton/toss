/** Geometry of the radar: where the center sits and where devices orbit it. */

export const CENTER_DIAMETER = 96;
export const DEVICE_DIAMETER = 72;
/** Radii at the default 480px window, kept as ratios so the radar scales. */
const FIRST_ORBIT_RATIO = 160 / 480;
const SECOND_ORBIT_RATIO = 230 / 480;
/** Beyond this the circles on one orbit start touching. */
export const MAX_PER_ORBIT = 8;
/** Breathing room between a device circle and the window edge. */
const EDGE_MARGIN = 10;

export interface Placement {
  /** Centre of the device circle, in CSS pixels from the window's top left. */
  x: number;
  y: number;
  orbit: 0 | 1;
}

function orbitRadius(size: number, orbit: 0 | 1): number {
  const ratio = orbit === 0 ? FIRST_ORBIT_RATIO : SECOND_ORBIT_RATIO;
  const largest = size / 2 - DEVICE_DIAMETER / 2 - EDGE_MARGIN;
  const smallest = CENTER_DIAMETER / 2 + DEVICE_DIAMETER / 2;
  return Math.max(Math.min(size * ratio, largest), smallest);
}

/**
 * Places `count` devices around the centre of a `size`x`size` window.
 *
 * The first device sits at twelve o'clock and the rest follow clockwise. Once
 * an orbit is full the next devices go to a wider one, turned half a step so
 * they do not line up behind the inner ring.
 */
export function placeDevices(count: number, size: number): Placement[] {
  if (count <= 0) return [];
  const center = size / 2;
  const inner = Math.min(count, MAX_PER_ORBIT);
  const outer = count - inner;

  const place = (index: number, total: number, orbit: 0 | 1): Placement => {
    const radius = orbitRadius(size, orbit);
    const step = (2 * Math.PI) / total;
    const offset = orbit === 0 ? 0 : step / 2;
    const angle = index * step + offset;
    return {
      // Screen y grows downwards, so twelve o'clock is a subtraction.
      x: center + radius * Math.sin(angle),
      y: center - radius * Math.cos(angle),
      orbit,
    };
  };

  const placements: Placement[] = [];
  for (let index = 0; index < inner; index++) {
    placements.push(place(index, inner, 0));
  }
  for (let index = 0; index < outer; index++) {
    placements.push(place(index, outer, 1));
  }
  return placements;
}

/** How far the pulse rings grow: from the centre circle's edge to the corner. */
export function pulseScale(size: number): number {
  return size / 2 / (CENTER_DIAMETER / 2);
}

/** The device circle under a pointer, or -1. Generous by a few pixels. */
export function hitTest(
  placements: { x: number; y: number }[],
  x: number,
  y: number,
  radius = DEVICE_DIAMETER / 2 + 8,
): number {
  for (let index = 0; index < placements.length; index++) {
    const dx = placements[index].x - x;
    const dy = placements[index].y - y;
    if (Math.hypot(dx, dy) <= radius) return index;
  }
  return -1;
}
