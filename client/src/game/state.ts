import {
  DEFAULT_BALANCE,
  getBuildingConfigByKind,
  type BalanceConfig,
} from "./balance.ts";
import type {
  LeaderboardEntry,
  ServerMessage,
  SnapshotBuilding,
  SnapshotProjectile,
  SnapshotUnit,
  SnapshotZone,
} from "./protocol.ts";

export interface VisualEffect {
  x: number;
  y: number;
  radius: number;
  maxRadius: number;
  color: string;
  alpha: number;
}

export const HEX_SIZE = 34;
const SQRT3 = Math.sqrt(3);

export const HEX_EDGE_DIRS: readonly [number, number][] = [
  [1, 0],
  [0, 1],
  [-1, 1],
  [-1, 0],
  [0, -1],
  [1, -1],
];

export interface HexCoord {
  q: number;
  r: number;
}

export interface TriCoord {
  q: number;
  r: number;
  s: number;
}

export interface Point2D {
  x: number;
  y: number;
}

export interface VisionCircle {
  x: number;
  y: number;
  r: number;
}

export interface MinimapFlash {
  x: number;
  y: number;
  startMs: number;
  untilMs: number;
}

export interface CombatHeat {
  x: number;
  y: number;
  power: number;
  showAtMs: number;
  untilMs: number;
}

export interface Rhombus {
  tris: [TriCoord, TriCoord];
  center: Point2D;
  verts: [Point2D, Point2D, Point2D, Point2D];
}

export interface BuildPreview {
  isLarge: boolean;
  centerRel: Point2D;
  rhombusVerts: [Point2D, Point2D, Point2D, Point2D] | null;
  insideTerritory: boolean;
  overlapsExisting: boolean;
  footprintValid: boolean;
  withinLimit: boolean;
  affordable: boolean;
  valid: boolean;
}

export function hexKey(q: number, r: number): string {
  return `${q},${r}`;
}

export function triKey(q: number, r: number, s: number): string {
  return `${q},${r},${s}`;
}

export function isLargeBuilding(kind: number): boolean {
  return kind === 0 || kind === 2 || kind === 3;
}

export function hexToRel(q: number, r: number): Point2D {
  return {
    x: HEX_SIZE * (SQRT3 * q + (SQRT3 / 2) * r),
    y: HEX_SIZE * 1.5 * r,
  };
}

export function relToHexRound(x: number, y: number): HexCoord {
  const qf = ((SQRT3 / 3) * x - (1 / 3) * y) / HEX_SIZE;
  const rf = ((2 / 3) * y) / HEX_SIZE;
  const sf = -qf - rf;

  let rq = Math.round(qf);
  let rr = Math.round(rf);
  const rs = Math.round(sf);

  const dq = Math.abs(rq - qf);
  const dr = Math.abs(rr - rf);
  const ds = Math.abs(rs - sf);

  if (dq > dr && dq > ds) {
    rq = -rr - rs;
  } else if (dr > ds) {
    rr = -rq - rs;
  }

  return { q: rq === 0 ? 0 : rq, r: rr === 0 ? 0 : rr };
}

export function hexDist(q1: number, r1: number, q2: number, r2: number): number {
  return (Math.abs(q1 - q2) + Math.abs(q1 + r1 - q2 - r2) + Math.abs(r1 - r2)) / 2;
}

export function hexVertex(q: number, r: number, i: number): Point2D {
  const c = hexToRel(q, r);
  const ang = (Math.PI / 3) * i - Math.PI / 6;
  return { x: c.x + Math.cos(ang) * HEX_SIZE, y: c.y + Math.sin(ang) * HEX_SIZE };
}

