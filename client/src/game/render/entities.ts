import { getBuildingConfigByKind, getBuildingRadius } from "../balance.ts";
import {
  HEX_SIZE,
  findLocalRhombusVerts,
  isLargeBuilding,
  type ClientGameState,
  type VisionCircle,
} from "../state.ts";
import {
  drawBar,
  drawHexPolygon,
  drawPolygon,
  drawRhombusPoly,
  isCircleInVision,
  isPointInVision,
  type ZonePosition,
} from "./primitives.ts";

export function drawBuildings(
  ctx: CanvasRenderingContext2D,
  state: ClientGameState,
  zonePositions: Map<number, ZonePosition>,
  now: number,
  visionCircles: VisionCircle[],
): void {
  for (const b of state.buildings.values()) {
    const zone = zonePositions.get(b.ownerId);
    if (!zone) continue;

    const isOwn = b.ownerId === state.playerId;
    const radius = getBuildingRadius(state.balance, b.kind);
    if (!isOwn && !isCircleInVision(zone.x + b.relX, zone.y + b.relY, radius, visionCircles)) {
      continue;
    }

    const hit = state.buildingHitEffects.get(b.id);
    const hitT =
      hit && now < hit.untilMs
        ? Math.max(0, (hit.untilMs - now) / Math.max(1, hit.untilMs - hit.startMs))
        : 0;
    const shakeAmp = hitT * 3.5;
    const shakeX = hitT > 0 ? Math.sin(now * 0.14 + b.id) * shakeAmp : 0;
    const shakeY = hitT > 0 ? Math.cos(now * 0.17 + b.id) * shakeAmp : 0;

    const zx = zone.x + shakeX;
    const zy = zone.y + shakeY;
    const x = zx + b.relX;
    const y = zy + b.relY;
    const cfg = getBuildingConfigByKind(state.balance, b.kind);
    const rhVerts = !isLargeBuilding(b.kind) ? findLocalRhombusVerts(b.relX, b.relY) : null;

    if (b.id === state.selectedBuildingId) {
      if (isLargeBuilding(b.kind) || !rhVerts) {
        drawHexPolygon(ctx, x, y, HEX_SIZE + 2, undefined, "#e8c584", 2);
      } else {
        drawRhombusPoly(ctx, zx, zy, b.relX, b.relY, rhVerts, 1.04, undefined, "#e8c584", 2);
      }

      const range = cfg?.levels[b.level - 1]?.range ?? 0;
      if (range > 0) {
        ctx.strokeStyle = "rgba(232, 197, 132, 0.30)";
        ctx.setLineDash([6, 6]);
        ctx.beginPath();
        ctx.arc(x, y, range, 0, Math.PI * 2);
        ctx.stroke();
        ctx.setLineDash([]);
      }
    }

    ctx.fillStyle = isOwn ? "#4a78c2" : "#c95b62";
    ctx.strokeStyle = "#0e1013";
    ctx.lineWidth = 2;

    switch (b.kind) {
      case 0: // Ядро (Large hex)
        if (isOwn && state.canUpgradeHq()) {
          const pulse = 0.55 + 0.45 * Math.sin(now * 0.008);
          drawHexPolygon(
            ctx,
            x,
            y,
            HEX_SIZE + 3.5 + pulse * 2.5,
            undefined,
            `rgba(255, 209, 102, ${(0.55 + 0.4 * pulse).toFixed(2)})`,
            2.6,
          );
        }
        drawPolygon(ctx, x, y, 30, 6);
        ctx.fillStyle = isOwn ? "#a8c7fa" : "#f2b8b5";
        drawPolygon(ctx, x, y, 10, 6);
        break;
      case 1: // Генератор (2-triangle Rhombus)
        if (rhVerts) {
          drawRhombusPoly(
            ctx,
            zx,
            zy,
            b.relX,
            b.relY,
            rhVerts,
            0.88,
            isOwn ? "#4a78c2" : "#c95b62",
            "#0e1013",
            2,
          );
        } else {
          drawPolygon(ctx, x, y, 18.5, 6);
        }
        ctx.save();
        ctx.translate(x, y);
        ctx.rotate(now * 0.002);
        ctx.fillStyle = "#e8c584";
        drawPolygon(ctx, 0, 0, 8.5, 4);
        ctx.restore();
        break;
      case 2: {
        // Турель (Large hex)
        const rad = (b.angle / 255) * Math.PI * 2;
        drawPolygon(ctx, x, y, 30, 6);
        ctx.fillStyle = isOwn ? "#385b96" : "#a6454c";
        ctx.beginPath();
        ctx.arc(x, y, 16, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
        ctx.save();
        ctx.translate(x, y);
        ctx.rotate(rad);
        ctx.fillStyle = "#c2c7cf";
        ctx.fillRect(0, -4.5, 29, 9);
        ctx.strokeRect(0, -4.5, 29, 9);
        ctx.restore();
        break;
      }
      case 3: // Дроны (Large hex)
        drawPolygon(ctx, x, y, 30, 6);
        ctx.fillStyle = isOwn ? "#284777" : "#87373d";
        drawPolygon(ctx, x, y, 16, 4);
        if ((b.meta & 1) !== 0) {
          ctx.fillStyle = "rgba(18, 19, 22, 0.85)";
          ctx.beginPath();
          ctx.arc(x, y, 11, 0, Math.PI * 2);
          ctx.fill();
          ctx.fillStyle = "#e8c584";
          ctx.fillRect(x - 4.5, y - 5, 3, 10);
          ctx.fillRect(x + 1.5, y - 5, 3, 10);
        }
        break;
      case 5: // Узел (Large 1-hex module)
        ctx.fillStyle = isOwn ? "#3f8ca8" : "#d4556b";
        drawPolygon(ctx, x, y, 30, 6);
        ctx.fillStyle = isOwn ? "#255b70" : "#8f3344";
        drawPolygon(ctx, x, y, 16, 6);
        ctx.fillStyle = isOwn ? "#a8c7fa" : "#f2b8b5";
        drawPolygon(ctx, x, y, 8, 3);
        break;
      default:
        ctx.beginPath();
        ctx.arc(x, y, radius, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
    }

    if (b.level > 1) {
      if (isLargeBuilding(b.kind) || !rhVerts) {
        for (let l = 1; l < b.level; l++) {
          drawHexPolygon(ctx, x, y, 30 - 5 * l, undefined, "#e8c584", 1.3);
        }
      } else {
        for (let l = 1; l < b.level; l++) {
          drawRhombusPoly(
            ctx,
            zx,
            zy,
            b.relX,
            b.relY,
            rhVerts,
            0.88 - 0.18 * l,
            undefined,
            "#e8c584",
            1.3,
          );
        }
      }
    }

    if (hitT > 0) {
      const hitFill = `rgba(255, 220, 185, ${(0.52 * hitT).toFixed(2)})`;
      const hitStroke = `rgba(255, 110, 96, ${(0.92 * hitT).toFixed(2)})`;
      if (isLargeBuilding(b.kind) || !rhVerts) {
        drawHexPolygon(ctx, x, y, 30, hitFill, hitStroke, 2.5);
      } else {
        drawRhombusPoly(ctx, zx, zy, b.relX, b.relY, rhVerts, 0.88, hitFill, hitStroke, 2.2);
      }
    }
  }
}

export function drawUnits(
  ctx: CanvasRenderingContext2D,
  state: ClientGameState,
  alpha: number,
  visionCircles: VisionCircle[],
  unitAngles: Map<number, number>,
): void {
  for (const id of unitAngles.keys()) {
    if (!state.units.has(id)) {
      unitAngles.delete(id);
    }
  }

  const squadCenters = new Map<number, { sx: number; sy: number; cnt: number }>();

  for (const u of state.units.values()) {
    if (u.ownerId === 0) continue;
    const prev = state.prevUnits.get(u.id);
    const x = prev ? prev.x + (u.x - prev.x) * alpha : u.x;
    const y = prev ? prev.y + (u.y - prev.y) * alpha : u.y;
    const isOwn = u.ownerId === state.playerId;
    if (!isOwn && !isPointInVision(x, y, visionCircles)) continue;
    if (isOwn && u.kind !== 4 && u.squad > 0) {
      const sc = squadCenters.get(u.squad) ?? { sx: 0, sy: 0, cnt: 0 };
      sc.sx += x;
      sc.sy += y;
      sc.cnt += 1;
      squadCenters.set(u.squad, sc);
    }
    if (prev) {
      const dx = u.x - prev.x;
      const dy = u.y - prev.y;
      if (dx * dx + dy * dy > 0.15) {
        unitAngles.set(u.id, Math.atan2(dy, dx));
      }
    }
    const angle = unitAngles.get(u.id) ?? -Math.PI / 2;

    if (u.kind === 2) {
      // Титан — тяжёлый гекс с контрастным броне-кольцом и ядром
      ctx.strokeStyle = "#090b0e";
      ctx.lineWidth = 3.5;
      ctx.fillStyle = isOwn ? "#ffd166" : "#ff6b4a";
      drawPolygon(ctx, x, y, 17, 6);
      ctx.strokeStyle = "#181c24";
      ctx.lineWidth = 2;
      ctx.fillStyle = isOwn ? "#ffe8a3" : "#ffb09c";
      drawPolygon(ctx, x, y, 8.5, 6);
    } else if (u.kind === 1) {
      // Стрелок — яркий ромб с контрастным контуром и светлой сердцевиной
      ctx.strokeStyle = "#090b0e";
      ctx.lineWidth = 3;
      ctx.fillStyle = isOwn ? "#5ce68a" : "#ff9248";
      drawPolygon(ctx, x, y, 12, 4);
      ctx.strokeStyle = "#090b0e";
      ctx.lineWidth = 1.2;
      ctx.fillStyle = isOwn ? "#e6fff0" : "#fff0e0";
      drawPolygon(ctx, x, y, 5, 4);
    } else {
      // Рейдер (0) — направленный истребитель-треугольник с ярким бликом
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(angle);
      ctx.strokeStyle = "#090b0e";
      ctx.lineWidth = 2.8;
      ctx.lineJoin = "round";
      ctx.fillStyle = isOwn ? "#69b8ff" : "#ff5757";
      ctx.beginPath();
      ctx.moveTo(12.5, 0);
      ctx.lineTo(-9, -8.5);
      ctx.lineTo(-5.5, 0);
      ctx.lineTo(-9, 8.5);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = isOwn ? "#eef7ff" : "#ffe6e6";
      ctx.beginPath();
      ctx.moveTo(7.5, 0);
      ctx.lineTo(-3, -3.5);
      ctx.lineTo(-3, 3.5);
      ctx.closePath();
      ctx.fill();
      ctx.restore();
    }
  }

  for (const [sid, sc] of squadCenters) {
    const cx = sc.sx / sc.cnt;
    const cy = sc.sy / sc.cnt - 24;
    const active = state.activeSquad === sid;
    ctx.fillStyle = active ? "#ffd166" : "rgba(227, 229, 232, 0.86)";
    ctx.font = "700 12px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText(`#${sid} (${sc.cnt})`, cx, cy);
  }
}

export function drawProjectiles(
  ctx: CanvasRenderingContext2D,
  state: ClientGameState,
  alpha: number,
  visionCircles: VisionCircle[],
): void {
  for (const p of state.projectiles.values()) {
    const prev = state.prevProjectiles.get(p.id);
    const x = prev ? prev.x + (p.x - prev.x) * alpha : p.x;
    const y = prev ? prev.y + (p.y - prev.y) * alpha : p.y;
    const isOwn = p.ownerId === state.playerId;
    if (!isOwn && !isPointInVision(x, y, visionCircles)) continue;

    if (p.kind === 1) {
      ctx.fillStyle = isOwn ? "#e09f5a" : "#c95b62";
      ctx.strokeStyle = "#0e1013";
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(x, y, 8, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
    } else if (p.kind === 2) {
      ctx.fillStyle = isOwn ? "#81c995" : "#f28b82";
      ctx.beginPath();
      ctx.arc(x, y, 5, 0, Math.PI * 2);
      ctx.fill();
    } else {
      ctx.fillStyle = isOwn ? "#e8c584" : "#f28b82";
      ctx.beginPath();
      ctx.arc(x, y, 4.5, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

export function drawHpBars(
  ctx: CanvasRenderingContext2D,
  state: ClientGameState,
  zonePositions: Map<number, ZonePosition>,
  alpha: number,
  visionCircles: VisionCircle[],
): void {
  for (const b of state.buildings.values()) {
    if (b.hp >= b.maxHp) continue;
    const zone = zonePositions.get(b.ownerId);
    if (!zone) continue;
    const r = getBuildingRadius(state.balance, b.kind);
    const bx = zone.x + b.relX;
    const by = zone.y + b.relY;
    if (b.ownerId !== state.playerId && !isCircleInVision(bx, by, r, visionCircles)) {
      continue;
    }
    drawBar(ctx, bx, by - r - 8, 32, 4, b.hp / Math.max(1, b.maxHp));
  }

  for (const u of state.units.values()) {
    if (u.hp >= u.maxHp) continue;
    const prev = state.prevUnits.get(u.id);
    const x = prev ? prev.x + (u.x - prev.x) * alpha : u.x;
    const y = prev ? prev.y + (u.y - prev.y) * alpha : u.y;
    if (u.ownerId !== state.playerId && !isPointInVision(x, y, visionCircles)) continue;
    drawBar(ctx, x, y - 16, 20, 3, u.hp / Math.max(1, u.maxHp));
  }
}

export function drawEffects(ctx: CanvasRenderingContext2D, state: ClientGameState): void {
  for (const e of state.effects) {
    ctx.save();
    ctx.globalAlpha = Math.max(0, e.alpha);
    ctx.strokeStyle = e.color;
    ctx.lineWidth = 2.5;
    ctx.beginPath();
    ctx.arc(e.x, e.y, e.radius, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  }
}
