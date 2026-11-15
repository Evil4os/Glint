export interface PointerGesture {
  durationMs: number;
  distancePx: number;
  dragStarted: boolean;
}

export const MAX_CLICK_DURATION_MS = 250;
export const DRAG_INTENT_DISTANCE_PX = 4;

export function shouldStartDragging(distancePx: number): boolean {
  return distancePx >= DRAG_INTENT_DISTANCE_PX;
}

export function shouldActivateAfterPointerGesture(gesture: PointerGesture): boolean {
  return !gesture.dragStarted
    && gesture.durationMs < MAX_CLICK_DURATION_MS
    && gesture.distancePx < DRAG_INTENT_DISTANCE_PX;
}