export function collectRhombuses(openHexes: Map<string, HexCoord>): Rhombus[] {
  const out: Rhombus[] = [];
  for (const hex of openHexes.values()) {
    const c = hexToRel(hex.q, hex.r);
    for (let s = 0; s < 6; s++) {
      const sNext = (s + 1) % 6;
      const v0 = hexVertex(hex.q, hex.r, s);
      const v1 = hexVertex(hex.q, hex.r, sNext);
      const v2 = hexVertex(hex.q, hex.r, (s + 2) % 6);

      // 1. Internal rhombus (triangles s and s+1 inside hex)
      out.push({
        tris: [
          { q: hex.q, r: hex.r, s },
          { q: hex.q, r: hex.r, s: sNext },
        ],
        center: { x: (c.x + v1.x) * 0.5, y: (c.y + v1.y) * 0.5 },
        verts: [c, v0, v1, v2],
      });

      // 2. Boundary rhombus across edge s into neighbor hex
      const [dq, dr] = HEX_EDGE_DIRS[s];
      const nq = hex.q + dq;
      const nr = hex.r + dr;
      if (openHexes.has(hexKey(nq, nr)) && (hex.q < nq || (hex.q === nq && hex.r < nr))) {
        const nc = hexToRel(nq, nr);
        const sOpp = (s + 3) % 6;
        out.push({
          tris: [
            { q: hex.q, r: hex.r, s },
            { q: nq, r: nr, s: sOpp },
          ],
          center: { x: (v0.x + v1.x) * 0.5, y: (v0.y + v1.y) * 0.5 },
          verts: [c, v0, nc, v1],
        });
      }
    }
  }
  return out;
}

export function findLocalRhombusVerts(
  relX: number,
  relY: number,
): [Point2D, Point2D, Point2D, Point2D] | null {
  const base = relToHexRound(relX, relY);
  let bestVerts: [Point2D, Point2D, Point2D, Point2D] | null = null;
  let bestDist = Infinity;

  for (let dq = -1; dq <= 1; dq++) {
    for (let dr = -1; dr <= 1; dr++) {
      const q = base.q + dq;
      const r = base.r + dr;
      const c = hexToRel(q, r);
      for (let s = 0; s < 6; s++) {
        const sNext = (s + 1) % 6;
        const v0 = hexVertex(q, r, s);
        const v1 = hexVertex(q, r, sNext);
        const v2 = hexVertex(q, r, (s + 2) % 6);

        const dIn = Math.hypot((c.x + v1.x) * 0.5 - relX, (c.y + v1.y) * 0.5 - relY);
        if (dIn < bestDist) {
          bestDist = dIn;
          bestVerts = [c, v0, v1, v2];
        }

        const [edq, edr] = HEX_EDGE_DIRS[s];
        const nc = hexToRel(q + edq, r + edr);
        const dOut = Math.hypot((v0.x + v1.x) * 0.5 - relX, (v0.y + v1.y) * 0.5 - relY);
        if (dOut < bestDist) {
          bestDist = dOut;
          bestVerts = [c, v0, nc, v1];
        }
      }
    }
  }
  return bestVerts;
}

export function coreHexRadius(level: number): number {
  if (level <= 1) return 1;
  if (level === 2) return 2;
  return 3;
}

export function hqZoneRadius(level: number): number {
  if (level <= 1) return 95;
  if (level === 2) return 145;
  return 200;
}

export function beaconExtRadius(level: number): number {
  if (level <= 1) return 56;
  if (level === 2) return 70;
  return 85;
}

export function beaconExtensionCenter(
  relX: number,
  relY: number,
  _extR?: number,
): Point2D {
  return { x: relX, y: relY };
}

export class ClientGameState {
  balance: BalanceConfig = DEFAULT_BALANCE;
  playerId: number | null = null;
  mapRadius = 3200;
  tickRate = 20;

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
  lostSquadHold: { x: number; y: number; untilMs: number } | null = null;
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
  tickIntervalMs = 50;

  playerNames = new Map<number, string>();
  leaderboard: LeaderboardEntry[] = [];

  prevZones = new Map<number, SnapshotZone>();
  zones = new Map<number, SnapshotZone>();

  buildings = new Map<number, SnapshotBuilding>();

  prevUnits = new Map<number, SnapshotUnit>();
  units = new Map<number, SnapshotUnit>();

  prevProjectiles = new Map<number, SnapshotProjectile>();
  projectiles = new Map<number, SnapshotProjectile>();

