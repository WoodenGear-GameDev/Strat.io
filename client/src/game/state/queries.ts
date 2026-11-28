import { getBuildingConfigByKind, getNodeTrophyCost } from "../balance.ts";
import { beaconExtRadius, type Point2D } from "../hex.ts";
import type { SnapshotBuilding } from "../protocol.ts";
import type { ClientGameState } from "./index.ts";
import type { VisionCircle } from "./types.ts";

export function getHqBuildingOf(state: ClientGameState): SnapshotBuilding | undefined {
  if (state.playerId === null) return undefined;
  for (const b of state.buildings.values()) {
    if (b.ownerId === state.playerId && b.kind === 0) return b;
  }
  return undefined;
}

export function collectBuildingsByKindOf(
  state: ClientGameState,
  kind: number,
): SnapshotBuilding[] {
  if (state.playerId === null) return [];
  const list: SnapshotBuilding[] = [];
  for (const b of state.buildings.values()) {
    if (b.ownerId === state.playerId && b.kind === kind) list.push(b);
  }
  return list;
}

export function dynamicBuildTrophyCostOf(
  state: ClientGameState,
  kind: number,
  extraBeacons = 0,
): number {
  if (kind === 5) {
    const existing = collectBuildingsByKindOf(state, 5).length + extraBeacons;
    return getNodeTrophyCost(state.balance, existing);
  }
  const cfg = getBuildingConfigByKind(state.balance, kind);
  return cfg?.levels[0]?.trophy_cost ?? 0;
}

export function canUpgradeHqOf(state: ClientGameState): boolean {
  if (state.playerId === null) return false;
  const hqLevel = state.getHqLevel();
  const hqCfg = getBuildingConfigByKind(state.balance, 0);
  const next = hqCfg?.levels[hqLevel];
  if (!next) return false;
  return state.energy >= (next.cost ?? 0) && state.trophies >= (next.trophy_cost ?? 0);
}

export function computeSellRefundOf(state: ClientGameState, b: SnapshotBuilding): number {
  if (b.kind === 0) return 0;
  const cfg = getBuildingConfigByKind(state.balance, b.kind);
  if (!cfg) return 0;
  let invested = 0;
  for (let i = 0; i < b.level; i++) {
    invested += cfg.levels[i]?.cost ?? 0;
  }
  return Math.round(invested * state.balance.world.sell_refund_ratio);
}

export function computeSellTrophyRefundOf(state: ClientGameState, b: SnapshotBuilding): number {
  if (b.kind === 0) return 0;
  const cfg = getBuildingConfigByKind(state.balance, b.kind);
  if (!cfg) return 0;
  let trophies = cfg.levels[0]?.trophy_cost ?? 0;
  for (let i = 1; i < b.level; i++) {
    trophies += cfg.levels[i]?.trophy_cost ?? 0;
  }
  return Math.round(trophies * 0.5);
}

export function getEnergyCapOf(state: ClientGameState): number {
  const hqLevel = state.getHqLevel();
  const hqCfg = getBuildingConfigByKind(state.balance, 0);
  let cap = hqCfg?.levels[hqLevel - 1]?.energy_cap ?? 0;
  const genCfg = getBuildingConfigByKind(state.balance, 1);
  if (state.playerId !== null && genCfg) {
    for (const b of state.buildings.values()) {
      if (b.ownerId === state.playerId && b.kind === 1 && b.hp > 0) {
        cap += genCfg.levels[b.level - 1]?.energy_cap ?? 0;
      }
    }
  }
  return cap;
}

export function maxRadiusForHqOf(
  state: ClientGameState,
  hqLevel: number,
  zoneRadius: number,
): number {
  const ring3R = state.balance.world.ring3_radius;
  const ring2R = state.balance.world.ring2_radius;
  if (hqLevel <= 1) return Math.max(0, state.mapRadius - zoneRadius);
  if (hqLevel === 2) return Math.max(0, ring2R - zoneRadius - 60);
  return Math.max(0, ring3R - zoneRadius - 60);
}

export function getSquadSummariesOf(
  state: ClientGameState,
): Array<{ squad: number; count: number }> {
  if (state.playerId === null) return [];
  const counts = new Map<number, number>();
  for (const u of state.units.values()) {
    if (u.ownerId !== state.playerId) continue;
    counts.set(u.squad, (counts.get(u.squad) ?? 0) + 1);
  }
  const list: Array<{ squad: number; count: number }> = [
    { squad: 0, count: counts.get(0) ?? 0 },
  ];
  const combatIds = Array.from(counts.keys())
    .filter((s) => s > 0)
    .sort((a, b) => a - b);
  for (const sid of combatIds) {
    list.push({ squad: sid, count: counts.get(sid) ?? 0 });
  }
  return list;
}

