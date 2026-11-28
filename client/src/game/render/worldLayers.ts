import {
  HEX_EDGE_DIRS,
  HEX_SIZE,
  hexKey,
  hexToRel,
  hexVertex,
  isLargeBuilding,
  triKey,
  type ClientGameState,
  type VisionCircle,
} from "../state.ts";
import { drawHexPolygon, isCircleInVision, type ZonePosition } from "./primitives.ts";

export function drawGrid(
  ctx: CanvasRenderingContext2D,
  camX: number,
  camY: number,
  zoom: number,
  width: number,
  height: number,
  mapRadius: number,
): void {
  const step = 100;
  const halfW = width / (2 * zoom);
  const halfH = height / (2 * zoom);

  const minX = Math.max(-mapRadius, Math.floor((camX - halfW) / step) * step);
  const maxX = Math.min(mapRadius, Math.ceil((camX + halfW) / step) * step);
  const minY = Math.max(-mapRadius, Math.floor((camY - halfH) / step) * step);
  const maxY = Math.min(mapRadius, Math.ceil((camY + halfH) / step) * step);

  ctx.strokeStyle = "#1a1d23";
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let x = minX; x <= maxX; x += step) {
    ctx.moveTo(x, minY);
    ctx.lineTo(x, maxY);
  }
  for (let y = minY; y <= maxY; y += step) {
    ctx.moveTo(minX, y);
    ctx.lineTo(maxX, y);
  }
  ctx.stroke();
}

export function drawArenaBorder(ctx: CanvasRenderingContext2D, mapRadius: number): void {
  ctx.strokeStyle = "#c95b62";
  ctx.lineWidth = 5;
  ctx.beginPath();
  ctx.arc(0, 0, mapRadius, 0, Math.PI * 2);
  ctx.stroke();
}