  effects: VisualEffect[] = [];
  buildingHitEffects = new Map<number, { startMs: number; untilMs: number }>();
  teleportFlashes: MinimapFlash[] = [];
  combatHeats: CombatHeat[] = [];
  lastDamagedAtMs = -100_000;

  applySnapshot(msg: Extract<ServerMessage, { type: "Snapshot" }>): void {
    const now = performance.now();
    this.tickIntervalMs = 1000 / Math.max(1, this.tickRate);
    this.snapshotTime = now;

    this.prevZones = new Map(this.zones);
    this.prevUnits = new Map(this.units);
    this.prevProjectiles = new Map(this.projectiles);

    const nextUnits = new Map<number, SnapshotUnit>();
    for (const u of msg.units) {
      nextUnits.set(u.id, u);
      if (u.ownerId === this.playerId) {
        const oldU = this.units.get(u.id);
        if (oldU && u.hp < oldU.hp) {
          this.lastDamagedAtMs = now;
        }
      }
    }

    for (const [id, oldU] of this.units) {
      if (!nextUnits.has(id)) {
        if (oldU.ownerId === this.playerId) {
          this.lastDamagedAtMs = now;
        }
        this.recordCombatHeat(oldU.x, oldU.y, 1.0, now);
        this.effects.push({
          x: oldU.x,
          y: oldU.y,
          radius: 4,
          maxRadius: 22,
          color: oldU.ownerId === this.playerId ? "#6bb8ff" : "#ff6b6b",
          alpha: 0.9,
        });
      }
    }

    const nextZonesMap = new Map<number, SnapshotZone>();
    for (const z of msg.zones) {
      nextZonesMap.set(z.playerId, z);
      const oldZ = this.zones.get(z.playerId);
      if (oldZ && Math.hypot(z.x - oldZ.x, z.y - oldZ.y) > 220) {
        this.teleportFlashes.push({ x: oldZ.x, y: oldZ.y, startMs: now, untilMs: now + 6500 });
        this.teleportFlashes.push({ x: z.x, y: z.y, startMs: now, untilMs: now + 6500 });
      }
    }

    const nextBuildings = new Map<number, SnapshotBuilding>();
    for (const b of msg.buildings) {
      nextBuildings.set(b.id, b);
      const oldB = this.buildings.get(b.id);
      if (oldB && b.hp < oldB.hp) {
        if (b.ownerId === this.playerId) {
          this.lastDamagedAtMs = now;
        }
        this.buildingHitEffects.set(b.id, { startMs: now, untilMs: now + 220 });
        const z = nextZonesMap.get(b.ownerId) ?? this.zones.get(b.ownerId);
        if (z) {
          const bx = z.x + b.relX;
          const by = z.y + b.relY;
          this.recordCombatHeat(bx, by, 0.7, now);
          for (let k = 0; k < 2; k++) {
            const ang = Math.random() * Math.PI * 2;
            const off = 8 + Math.random() * 14;
            this.effects.push({
              x: bx + Math.cos(ang) * off,
              y: by + Math.sin(ang) * off,
              radius: 3,
              maxRadius: 16,
              color: k === 0 ? "#ffb454" : "#ff6b6b",
              alpha: 0.95,
            });
          }
        }
      }
    }

    for (const [id, oldB] of this.buildings) {
      if (!nextBuildings.has(id)) {
        this.buildingHitEffects.delete(id);
        const z = this.zones.get(oldB.ownerId);
        if (z) {
          this.recordCombatHeat(z.x + oldB.relX, z.y + oldB.relY, 2.5, now);
          this.effects.push({
            x: z.x + oldB.relX,
            y: z.y + oldB.relY,
            radius: 10,
            maxRadius: 50,
            color: "#ffb454",
            alpha: 0.98,
          });
        }
      }
    }

    this.tick = msg.tick;
    this.score = msg.score;
    this.rEq = msg.rEq;
    this.energy = msg.energy;
    this.trophies = msg.trophies;
    this.unitCount = msg.unitCount;
    this.unitCap = msg.unitCap;
    this.teleportCdSec = msg.teleportCdSec;

    this.zones.clear();
    for (const z of msg.zones) this.zones.set(z.playerId, z);

    this.buildings = nextBuildings;
    this.units = nextUnits;

    // Track squad continuity, auto-merges, and short hold on squad wipe
    if (this.playerId !== null) {
      const liveSquads = new Map<number, { sx: number; sy: number; cnt: number }>();
      for (const u of nextUnits.values()) {
        if (u.ownerId !== this.playerId || u.kind === 4) continue;
        const cur = liveSquads.get(u.squad) ?? { sx: 0, sy: 0, cnt: 0 };
        cur.sx += u.x;
        cur.sy += u.y;
        cur.cnt += 1;
        liveSquads.set(u.squad, cur);
      }
      for (const sid of Array.from(this.squadOrders.keys())) {
        if (!liveSquads.has(sid)) {
          this.squadOrders.delete(sid);
        }
      }
      if (this.reinforceSquad > 0 && !liveSquads.has(this.reinforceSquad)) {
        this.reinforceSquad = 0;
      }
      if (this.pendingNewSquadTarget) {
        let newSid: number | null = null;
        for (const [sid, info] of liveSquads) {
          if (sid <= 0 || info.cnt === 0) continue;
          if (!this.pendingNewSquadPrevIds?.has(sid)) {
            if (newSid === null || sid > newSid) newSid = sid;
          }
        }
        if (newSid !== null) {
          this.activeSquad = newSid;
          this.squadOrders.set(newSid, this.pendingNewSquadTarget);
          this.pendingNewSquadTarget = null;
          this.pendingNewSquadPrevIds = null;
          this.lostSquadHold = null;
        }
      }
      if (this.activeSquad > 0) {
        const cur = liveSquads.get(this.activeSquad);
        if (cur && cur.cnt > 0) {
          this.lastSquadCentroid = { x: cur.sx / cur.cnt, y: cur.sy / cur.cnt };
          this.lostSquadHold = null;
        } else if (this.lastSquadCentroid) {
          let mergedId: number | null = null;
          let bestDist = 230;
          for (const [sid, info] of liveSquads) {
            if (sid <= 0 || info.cnt === 0) continue;
            const d = Math.hypot(
              info.sx / info.cnt - this.lastSquadCentroid.x,
              info.sy / info.cnt - this.lastSquadCentroid.y,
            );
            if (d < bestDist) {
              bestDist = d;
              mergedId = sid;
            }
          }
          if (mergedId !== null) {
            this.activeSquad = mergedId;
          } else {
            const ownZone = this.zones.get(this.playerId);
            const returnedToBase =
              ownZone !== undefined &&
              Math.hypot(this.lastSquadCentroid.x - ownZone.x, this.lastSquadCentroid.y - ownZone.y) <=
                ownZone.radius + 390;
            if (!returnedToBase && this.cameraAnchor === "drones") {
              this.lostSquadHold = {
                x: this.lastSquadCentroid.x,
                y: this.lastSquadCentroid.y,
                untilMs: now + 1400,
              };
            }
            this.activeSquad = 0;
            this.lastSquadCentroid = null;
          }
        } else {
          this.activeSquad = 0;
        }
      }
    }

    this.projectiles.clear();
    for (const p of msg.projectiles) this.projectiles.set(p.id, p);

    if (this.selectedBuildingId !== null && !this.buildings.has(this.selectedBuildingId)) {
      this.selectedBuildingId = null;
    }
  }

