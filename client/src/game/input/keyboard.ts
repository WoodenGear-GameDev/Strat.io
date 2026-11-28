import type { InputController } from "./controller.ts";

const CAMERA_PAN_KEYS = new Set([
  "KeyW",
  "KeyA",
  "KeyS",
  "KeyD",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);

export function bindKeyboardEvents(ctrl: InputController): void {
  window.addEventListener("keydown", (e) => {
    const tag = document.activeElement?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;

    if (ctrl.state.playerId === null) return;
    if (CAMERA_PAN_KEYS.has(e.code)) {
      e.preventDefault();
      ctrl.pressedMoveKeys.add(e.code);
      return;
    }
    if (e.code === "Digit1") ctrl.toggleBuild(1);
    else if (e.code === "Digit2") ctrl.toggleBuild(2);
    else if (e.code === "Digit3") ctrl.toggleBuild(3);
    else if (e.code === "Digit4") ctrl.toggleBuild(5);
    else if (
      e.code === "Digit5" ||
      e.code === "Digit6" ||
      e.code === "Digit7" ||
      e.code === "Digit8" ||
      e.code === "Digit9"
    ) {
      const idx = Number(e.code.slice(-1)) - 5;
      const combat = ctrl.state.getSquadSummaries().filter((s) => s.squad > 0 && s.count > 0);
      if (combat[idx]) {
        ctrl.selectSquad(combat[idx].squad);
      } else {
        ctrl.callbacks.onToast(`Нет отряда #${idx + 1}`);
      }
    } else if (e.code === "Backquote") {
      e.preventDefault();
      ctrl.selectSquad(0);
    } else if (e.code === "KeyQ") {
      e.preventDefault();
      ctrl.cycleSquad(-1);
    } else if (e.code === "KeyE" || e.code === "Tab") {
      e.preventDefault();
      ctrl.cycleSquad(1);
    } else if (e.code === "KeyU") ctrl.toggleActionMode("upgrade");
    else if (e.code === "KeyX") ctrl.toggleActionMode("sell");
    else if (e.code === "KeyT") ctrl.toggleTeleportMode();
    else if (e.code === "Space") {
      e.preventDefault();
      ctrl.centerOnActiveFocus();
    } else if (e.code === "Escape") {
      ctrl.cancelModes();
    }
  });

  window.addEventListener("keyup", (e) => {
    ctrl.pressedMoveKeys.delete(e.code);
  });

  window.addEventListener("blur", () => {
    ctrl.pressedMoveKeys.clear();
  });
}
