import { getBuildingConfigByKind, getBuildingRadius } from "./balance.ts";
import {
  encodeArmyCommand,
  encodeBuild,
  encodeSellBuilding,
  encodeTeleportZone,
  encodeUpgradeBuilding,
} from "./protocol.ts";
import type { CanvasRenderer } from "./renderer.ts";
import { beaconExtRadius, type ClientGameState } from "./state.ts";
import type { Transport } from "../transport/transport.ts";

export interface InputCallbacks {
  onUiChange: () => void;
  onToast: (text: string) => void;
}

export class InputController {
  private dragPainting = false;
  private touchPainting = false;
  private lastPaintedSlotKey: string | null = null;
  private lastActedBuildingId: number | null = null;

  private touchPoints = new Map<number, { x: number; y: number }>();
  private touchStartX = 0;
  private touchStartY = 0;
  private touchMoved = false;
  private lastPinchDist: number | null = null;
  private lastPinchMid: { x: number; y: number } | null = null;
  private wasPinching = false;

  private mousePanActive = false;
  private mousePanMoved = false;
  private mouseDownX = 0;
  private mouseDownY = 0;
  private lastMouseX = 0;
  private lastMouseY = 0;
  private mouseDownButton = -1;

  constructor(
    private canvas: HTMLCanvasElement,
    private state: ClientGameState,
    private renderer: CanvasRenderer,
    private transport: Transport,
    private callbacks: InputCallbacks,
  ) {
    this.bindEvents();
  }