  private recordCombatHeat(x: number, y: number, weight: number, now: number): void {
    for (const h of this.combatHeats) {
      if (Math.hypot(h.x - x, h.y - y) <= 320) {
        h.x = h.x * 0.7 + x * 0.3;
        h.y = h.y * 0.7 + y * 0.3;
        h.power = Math.min(12, h.power + weight);
        h.untilMs = Math.max(h.untilMs, now + 7500);
        return;
      }
    }
    this.combatHeats.push({
      x,
      y,
      power: weight,
      showAtMs: now + 1200,
      untilMs: now + 7500,
    });
  }

  stepEffects(dt: number): void {
    const now = performance.now();
    for (const [id, hit] of this.buildingHitEffects) {
      if (now >= hit.untilMs) {
        this.buildingHitEffects.delete(id);
      }
    }
    this.teleportFlashes = this.teleportFlashes.filter((f) => now < f.untilMs);
    this.combatHeats = this.combatHeats.filter((h) => now < h.untilMs);
    for (let i = this.effects.length - 1; i >= 0; i--) {
      const e = this.effects[i];
      e.radius += (e.maxRadius - e.radius) * Math.min(1, dt * 10);
      e.alpha -= dt * 2.5;
      if (e.alpha <= 0) {
        this.effects.splice(i, 1);
      }
    }
  }

