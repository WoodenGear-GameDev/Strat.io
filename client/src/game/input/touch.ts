import type { InputController } from "./controller.ts";

export function bindTouchEvents(ctrl: InputController): void {
  const touchPoints = new Map<number, { x: number; y: number }>();
  let touchStartX = 0;
  let touchStartY = 0;
  let touchMoved = false;
  let lastPinchDist: number | null = null;
  let lastPinchMid: { x: number; y: number } | null = null;
  let wasPinching = false;

  const getPinchDistance = (): number => {
    const pts = Array.from(touchPoints.values());
    if (pts.length < 2) return 0;
    return Math.hypot(pts[0].x - pts[1].x, pts[0].y - pts[1].y);
  };

  ctrl.canvas.addEventListener(
    "touchstart",
    (e) => {
      e.preventDefault();
      ctrl.state.hasPointerHover = false;
      for (const t of Array.from(e.changedTouches)) {
        touchPoints.set(t.identifier, { x: t.clientX, y: t.clientY });
      }

      if (touchPoints.size === 1) {
        const only = Array.from(touchPoints.values())[0];
        touchStartX = only.x;
        touchStartY = only.y;
        touchMoved = false;
        wasPinching = false;
        lastPinchDist = null;
        const w = ctrl.renderer.screenToWorld(only.x, only.y);
        ctrl.touchPainting =
          ctrl.state.playerId !== null &&
          (ctrl.state.buildKind !== null || ctrl.state.actionMode !== null) &&
          ctrl.isPointInBaseBuildArea(w.x, w.y);
      } else if (touchPoints.size === 2) {
        wasPinching = true;
        ctrl.touchPainting = false;
        lastPinchDist = getPinchDistance();
        const pts = Array.from(touchPoints.values());
        lastPinchMid = {
          x: (pts[0].x + pts[1].x) / 2,
          y: (pts[0].y + pts[1].y) / 2,
        };
      }
    },
    { passive: false },
  );

  ctrl.canvas.addEventListener(
    "touchmove",
    (e) => {
      e.preventDefault();
      if (touchPoints.size === 1 && !wasPinching) {
        const t = e.changedTouches[0];
        const prev = touchPoints.get(t.identifier);
        if (prev) {
          if (Math.hypot(t.clientX - touchStartX, t.clientY - touchStartY) > 10) {
            touchMoved = true;
          }
          if (touchMoved) {
            if (
              ctrl.touchPainting &&
              ctrl.state.playerId !== null &&
              (ctrl.state.buildKind !== null || ctrl.state.actionMode !== null)
            ) {
              const w = ctrl.renderer.screenToWorld(t.clientX, t.clientY);
              ctrl.state.hoverWorldX = w.x;
              ctrl.state.hoverWorldY = w.y;
              ctrl.state.hasPointerHover = true;
              ctrl.tryDragActionAtWorld(w.x, w.y, true);
            } else {
              ctrl.renderer.panBy(t.clientX - prev.x, t.clientY - prev.y);
            }
          }
          touchPoints.set(t.identifier, { x: t.clientX, y: t.clientY });
        }
      } else if (touchPoints.size === 2) {
        ctrl.state.hasPointerHover = false;
        for (const t of Array.from(e.changedTouches)) {
          if (touchPoints.has(t.identifier)) {
            touchPoints.set(t.identifier, { x: t.clientX, y: t.clientY });
          }
        }
        const dist = getPinchDistance();
        const pts = Array.from(touchPoints.values());
        const midX = (pts[0].x + pts[1].x) / 2;
        const midY = (pts[0].y + pts[1].y) / 2;
        if (lastPinchMid) {
          ctrl.renderer.panBy(midX - lastPinchMid.x, midY - lastPinchMid.y);
        }
        if (lastPinchDist !== null && dist > 0) {
          const factor = dist / lastPinchDist;
          ctrl.renderer.zoomAt(factor, midX, midY);
        }
        lastPinchDist = dist;
        lastPinchMid = { x: midX, y: midY };
      }
    },
    { passive: false },
  );

  ctrl.canvas.addEventListener(
    "touchend",
    (e) => {
      e.preventDefault();
      const wasSingleTap = touchPoints.size === 1 && !touchMoved && !wasPinching;
      const endedTouch = e.changedTouches[0];

      for (const t of Array.from(e.changedTouches)) {
        touchPoints.delete(t.identifier);
      }
      if (touchPoints.size === 0) {
        ctrl.touchPainting = false;
        ctrl.lastPaintedSlotKey = null;
        ctrl.lastActedBuildingId = null;
        ctrl.state.hasPointerHover = false;
      }
      if (touchPoints.size < 2) {
        lastPinchDist = null;
        lastPinchMid = null;
      }

      if (wasSingleTap && endedTouch && ctrl.state.playerId !== null) {
        ctrl.handlePrimaryAction(endedTouch.clientX, endedTouch.clientY, true);
      }
    },
    { passive: false },
  );

  ctrl.canvas.addEventListener("touchcancel", () => {
    touchPoints.clear();
    lastPinchDist = null;
    lastPinchMid = null;
    ctrl.touchPainting = false;
    ctrl.lastPaintedSlotKey = null;
    ctrl.lastActedBuildingId = null;
    ctrl.state.hasPointerHover = false;
  });
}
