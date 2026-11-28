import { DEFAULT_BALANCE, getNodeHexRadius } from "./balance.ts";

export const HEX_SIZE = DEFAULT_BALANCE.world.hex_size;
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

export interface Rhombus {
  tris: [TriCoord, TriCoord];
  center: Point2D;
  verts: [Point2D, Point2D, Point2D, Point2D];
}

export function hexKey(q: number, r: number): string {
  return `${q},${r}`;
}

export function triKey(q: number, r: number, s: number): string {
  return `${q},${r},${s}`;
}

export function isLargeBuilding(kind: number): boolean {
  return kind === 0 || kind === 2 || kind === 3 || kind === 5;
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

export function triNeighbors(q: number, r: number, s: number): TriCoord[] {
  const [dq, dr] = HEX_EDGE_DIRS[s];
  return [
    { q, r, s: (s + 1) % 6 },
    { q, r, s: (s + 5) % 6 },
    { q: q + dq, r: r + dr, s: (s + 3) % 6 },
  ];
}

export function insertHexDisk(
  centerQ: number,
  centerR: number,
  hexR: number,
  out: Map<string, HexCoord>,
): void {
  for (let dq = -hexR; dq <= hexR; dq++) {
    const minDr = Math.max(-hexR, -dq - hexR);
    const maxDr = Math.min(hexR, -dq + hexR);
    for (let dr = minDr; dr <= maxDr; dr++) {
      const q = centerQ + dq;
      const r = centerR + dr;
      out.set(hexKey(q, r), { q, r });
    }
  }
}

export function collectRhombuses(openHexes: Map<string, HexCoord>): Rhombus[] {
  const out: Rhombus[] = [];
  for (const hex of openHexes.values()) {
    const c = hexToRel(hex.q, hex.r);
    for (let sector = 0; sector < 3; sector++) {
      const s = sector * 2;
      const sNext = s + 1;
      const v0 = hexVertex(hex.q, hex.r, s);
      const v1 = hexVertex(hex.q, hex.r, sNext);
      const v2 = hexVertex(hex.q, hex.r, (s + 2) % 6);

      out.push({
        tris: [
          { q: hex.q, r: hex.r, s },
          { q: hex.q, r: hex.r, s: sNext },
        ],
        center: { x: (c.x + v1.x) * 0.5, y: (c.y + v1.y) * 0.5 },
        verts: [c, v0, v1, v2],
      });
    }
  }
  return out;
}

export function findLocalRhombusVerts(
  relX: number,
  relY: number,
): [Point2D, Point2D, Point2D, Point2D] | null {
  const base = relToHexRound(relX, relY);
  const c = hexToRel(base.q, base.r);
  let bestVerts: [Point2D, Point2D, Point2D, Point2D] | null = null;
  let bestDist = Infinity;

  for (let sector = 0; sector < 3; sector++) {
    const s = sector * 2;
    const sNext = s + 1;
    const v0 = hexVertex(base.q, base.r, s);
    const v1 = hexVertex(base.q, base.r, sNext);
    const v2 = hexVertex(base.q, base.r, (s + 2) % 6);
    const dIn = Math.hypot((c.x + v1.x) * 0.5 - relX, (c.y + v1.y) * 0.5 - relY);
    if (dIn < bestDist) {
      bestDist = dIn;
      bestVerts = [c, v0, v1, v2];
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
  if (level <= 1) return DEFAULT_BALANCE.world.zone_base_radius;
  if (level === 2) return 145;
  return 200;
}

export function beaconExtRadius(_level: number): number {
  return getNodeHexRadius(DEFAULT_BALANCE) * HEX_SIZE * SQRT3;
}

export function beaconExtensionCenter(
  relX: number,
  relY: number,
  _extR?: number,
): Point2D {
  return { x: relX, y: relY };
}