export function getDroneFocusTargetOf(state: ClientGameState, alpha: number): Point2D | null {
  if (state.playerId === null) return null;

  let targetSquad = state.activeSquad;
  if (targetSquad === 0) {
    let firstCombat: number | null = null;
    for (const u of state.units.values()) {
      if (u.ownerId === state.playerId && u.squad > 0) {
        if (firstCombat === null || u.squad < firstCombat) firstCombat = u.squad;
      }
    }
    if (firstCombat !== null) targetSquad = firstCombat;
  }

  const pts: Point2D[] = [];
  for (const u of state.units.values()) {
    if (u.ownerId !== state.playerId || u.squad !== targetSquad) continue;
    const prev = state.prevUnits.get(u.id);
    const x = prev ? prev.x + (u.x - prev.x) * alpha : u.x;
    const y = prev ? prev.y + (u.y - prev.y) * alpha : u.y;
    pts.push({ x, y });
  }

  // If frozen at leader's death spot, hold until a drone of this squad catches up
  if (state.droneFreezeHold && state.droneFreezeHold.squad === targetSquad) {
    let caughtUp = false;
    for (const p of pts) {
      if (Math.hypot(p.x - state.droneFreezeHold.x, p.y - state.droneFreezeHold.y) <= 180) {
        caughtUp = true;
        break;
      }
    }
    if (!caughtUp) {
      return { x: state.droneFreezeHold.x, y: state.droneFreezeHold.y };
    }
    state.droneFreezeHold = null;
  }

  if (pts.length > 0) {
    const orderPt = state.squadOrders.get(targetSquad);
    if (targetSquad > 0 && state.reinforceSquad === targetSquad) {
      const holdRef = state.lastSquadCentroid ?? orderPt;
      if (holdRef) {
        let minD = Infinity;
        for (const p of pts) {
          const d = Math.hypot(p.x - holdRef.x, p.y - holdRef.y);
          if (d < minD) minD = d;
        }
        if (minD > 420) return holdRef;
      }
    }
    const refPt = orderPt ?? state.lastSquadCentroid;
    let lead = pts[0];
    if (refPt && pts.length > 1) {
      let bestD = Infinity;
      for (const p of pts) {
        const d = Math.hypot(p.x - refPt.x, p.y - refPt.y);
        if (d < bestD) {
          bestD = d;
          lead = p;
        }
      }
    }
    let sx = 0;
    let sy = 0;
    let cnt = 0;
    for (const p of pts) {
      if (!refPt || Math.hypot(p.x - lead.x, p.y - lead.y) <= 360) {
        sx += p.x;
        sy += p.y;
        cnt += 1;
      }
    }
    const pt = cnt > 0 ? { x: sx / cnt, y: sy / cnt } : lead;
    state.lastSquadCentroid = pt;
    return pt;
  }

  if (targetSquad > 0 && state.reinforceSquad === targetSquad) {
    const holdRef = state.lastSquadCentroid ?? state.squadOrders.get(targetSquad);
    if (holdRef) return holdRef;
  }

  return null;
}

export function getVisionCirclesOf(
  state: ClientGameState,
  alpha = 1,
  zonePositions?: Map<number, { x: number; y: number; radius: number }>,
): VisionCircle[] {
  const circles: VisionCircle[] = [];
  if (state.playerId === null) return circles;

  const zPos = zonePositions?.get(state.playerId);
  const ownZone = zPos ?? state.zones.get(state.playerId);
  if (ownZone) {
    let zx = ownZone.x;
    let zy = ownZone.y;
    if (!zPos) {
      const prevZ = state.prevZones.get(state.playerId);
      if (prevZ) {
        zx = prevZ.x + (ownZone.x - prevZ.x) * alpha;
        zy = prevZ.y + (ownZone.y - prevZ.y) * alpha;
      }
    }
    circles.push({ x: zx, y: zy, r: ownZone.radius + 360 });
    for (const b of state.buildings.values()) {
      if (b.ownerId === state.playerId && b.kind === 5) {
        circles.push({
          x: zx + b.relX,
          y: zy + b.relY,
          r: beaconExtRadius(b.level) + 220,
        });
      }
    }
  }

  for (const u of state.units.values()) {
    if (u.ownerId !== state.playerId) continue;
    const prev = state.prevUnits.get(u.id);
    const x = prev ? prev.x + (u.x - prev.x) * alpha : u.x;
    const y = prev ? prev.y + (u.y - prev.y) * alpha : u.y;
    circles.push({ x, y, r: 290 });
  }

  return circles;
}

export function computeTeleportCostOf(state: ClientGameState, tx: number, ty: number): number {
  if (state.playerId === null) return 0;
  const zone = state.zones.get(state.playerId);
  if (!zone) return 0;
  const dist = Math.hypot(tx - zone.x, ty - zone.y);
  const targetR = Math.hypot(tx, ty);
  const { teleport_base_cost, teleport_dist_factor, teleport_ring_penalty } = state.balance.world;
  const raw =
    teleport_base_cost + dist * teleport_dist_factor + Math.abs(targetR - state.rEq) * teleport_ring_penalty;
  return Math.round(raw);
}
