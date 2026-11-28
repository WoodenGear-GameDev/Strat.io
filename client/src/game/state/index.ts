import { DEFAULT_BALANCE, type BalanceConfig } from "../balance.ts";
import type { HexCoord, Point2D, Rhombus } from "../hex.ts";
import type {
  LeaderboardEntry,
  ServerMessage,
  SnapshotAttractor,
  SnapshotBuilding,
  SnapshotHeat,
  SnapshotProjectile,
  SnapshotTeleport,
  SnapshotUnit,
  SnapshotZone,
} from "../protocol.ts";
import {
  getOccupiedTrisForState,
  getOpenLargeHexesForState,
  resolveBuildPreviewForState,
} from "./placement.ts";
import {
  canUpgradeHqOf,
  collectBuildingsByKindOf,
  computeSellRefundOf,
  computeSellTrophyRefundOf,
  computeTeleportCostOf,
  dynamicBuildTrophyCostOf,
  getDroneFocusTargetOf,
  getEnergyCapOf,
  getHqBuildingOf,
  getSquadSummariesOf,
  getVisionCirclesOf,
  maxRadiusForHqOf,
} from "./queries.ts";
import { applySnapshotToState, stepStateEffects } from "./snapshot.ts";
import type {
  BuildPreview,
  CombatHeat,
  MinimapFlash,
  VisionCircle,
  VisualEffect,
} from "./types.ts";

export * from "../hex.ts";
export * from "./types.ts";

export class ClientGameState {
  balance: BalanceConfig = DEFAULT_BALANCE;
  playerId: number | null = null;
  mapRadius = DEFAULT_BALANCE.world.map_radius;
  tickRate = DEFAULT_BALANCE.world.tick_rate;

  tick = 0;
  score = 0;
  rEq = 450;
  energy = 0;
  trophies = 0;
  unitCount = 0;
  unitCap = 0;
  teleportCdSec = 0;
  pingMs = 0;

  armyMode: 0 | 1 = 0;
  rallyPoint: Point2D = { x: 0, y: 0 };
  activeSquad = 0;
  reinforceSquad = 0;
  squadOrders = new Map<number, Point2D>();
  lastSquadCentroid: Point2D | null = null;
  droneFreezeHold: { squad: number; x: number; y: number } | null = null;
  pendingNewSquadTarget: Point2D | null = null;
  pendingNewSquadPrevIds: Set<number> | null = null;

  buildKind: number | null = null;
  actionMode: "upgrade" | "sell" | null = null;
  rallySelectMode = false;
  teleportSelectMode = false;
  pendingTeleport: Point2D | null = null;
  selectedBuildingId: number | null = null;
  cameraAnchor: "hq" | "drones" = "hq";

  hoverWorldX = 0;
  hoverWorldY = 0;
  hasPointerHover = false;

  snapshotTime = performance.now();
  lastSnapArrivalMs = 0;
  tickIntervalMs = 1000 / Math.max(1, DEFAULT_BALANCE.world.tick_rate);

  playerNames = new Map<number, string>();
  leaderboard: LeaderboardEntry[] = [];

  prevZones = new Map<number, SnapshotZone>();
  zones = new Map<number, SnapshotZone>();

  buildingBuf = new Map<number, SnapshotBuilding>();
  buildings = new Map<number, SnapshotBuilding>();

  prevUnits = new Map<number, SnapshotUnit>();
  units = new Map<number, SnapshotUnit>();

  prevProjectiles = new Map<number, SnapshotProjectile>();
  projectiles = new Map<number, SnapshotProjectile>();
  teleports: SnapshotTeleport[] = [];
  attractors: SnapshotAttractor[] = [];
  serverHeats: SnapshotHeat[] = [];

  effects: VisualEffect[] = [];
  buildingHitEffects = new Map<number, { startMs: number; untilMs: number }>();
  teleportFlashes: MinimapFlash[] = [];
  combatHeats: CombatHeat[] = [];
  lastDamagedAtMs = -100_000;

  applySnapshot(msg: Extract<ServerMessage, { type: "Snapshot" }>): void {
    applySnapshotToState(this, msg);
  }

  stepEffects(dt: number): void {
    stepStateEffects(this, dt);
  }

  getHqBuilding(): SnapshotBuilding | undefined {
    return getHqBuildingOf(this);
  }

  getHqLevel(): number {
    return this.getHqBuilding()?.level ?? 1;
  }