  getHqBuilding(): SnapshotBuilding | undefined {
    if (this.playerId === null) return undefined;
    for (const b of this.buildings.values()) {
      if (b.ownerId === this.playerId && b.kind === 0) return b;
    }
    return undefined;
  }

  getHqLevel(): number {
    return this.getHqBuilding()?.level ?? 1;
  }

  getOpenLargeHexes(ownerId: number | null = this.playerId): Map<string, HexCoord> {
    const open = new Map<string, HexCoord>();
    if (ownerId === null) return open;

    let hqLevel = 1;
    for (const b of this.buildings.values()) {
      if (b.ownerId === ownerId && b.kind === 0) {
        hqLevel = b.level;
        break;
      }
    }

    const coreR = coreHexRadius(hqLevel);
    for (let q = -coreR; q <= coreR; q++) {
      const rMin = Math.max(-coreR, -q - coreR);
      const rMax = Math.min(coreR, -q + coreR);
      for (let r = rMin; r <= rMax; r++) {
        open.set(hexKey(q, r), { q, r });
      }
    }

    for (const b of this.buildings.values()) {
      if (b.ownerId === ownerId && b.kind === 5) {
        const rad = beaconExtRadius(b.level);
        const hexSpan = Math.ceil(rad / (HEX_SIZE * 1.5)) + 1;
        const centerAnchor = relToHexRound(b.relX, b.relY);
        for (let dq = -hexSpan; dq <= hexSpan; dq++) {
          for (let dr = -hexSpan; dr <= hexSpan; dr++) {
            const lq = centerAnchor.q + dq;
            const lr = centerAnchor.r + dr;
            const lpos = hexToRel(lq, lr);
            if (Math.hypot(lpos.x - b.relX, lpos.y - b.relY) <= rad) {
              open.set(hexKey(lq, lr), { q: lq, r: lr });
            }
          }
        }
      }
    }

    return open;
  }

  getOccupiedTris(
    ownerId: number | null = this.playerId,
    rhombuses?: Rhombus[],
  ): Set<string> {
    const occupied = new Set<string>();
    if (ownerId === null) return occupied;
    const open = this.getOpenLargeHexes(ownerId);
    const list = rhombuses ?? collectRhombuses(open);

    for (const b of this.buildings.values()) {
      if (b.ownerId !== ownerId) continue;
      if (isLargeBuilding(b.kind)) {
        const h = relToHexRound(b.relX, b.relY);
        for (let s = 0; s < 6; s++) {
          occupied.add(triKey(h.q, h.r, s));
        }
      } else {
        let best: Rhombus | null = null;
        let bestD = Infinity;
        for (const rh of list) {
          const d = Math.hypot(rh.center.x - b.relX, rh.center.y - b.relY);
          if (d < bestD) {
            bestD = d;
            best = rh;
          }
        }
        if (best) {
          occupied.add(triKey(best.tris[0].q, best.tris[0].r, best.tris[0].s));
          occupied.add(triKey(best.tris[1].q, best.tris[1].r, best.tris[1].s));
        }
      }
    }
    return occupied;
  }

