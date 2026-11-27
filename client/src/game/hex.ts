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
