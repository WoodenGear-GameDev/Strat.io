import { getBuildingConfigByKind, getBuildingRadius } from "../balance.ts";
import {
  encodeArmyCommand,
  encodeBuild,
  encodeSellBuilding,
  encodeTeleportZone,
  encodeUpgradeBuilding,
} from "../protocol.ts";
import type { InputController } from "./controller.ts";

export function findOwnBuildingAt(
  ctrl: InputController,
  wx: number,
  wy: number,
  isTouch: boolean,
): number | null {
  if (ctrl.state.playerId === null) return null;
  const zone = ctrl.state.zones.get(ctrl.state.playerId);
  if (!zone) return null;
  const pad = isTouch ? 4 : 2;
  let foundId: number | null = null;
  let bestDist = Infinity;
  for (const b of ctrl.state.buildings.values()) {
    if (b.ownerId !== ctrl.state.playerId) continue;
    const r = getBuildingRadius(ctrl.state.balance, b.kind);
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

export function tryActionOnBuildingAtWorld(
  ctrl: InputController,
  wx: number,
  wy: number,
  isTouch: boolean,
  isDirectClick: boolean,
): void {
  const mode = ctrl.state.actionMode;
  if (!mode) return;
  const hitId = findOwnBuildingAt(ctrl, wx, wy, isTouch);
  if (hitId === null) return;
  if (!isDirectClick && hitId === ctrl.lastActedBuildingId) return;

  const b = ctrl.state.buildings.get(hitId);
  if (!b) return;

  if (mode === "sell") {
    if (b.kind === 0) {
      if (isDirectClick) ctrl.callbacks.onToast("Ядро нельзя снести");
      return;
    }
    ctrl.lastActedBuildingId = hitId;
    ctrl.transport.send(encodeSellBuilding(hitId));
    return;
  }

  const cfg = getBuildingConfigByKind(ctrl.state.balance, b.kind);
  const nextLvl = cfg?.levels[b.level];
  const hqLevel = ctrl.state.getHqLevel();
  const maxAllowedLvl = Math.min(3, hqLevel + 1);
  const blockedByHq = b.kind !== 0 && b.level >= maxAllowedLvl;
  const affordable =
    !!nextLvl &&
    ctrl.state.energy >= (nextLvl.cost ?? 0) &&
    ctrl.state.trophies >= (nextLvl.trophy_cost ?? 0);

  if (!isDirectClick && (!nextLvl || blockedByHq || !affordable)) {
    return;
  }
  ctrl.lastActedBuildingId = hitId;
  ctrl.transport.send(encodeUpgradeBuilding(hitId));
}

export function tryPaintBuildAtWorld(
  ctrl: InputController,
  wx: number,
  wy: number,
  showFeedbackToast: boolean,
): void {
  if (ctrl.state.playerId === null || ctrl.state.buildKind === null) return;
  const zone = ctrl.state.zones.get(ctrl.state.playerId);
  if (!zone) return;

  const rawRelX = wx - zone.x;
  const rawRelY = wy - zone.y;
  const preview = ctrl.state.resolveBuildPreview(ctrl.state.buildKind, rawRelX, rawRelY, false);
  if (!preview) return;

  if (!preview.footprintValid) {
    if (showFeedbackToast) {
      ctrl.callbacks.onToast(
        preview.overlapsExisting ? "Место занято другой постройкой" : "Стройка за пределами зоны",
      );
    }
    return;
  }

  const relX = preview.centerRel.x;
  const relY = preview.centerRel.y;
  const slotKey = `${ctrl.state.buildKind}:${relX.toFixed(1)},${relY.toFixed(1)}`;
  if (slotKey === ctrl.lastPaintedSlotKey) return;

  if (!preview.affordable) {
    if (showFeedbackToast) {
      const tCost = ctrl.state.dynamicBuildTrophyCost(ctrl.state.buildKind);
      ctrl.callbacks.onToast(
        ctrl.state.trophies < tCost ? "Недостаточно Трофеев" : "Недостаточно Энергии",
      );
    }
    return;
  }

  ctrl.lastPaintedSlotKey = slotKey;
  ctrl.transport.send(encodeBuild(ctrl.state.buildKind, relX, relY));
}

export function issueSquadOrder(
  ctrl: InputController,
  wx: number,
  wy: number,
  fallbackToCombat: boolean,
): void {
  const summaries = ctrl.state.getSquadSummaries();
  let targetSquad = ctrl.state.activeSquad;
  const garrison = summaries.find((s) => s.squad === 0)?.count ?? 0;
  const combat = summaries.filter((s) => s.squad > 0 && s.count > 0);

  if (targetSquad === 0 && garrison === 0) {
    if (fallbackToCombat && combat.length > 0) {
      targetSquad = combat[0].squad;
      ctrl.state.activeSquad = targetSquad;
    } else {
      ctrl.callbacks.onToast(
        combat.length > 0
          ? "На базе нет свободных дронов — выберите отряд"
          : "Нет дронов — постройте казарму",
      );
      return;
    }
  }

  if (targetSquad === 0) {
    ctrl.state.pendingNewSquadTarget = { x: wx, y: wy };
    ctrl.state.pendingNewSquadPrevIds = new Set(combat.map((c) => c.squad));
  }

  ctrl.state.armyMode = 1;
  ctrl.state.rallyPoint = { x: wx, y: wy };
  if (targetSquad > 0) {
    ctrl.state.squadOrders.set(targetSquad, { x: wx, y: wy });
    ctrl.renderer.resumeFollow();
  }
  ctrl.transport.send(encodeArmyCommand(targetSquad, 1, wx, wy));
}

export function handlePrimaryAction(
  ctrl: InputController,
  screenX: number,
  screenY: number,
  isTouch: boolean,
): boolean {
  if (ctrl.state.playerId === null) return false;

  const minimapHit = ctrl.renderer.hitTestMinimap(screenX, screenY, ctrl.state.mapRadius);
  if (minimapHit) {
    if (ctrl.state.teleportSelectMode) {
      issueSquadOrder(ctrl, minimapHit.x, minimapHit.y, true);
      if (ctrl.state.getDroneFocusTarget(1) !== null) {
        ctrl.state.cameraAnchor = "drones";
        ctrl.callbacks.onToast("Дроны летят в точку — выберите место в их взоре");
      } else {
        ctrl.callbacks.onToast("Выпустите дронов — они полетят на разведку");
      }
      ctrl.state.pendingTeleport = null;
      ctrl.callbacks.onUiChange();
      return false;
    }
    if (ctrl.state.rallySelectMode) {
      issueSquadOrder(ctrl, minimapHit.x, minimapHit.y, false);
      ctrl.callbacks.onUiChange();
    } else {
      ctrl.centerOnBase();
    }
    return false;
  }

  const w = ctrl.renderer.screenToWorld(screenX, screenY);

  if (ctrl.state.actionMode !== null) {
    ctrl.state.selectedBuildingId = null;
    ctrl.state.pendingTeleport = null;
    tryActionOnBuildingAtWorld(ctrl, w.x, w.y, isTouch, true);
    ctrl.callbacks.onUiChange();
    return true;
  }

  const hitBuildingId = findOwnBuildingAt(ctrl, w.x, w.y, isTouch);
  if (hitBuildingId !== null) {
    ctrl.state.selectedBuildingId = hitBuildingId;
    ctrl.state.pendingTeleport = null;
    ctrl.callbacks.onUiChange();
    return false;
  }

  const hadSelection = ctrl.state.selectedBuildingId !== null;
  ctrl.state.selectedBuildingId = null;

  if (ctrl.state.teleportSelectMode) {
    if (!ctrl.state.hasVisionAt(w.x, w.y)) {
      ctrl.callbacks.onToast("Телепорт возможен только в зоне взора (у дронов)");
      ctrl.callbacks.onUiChange();
      return false;
    }
    if (isTouch) {
      ctrl.state.pendingTeleport = { x: w.x, y: w.y };
      ctrl.callbacks.onUiChange();
      return false;
    }
    ctrl.transport.send(encodeTeleportZone(w.x, w.y));
    ctrl.state.teleportSelectMode = false;
    ctrl.state.activeSquad = 0;
    ctrl.state.droneFreezeHold = null;
    ctrl.state.cameraAnchor = "hq";
    ctrl.callbacks.onUiChange();
    return false;
  }

  if (ctrl.state.buildKind !== null) {
    tryPaintBuildAtWorld(ctrl, w.x, w.y, !hadSelection);
    if (hadSelection) ctrl.callbacks.onUiChange();
    return true;
  }

  if (ctrl.state.rallySelectMode) {
    ctrl.sendRally(screenX, screenY);
    return false;
  }

  ctrl.callbacks.onUiChange();
  return false;
}
