import type { InputController } from "./controller.ts";

export function bindPointerEvents(ctrl: InputController): void {
  let mousePanActive = false;
  let mousePanMoved = false;
  let mouseDownX = 0;
  let mouseDownY = 0;
  let lastMouseX = 0;
  let lastMouseY = 0;
  let mouseDownButton = -1;

  ctrl.canvas.addEventListener("contextmenu", (e) => e.preventDefault());

  ctrl.canvas.addEventListener("mousemove", (e) => {
    const w = ctrl.renderer.screenToWorld(e.clientX, e.clientY);
    ctrl.state.hoverWorldX = w.x;
    ctrl.state.hoverWorldY = w.y;
    ctrl.state.hasPointerHover = true;
    if (mousePanActive) {
      const dx = e.clientX - lastMouseX;
      const dy = e.clientY - lastMouseY;
      if (!mousePanMoved && Math.hypot(e.clientX - mouseDownX, e.clientY - mouseDownY) > 6) {
        mousePanMoved = true;
      }
      if (mousePanMoved) {
        ctrl.renderer.panBy(dx, dy);
      }
      lastMouseX = e.clientX;
      lastMouseY = e.clientY;
      return;
    }
    if (
      ctrl.dragPainting &&
      ctrl.state.playerId !== null &&
      (ctrl.state.buildKind !== null || ctrl.state.actionMode !== null)
    ) {
      ctrl.tryDragActionAtWorld(w.x, w.y, false);
    }
  });

  ctrl.canvas.addEventListener("mouseleave", () => {
    ctrl.state.hasPointerHover = false;
    ctrl.dragPainting = false;
    mousePanActive = false;
    mousePanMoved = false;
    ctrl.lastPaintedSlotKey = null;
    ctrl.lastActedBuildingId = null;
  });

  ctrl.canvas.addEventListener("mousedown", (e) => {
    if (ctrl.state.playerId === null) return;
    if (e.button === 1) {
      e.preventDefault();
      mousePanActive = true;
      mousePanMoved = true;
      mouseDownButton = 1;
      mouseDownX = e.clientX;
      mouseDownY = e.clientY;
      lastMouseX = e.clientX;
      lastMouseY = e.clientY;
    } else if (e.button === 0) {
      const w = ctrl.renderer.screenToWorld(e.clientX, e.clientY);
      const inBuildArea = ctrl.isPointInBaseBuildArea(w.x, w.y);
      if (inBuildArea && (ctrl.state.buildKind !== null || ctrl.state.actionMode !== null)) {
        const startedDrag = ctrl.handlePrimaryAction(e.clientX, e.clientY, false);
        if (startedDrag) {
          ctrl.dragPainting = true;
        }
      } else {
        mousePanActive = true;
        mousePanMoved = false;
        mouseDownButton = 0;
        mouseDownX = e.clientX;
        mouseDownY = e.clientY;
        lastMouseX = e.clientX;
        lastMouseY = e.clientY;
      }
    } else if (e.button === 2) {
      if (
        ctrl.state.buildKind !== null ||
        ctrl.state.actionMode !== null ||
        ctrl.state.teleportSelectMode
      ) {
        ctrl.cancelModes();
      } else {
        ctrl.sendRally(e.clientX, e.clientY);
      }
    }
  });

  window.addEventListener("mouseup", (e) => {
    if (e.button === 0) {
      if (mousePanActive && mouseDownButton === 0) {
        const wasClick = !mousePanMoved;
        mousePanActive = false;
        mousePanMoved = false;
        mouseDownButton = -1;
        if (wasClick && ctrl.state.playerId !== null) {
          ctrl.handlePrimaryAction(e.clientX, e.clientY, false);
        }
      }
      ctrl.dragPainting = false;
      ctrl.lastPaintedSlotKey = null;
      ctrl.lastActedBuildingId = null;
    } else if (e.button === 1 && mouseDownButton === 1) {
      mousePanActive = false;
      mousePanMoved = false;
      mouseDownButton = -1;
    }
  });

  ctrl.canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      const factor = e.deltaY < 0 ? 1.1 : 0.9;
      ctrl.renderer.zoomAt(factor, e.clientX, e.clientY);
    },
    { passive: false },
  );
}
