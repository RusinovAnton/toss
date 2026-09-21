/** Where a drag from Finder or Explorer actually landed. */

/**
 * Converts a drag-drop point to CSS pixels.
 *
 * Tauri types the point as a `PhysicalPosition`, but wry does not fill it the
 * same way on every platform: macOS reads `draggingLocation` off the view, and
 * Linux the drop controller's widget coordinates, both of which are logical
 * points; Windows passes the result of `ScreenToClient`, which is physical
 * pixels. Nothing converts them before they reach us, so only Windows needs
 * the divide.
 *
 * Dividing everywhere halved every coordinate on a Retina Mac, which put every
 * drop in empty space: the radar shook its centre and no transfer ever began.
 */
export function dropPoint(
  position: { x: number; y: number },
  userAgent: string,
  devicePixelRatio: number,
): { x: number; y: number } {
  const ratio = userAgent.includes("Windows") ? devicePixelRatio || 1 : 1;
  return { x: position.x / ratio, y: position.y / ratio };
}