  getOpenLargeHexes(ownerId: number | null = this.playerId): Map<string, HexCoord> {
    return getOpenLargeHexesForState(this, ownerId);
  }

  getOccupiedTris(ownerId: number | null = this.playerId, rhombuses?: Rhombus[]): Set<string> {
    return getOccupiedTrisForState(this, ownerId, rhombuses);
  }

  resolveBuildPreview(
    kind: number,
    relX: number,
    relY: number,
    useExistingSnap = false,
  ): BuildPreview | null {
    return resolveBuildPreviewForState(this, kind, relX, relY, useExistingSnap);
  }

  canPlaceBuilding(kind: number, relX: number, relY: number): boolean {
    const p = this.resolveBuildPreview(kind, relX, relY, true);
    return p ? p.valid : false;
  }

  collectBuildingsByKind(kind: number): SnapshotBuilding[] {
    return collectBuildingsByKindOf(this, kind);
  }

  isInCombat(nowMs = performance.now()): boolean {
    return nowMs - this.lastDamagedAtMs < 6000;
  }

  dynamicBuildTrophyCost(kind: number, extraBeacons = 0): number {
    return dynamicBuildTrophyCostOf(this, kind, extraBeacons);
  }

  canUpgradeHq(): boolean {
    return canUpgradeHqOf(this);
  }

  computeSellRefund(b: SnapshotBuilding): number {
    return computeSellRefundOf(this, b);
  }

  computeSellTrophyRefund(b: SnapshotBuilding): number {
    return computeSellTrophyRefundOf(this, b);
  }

  getEnergyCap(): number {
    return getEnergyCapOf(this);
  }

  isBaseRegenEnabled(): boolean {
    if (this.playerId === null) return false;
    const z = this.zones.get(this.playerId);
    return z ? (z.flags & 8) !== 0 : false;
  }

  maxRadiusForHq(hqLevel: number, zoneRadius: number): number {
    return maxRadiusForHqOf(this, hqLevel, zoneRadius);
  }

  getSquadSummaries(): Array<{ squad: number; count: number }> {
    return getSquadSummariesOf(this);
  }

  getDroneFocusTarget(alpha: number): Point2D | null {
    return getDroneFocusTargetOf(this, alpha);
  }

  getVisionCircles(
    alpha = 1,
    zonePositions?: Map<number, { x: number; y: number; radius: number }>,
  ): VisionCircle[] {
    return getVisionCirclesOf(this, alpha, zonePositions);
  }

  hasVisionAt(wx: number, wy: number): boolean {
    const circles = this.getVisionCircles(1);
    for (let i = 0; i < circles.length; i++) {
      const c = circles[i];
      const dx = wx - c.x;
      const dy = wy - c.y;
      if (dx * dx + dy * dy <= c.r * c.r) return true;
    }
    return false;
  }

  equilibriumRadius(): number {
    return this.rEq;
  }

  computeTeleportCost(tx: number, ty: number): number {
    return computeTeleportCostOf(this, tx, ty);
  }

  reset(): void {
    this.playerId = null;
    this.tick = 0;
    this.score = 0;
    this.rEq = 450;
    this.energy = 0;
    this.trophies = 0;
    this.teleportCdSec = 0;
    this.armyMode = 0;
    this.rallyPoint = { x: 0, y: 0 };
    this.activeSquad = 0;
    this.squadOrders.clear();
    this.lastSquadCentroid = null;
    this.droneFreezeHold = null;
    this.buildKind = null;
    this.actionMode = null;
    this.rallySelectMode = false;
    this.teleportSelectMode = false;
    this.pendingTeleport = null;
    this.selectedBuildingId = null;
    this.cameraAnchor = "hq";
    this.lastSnapArrivalMs = 0;
    this.tickIntervalMs = 1000 / Math.max(1, this.tickRate);
    this.playerNames.clear();
    this.leaderboard = [];
    this.prevZones.clear();
    this.zones.clear();
    this.buildingBuf.clear();
    this.buildings.clear();
    this.prevUnits.clear();
    this.units.clear();
    this.prevProjectiles.clear();
    this.projectiles.clear();
    this.teleports = [];
    this.attractors = [];
    this.serverHeats = [];
    this.effects = [];
    this.buildingHitEffects.clear();
    this.teleportFlashes = [];
    this.combatHeats = [];
    this.lastDamagedAtMs = -100_000;
  }
}
