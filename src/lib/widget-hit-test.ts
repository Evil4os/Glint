interface WidgetPoint {
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  expanded: boolean;
  sidebarDocked: boolean;
}

export function isPointInsideWidget({ x, y, width, height, scaleFactor, expanded, sidebarDocked }: WidgetPoint): boolean {
  const inset = (expanded || sidebarDocked ? 0 : 8) * scaleFactor;
  const left = inset;
  const top = inset;
  const right = width - inset;
  const bottom = height - inset;
  if (x < left || x > right || y < top || y > bottom) return false;

  if (expanded && sidebarDocked) {
    // Preserve the full rounded badge footprint as it opens into a card with
    // larger corners, including while the native window is still resizing.
    const side = 52 * scaleFactor;
    const radius = 14 * scaleFactor;
    const badgeTop = height - side;
    if (x <= side && y >= badgeTop) {
      const nearestX = Math.max(radius, Math.min(x, side - radius));
      const nearestY = Math.max(badgeTop + radius, Math.min(y, height - radius));
      if ((x - nearestX) ** 2 + (y - nearestY) ** 2 <= radius ** 2) return true;
    }
  }

  // Native resize delivery can lag behind the frontend by a frame. Clamp the
  // radius to the measured window instead of rejecting a still-small window.
  const radius = Math.min((expanded ? 38 : sidebarDocked ? 14 : 28) * scaleFactor, (right - left) / 2, (bottom - top) / 2);
  const nearestX = Math.max(left + radius, Math.min(x, right - radius));
  const nearestY = Math.max(top + radius, Math.min(y, bottom - radius));
  return (x - nearestX) ** 2 + (y - nearestY) ** 2 <= radius ** 2;
}
