const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

export interface SnapshotZone {
  playerId: number;
  x: number;
  y: number;
  radius: number;
  teamId: number;
  skinId: number;
  flags: number;
}

export interface SnapshotBuilding {
  id: number;
  ownerId: number;
  kind: number;
  level: number;
  mutation: number;
  meta: number;
  relX: number;
  relY: number;
  hp: number;
  maxHp: number;
  angle: number;
}

export interface SnapshotUnit {
  id: number;
  ownerId: number;
  kind: number;
  squad: number;
  x: number;
  y: number;
  hp: number;
  maxHp: number;
}

export interface SnapshotProjectile {
  id: number;
  ownerId: number;
  kind: number;
  x: number;
  y: number;
}

export interface PlayerMetaEntry {
  playerId: number;
  name: string;
  teamId: number;
  skinId: number;
}

export interface LeaderboardEntry {
  playerId: number;
  score: number;
}

export type ServerMessage =
  | { type: "Welcome"; playerId: number; mapRadius: number; tickRate: number }
  | {
      type: "Snapshot";
      tick: number;
      score: number;
      rEq: number;
      energy: number;
      trophies: number;
      unitCount: number;
      unitCap: number;
      teleportCdSec: number;
      zones: SnapshotZone[];
      buildings: SnapshotBuilding[];
      units: SnapshotUnit[];
      projectiles: SnapshotProjectile[];
    }
  | { type: "PlayerMeta"; players: PlayerMetaEntry[] }
  | { type: "MatchLeaderboard"; entries: LeaderboardEntry[] }
  | {
      type: "GameOver";
      score: number;
      survivalSec: number;
      kills: number;
      buildingsDestroyed: number;
      reason: number;
    }
  | { type: "ActionError"; code: number }
  | { type: "Pong"; clientTimeMs: number; serverTick: number };

export function encodeJoin(authOrName: string, skinId = 0, teamId = 0): Uint8Array {
  const nameBytes = textEncoder.encode(authOrName);
  const buf = new ArrayBuffer(1 + 2 + nameBytes.length + 2 + 1);
  const view = new DataView(buf);
  let offset = 0;

  view.setUint8(offset++, 0x01);
  view.setUint16(offset, nameBytes.length, true);
  offset += 2;
  new Uint8Array(buf, offset, nameBytes.length).set(nameBytes);
  offset += nameBytes.length;
  view.setUint16(offset, skinId, true);
  offset += 2;
  view.setUint8(offset, teamId);

  return new Uint8Array(buf);
}

export function encodeBuild(buildingKind: number, relX: number, relY: number): Uint8Array {
  const buf = new ArrayBuffer(10);
  const view = new DataView(buf);
  view.setUint8(0, 0x02);
  view.setUint8(1, buildingKind);
  view.setFloat32(2, relX, true);
  view.setFloat32(6, relY, true);
  return new Uint8Array(buf);
}

export function encodeUpgradeBuilding(buildingId: number): Uint8Array {
  const buf = new ArrayBuffer(5);
  const view = new DataView(buf);
  view.setUint8(0, 0x03);
  view.setUint32(1, buildingId >>> 0, true);
  return new Uint8Array(buf);
}

export function encodeSellBuilding(buildingId: number): Uint8Array {
  const buf = new ArrayBuffer(5);
  const view = new DataView(buf);
  view.setUint8(0, 0x04);
  view.setUint32(1, buildingId >>> 0, true);
  return new Uint8Array(buf);
}

export function encodeArmyCommand(
  squad: number,
  mode: number,
  targetX: number,
  targetY: number,
): Uint8Array {
  const buf = new ArrayBuffer(11);
  const view = new DataView(buf);
  view.setUint8(0, 0x05);
  view.setUint8(1, squad & 0xff);
  view.setUint8(2, mode);
  view.setFloat32(3, targetX, true);
  view.setFloat32(7, targetY, true);
  return new Uint8Array(buf);
}

export function encodeTeleportZone(targetX: number, targetY: number): Uint8Array {
  const buf = new ArrayBuffer(9);
  const view = new DataView(buf);
  view.setUint8(0, 0x07);
  view.setFloat32(1, targetX, true);
  view.setFloat32(5, targetY, true);
  return new Uint8Array(buf);
}

export function encodePing(clientTimeMs: number): Uint8Array {
  const buf = new ArrayBuffer(5);
  const view = new DataView(buf);
  view.setUint8(0, 0x08);
  view.setUint32(1, clientTimeMs >>> 0, true);
  return new Uint8Array(buf);
}