  private bindEvents(): void {
    window.addEventListener("keydown", (e) => {
      const tag = document.activeElement?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;

      if (this.state.playerId === null) return;
      if (e.code === "Digit1") this.toggleBuild(1);
      else if (e.code === "Digit2") this.toggleBuild(2);
      else if (e.code === "Digit3") this.toggleBuild(3);
      else if (e.code === "Digit4") this.toggleBuild(5);
      else if (e.code === "Digit5" || e.code === "Digit6" || e.code === "Digit7" || e.code === "Digit8" || e.code === "Digit9") {
        const idx = Number(e.code.slice(-1)) - 5;
        const combat = this.state.getSquadSummaries().filter((s) => s.squad > 0 && s.count > 0);
        if (combat[idx]) {
          this.selectSquad(combat[idx].squad);
        } else {
          this.callbacks.onToast(`Нет отряда #${idx + 1}`);
        }
      } else if (e.code === "Backquote") {
        e.preventDefault();
        this.selectSquad(0);
      } else if (e.code === "KeyQ") {
        e.preventDefault();
        this.cycleSquad(-1);
      } else if (e.code === "KeyE" || e.code === "Tab") {
        e.preventDefault();
        this.cycleSquad(1);
      } else if (e.code === "KeyU") this.toggleActionMode("upgrade");
      else if (e.code === "KeyX") this.toggleActionMode("sell");
      else if (e.code === "KeyT") this.toggleTeleportMode();
      else if (e.code === "Space") {
        e.preventDefault();
        this.centerOnBase();
      } else if (e.code === "Escape") {
        this.cancelModes();
      }
    });

    this.canvas.addEventListener("contextmenu", (e) => e.preventDefault());

    this.canvas.addEventListener("mousemove", (e) => {
      const w = this.renderer.screenToWorld(e.clientX, e.clientY);
      this.state.hoverWorldX = w.x;
      this.state.hoverWorldY = w.y;
      this.state.hasPointerHover = true;
      if (this.mousePanActive) {
        const dx = e.clientX - this.lastMouseX;
        const dy = e.clientY - this.lastMouseY;
        if (!this.mousePanMoved && Math.hypot(e.clientX - this.mouseDownX, e.clientY - this.mouseDownY) > 6) {
          this.mousePanMoved = true;
        }
        if (this.mousePanMoved) {
          this.renderer.panBy(dx, dy);
        }
        this.lastMouseX = e.clientX;
        this.lastMouseY = e.clientY;
        return;
      }
      if (
        this.dragPainting &&
        this.state.playerId !== null &&
        (this.state.buildKind !== null || this.state.actionMode !== null)
      ) {
        this.tryDragActionAtWorld(w.x, w.y, false);
      }
    });

    this.canvas.addEventListener("mouseleave", () => {
      this.state.hasPointerHover = false;
      this.dragPainting = false;
      this.mousePanActive = false;
      this.mousePanMoved = false;
      this.lastPaintedSlotKey = null;
      this.lastActedBuildingId = null;
    });

    this.canvas.addEventListener("mousedown", (e) => {
      if (this.state.playerId === null) return;
      if (e.button === 1) {
        e.preventDefault();
        this.mousePanActive = true;
        this.mousePanMoved = true;
        this.mouseDownButton = 1;
        this.mouseDownX = e.clientX;
        this.mouseDownY = e.clientY;
        this.lastMouseX = e.clientX;
        this.lastMouseY = e.clientY;
      } else if (e.button === 0) {
        const w = this.renderer.screenToWorld(e.clientX, e.clientY);
        const inBuildArea = this.isPointInBaseBuildArea(w.x, w.y);
        if (
          inBuildArea &&
          (this.state.buildKind !== null || this.state.actionMode !== null)
        ) {
          const startedDrag = this.handlePrimaryAction(e.clientX, e.clientY, false);
          if (startedDrag) {
            this.dragPainting = true;
          }
        } else {
          this.mousePanActive = true;
          this.mousePanMoved = false;
          this.mouseDownButton = 0;
          this.mouseDownX = e.clientX;
          this.mouseDownY = e.clientY;
          this.lastMouseX = e.clientX;
          this.lastMouseY = e.clientY;
        }
      } else if (e.button === 2) {
        if (
          this.state.buildKind !== null ||
          this.state.actionMode !== null ||
          this.state.teleportSelectMode
        ) {
          this.cancelModes();
        } else {
          this.sendRally(e.clientX, e.clientY);
        }
      }
    });

    window.addEventListener("mouseup", (e) => {
      if (e.button === 0) {
        if (this.mousePanActive && this.mouseDownButton === 0) {
          const wasClick = !this.mousePanMoved;
          this.mousePanActive = false;
          this.mousePanMoved = false;
          this.mouseDownButton = -1;
          if (wasClick && this.state.playerId !== null) {
            this.handlePrimaryAction(e.clientX, e.clientY, false);
          }
        }
        this.dragPainting = false;
        this.lastPaintedSlotKey = null;
        this.lastActedBuildingId = null;
      } else if (e.button === 1 && this.mouseDownButton === 1) {
        this.mousePanActive = false;
        this.mousePanMoved = false;
        this.mouseDownButton = -1;
      }
    });

    this.canvas.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        const factor = e.deltaY < 0 ? 1.1 : 0.9;
        this.renderer.zoomAt(factor, e.clientX, e.clientY);
      },
      { passive: false },
    );

    // Touch controls for tablets and phones
    this.canvas.addEventListener(
      "touchstart",
      (e) => {
        e.preventDefault();
        this.state.hasPointerHover = false;
        for (const t of Array.from(e.changedTouches)) {
          this.touchPoints.set(t.identifier, { x: t.clientX, y: t.clientY });
        }

        if (this.touchPoints.size === 1) {
          const only = Array.from(this.touchPoints.values())[0];
          this.touchStartX = only.x;
          this.touchStartY = only.y;
          this.touchMoved = false;
          this.wasPinching = false;
          this.lastPinchDist = null;
          const w = this.renderer.screenToWorld(only.x, only.y);
          this.touchPainting =
            this.state.playerId !== null &&
            (this.state.buildKind !== null || this.state.actionMode !== null) &&
            this.isPointInBaseBuildArea(w.x, w.y);
        } else if (this.touchPoints.size === 2) {
          this.wasPinching = true;
          this.touchPainting = false;
          this.lastPinchDist = this.getPinchDistance();
          const pts = Array.from(this.touchPoints.values());
          this.lastPinchMid = {
            x: (pts[0].x + pts[1].x) / 2,
            y: (pts[0].y + pts[1].y) / 2,
          };
        }
      },
      { passive: false },
    );

    this.canvas.addEventListener(
      "touchmove",
      (e) => {
        e.preventDefault();
        if (this.touchPoints.size === 1 && !this.wasPinching) {
          const t = e.changedTouches[0];
          const prev = this.touchPoints.get(t.identifier);
          if (prev) {
            if (Math.hypot(t.clientX - this.touchStartX, t.clientY - this.touchStartY) > 10) {
              this.touchMoved = true;
            }
            if (this.touchMoved) {
              if (
                this.touchPainting &&
                this.state.playerId !== null &&
                (this.state.buildKind !== null || this.state.actionMode !== null)
              ) {
                const w = this.renderer.screenToWorld(t.clientX, t.clientY);
                this.state.hoverWorldX = w.x;
                this.state.hoverWorldY = w.y;
                this.state.hasPointerHover = true;
                this.tryDragActionAtWorld(w.x, w.y, true);
              } else {
                this.renderer.panBy(t.clientX - prev.x, t.clientY - prev.y);
              }
            }
            this.touchPoints.set(t.identifier, { x: t.clientX, y: t.clientY });
          }
        } else if (this.touchPoints.size === 2) {
          this.state.hasPointerHover = false;
          for (const t of Array.from(e.changedTouches)) {
            if (this.touchPoints.has(t.identifier)) {
              this.touchPoints.set(t.identifier, { x: t.clientX, y: t.clientY });
            }
          }
          const dist = this.getPinchDistance();
          const pts = Array.from(this.touchPoints.values());
          const midX = (pts[0].x + pts[1].x) / 2;
          const midY = (pts[0].y + pts[1].y) / 2;
          if (this.lastPinchMid) {
            this.renderer.panBy(midX - this.lastPinchMid.x, midY - this.lastPinchMid.y);
          }
          if (this.lastPinchDist !== null && dist > 0) {
            const factor = dist / this.lastPinchDist;
            this.renderer.zoomAt(factor, midX, midY);
          }
          this.lastPinchDist = dist;
          this.lastPinchMid = { x: midX, y: midY };
        }
      },
      { passive: false },
    );

    this.canvas.addEventListener(
      "touchend",
      (e) => {
        e.preventDefault();
        const wasSingleTap =
          this.touchPoints.size === 1 && !this.touchMoved && !this.wasPinching;
        const endedTouch = e.changedTouches[0];

        for (const t of Array.from(e.changedTouches)) {
          this.touchPoints.delete(t.identifier);
        }
        if (this.touchPoints.size === 0) {
          this.touchPainting = false;
          this.lastPaintedSlotKey = null;
          this.lastActedBuildingId = null;
          this.state.hasPointerHover = false;
        }
        if (this.touchPoints.size < 2) {
          this.lastPinchDist = null;
          this.lastPinchMid = null;
        }

        if (wasSingleTap && endedTouch && this.state.playerId !== null) {
          this.handlePrimaryAction(endedTouch.clientX, endedTouch.clientY, true);
        }
      },
      { passive: false },
    );

    this.canvas.addEventListener("touchcancel", () => {
      this.touchPoints.clear();
      this.lastPinchDist = null;
      this.lastPinchMid = null;
      this.touchPainting = false;
      this.lastPaintedSlotKey = null;
      this.lastActedBuildingId = null;
      this.state.hasPointerHover = false;
    });
  }

  private getPinchDistance(): number {
    const pts = Array.from(this.touchPoints.values());
    if (pts.length < 2) return 0;
    return Math.hypot(pts[0].x - pts[1].x, pts[0].y - pts[1].y);
  }

  private isPointInBaseBuildArea(wx: number, wy: number): boolean {
    if (this.state.playerId === null) return false;
    const ownZone = this.state.zones.get(this.state.playerId);
    if (!ownZone) return false;
    if (Math.hypot(wx - ownZone.x, wy - ownZone.y) <= ownZone.radius + 36) {
      return true;
    }
    for (const b of this.state.buildings.values()) {
      if (b.ownerId === this.state.playerId && b.kind === 5) {
        const bx = ownZone.x + b.relX;
        const by = ownZone.y + b.relY;
        if (Math.hypot(wx - bx, wy - by) <= beaconExtRadius(b.level) + 28) {
          return true;
        }
      }
    }
    return false;
  }

  toggleBuild(kind: number): void {
    if (kind === 5 && this.state.getHqLevel() < 3) {
      this.callbacks.onToast("Маяк доступен с Ядра Ур. 3");
      return;
    }
    this.state.actionMode = null;
    this.state.rallySelectMode = false;
    this.state.teleportSelectMode = false;
    this.state.pendingTeleport = null;
    this.state.selectedBuildingId = null;
    this.lastPaintedSlotKey = null;
    this.lastActedBuildingId = null;
    this.state.buildKind = this.state.buildKind === kind ? null : kind;
    this.callbacks.onUiChange();
  }

  toggleActionMode(mode: "upgrade" | "sell"): void {
    this.state.buildKind = null;
    this.state.rallySelectMode = false;
    this.state.teleportSelectMode = false;
    this.state.pendingTeleport = null;
    this.state.selectedBuildingId = null;
    this.lastPaintedSlotKey = null;
    this.lastActedBuildingId = null;
    this.state.actionMode = this.state.actionMode === mode ? null : mode;
    this.callbacks.onUiChange();
  }

  toggleRallyMode(): void {
    this.state.buildKind = null;
    this.state.actionMode = null;
    this.state.teleportSelectMode = false;
    this.state.pendingTeleport = null;
    this.state.selectedBuildingId = null;
    this.state.rallySelectMode = !this.state.rallySelectMode;
    this.callbacks.onUiChange();
  }

  toggleTeleportMode(): void {
    this.state.buildKind = null;
    this.state.actionMode = null;
    this.state.rallySelectMode = false;
    this.state.pendingTeleport = null;
    this.state.selectedBuildingId = null;
    this.state.teleportSelectMode = !this.state.teleportSelectMode;
    if (this.state.teleportSelectMode) {
      if (this.state.getDroneFocusTarget(1) !== null) {
        this.state.cameraAnchor = "drones";
      } else {
        this.callbacks.onToast("Нет дронов — отправьте дронов для выбора дальнего места");
      }
    }
    this.callbacks.onUiChange();
  }

  confirmPendingAction(): void {
    if (this.state.playerId === null) return;
    if (this.state.pendingTeleport && this.state.teleportSelectMode) {
      const { x, y } = this.state.pendingTeleport;
      if (!this.state.hasVisionAt(x, y)) {
        this.state.pendingTeleport = null;
        this.callbacks.onToast("Телепорт возможен только в зоне взора дронов или базы");
        this.callbacks.onUiChange();
        return;
      }
      this.transport.send(encodeTeleportZone(x, y));
      this.state.pendingTeleport = null;
      this.state.teleportSelectMode = false;
      this.state.cameraAnchor = "hq";
      this.callbacks.onUiChange();
    }
  }

  selectSquad(squadId: number): void {
    if (this.state.playerId === null) return;
    this.state.lostSquadHold = null;
    this.state.buildKind = null;
    this.state.actionMode = null;
    this.state.teleportSelectMode = false;
    this.state.pendingTeleport = null;
    if (squadId === 0) {
      this.state.activeSquad = 0;
      this.state.cameraAnchor = "hq";
      this.state.rallySelectMode = false;
      this.transport.send(encodeArmyCommand(0, 0, 0, 0));
      const ownZone = this.state.zones.get(this.state.playerId);
      if (ownZone) this.renderer.flyTo(ownZone.x, ownZone.y);
    } else {
      const summaries = this.state.getSquadSummaries();
      const target = summaries.find((s) => s.squad === squadId && s.count > 0);
      if (target) {
        this.state.activeSquad = squadId;
        this.state.cameraAnchor = "drones";
        this.state.rallySelectMode = true;
        const focus = this.state.getDroneFocusTarget(1);
        if (focus) this.renderer.flyTo(focus.x, focus.y);
      } else {
        this.state.activeSquad = 0;
        this.state.cameraAnchor = "hq";
        this.state.rallySelectMode = false;
        this.transport.send(encodeArmyCommand(0, 0, 0, 0));
        const ownZone = this.state.zones.get(this.state.playerId);
        if (ownZone) this.renderer.flyTo(ownZone.x, ownZone.y);
      }
    }
    this.callbacks.onUiChange();
  }

  cycleSquad(dir: 1 | -1 = 1): void {
    if (this.state.playerId === null) return;
    const combat = this.state.getSquadSummaries().filter((s) => s.squad > 0 && s.count > 0);
    if (combat.length === 0) {
      this.selectSquad(0);
      if ((this.state.getSquadSummaries()[0]?.count ?? 0) === 0) {
        this.callbacks.onToast("Нет активных дронов");
      }
      return;
    }
    const order = [0, ...combat.map((s) => s.squad)];
    const cur = this.state.cameraAnchor === "hq" ? 0 : this.state.activeSquad;
    const idx = Math.max(0, order.indexOf(cur));
    const nextIdx = (idx + dir + order.length) % order.length;
    this.selectSquad(order[nextIdx]);
  }

  centerOnBase(): void {
    if (this.state.playerId === null) return;
    if (this.state.cameraAnchor === "hq") {
      const summaries = this.state.getSquadSummaries();
      const combat = summaries.filter((s) => s.squad > 0 && s.count > 0);
      if (combat.length > 0) {
        this.selectSquad(combat[0].squad);
        return;
      }
      const garrison = summaries.find((s) => s.squad === 0)?.count ?? 0;
      const focus = this.state.getDroneFocusTarget(1);
      if (garrison > 0 || focus !== null) {
        this.state.buildKind = null;
        this.state.actionMode = null;
        this.state.teleportSelectMode = false;
        this.state.pendingTeleport = null;
        this.state.cameraAnchor = "drones";
        this.state.rallySelectMode = true;
        if (focus) this.renderer.flyTo(focus.x, focus.y);
      } else {
        this.callbacks.onToast("Нет активных дронов");
      }
    } else {
      this.state.activeSquad = 0;
      this.state.lostSquadHold = null;
      this.state.cameraAnchor = "hq";
      this.state.rallySelectMode = false;
      this.transport.send(encodeArmyCommand(0, 0, 0, 0));
      const ownZone = this.state.zones.get(this.state.playerId);
      if (ownZone) this.renderer.flyTo(ownZone.x, ownZone.y);
    }
    this.callbacks.onUiChange();
  }

  toggleReinforce(squadId: number): void {
    if (this.state.playerId === null) return;
    if (squadId <= 0) {
      this.state.reinforceSquad = 0;
      this.transport.send(encodeArmyCommand(0, 2, 0, 0));
    } else {
      const next = this.state.reinforceSquad === squadId ? 0 : squadId;
      this.state.reinforceSquad = next;
      this.transport.send(encodeArmyCommand(squadId, 2, 0, 0));
      this.callbacks.onToast(
        next > 0
          ? `Автоподкрепление → отряд #${next}`
          : "Автоподкрепление выключено",
      );
    }
    this.callbacks.onUiChange();
  }

  cancelModes(): void {
    this.state.buildKind = null;
    this.state.actionMode = null;
    this.state.rallySelectMode = false;
    this.state.teleportSelectMode = false;
    this.state.pendingTeleport = null;
    this.state.selectedBuildingId = null;
    this.dragPainting = false;
    this.touchPainting = false;
    this.lastPaintedSlotKey = null;
    this.lastActedBuildingId = null;
    this.callbacks.onUiChange();
  }

  private findOwnBuildingAt(wx: number, wy: number, isTouch: boolean): number | null {
    if (this.state.playerId === null) return null;
    const zone = this.state.zones.get(this.state.playerId);
    if (!zone) return null;
    const pad = isTouch ? 4 : 2;
    let foundId: number | null = null;
    let bestDist = Infinity;
    for (const b of this.state.buildings.values()) {
      if (b.ownerId !== this.state.playerId) continue;
      const r = getBuildingRadius(this.state.balance, b.kind);
      const bx = zone.x + b.relX;
      const by = zone.y + b.relY;
      const d = Math.hypot(wx - bx, wy - by);
      if (d <= r + pad && d < bestDist) {
        bestDist = d;
        foundId = b.id;
      }
    }
    return foundId;
  }

  private tryDragActionAtWorld(wx: number, wy: number, isTouch: boolean): void {
    if (this.state.actionMode !== null) {
      this.tryActionOnBuildingAtWorld(wx, wy, isTouch, false);
    } else if (this.state.buildKind !== null) {
      this.tryPaintBuildAtWorld(wx, wy, false);
    }
  }

  private tryActionOnBuildingAtWorld(
    wx: number,
    wy: number,
    isTouch: boolean,
    isDirectClick: boolean,
  ): void {
    const mode = this.state.actionMode;
    if (!mode) return;
    const hitId = this.findOwnBuildingAt(wx, wy, isTouch);
    if (hitId === null) return;
    if (!isDirectClick && hitId === this.lastActedBuildingId) return;

    const b = this.state.buildings.get(hitId);
    if (!b) return;

    if (mode === "sell") {
      if (b.kind === 0) {
        if (isDirectClick) this.callbacks.onToast("Ядро нельзя снести");
        return;
      }
      this.lastActedBuildingId = hitId;
      this.transport.send(encodeSellBuilding(hitId));
      return;
    }

    // Upgrade mode: on drag, silently skip buildings that are maxed, HQ-locked, or unaffordable
    const cfg = getBuildingConfigByKind(this.state.balance, b.kind);
    const nextLvl = cfg?.levels[b.level];
    const hqLevel = this.state.getHqLevel();
    const blockedByHq = b.kind !== 0 && b.level >= hqLevel;
    const blockedByCombat = this.state.isInCombat() && b.kind !== 0 && b.level + 1 >= 3;
    const affordable =
      !!nextLvl &&
      this.state.energy >= (nextLvl.cost ?? 0) &&
      this.state.trophies >= (nextLvl.trophy_cost ?? 0);

    if (!isDirectClick && (!nextLvl || blockedByHq || blockedByCombat || !affordable)) {
      return;
    }
    this.lastActedBuildingId = hitId;
    this.transport.send(encodeUpgradeBuilding(hitId));
  }

  private tryPaintBuildAtWorld(wx: number, wy: number, showFeedbackToast: boolean): void {
    if (this.state.playerId === null || this.state.buildKind === null) return;
    const zone = this.state.zones.get(this.state.playerId);
    if (!zone) return;

    const rawRelX = wx - zone.x;
    const rawRelY = wy - zone.y;
    const preview = this.state.resolveBuildPreview(this.state.buildKind, rawRelX, rawRelY, false);
    if (!preview) return;

    if (!preview.footprintValid) {
      if (showFeedbackToast) {
        this.callbacks.onToast(
          preview.overlapsExisting ? "Место занято другой постройкой" : "Стройка за пределами зоны",
        );
      }
      return;
    }

    const relX = preview.centerRel.x;
    const relY = preview.centerRel.y;
    const slotKey = `${this.state.buildKind}:${relX.toFixed(1)},${relY.toFixed(1)}`;
    if (slotKey === this.lastPaintedSlotKey) return;

    if (this.state.buildKind === 5 && this.state.isInCombat()) {
      if (showFeedbackToast) {
        this.callbacks.onToast("В бою нельзя строить Маяки");
      }
      return;
    }

    if (!preview.affordable) {
      if (showFeedbackToast) {
        const tCost = this.state.dynamicBuildTrophyCost(this.state.buildKind);
        this.callbacks.onToast(
          this.state.trophies < tCost ? "Недостаточно Трофеев" : "Недостаточно Энергии",
        );
      }
      return;
    }

    this.lastPaintedSlotKey = slotKey;
    this.transport.send(encodeBuild(this.state.buildKind, relX, relY));
  }

  private handlePrimaryAction(screenX: number, screenY: number, isTouch: boolean): boolean {
    if (this.state.playerId === null) return false;

    const minimapHit = this.renderer.hitTestMinimap(screenX, screenY, this.state.mapRadius);
    if (minimapHit) {
      if (this.state.teleportSelectMode) {
        this.issueSquadOrder(minimapHit.x, minimapHit.y, true);
        if (this.state.getDroneFocusTarget(1) !== null) {
          this.state.cameraAnchor = "drones";
          this.callbacks.onToast("Дроны летят в точку — выберите место в их взоре");
        } else {
          this.callbacks.onToast("Выпустите дронов — они полетят на разведку");
        }
        this.state.pendingTeleport = null;
        this.callbacks.onUiChange();
        return false;
      }
      if (this.state.rallySelectMode) {
        this.issueSquadOrder(minimapHit.x, minimapHit.y, false);
        this.callbacks.onUiChange();
      } else {
        this.centerOnBase();
      }
      return false;
    }

    const w = this.renderer.screenToWorld(screenX, screenY);

    if (this.state.actionMode !== null) {
      this.state.selectedBuildingId = null;
      this.state.pendingTeleport = null;
      this.tryActionOnBuildingAtWorld(w.x, w.y, isTouch, true);
      this.callbacks.onUiChange();
      return true;
    }

    // Selecting any existing own building works regardless of the active bottom dock mode.
    const hitBuildingId = this.findOwnBuildingAt(w.x, w.y, isTouch);
    if (hitBuildingId !== null) {
      this.state.selectedBuildingId = hitBuildingId;
      this.state.pendingTeleport = null;
      this.callbacks.onUiChange();
      return false;
    }

    const hadSelection = this.state.selectedBuildingId !== null;
    this.state.selectedBuildingId = null;

    if (this.state.teleportSelectMode) {
      if (!this.state.hasVisionAt(w.x, w.y)) {
        this.callbacks.onToast("Телепорт возможен только в зоне взора (у дронов)");
        this.callbacks.onUiChange();
        return false;
      }
      if (isTouch) {
        this.state.pendingTeleport = { x: w.x, y: w.y };
        this.callbacks.onUiChange();
        return false;
      }
      this.transport.send(encodeTeleportZone(w.x, w.y));
      this.state.teleportSelectMode = false;
      this.state.activeSquad = 0;
      this.state.lostSquadHold = null;
      this.state.cameraAnchor = "hq";
      this.callbacks.onUiChange();
      return false;
    }

    if (this.state.buildKind !== null) {
      this.tryPaintBuildAtWorld(w.x, w.y, !hadSelection);
      if (hadSelection) this.callbacks.onUiChange();
      return true;
    }

    if (this.state.rallySelectMode) {
      this.sendRally(screenX, screenY);
      return false;
    }

    this.callbacks.onUiChange();
    return false;
  }

  private issueSquadOrder(wx: number, wy: number, fallbackToCombat: boolean): void {
    const summaries = this.state.getSquadSummaries();
    let targetSquad = this.state.activeSquad;
    const garrison = summaries.find((s) => s.squad === 0)?.count ?? 0;
    const combat = summaries.filter((s) => s.squad > 0 && s.count > 0);

    if (targetSquad === 0 && garrison === 0) {
      if (fallbackToCombat && combat.length > 0) {
        targetSquad = combat[0].squad;
        this.state.activeSquad = targetSquad;
      } else {
        this.callbacks.onToast(
          combat.length > 0
            ? "На базе нет свободных дронов — выберите отряд"
            : "Нет дронов — постройте казарму",
        );
        return;
      }
    }

    if (targetSquad === 0) {
      const ownZone = this.state.playerId !== null ? this.state.zones.get(this.state.playerId) : undefined;
      const inBaseVision =
        ownZone !== undefined &&
        Math.hypot(wx - ownZone.x, wy - ownZone.y) <= ownZone.radius + 360;
      if (!inBaseVision) {
        this.state.pendingNewSquadTarget = { x: wx, y: wy };
        this.state.pendingNewSquadPrevIds = new Set(combat.map((c) => c.squad));
      } else {
        this.state.pendingNewSquadTarget = null;
        this.state.pendingNewSquadPrevIds = null;
      }
    }

    this.state.armyMode = 1;
    this.state.rallyPoint = { x: wx, y: wy };
    if (targetSquad > 0) {
      this.state.squadOrders.set(targetSquad, { x: wx, y: wy });
      this.renderer.resumeFollow();
    }
    this.transport.send(encodeArmyCommand(targetSquad, 1, wx, wy));
  }

  private sendRally(screenX: number, screenY: number): void {
    const w = this.renderer.screenToWorld(screenX, screenY);
    this.issueSquadOrder(w.x, w.y, false);
    this.callbacks.onUiChange();
  }
}