export function drawCoreAndRings(ctx: CanvasRenderingContext2D, state: ClientGameState): void {
  const ring3R = state.balance.world.ring3_radius;
  const ring2R = state.balance.world.ring2_radius;

  // Dense central Ring 3 tint (no empty core void)
  ctx.fillStyle = "rgba(186, 165, 224, 0.045)";
  ctx.beginPath();
  ctx.arc(0, 0, ring3R, 0, Math.PI * 2);
  ctx.fill();

  // Subtle equilibrium radius ring for own player
  if (state.playerId !== null) {
    const rEq = state.equilibriumRadius();
    ctx.strokeStyle = "rgba(168, 199, 250, 0.14)";
    ctx.lineWidth = 1.5;
    ctx.setLineDash([12, 12]);
    ctx.beginPath();
    ctx.arc(0, 0, rEq, 0, Math.PI * 2);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  // Ring boundaries: Ring 3 (center, 0..ring3R), Ring 2 (ring3R..ring2R), Ring 1 (outer, ring2R..mapR)
  ctx.strokeStyle = "rgba(186, 165, 224, 0.28)";
  ctx.lineWidth = 2;
  ctx.setLineDash([16, 12]);
  ctx.beginPath();
  ctx.arc(0, 0, ring3R, 0, Math.PI * 2);
  ctx.stroke();

  ctx.strokeStyle = "rgba(168, 199, 250, 0.20)";
  ctx.beginPath();
  ctx.arc(0, 0, ring2R, 0, Math.PI * 2);
  ctx.stroke();
  ctx.setLineDash([]);

  // Ring labels
  ctx.fillStyle = "rgba(194, 199, 207, 0.44)";
  ctx.font = "600 13px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.fillText("Кольцо 3 · Центр (Ядро 3+ / Узлы)", 0, -(ring3R - 22));
  ctx.fillText("Кольцо 2 · Мидгейм (Ядро 2)", 0, -(ring2R - 22));
  ctx.fillText("Кольцо 1 · Старт (Ядро 1)", 0, -(state.mapRadius - 36));
}

export function drawZones(
  ctx: CanvasRenderingContext2D,
  state: ClientGameState,
  positions: Map<number, ZonePosition>,
  visionCircles: VisionCircle[],
  now: number,
): void {
  for (const z of state.zones.values()) {
    const pos = positions.get(z.playerId);
    if (!pos) continue;
    const { x, y } = pos;

    const isOwn = z.playerId === state.playerId;
    if (!isOwn && !isCircleInVision(x, y, z.radius, visionCircles)) {
      continue;
    }
    const openLarge = state.getOpenLargeHexes(z.playerId);

    // Fill polygonal territory from open large hexes (no circular zone!)
    ctx.fillStyle = isOwn ? "rgba(168, 199, 250, 0.08)" : "rgba(242, 139, 130, 0.08)";
    for (const hex of openLarge.values()) {
      const p = hexToRel(hex.q, hex.r);
      drawHexPolygon(ctx, x + p.x, y + p.y, HEX_SIZE, ctx.fillStyle, undefined, 0);
    }

    // Draw outer hexagonal perimeter (boundary edges only)
    let minTopY = y;
    ctx.strokeStyle = isOwn ? "rgba(168, 199, 250, 0.78)" : "rgba(242, 139, 130, 0.78)";
    ctx.lineWidth = 2.2;
    ctx.beginPath();
    for (const hex of openLarge.values()) {
      const p = hexToRel(hex.q, hex.r);
      const cx = x + p.x;
      const cy = y + p.y;
      minTopY = Math.min(minTopY, cy - HEX_SIZE);
      for (let i = 0; i < 6; i++) {
        const [dq, dr] = HEX_EDGE_DIRS[i];
        if (openLarge.has(hexKey(hex.q + dq, hex.r + dr))) continue;
        const a0 = (Math.PI / 3) * i - Math.PI / 6;
        const a1 = (Math.PI / 3) * (i + 1) - Math.PI / 6;
        ctx.moveTo(cx + Math.cos(a0) * HEX_SIZE, cy + Math.sin(a0) * HEX_SIZE);
        ctx.lineTo(cx + Math.cos(a1) * HEX_SIZE, cy + Math.sin(a1) * HEX_SIZE);
      }
    }
    ctx.stroke();

    // Spawn protection dashed hex outer halo
    if ((z.flags & 1) !== 0) {
      const pulse = 0.4 + 0.2 * Math.sin(now * 0.005);
      ctx.save();
      ctx.strokeStyle = `rgba(194, 199, 207, ${pulse.toFixed(2)})`;
      ctx.lineWidth = 2;
      ctx.setLineDash([8, 6]);
      ctx.beginPath();
      const haloR = HEX_SIZE + 6;
      for (const hex of openLarge.values()) {
        const p = hexToRel(hex.q, hex.r);
        const cx = x + p.x;
        const cy = y + p.y;
        for (let i = 0; i < 6; i++) {
          const [dq, dr] = HEX_EDGE_DIRS[i];
          if (openLarge.has(hexKey(hex.q + dq, hex.r + dr))) continue;
          const a0 = (Math.PI / 3) * i - Math.PI / 6;
          const a1 = (Math.PI / 3) * (i + 1) - Math.PI / 6;
          ctx.moveTo(cx + Math.cos(a0) * haloR, cy + Math.sin(a0) * haloR);
          ctx.lineTo(cx + Math.cos(a1) * haloR, cy + Math.sin(a1) * haloR);
        }
      }
      ctx.stroke();
      ctx.restore();
    }

    const name = state.playerNames.get(z.playerId);
    if (name) {
      const prefix = (z.flags & 4) !== 0 ? "👑 " : "";
      ctx.fillStyle = (z.flags & 4) !== 0 ? "#e8c584" : "#e3e5e8";
      ctx.font = "600 13px system-ui, sans-serif";
      ctx.textAlign = "center";
      ctx.fillText(`${prefix}${name}`, x, minTopY - 10);
    }
  }

  // Beacon link lines only for visible bases
  for (const b of state.buildings.values()) {
    if (b.kind !== 5) continue;
    const zone = positions.get(b.ownerId);
    if (!zone) continue;
    const isOwn = b.ownerId === state.playerId;
    if (!isOwn && !isCircleInVision(zone.x + b.relX, zone.y + b.relY, 24, visionCircles)) {
      continue;
    }

    const myDist = Math.hypot(b.relX, b.relY);
    let parentX = 0;
    let parentY = 0;
    let bestDist = Infinity;
    for (const ob of state.buildings.values()) {
      if (ob.ownerId !== b.ownerId || ob.kind !== 5 || ob.id === b.id) continue;
      if (Math.hypot(ob.relX, ob.relY) >= myDist) continue;
      const d = Math.hypot(b.relX - ob.relX, b.relY - ob.relY);
      if (d < bestDist) {
        bestDist = d;
        parentX = ob.relX;
        parentY = ob.relY;
      }
    }
    if (myDist < bestDist) {
      parentX = 0;
      parentY = 0;
    }
    ctx.strokeStyle = isOwn ? "rgba(168, 199, 250, 0.26)" : "rgba(242, 139, 130, 0.26)";
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.moveTo(zone.x + parentX, zone.y + parentY);
    ctx.lineTo(zone.x + b.relX, zone.y + b.relY);
    ctx.stroke();
  }
}

export function drawOwnHexGrid(
  ctx: CanvasRenderingContext2D,
  state: ClientGameState,
  zonePositions: Map<number, ZonePosition>,
): void {
  if (state.playerId === null) return;
  const zone = zonePositions.get(state.playerId);
  if (!zone) return;

  const openLarge = state.getOpenLargeHexes();
  const occupiedTris = state.getOccupiedTris(state.playerId);
  const buildKind = state.buildKind;

  if (buildKind === null) {
    for (const hex of openLarge.values()) {
      const pos = hexToRel(hex.q, hex.r);
      drawHexPolygon(
        ctx,
        zone.x + pos.x,
        zone.y + pos.y,
        HEX_SIZE,
        undefined,
        "rgba(168, 199, 250, 0.11)",
        1,
      );
    }
    return;
  }

  if (isLargeBuilding(buildKind)) {
    for (const hex of openLarge.values()) {
      const pos = hexToRel(hex.q, hex.r);
      const cx = zone.x + pos.x;
      const cy = zone.y + pos.y;
      const anyOccupied = [0, 1, 2, 3, 4, 5].some((s) =>
        occupiedTris.has(triKey(hex.q, hex.r, s)),
      );
      if (!anyOccupied) {
        drawHexPolygon(
          ctx,
          cx,
          cy,
          HEX_SIZE - 1,
          "rgba(168, 199, 250, 0.08)",
          "rgba(168, 199, 250, 0.44)",
          1.3,
        );
        ctx.fillStyle = "rgba(168, 199, 250, 0.42)";
        ctx.beginPath();
        ctx.arc(cx, cy, 2.2, 0, Math.PI * 2);
        ctx.fill();
      } else {
        drawHexPolygon(
          ctx,
          cx,
          cy,
          HEX_SIZE,
          "rgba(13, 14, 17, 0.24)",
          "rgba(168, 199, 250, 0.12)",
          1,
        );
      }
    }
  } else {
    // Draw the 3 non-overlapping rhombus sectors (0..2) inside each open hex
    for (const hex of openLarge.values()) {
      const c = hexToRel(hex.q, hex.r);
      const cx = zone.x + c.x;
      const cy = zone.y + c.y;
      for (let sector = 0; sector < 3; sector++) {
        const s = sector * 2;
        const sNext = s + 1;
        const v0 = hexVertex(hex.q, hex.r, s);
        const v1 = hexVertex(hex.q, hex.r, sNext);
        const v2 = hexVertex(hex.q, hex.r, (s + 2) % 6);
        const busy =
          occupiedTris.has(triKey(hex.q, hex.r, s)) ||
          occupiedTris.has(triKey(hex.q, hex.r, sNext));
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.lineTo(zone.x + v0.x, zone.y + v0.y);
        ctx.lineTo(zone.x + v1.x, zone.y + v1.y);
        ctx.lineTo(zone.x + v2.x, zone.y + v2.y);
        ctx.closePath();
        ctx.fillStyle = busy ? "rgba(13, 14, 17, 0.24)" : "rgba(168, 199, 250, 0.06)";
        ctx.fill();
        ctx.strokeStyle = busy ? "rgba(168, 199, 250, 0.09)" : "rgba(168, 199, 250, 0.28)";
        ctx.lineWidth = busy ? 0.8 : 1.05;
        ctx.stroke();
      }
    }
  }
}

export function drawVisionFog(
  ctx: CanvasRenderingContext2D,
  fogCtx: CanvasRenderingContext2D,
  fogCanvas: HTMLCanvasElement,
  width: number,
  height: number,
  camX: number,
  camY: number,
  zoom: number,
  state: ClientGameState,
  visionCircles: VisionCircle[],
): void {
  if (state.playerId === null || visionCircles.length === 0) return;
  fogCtx.globalCompositeOperation = "source-over";
  fogCtx.clearRect(0, 0, width, height);
  fogCtx.fillStyle = "rgba(8, 10, 15, 0.56)";
  fogCtx.fillRect(0, 0, width, height);

  fogCtx.globalCompositeOperation = "destination-out";
  for (let i = 0; i < visionCircles.length; i++) {
    const c = visionCircles[i];
    const sx = (c.x - camX) * zoom + width / 2;
    const sy = (c.y - camY) * zoom + height / 2;
    const sr = c.r * zoom;
    if (sx + sr < 0 || sx - sr > width || sy + sr < 0 || sy - sr > height) {
      continue;
    }
    const grad = fogCtx.createRadialGradient(sx, sy, sr * 0.72, sx, sy, sr);
    grad.addColorStop(0, "rgba(0, 0, 0, 1)");
    grad.addColorStop(1, "rgba(0, 0, 0, 0)");
    fogCtx.fillStyle = grad;
    fogCtx.beginPath();
    fogCtx.arc(sx, sy, sr, 0, Math.PI * 2);
    fogCtx.fill();
  }

  ctx.drawImage(fogCanvas, 0, 0, width, height);
}