export function encodeConfigureBuilding(buildingId: number, meta: number): Uint8Array {
  const buf = new ArrayBuffer(6);
  const view = new DataView(buf);
  view.setUint8(0, 0x0a);
  view.setUint32(1, buildingId >>> 0, true);
  view.setUint8(5, meta);
  return new Uint8Array(buf);
}

export function decodeServerMessage(data: Uint8Array): ServerMessage | null {
  if (data.length === 0) return null;
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  let pos = 0;

  const readU8 = (): number => view.getUint8(pos++);
  const readU16 = (): number => {
    const v = view.getUint16(pos, true);
    pos += 2;
    return v;
  };
  const readU32 = (): number => {
    const v = view.getUint32(pos, true);
    pos += 4;
    return v;
  };
  const readF32 = (): number => {
    const v = view.getFloat32(pos, true);
    pos += 4;
    return v;
  };
  const readString = (): string => {
    const len = readU16();
    const slice = data.subarray(pos, pos + len);
    pos += len;
    return textDecoder.decode(slice);
  };

  const opcode = readU8();
  switch (opcode) {
    case 0x81:
      return {
        type: "Welcome",
        playerId: readU32(),
        mapRadius: readF32(),
        tickRate: readU8(),
      };
    case 0x82: {
      const tick = readU32();
      const score = readU32();
      const rEq = readF32();
      const energy = readF32();
      const trophies = readF32();
      const unitCount = readU16();
      const unitCap = readU16();
      const teleportCdSec = readU8();

      const zoneCount = readU16();
      const zones: SnapshotZone[] = [];
      for (let i = 0; i < zoneCount; i++) {
        zones.push({
          playerId: readU32(),
          x: readF32(),
          y: readF32(),
          radius: readF32(),
          teamId: readU8(),
          skinId: readU16(),
          flags: readU8(),
        });
      }

      const buildingCount = readU16();
      const buildings: SnapshotBuilding[] = [];
      for (let i = 0; i < buildingCount; i++) {
        buildings.push({
          id: readU32(),
          ownerId: readU32(),
          kind: readU8(),
          level: readU8(),
          mutation: readU8(),
          meta: readU8(),
          relX: readF32(),
          relY: readF32(),
          hp: readU16(),
          maxHp: readU16(),
          angle: readU8(),
        });
      }

      const unitLen = readU16();
      const units: SnapshotUnit[] = [];
      for (let i = 0; i < unitLen; i++) {
        units.push({
          id: readU32(),
          ownerId: readU32(),
          kind: readU8(),
          squad: readU8(),
          x: readF32(),
          y: readF32(),
          hp: readU16(),
          maxHp: readU16(),
        });
      }

      const projLen = readU16();
      const projectiles: SnapshotProjectile[] = [];
      for (let i = 0; i < projLen; i++) {
        projectiles.push({
          id: readU32(),
          ownerId: readU32(),
          kind: readU8(),
          x: readF32(),
          y: readF32(),
        });
      }

      return {
        type: "Snapshot",
        tick,
        score,
        rEq,
        energy,
        trophies,
        unitCount,
        unitCap,
        teleportCdSec,
        zones,
        buildings,
        units,
        projectiles,
      };
    }
    case 0x83: {
      const count = readU16();
      const players: PlayerMetaEntry[] = [];
      for (let i = 0; i < count; i++) {
        players.push({
          playerId: readU32(),
          name: readString(),
          teamId: readU8(),
          skinId: readU16(),
        });
      }
      return { type: "PlayerMeta", players };
    }
    case 0x84: {
      const count = readU8();
      const entries: LeaderboardEntry[] = [];
      for (let i = 0; i < count; i++) {
        entries.push({
          playerId: readU32(),
          score: readU32(),
        });
      }
      return { type: "MatchLeaderboard", entries };
    }
    case 0x85:
      return {
        type: "GameOver",
        score: readU32(),
        survivalSec: readU32(),
        kills: readU32(),
        buildingsDestroyed: readU32(),
        reason: readU8(),
      };
    case 0x86:
      return {
        type: "ActionError",
        code: readU8(),
      };
    case 0x87:
      return {
        type: "Pong",
        clientTimeMs: readU32(),
        serverTick: readU32(),
      };
    default:
      return null;
  }
}
