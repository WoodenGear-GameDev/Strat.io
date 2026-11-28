import { getBuildingConfigByKind, getBuildingRadius, getNodeHexRadius } from "../balance.ts";
import {
  HEX_EDGE_DIRS,
  HEX_SIZE,
  beaconExtRadius,
  beaconExtensionCenter,
  hexKey,
  hexToRel,
  insertHexDisk,
  relToHexRound,
  type ClientGameState,
  type VisionCircle,
} from "../state.ts";
import {
  drawHexPolygon,
  drawRhombusPoly,
  isPointInVision,
  type ZonePosition,
} from "./primitives.ts";

export function drawOverlays(
  ctx: CanvasRenderingContext2D,
  state: ClientGameState,
  zonePositions: Map<number, ZonePosition>,
  visionCircles: VisionCircle[],
): void {
  // Squad target markers & lines
  if (state.playerId !== null) {
    const squadPts = new Map<number, { sx: number; sy: number; cnt: number }>();
    for (const u of state.units.values()) {
      if (u.ownerId !== state.playerId || u.squad <= 0) continue;
      const e = squadPts.get(u.squad) ?? { sx: 0, sy: 0, cnt: 0 };
      e.sx += u.x;
      e.sy += u.y;
      e.cnt += 1;
      squadPts.set(u.squad, e);
    }
    for (const [sid, pt] of squadPts) {
      const order = state.squadOrders.get(sid);
      if (!order) continue;
      const cx = pt.sx / pt.cnt;
      const cy = pt.sy / pt.cnt;
      const isActive = sid === state.activeSquad;
      ctx.save();
      ctx.strokeStyle = isActive ? "rgba(232, 197, 132, 0.72)" : "rgba(232, 197, 132, 0.25)";
      ctx.lineWidth = isActive ? 1.8 : 1;
      ctx.setLineDash([6, 6]);
      ctx.beginPath();
      ctx.moveTo(cx, cy);
      const dist = Math.hypot(order.x - cx, order.y - cy);
      const ur = Math.hypot(cx, cy);
      const tr = Math.hypot(order.x, order.y);
      if (dist >= 180 && ur >= 90 && tr >= 90) {
        const ut = Math.atan2(cy, cx);
        const tt = Math.atan2(order.y, order.x);
        let dtheta = tt - ut;
        while (dtheta > Math.PI) dtheta -= Math.PI * 2;
        while (dtheta < -Math.PI) dtheta += Math.PI * 2;
        if (Math.abs(dtheta) >= 0.14) {
          const steps = 16;
          for (let s = 1; s <= steps; s++) {
            const t = s / steps;
            const r = ur + (tr - ur) * t;
            const th = ut + dtheta * t;
            ctx.lineTo(Math.cos(th) * r, Math.sin(th) * r);
          }
        } else {
          ctx.lineTo(order.x, order.y);
        }
      } else {
        ctx.lineTo(order.x, order.y);
      }
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.beginPath();
      ctx.arc(order.x, order.y, isActive ? 9 : 6, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }

    // Показываем цель сразу при отправке гарнизона (пока сервер не прислал ID нового отряда)
    const pendingTarget =
      state.pendingNewSquadTarget ??
      (state.armyMode === 1 && squadPts.size === 0 ? state.rallyPoint : null);
    if (pendingTarget) {
      const ownZone = zonePositions.get(state.playerId);
      ctx.save();
      ctx.strokeStyle = "rgba(232, 197, 132, 0.78)";
      ctx.lineWidth = 1.8;
      if (ownZone) {
        ctx.setLineDash([6, 6]);
        ctx.beginPath();
        ctx.moveTo(ownZone.x, ownZone.y);
        ctx.lineTo(pendingTarget.x, pendingTarget.y);
        ctx.stroke();
        ctx.setLineDash([]);
      }
      ctx.beginPath();
      ctx.arc(pendingTarget.x, pendingTarget.y, 9, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }
  }

  if (state.teleportSelectMode && state.playerId !== null) {
    ctx.save();
    ctx.strokeStyle = "rgba(186, 165, 224, 0.32)";
    ctx.lineWidth = 1.5;
    ctx.setLineDash([6, 6]);
    for (const c of visionCircles) {
      ctx.beginPath();
      ctx.arc(c.x, c.y, c.r, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.restore();
  }

  // Active teleport charge telegraphs
  if (state.teleports.length > 0) {
    for (const tp of state.teleports) {
      const isOwn = tp.ownerId === state.playerId;
      const srcZone = zonePositions.get(tp.ownerId);
      const srcVisible =
        srcZone !== undefined && (isOwn || isPointInVision(srcZone.x, srcZone.y, visionCircles));
      const dstVisible = isOwn || isPointInVision(tp.targetX, tp.targetY, visionCircles);
      if (!srcVisible && !dstVisible) continue;

      const progress = Math.max(0, Math.min(1, tp.progressU8 / 255));
      const radius = srcZone?.radius ?? state.balance.world.zone_base_radius;
      const strokeColor = isOwn ? "#baa5e0" : "#ff8a80";
      const fillColor = isOwn ? "rgba(186, 165, 224, 0.14)" : "rgba(255, 138, 128, 0.14)";
      const startAng = -Math.PI / 2;
      const endAng = startAng + progress * Math.PI * 2;

      ctx.save();
      if (srcVisible && srcZone) {
        ctx.strokeStyle = isOwn ? "rgba(186, 165, 224, 0.35)" : "rgba(255, 138, 128, 0.35)";
        ctx.lineWidth = 4;
        ctx.beginPath();
        ctx.arc(srcZone.x, srcZone.y, srcZone.radius + 8, 0, Math.PI * 2);
        ctx.stroke();

        ctx.strokeStyle = strokeColor;
        ctx.lineWidth = 4.5;
        ctx.beginPath();
        ctx.arc(srcZone.x, srcZone.y, srcZone.radius + 8, startAng, endAng);
        ctx.stroke();
      }

      if (srcVisible && dstVisible && srcZone) {
        ctx.strokeStyle = isOwn ? "rgba(186, 165, 224, 0.32)" : "rgba(255, 138, 128, 0.32)";
        ctx.lineWidth = 1.6;
        ctx.setLineDash([8, 6]);
        ctx.beginPath();
        ctx.moveTo(srcZone.x, srcZone.y);
        ctx.lineTo(tp.targetX, tp.targetY);
        ctx.stroke();
        ctx.setLineDash([]);
      }

      if (dstVisible) {
        ctx.fillStyle = fillColor;
        ctx.strokeStyle = isOwn ? "rgba(186, 165, 224, 0.55)" : "rgba(255, 138, 128, 0.55)";
        ctx.lineWidth = 2;
        ctx.setLineDash([8, 6]);
        ctx.beginPath();
        ctx.arc(tp.targetX, tp.targetY, radius, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
        ctx.setLineDash([]);

        ctx.strokeStyle = strokeColor;
        ctx.lineWidth = 4;
        ctx.beginPath();
        ctx.arc(tp.targetX, tp.targetY, radius + 4, startAng, endAng);
        ctx.stroke();
      }
      ctx.restore();
    }
  }

  const tpTarget =
    state.pendingTeleport ??
    (state.hasPointerHover ? { x: state.hoverWorldX, y: state.hoverWorldY } : null);
  if (state.teleportSelectMode && tpTarget && state.playerId !== null) {
    const zone = zonePositions.get(state.playerId);
    if (zone) {
      const tx = tpTarget.x;
      const ty = tpTarget.y;
      const r = Math.hypot(tx, ty);
      const cost = state.computeTeleportCost(tx, ty);
      const maxR = state.maxRadiusForHq(state.getHqLevel(), zone.radius);
      const inVision = isPointInVision(tx, ty, visionCircles);
      let valid = inVision && r <= maxR && state.energy >= cost;
      if (valid) {
        const zoneClearance = state.balance.world.teleport_zone_clearance;
        for (const [pid, other] of zonePositions) {
          if (pid === state.playerId) continue;
          if (Math.hypot(tx - other.x, ty - other.y) < zone.radius + other.radius + zoneClearance) {
            valid = false;
            break;
          }
        }
      }
      if (valid) {
        for (const b of state.buildings.values()) {
          if (b.ownerId === state.playerId || b.kind !== 5) continue;
          const other = zonePositions.get(b.ownerId);
          if (!other) continue;
          const beaconSubR = beaconExtRadius(b.level);
          const ext = beaconExtensionCenter(b.relX, b.relY, beaconSubR);
          if (
            Math.hypot(tx - (other.x + ext.x), ty - (other.y + ext.y)) <
            zone.radius + beaconSubR + 10
          ) {
            valid = false;
            break;
          }
        }
      }

      ctx.fillStyle = valid ? "rgba(186, 165, 224, 0.18)" : "rgba(242, 139, 130, 0.18)";
      ctx.strokeStyle = valid ? "#baa5e0" : "#f28b82";
      ctx.lineWidth = 2;
      ctx.setLineDash([8, 6]);
      ctx.beginPath();
      ctx.arc(tx, ty, zone.radius, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ctx.setLineDash([]);

      ctx.fillStyle = valid ? "#e3e5e8" : "#f2b8b5";
      ctx.font = "700 14px system-ui, sans-serif";
      ctx.textAlign = "center";
      ctx.fillText(inVision ? `Телепорт: ${cost}⚡` : "Вне взора дронов", tx, ty - 8);
    }
    return;
  }

  if (state.actionMode !== null && state.playerId !== null && state.hasPointerHover) {
    const zone = zonePositions.get(state.playerId);
    if (zone) {
      let hoveredId: number | null = null;
      let hoveredPos: { x: number; y: number; r: number } | null = null;
      let bestDist = Infinity;
      for (const b of state.buildings.values()) {
        if (b.ownerId !== state.playerId) continue;
        const r = getBuildingRadius(state.balance, b.kind);
        const bx = zone.x + b.relX;
        const by = zone.y + b.relY;
        const d = Math.hypot(state.hoverWorldX - bx, state.hoverWorldY - by);
        if (d <= r + 4 && d < bestDist) {
          bestDist = d;
          hoveredId = b.id;
          hoveredPos = { x: bx, y: by, r };
        }
      }
      if (hoveredId !== null && hoveredPos) {
        const isSell = state.actionMode === "sell";
        ctx.save();
        ctx.strokeStyle = isSell ? "#ff7a92" : "#34e5b5";
        ctx.lineWidth = 3;
        ctx.beginPath();
        ctx.arc(hoveredPos.x, hoveredPos.y, hoveredPos.r + 6, 0, Math.PI * 2);
        ctx.stroke();
        ctx.restore();
      }
    }
    return;
  }

  if (state.buildKind !== null && state.playerId !== null) {
    const zone = zonePositions.get(state.playerId);
    const cfg = getBuildingConfigByKind(state.balance, state.buildKind);
    if (!zone || !cfg) return;

    if (!state.hasPointerHover) return;
    const rawRelX = state.hoverWorldX - zone.x;
    const rawRelY = state.hoverWorldY - zone.y;
    const preview = state.resolveBuildPreview(
      state.buildKind,
      rawRelX,
      rawRelY,
      false,
    );
    if (!preview) return;

    const previewFill = preview.valid
      ? "rgba(129, 201, 149, 0.34)"
      : "rgba(242, 139, 130, 0.28)";
    const previewStroke = preview.valid ? "#81c995" : "#f28b82";

    // If placing a Beacon, draw contour of newly opened large hexes first
    if (state.buildKind === 5) {
      drawBeaconNewHexContour(ctx, state, zone, preview.centerRel, preview.valid);
    }

    // Highlight exact hex (large) or rhombus (2 triangles for small)
    if (preview.isLarge || !preview.rhombusVerts) {
      drawHexPolygon(
        ctx,
        zone.x + preview.centerRel.x,
        zone.y + preview.centerRel.y,
        HEX_SIZE - 0.8,
        previewFill,
        previewStroke,
        2.2,
      );
    } else {
      drawRhombusPoly(
        ctx,
        zone.x,
        zone.y,
        preview.centerRel.x,
        preview.centerRel.y,
        preview.rhombusVerts,
        0.95,
        previewFill,
        previewStroke,
        2.2,
      );
    }

    const previewX = zone.x + preview.centerRel.x;
    const previewY = zone.y + preview.centerRel.y;

    const range = cfg.levels[0]?.range ?? 0;
    if (range > 0) {
      ctx.strokeStyle = "rgba(168, 199, 250, 0.34)";
      ctx.lineWidth = 1.5;
      ctx.setLineDash([6, 6]);
      ctx.beginPath();
      ctx.arc(previewX, previewY, range, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
    }
  }
}

function drawBeaconNewHexContour(
  ctx: CanvasRenderingContext2D,
  state: ClientGameState,
  zone: { x: number; y: number },
  centerRel: { x: number; y: number },
  valid: boolean,
): void {
  const openLarge = state.getOpenLargeHexes();
  const nodeHexR = getNodeHexRadius(state.balance);
  const candidateHexes = new Map<string, { q: number; r: number }>();
  const base = relToHexRound(centerRel.x, centerRel.y);
  insertHexDisk(base.q, base.r, nodeHexR, candidateHexes);

  const newHexes = new Map<string, { q: number; r: number }>();
  for (const [key, hex] of candidateHexes) {
    if (!openLarge.has(key)) {
      newHexes.set(key, hex);
    }
  }

  if (newHexes.size === 0) return;

  const cellFill = valid ? "rgba(168, 199, 250, 0.08)" : "rgba(242, 139, 130, 0.06)";
  const cellStroke = valid ? "rgba(168, 199, 250, 0.22)" : "rgba(242, 139, 130, 0.18)";
  for (const hex of newHexes.values()) {
    const p = hexToRel(hex.q, hex.r);
    drawHexPolygon(ctx, zone.x + p.x, zone.y + p.y, HEX_SIZE, cellFill, cellStroke, 1);
  }

  ctx.save();
  ctx.strokeStyle = valid ? "rgba(168, 199, 250, 0.85)" : "rgba(242, 139, 130, 0.55)";
  ctx.lineWidth = 2;
  ctx.setLineDash([6, 4]);
  ctx.beginPath();
  for (const hex of newHexes.values()) {
    const p = hexToRel(hex.q, hex.r);
    const cx = zone.x + p.x;
    const cy = zone.y + p.y;
    for (let i = 0; i < 6; i++) {
      const [dq, dr] = HEX_EDGE_DIRS[i];
      if (newHexes.has(hexKey(hex.q + dq, hex.r + dr))) continue;
      const a0 = (Math.PI / 3) * i - Math.PI / 6;
      const a1 = (Math.PI / 3) * (i + 1) - Math.PI / 6;
      ctx.moveTo(cx + Math.cos(a0) * HEX_SIZE, cy + Math.sin(a0) * HEX_SIZE);
      ctx.lineTo(cx + Math.cos(a1) * HEX_SIZE, cy + Math.sin(a1) * HEX_SIZE);
    }
  }
  ctx.stroke();
  ctx.restore();
}
