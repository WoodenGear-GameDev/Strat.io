import type { ClientGameState, VisionCircle } from "../state.ts";
import { isCircleInVision, type ZonePosition } from "./primitives.ts";

export function getMinimapGeometry(
  width: number,
  height: number,
): { cx: number; cy: number; r: number } {
  const r = 58;
  const cx = width - r - 16;
  const cy = height - r - 86;
  return { cx, cy, r };
}

export function hitTestMinimap(
  sx: number,
  sy: number,
  width: number,
  height: number,
  mapRadius: number,
): { x: number; y: number } | null {
  const { cx, cy, r } = getMinimapGeometry(width, height);
  const dx = sx - cx;
  const dy = sy - cy;
  if (dx * dx + dy * dy <= r * r) {
    return {
      x: (dx / r) * mapRadius,
      y: (dy / r) * mapRadius,
    };
  }
  return null;
}

export function drawMinimap(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  camX: number,
  camY: number,
  state: ClientGameState,
  zonePositions: Map<number, ZonePosition>,
  visionCircles: VisionCircle[],
  now: number,
): void {
  const { cx, cy, r } = getMinimapGeometry(width, height);
  const scale = r / Math.max(1, state.mapRadius);

  ctx.save();
  ctx.fillStyle = "rgba(18, 19, 22, 0.88)";
  ctx.strokeStyle = "#434954";
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.stroke();

  // Inverted rings on minimap: Ring 3 dense center, Ring 2 middle, Ring 1 outer
  const ring3R = state.balance.world.ring3_radius * scale;
  const ring2R = state.balance.world.ring2_radius * scale;
  ctx.fillStyle = "rgba(186, 165, 224, 0.12)";
  ctx.beginPath();
  ctx.arc(cx, cy, ring3R, 0, Math.PI * 2);
  ctx.fill();

  ctx.strokeStyle = "rgba(186, 165, 224, 0.30)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.arc(cx, cy, ring3R, 0, Math.PI * 2);
  ctx.stroke();
  ctx.strokeStyle = "rgba(168, 199, 250, 0.22)";
  ctx.beginPath();
  ctx.arc(cx, cy, ring2R, 0, Math.PI * 2);
  ctx.stroke();

  // Server-authoritative heat zones of major combat ("red fog")
  for (const h of state.serverHeats) {
    const hx = cx + h.x * scale;
    const hy = cy + h.y * scale;
    const norm = Math.max(0.15, h.intensityU8 / 255);
    const hr = 4 + norm * 8;
    const grad = ctx.createRadialGradient(hx, hy, 1, hx, hy, hr);
    grad.addColorStop(0, `rgba(255, 118, 87, ${(0.76 * norm).toFixed(2)})`);
    grad.addColorStop(1, "rgba(255, 118, 87, 0)");
    ctx.fillStyle = grad;
    ctx.beginPath();
    ctx.arc(hx, hy, hr, 0, Math.PI * 2);
    ctx.fill();
  }

  // Global Points of Interest (Attractors) visible through fog of war
  const attrPulse = (now % 1800) / 1800;
  for (const a of state.attractors) {
    const ax = cx + a.x * scale;
    const ay = cy + a.y * scale;
    ctx.strokeStyle = `rgba(255, 209, 102, ${((1 - attrPulse) * 0.9).toFixed(2)})`;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(ax, ay, 3 + attrPulse * 8, 0, Math.PI * 2);
    ctx.stroke();

    ctx.fillStyle = "#ffd166";
    ctx.beginPath();
    ctx.moveTo(ax, ay - 4);
    ctx.lineTo(ax + 4, ay);
    ctx.lineTo(ax, ay + 4);
    ctx.lineTo(ax - 4, ay);
    ctx.closePath();
    ctx.fill();
  }

  // Teleport flashes (global noisy event)
  for (const f of state.teleportFlashes) {
    const prog = Math.max(0, Math.min(1, (now - f.startMs) / Math.max(1, f.untilMs - f.startMs)));
    const fx = cx + f.x * scale;
    const fy = cy + f.y * scale;
    ctx.strokeStyle = `rgba(186, 165, 224, ${(1 - prog).toFixed(2)})`;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.arc(fx, fy, 3 + (prog * 14) % 9, 0, Math.PI * 2);
    ctx.stroke();
  }

  // Strategic minimap bases: own base, bases inside vision, and top-3 leaders only
  const topRanks = new Map<number, number>();
  for (let i = 0; i < Math.min(3, state.leaderboard.length); i++) {
    if (state.leaderboard[i].score > 0) {
      topRanks.set(state.leaderboard[i].playerId, i + 1);
    }
  }

  for (const [pid, z] of zonePositions) {
    const isOwn = pid === state.playerId;
    const snapZ = state.zones.get(pid);
    const rank = topRanks.get(pid) ?? ((snapZ && (snapZ.flags & 2) !== 0) ? 1 : (snapZ && (snapZ.flags & 4) !== 0) ? 2 : 0);
    const inVision = isOwn || isCircleInVision(z.x, z.y, z.radius, visionCircles);
    if (!isOwn && rank === 0 && !inVision) {
      continue;
    }

    const zx = cx + z.x * scale;
    const zy = cy + z.y * scale;
    const dotR = Math.max(3, z.radius * scale);

    if (rank > 0) {
      ctx.strokeStyle = rank === 1 ? "#ffd166" : rank === 2 ? "#e2e8f0" : "#e09f67";
      ctx.lineWidth = rank === 1 ? 2.2 : 1.6;
      ctx.beginPath();
      ctx.arc(zx, zy, dotR + 2.5, 0, Math.PI * 2);
      ctx.stroke();
    }

    const isHive = snapZ !== undefined && (snapZ.flags & 16) !== 0;
    ctx.fillStyle = isOwn
      ? "#a8c7fa"
      : rank === 1
        ? "#ffd166"
        : isHive
          ? "#4ade80"
          : "#f28b82";
    ctx.beginPath();
    ctx.arc(zx, zy, dotR, 0, Math.PI * 2);
    ctx.fill();
  }

  // Camera crosshair
  const camMx = cx + camX * scale;
  const camMy = cy + camY * scale;
  ctx.strokeStyle = "#e3e5e8";
  ctx.lineWidth = 1;
  ctx.strokeRect(camMx - 5, camMy - 4, 10, 8);
  ctx.restore();
}
