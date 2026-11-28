import { encodeArmyCommand, encodeTeleportZone } from "../protocol.ts";
import type { CanvasRenderer } from "../renderer.ts";
import { beaconExtRadius, type ClientGameState } from "../state.ts";
import type { Transport } from "../../transport/transport.ts";
import {
  handlePrimaryAction as runPrimaryAction,
  issueSquadOrder,
  tryActionOnBuildingAtWorld,
  tryPaintBuildAtWorld,
} from "./actions.ts";
import { bindKeyboardEvents } from "./keyboard.ts";
import { bindPointerEvents } from "./pointer.ts";
import { bindTouchEvents } from "./touch.ts";

export interface InputCallbacks {
  onUiChange: () => void;
  onToast: (text: string) => void;
}

export class InputController {
  dragPainting = false;
  touchPainting = false;
  lastPaintedSlotKey: string | null = null;
  lastActedBuildingId: number | null = null;

  constructor(
    public readonly canvas: HTMLCanvasElement,
    public readonly state: ClientGameState,
    public readonly renderer: CanvasRenderer,
    public readonly transport: Transport,
    public readonly callbacks: InputCallbacks,
  ) {
    bindKeyboardEvents(this);
    bindPointerEvents(this);
    bindTouchEvents(this);
  }

  isPointInBaseBuildArea(wx: number, wy: number): boolean {
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
      this.callbacks.onToast("Узел доступен с Ядра Ур. 3");
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
        next > 0 ? `Автоподкрепление → отряд #${next}` : "Автоподкрепление выключено",
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

  tryDragActionAtWorld(wx: number, wy: number, isTouch: boolean): void {
    if (this.state.actionMode !== null) {
      tryActionOnBuildingAtWorld(this, wx, wy, isTouch, false);
    } else if (this.state.buildKind !== null) {
      tryPaintBuildAtWorld(this, wx, wy, false);
    }
  }

  handlePrimaryAction(screenX: number, screenY: number, isTouch: boolean): boolean {
    return runPrimaryAction(this, screenX, screenY, isTouch);
  }

  sendRally(screenX: number, screenY: number): void {
    const w = this.renderer.screenToWorld(screenX, screenY);
    issueSquadOrder(this, w.x, w.y, false);
    this.callbacks.onUiChange();
  }
}