  resolveBuildPreview(
    kind: number,
    relX: number,
    relY: number,
    useExistingSnap = false,
  ): BuildPreview | null {
    if (this.playerId === null || kind === 0) return null;
    const openHexes = this.getOpenLargeHexes();
    const rhombuses = collectRhombuses(openHexes);
    const occupiedTris = this.getOccupiedTris(this.playerId, rhombuses);
    const cfg = getBuildingConfigByKind(this.balance, kind);

    let withinLimit = !(kind === 5 && this.getHqLevel() < 3);
    if (withinLimit && cfg?.max_per_player !== undefined) {
      let count = 0;
      for (const b of this.buildings.values()) {
        if (b.ownerId === this.playerId && b.kind === kind) count++;
      }
      withinLimit = count < cfg.max_per_player;
    }

    const cost = cfg?.levels[0]?.cost ?? 0;
    const tCost = this.dynamicBuildTrophyCost(kind);
    const affordable = !!cfg && this.energy >= cost && this.trophies >= tCost;

    if (isLargeBuilding(kind)) {
      const direct = relToHexRound(relX, relY);
      let chosenHex = direct;
      if (!useExistingSnap) {
        const directFree =
          openHexes.has(hexKey(direct.q, direct.r)) &&
          ![0, 1, 2, 3, 4, 5].some((s) => occupiedTris.has(triKey(direct.q, direct.r, s)));
        if (!directFree) {
          let bestHex: HexCoord | null = null;
          let bestD = 50;
          for (const h of openHexes.values()) {
            const busy = [0, 1, 2, 3, 4, 5].some((s) => occupiedTris.has(triKey(h.q, h.r, s)));
            if (busy) continue;
            const c = hexToRel(h.q, h.r);
            const d = Math.hypot(c.x - relX, c.y - relY);
            if (d <= bestD) {
              bestD = d;
              bestHex = h;
            }
          }
          if (bestHex) chosenHex = bestHex;
        }
      }

      const centerRel = hexToRel(chosenHex.q, chosenHex.r);
      const insideTerritory = openHexes.has(hexKey(chosenHex.q, chosenHex.r));
      const overlapsExisting = [0, 1, 2, 3, 4, 5].some((s) =>
        occupiedTris.has(triKey(chosenHex.q, chosenHex.r, s)),
      );
      const footprintValid = insideTerritory && !overlapsExisting;
      return {
        isLarge: true,
        centerRel,
        rhombusVerts: null,
        insideTerritory,
        overlapsExisting,
        footprintValid,
        withinLimit,
        affordable,
        valid: !!cfg && footprintValid && withinLimit && affordable,
      };
    }

    // Small building (Rhombus = 2 triangles)
    let freeRh: Rhombus | null = null;
    let freeDist = useExistingSnap ? 6 : 55;
    let anyRh: Rhombus | null = null;
    let anyDist = Infinity;

    for (const rh of rhombuses) {
      const d = Math.hypot(rh.center.x - relX, rh.center.y - relY);
      if (d < anyDist) {
        anyDist = d;
        anyRh = rh;
      }
      const busy =
        occupiedTris.has(triKey(rh.tris[0].q, rh.tris[0].r, rh.tris[0].s)) ||
        occupiedTris.has(triKey(rh.tris[1].q, rh.tris[1].r, rh.tris[1].s));
      if (!busy && d <= freeDist) {
        freeDist = d;
        freeRh = rh;
      }
    }

    const picked = freeRh ?? (anyDist <= 42 ? anyRh : null);
    if (!picked) {
      const fallbackVerts = findLocalRhombusVerts(relX, relY);
      return {
        isLarge: false,
        centerRel: { x: relX, y: relY },
        rhombusVerts: fallbackVerts,
        insideTerritory: false,
        overlapsExisting: false,
        footprintValid: false,
        withinLimit,
        affordable,
        valid: false,
      };
    }

    const overlapsExisting =
      occupiedTris.has(triKey(picked.tris[0].q, picked.tris[0].r, picked.tris[0].s)) ||
      occupiedTris.has(triKey(picked.tris[1].q, picked.tris[1].r, picked.tris[1].s));
    const footprintValid = !overlapsExisting;

    return {
      isLarge: false,
      centerRel: picked.center,
      rhombusVerts: picked.verts,
      insideTerritory: true,
      overlapsExisting,
      footprintValid,
      withinLimit,
      affordable,
      valid: !!cfg && footprintValid && withinLimit && affordable,
    };
  }

