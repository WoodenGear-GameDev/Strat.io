import { getBuildingConfigByKind, getNodeHexRadius } from "../balance.ts";
import {
  collectRhombuses,
  coreHexRadius,
  findLocalRhombusVerts,
  hexKey,
  hexToRel,
  insertHexDisk,
  isLargeBuilding,
  relToHexRound,
  triKey,
  triNeighbors,
  type HexCoord,
  type Rhombus,
} from "../hex.ts";
import type { ClientGameState } from "./index.ts";
import type { BuildPreview } from "./types.ts";

export function getOpenLargeHexesForState(
  state: ClientGameState,
  ownerId: number | null = state.playerId,
): Map<string, HexCoord> {
  const open = new Map<string, HexCoord>();
  if (ownerId === null) return open;

  let hqLevel = 1;
  for (const b of state.buildings.values()) {
    if (b.ownerId === ownerId && b.kind === 0) {
      hqLevel = b.level;
      break;
    }
  }

  const coreR = coreHexRadius(hqLevel);
  insertHexDisk(0, 0, coreR, open);

  if (hqLevel >= 3) {
    const nodeHexR = getNodeHexRadius(state.balance);
    for (const b of state.buildings.values()) {
      if (b.ownerId === ownerId && b.kind === 5) {
        const centerAnchor = relToHexRound(b.relX, b.relY);
        insertHexDisk(centerAnchor.q, centerAnchor.r, nodeHexR, open);
      }
    }
  }

  return open;
}

export function getOccupiedTrisForState(
  state: ClientGameState,
  ownerId: number | null = state.playerId,
  rhombuses?: Rhombus[],
): Set<string> {
  const occupied = new Set<string>();
  if (ownerId === null) return occupied;
  const open = getOpenLargeHexesForState(state, ownerId);
  const list = rhombuses ?? collectRhombuses(open);

  for (const b of state.buildings.values()) {
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

export function resolveBuildPreviewForState(
  state: ClientGameState,
  kind: number,
  relX: number,
  relY: number,
  useExistingSnap = false,
): BuildPreview | null {
  if (state.playerId === null || kind === 0) return null;
  const openHexes = getOpenLargeHexesForState(state);
  const rhombuses = collectRhombuses(openHexes);
  const occupiedTris = getOccupiedTrisForState(state, state.playerId, rhombuses);
  const cfg = getBuildingConfigByKind(state.balance, kind);

  let withinLimit = !(kind === 5 && state.getHqLevel() < 3);
  if (withinLimit && cfg?.max_per_player !== undefined) {
    let count = 0;
    for (const b of state.buildings.values()) {
      if (b.ownerId === state.playerId && b.kind === kind) count++;
    }
    withinLimit = count < cfg.max_per_player;
  }

  const cost = cfg?.levels[0]?.cost ?? 0;
  const tCost = state.dynamicBuildTrophyCost(kind);
  const affordable = !!cfg && state.energy >= cost && state.trophies >= tCost;

  const isHexAdjacent = (q: number, r: number): boolean => {
    for (let s = 0; s < 6; s++) {
      for (const nt of triNeighbors(q, r, s)) {
        if (occupiedTris.has(triKey(nt.q, nt.r, nt.s))) return true;
      }
    }
    return false;
  };

  const isRhombusAdjacent = (rh: Rhombus): boolean => {
    for (const t of rh.tris) {
      for (const nt of triNeighbors(t.q, t.r, t.s)) {
        if (occupiedTris.has(triKey(nt.q, nt.r, nt.s))) return true;
      }
    }
    return false;
  };

  if (isLargeBuilding(kind)) {
    const direct = relToHexRound(relX, relY);
    let chosenHex = direct;
    if (!useExistingSnap) {
      const directFree =
        openHexes.has(hexKey(direct.q, direct.r)) &&
        ![0, 1, 2, 3, 4, 5].some((s) => occupiedTris.has(triKey(direct.q, direct.r, s))) &&
        isHexAdjacent(direct.q, direct.r);
      if (!directFree) {
        let bestHex: HexCoord | null = null;
        let bestD = 50;
        for (const h of openHexes.values()) {
          const busy = [0, 1, 2, 3, 4, 5].some((s) => occupiedTris.has(triKey(h.q, h.r, s)));
          if (busy || !isHexAdjacent(h.q, h.r)) continue;
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
    const adjacent = isHexAdjacent(chosenHex.q, chosenHex.r);
    const footprintValid = insideTerritory && !overlapsExisting && adjacent;
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

  // Small building (Rhombus = 2 triangles inside a single hex)
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
    if (!busy && isRhombusAdjacent(rh) && d <= freeDist) {
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
  const footprintValid = !overlapsExisting && isRhombusAdjacent(picked);

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
