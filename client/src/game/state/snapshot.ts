import type { ServerMessage } from "../protocol.ts";
import type { ClientGameState } from "./index.ts";

export function recordCombatHeat(
  state: ClientGameState,
  x: number,
  y: number,
  weight: number,
  now: number,
): void {
  for (const h of state.combatHeats) {
    if (Math.hypot(h.x - x, h.y - y) <= 320) {
      h.x = h.x * 0.7 + x * 0.3;
      h.y = h.y * 0.7 + y * 0.3;
      h.power = Math.min(12, h.power + weight);
      h.untilMs = Math.max(h.untilMs, now + 7500);
      return;
    }
  }
  state.combatHeats.push({
    x,
    y,
    power: weight,
    showAtMs: now + 1200,
    untilMs: now + 7500,
  });
}

export function applySnapshotToState(
  state: ClientGameState,
  msg: Extract<ServerMessage, { type: "Snapshot" }>,
): void {
  const now = performance.now();
  const baseInterval = 1000 / Math.max(1, state.tickRate);
  if (state.lastSnapArrivalMs > 0) {
    const rawInterval = now - state.lastSnapArrivalMs;
    if (rawInterval >= 15 && rawInterval <= 500) {
      const minInterval = Math.max(30, baseInterval * 0.8);
      const smoothed = state.tickIntervalMs * 0.75 + rawInterval * 0.25;
      state.tickIntervalMs = Math.max(minInterval, Math.min(400, smoothed));
    }
  } else {
    state.tickIntervalMs = Math.max(30, Math.min(400, baseInterval));
  }
  state.lastSnapArrivalMs = now;
  state.snapshotTime = now;

  // Double-buffer units
  const nextUnits = state.prevUnits;
  nextUnits.clear();
  for (const u of msg.units) {
    nextUnits.set(u.id, u);
    if (u.ownerId === state.playerId) {
      const oldU = state.units.get(u.id);
      if (oldU && u.hp < oldU.hp) {
        state.lastDamagedAtMs = now;
      }
    }
  }

  for (const [id, oldU] of state.units) {
    if (!nextUnits.has(id)) {
      if (oldU.ownerId === state.playerId) {
        state.lastDamagedAtMs = now;
      }
      recordCombatHeat(state, oldU.x, oldU.y, 1.0, now);
      state.effects.push({
        x: oldU.x,
        y: oldU.y,
        radius: 4,
        maxRadius: 22,
        color: oldU.ownerId === state.playerId ? "#6bb8ff" : "#ff6b6b",
        alpha: 0.9,
      });
    }
  }

  // Double-buffer zones
  const nextZones = state.prevZones;
  nextZones.clear();
  for (const z of msg.zones) {
    nextZones.set(z.playerId, z);
    const oldZ = state.zones.get(z.playerId);
    if (oldZ && Math.hypot(z.x - oldZ.x, z.y - oldZ.y) > 220) {
      state.teleportFlashes.push({ x: oldZ.x, y: oldZ.y, startMs: now, untilMs: now + 6500 });
      state.teleportFlashes.push({ x: z.x, y: z.y, startMs: now, untilMs: now + 6500 });
    }
  }

  // Double-buffer buildings
  const nextBuildings = state.buildingBuf;
  nextBuildings.clear();
  for (const b of msg.buildings) {
    nextBuildings.set(b.id, b);
    const oldB = state.buildings.get(b.id);
    if (oldB && b.hp < oldB.hp) {
      if (b.ownerId === state.playerId) {
        state.lastDamagedAtMs = now;
      }
      state.buildingHitEffects.set(b.id, { startMs: now, untilMs: now + 220 });
      const z = nextZones.get(b.ownerId) ?? state.zones.get(b.ownerId);
      if (z) {
        const bx = z.x + b.relX;
        const by = z.y + b.relY;
        recordCombatHeat(state, bx, by, 0.7, now);
        for (let k = 0; k < 2; k++) {
          const ang = Math.random() * Math.PI * 2;
          const off = 8 + Math.random() * 14;
          state.effects.push({
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

  for (const [id, oldB] of state.buildings) {
    if (!nextBuildings.has(id)) {
      state.buildingHitEffects.delete(id);
      const z = state.zones.get(oldB.ownerId);
      if (z) {
        recordCombatHeat(state, z.x + oldB.relX, z.y + oldB.relY, 2.5, now);
        state.effects.push({
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

  state.tick = msg.tick;
  state.score = msg.score;
  state.rEq = msg.rEq;
  state.energy = msg.energy;
  state.trophies = msg.trophies;
  state.unitCount = msg.unitCount;
  state.unitCap = msg.unitCap;
  state.teleportCdSec = msg.teleportCdSec;

  // Swap double-buffered maps
  state.prevZones = state.zones;
  state.zones = nextZones;

  state.buildingBuf = state.buildings;
  state.buildings = nextBuildings;

  state.prevUnits = state.units;
  state.units = nextUnits;

  // Track squad continuity, auto-merges, and short hold on squad wipe
  if (state.playerId !== null) {
    const liveSquads = new Map<number, { sx: number; sy: number; cnt: number }>();
    for (const u of nextUnits.values()) {
      if (u.ownerId !== state.playerId) continue;
      const cur = liveSquads.get(u.squad) ?? { sx: 0, sy: 0, cnt: 0 };
      cur.sx += u.x;
      cur.sy += u.y;
      cur.cnt += 1;
      liveSquads.set(u.squad, cur);
    }
    for (const sid of Array.from(state.squadOrders.keys())) {
      if (!liveSquads.has(sid) && sid !== state.reinforceSquad) {
        state.squadOrders.delete(sid);
      }
    }
    if (state.pendingNewSquadTarget) {
      let newSid: number | null = null;
      for (const [sid, info] of liveSquads) {
        if (sid <= 0 || info.cnt === 0) continue;
        if (!state.pendingNewSquadPrevIds?.has(sid)) {
          if (newSid === null || sid > newSid) newSid = sid;
        }
      }
      if (newSid !== null) {
        state.activeSquad = newSid;
        state.squadOrders.set(newSid, state.pendingNewSquadTarget);
        state.pendingNewSquadTarget = null;
        state.pendingNewSquadPrevIds = null;
        state.lostSquadHold = null;
      }
    }
    if (state.activeSquad > 0) {
      const cur = liveSquads.get(state.activeSquad);
      if (cur && cur.cnt > 0) {
        state.lostSquadHold = null;
      } else if (state.activeSquad === state.reinforceSquad) {
        state.lostSquadHold = null;
      } else if (state.lastSquadCentroid) {
        let mergedId: number | null = null;
        let bestDist = 230;
        for (const [sid, info] of liveSquads) {
          if (sid <= 0 || info.cnt === 0) continue;
          const d = Math.hypot(
            info.sx / info.cnt - state.lastSquadCentroid.x,
            info.sy / info.cnt - state.lastSquadCentroid.y,
          );
          if (d < bestDist) {
            bestDist = d;
            mergedId = sid;
          }
        }
        if (mergedId !== null) {
          state.activeSquad = mergedId;
        } else {
          const ownZone = state.zones.get(state.playerId);
          const returnedToBase =
            ownZone !== undefined &&
            Math.hypot(
              state.lastSquadCentroid.x - ownZone.x,
              state.lastSquadCentroid.y - ownZone.y,
            ) <=
              ownZone.radius + 390;
          if (!returnedToBase && state.cameraAnchor === "drones") {
            state.lostSquadHold = {
              x: state.lastSquadCentroid.x,
              y: state.lastSquadCentroid.y,
              untilMs: now + 1400,
            };
          }
          state.activeSquad = 0;
          state.lastSquadCentroid = null;
        }
      } else {
        state.activeSquad = 0;
      }
    }
  }

  // Double-buffer projectiles
  const nextProjectiles = state.prevProjectiles;
  nextProjectiles.clear();
  for (const p of msg.projectiles) nextProjectiles.set(p.id, p);
  state.prevProjectiles = state.projectiles;
  state.projectiles = nextProjectiles;

  if (state.selectedBuildingId !== null && !state.buildings.has(state.selectedBuildingId)) {
    state.selectedBuildingId = null;
  }
}

export function stepStateEffects(state: ClientGameState, dt: number): void {
  const now = performance.now();
  for (const [id, hit] of state.buildingHitEffects) {
    if (now >= hit.untilMs) {
      state.buildingHitEffects.delete(id);
    }
  }
  state.teleportFlashes = state.teleportFlashes.filter((f) => now < f.untilMs);
  state.combatHeats = state.combatHeats.filter((h) => now < h.untilMs);
  for (let i = state.effects.length - 1; i >= 0; i--) {
    const e = state.effects[i];
    e.radius += (e.maxRadius - e.radius) * Math.min(1, dt * 10);
    e.alpha -= dt * 2.5;
    if (e.alpha <= 0) {
      state.effects.splice(i, 1);
    }
  }
}