  canPlaceBuilding(kind: number, relX: number, relY: number): boolean {
    const p = this.resolveBuildPreview(kind, relX, relY, true);
    return p ? p.valid : false;
  }

  collectBuildingsByKind(kind: number): SnapshotBuilding[] {
    if (this.playerId === null) return [];
    const list: SnapshotBuilding[] = [];
    for (const b of this.buildings.values()) {
      if (b.ownerId === this.playerId && b.kind === kind) list.push(b);
    }
    return list;
  }

  isInCombat(nowMs = performance.now()): boolean {
    return nowMs - this.lastDamagedAtMs < 6000;
  }

  dynamicBuildTrophyCost(kind: number, extraBeacons = 0): number {
    if (kind === 5) {
      const existing = this.collectBuildingsByKind(5).length + extraBeacons;
      return Math.round(90 * Math.pow(1.65, existing));
    }
    const cfg = getBuildingConfigByKind(this.balance, kind);
    return cfg?.levels[0]?.trophy_cost ?? 0;
  }

  canUpgradeHq(): boolean {
    if (this.playerId === null) return false;
    const hqLevel = this.getHqLevel();
    const hqCfg = getBuildingConfigByKind(this.balance, 0);
    const next = hqCfg?.levels[hqLevel];
    if (!next) return false;
    return this.energy >= (next.cost ?? 0) && this.trophies >= (next.trophy_cost ?? 0);
  }

  computeSellRefund(b: SnapshotBuilding): number {
    if (b.kind === 0) return 0;
    const cfg = getBuildingConfigByKind(this.balance, b.kind);
    if (!cfg) return 0;
    let invested = 0;
    for (let i = 0; i < b.level; i++) {
      invested += cfg.levels[i]?.cost ?? 0;
    }
    return Math.round(invested * (this.balance.world.sell_refund_ratio ?? 0.75));
  }

  getEnergyCap(): number {
    const hqLevel = this.getHqLevel();
    const hqCfg = getBuildingConfigByKind(this.balance, 0);
    const cfgCap = hqCfg?.levels[hqLevel - 1]?.energy_cap ?? 0;
    if (cfgCap > 0) return cfgCap;
    if (hqLevel <= 1) return 400;
    if (hqLevel === 2) return 950;
    return 2000;
  }

  getTrophyCap(): number {
    const hqLevel = this.getHqLevel();
    const hqCfg = getBuildingConfigByKind(this.balance, 0);
    const cfgCap = hqCfg?.levels[hqLevel - 1]?.trophy_cap ?? 0;
    if (cfgCap > 0) return cfgCap;
    if (hqLevel <= 1) return 200;
    if (hqLevel === 2) return 420;
    return 600;
  }

