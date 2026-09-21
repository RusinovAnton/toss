/** Keeping the device menu inside the window. */

/** The menu's fixed width, in CSS pixels. */
export const MENU_WIDTH = 176;
/** Breathing room between the menu and the window edge. */
const MARGIN = 8;
/** How far below the circle's centre the menu opens when there is room. */
const GAP = 8;

/**
 * Where to put the menu's top-left corner so all of it stays on screen.
 *
 * The window is small and a circle can sit right against an edge, so the menu
 * opens below the circle when it fits, above it when it does not, and is
 * pinned to the window when neither side has room. A menu taller than the
 * window keeps its top visible rather than centring what cannot fit.
 */
export function menuPosition(
  point: { x: number; y: number },
  menu: { width: number; height: number },
  size: number,
): { left: number; top: number } {
  const left = Math.max(MARGIN, Math.min(point.x - menu.width / 2, size - MARGIN - menu.width));

  let top = point.y + GAP;
  if (top + menu.height > size - MARGIN) {
    const above = point.y - GAP - menu.height;
    top = above >= MARGIN ? above : Math.max(MARGIN, size - MARGIN - menu.height);
  }
  return { left, top };
}