  getSquadSummaries(): Array<{ squad: number; count: number }> {
    if (this.playerId === null) return [];
    const counts = new Map<number, number>();
    for (const u of this.units.values()) {
      if (u.ownerId !== this.playerId || u.kind === 4) continue;
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

  getDroneFocusTarget(alpha: number): Point2D | null {
    if (this.playerId === null) return null;
    const now = performance.now();

    let targetSquad = this.activeSquad;
    if (targetSquad === 0) {
      let firstCombat: number | null = null;
      for (const u of this.units.values()) {
        if (u.ownerId === this.playerId && u.kind !== 4 && u.squad > 0) {
          if (firstCombat === null || u.squad < firstCombat) firstCombat = u.squad;
        }
      }
      if (firstCombat !== null) targetSquad = firstCombat;
    }

    const pts: Point2D[] = [];
    for (const u of this.units.values()) {
      if (u.ownerId !== this.playerId || u.kind === 4 || u.squad !== targetSquad) continue;
      const prev = this.prevUnits.get(u.id);
      const x = prev ? prev.x + (u.x - prev.x) * alpha : u.x;
      const y = prev ? prev.y + (u.y - prev.y) * alpha : u.y;
      pts.push({ x, y });
    }

    if (pts.length > 0) {
      // Focus on the forward/lead cluster (within 360px of the unit closest to the squad order or last centroid)
      // so newly spawned reinforcements streaming from base don't drag the camera backward.
      const refPt = this.squadOrders.get(targetSquad) ?? this.lastSquadCentroid;
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
      this.lastSquadCentroid = pt;
      return pt;
    }

    if (this.lostSquadHold && now < this.lostSquadHold.untilMs) {
      return { x: this.lostSquadHold.x, y: this.lostSquadHold.y };
    }
    this.lostSquadHold = null;
    return null;
  }

  getVisionCircles(
    alpha = 1,
    zonePositions?: Map<number, { x: number; y: number; radius: number }>,
  ): VisionCircle[] {
    const circles: VisionCircle[] = [];
    if (this.playerId === null) return circles;

    const zPos = zonePositions?.get(this.playerId);
    const ownZone = zPos ?? this.zones.get(this.playerId);
    if (ownZone) {
      let zx = ownZone.x;
      let zy = ownZone.y;
      if (!zPos) {
        const prevZ = this.prevZones.get(this.playerId);
        if (prevZ) {
          zx = prevZ.x + (ownZone.x - prevZ.x) * alpha;
          zy = prevZ.y + (ownZone.y - prevZ.y) * alpha;
        }
      }
      circles.push({ x: zx, y: zy, r: ownZone.radius + 360 });
      for (const b of this.buildings.values()) {
        if (b.ownerId === this.playerId && b.kind === 5) {
          circles.push({
            x: zx + b.relX,
            y: zy + b.relY,
            r: beaconExtRadius(b.level) + 220,
          });
        }
      }
    }

    for (const u of this.units.values()) {
      if (u.ownerId !== this.playerId || u.kind === 4) continue;
      const prev = this.prevUnits.get(u.id);
      const x = prev ? prev.x + (u.x - prev.x) * alpha : u.x;
      const y = prev ? prev.y + (u.y - prev.y) * alpha : u.y;
      circles.push({ x, y, r: 290 });
    }

    return circles;
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

  computeSellTrophyRefund(b: SnapshotBuilding): number {
    if (b.kind === 0) return 0;
    const cfg = getBuildingConfigByKind(this.balance, b.kind);
    if (!cfg) return 0;
    let trophies = b.kind === 5 ? 90 : (cfg.levels[0]?.trophy_cost ?? 0);
    for (let i = 1; i < b.level; i++) {
      trophies += cfg.levels[i]?.trophy_cost ?? 0;
    }
    return Math.round(trophies * 0.5);
  }

  equilibriumRadius(): number {
    return this.rEq;
  }

  computeTeleportCost(tx: number, ty: number): number {
    if (this.playerId === null) return 0;
    const zone = this.zones.get(this.playerId);
    if (!zone) return 0;
    const dist = Math.hypot(tx - zone.x, ty - zone.y);
    const targetR = Math.hypot(tx, ty);
    const rEq = this.rEq;
    const base = this.balance.world.teleport_base_cost ?? 150;
    const distFactor = this.balance.world.teleport_dist_factor ?? 0.11;
    const basePenalty = this.balance.world.teleport_ring_penalty ?? 0.30;
    const ringPen = targetR < rEq ? basePenalty * 3.0 : basePenalty;
    const raw = base + dist * distFactor + Math.abs(targetR - rEq) * ringPen;
    return Math.round(raw);
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
    this.lostSquadHold = null;
    this.buildKind = null;
    this.actionMode = null;
    this.rallySelectMode = false;
    this.teleportSelectMode = false;
    this.pendingTeleport = null;
    this.selectedBuildingId = null;
    this.cameraAnchor = "hq";
    this.playerNames.clear();
    this.leaderboard = [];
    this.prevZones.clear();
    this.zones.clear();
    this.buildings.clear();
    this.prevUnits.clear();
    this.units.clear();
    this.prevProjectiles.clear();
    this.projectiles.clear();
    this.effects = [];
    this.buildingHitEffects.clear();
    this.teleportFlashes = [];
    this.combatHeats = [];
    this.lastDamagedAtMs = -100_000;
  }
}
