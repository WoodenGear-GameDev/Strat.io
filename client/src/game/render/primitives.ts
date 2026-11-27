import type { Point2D, VisionCircle } from "../state.ts";

export interface ZonePosition {
  x: number;
  y: number;
  radius: number;
}

export function isCircleInVision(
  x: number,
  y: number,
  radius: number,
  visionCircles: VisionCircle[],
): boolean {
  if (visionCircles.length === 0) return true;
  for (let i = 0; i < visionCircles.length; i++) {
    const c = visionCircles[i];
    const maxD = c.r + radius;
    const dx = x - c.x;
    const dy = y - c.y;
    if (dx * dx + dy * dy <= maxD * maxD) return true;
  }
  return false;
}

export function isPointInVision(x: number, y: number, visionCircles: VisionCircle[]): boolean {
  if (visionCircles.length === 0) return true;
  for (let i = 0; i < visionCircles.length; i++) {
    const c = visionCircles[i];
    const dx = x - c.x;
    const dy = y - c.y;
    if (dx * dx + dy * dy <= c.r * c.r) return true;
  }
  return false;
}

export function drawHexPolygon(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  size: number,
  fillStyle?: string,
  strokeStyle?: string,
  lineWidth = 1,
): void {
  ctx.beginPath();
  for (let i = 0; i < 6; i++) {
    const angle = (Math.PI / 3) * i - Math.PI / 6;
    const px = cx + Math.cos(angle) * size;
    const py = cy + Math.sin(angle) * size;
    if (i === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  }
  ctx.closePath();
  if (fillStyle) {
    ctx.fillStyle = fillStyle;
    ctx.fill();
  }
  if (strokeStyle) {
    ctx.strokeStyle = strokeStyle;
    ctx.lineWidth = lineWidth;
    ctx.stroke();
  }
}

export function drawPolygon(
  ctx: CanvasRenderingContext2D,
  cx: number,
  cy: number,
  radius: number,
  sides: number,
): void {
  ctx.beginPath();
  for (let i = 0; i < sides; i++) {
    const angle = (i / sides) * Math.PI * 2 - Math.PI / 2;
    const px = cx + Math.cos(angle) * radius;
    const py = cy + Math.sin(angle) * radius;
    if (i === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  }
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
}

export function drawRhombusPoly(
  ctx: CanvasRenderingContext2D,
  zoneX: number,
  zoneY: number,
  centerRelX: number,
  centerRelY: number,
  verts: [Point2D, Point2D, Point2D, Point2D],
  scale: number,
  fillStyle?: string,
  strokeStyle?: string,
  lineWidth = 1.8,
): void {
  ctx.beginPath();
  for (let i = 0; i < 4; i++) {
    const vx = zoneX + centerRelX + (verts[i].x - centerRelX) * scale;
    const vy = zoneY + centerRelY + (verts[i].y - centerRelY) * scale;
    if (i === 0) ctx.moveTo(vx, vy);
    else ctx.lineTo(vx, vy);
  }
  ctx.closePath();
  if (fillStyle) {
    ctx.fillStyle = fillStyle;
    ctx.fill();
  }
  if (strokeStyle) {
    ctx.strokeStyle = strokeStyle;
    ctx.lineWidth = lineWidth;
    ctx.stroke();
  }
}

export function drawBar(
  ctx: CanvasRenderingContext2D,
  cx: number,
  topY: number,
  w: number,
  h: number,
  ratio: number,
): void {
  const r = Math.max(0, Math.min(1, ratio));
  ctx.fillStyle = "rgba(0, 0, 0, 0.65)";
  ctx.fillRect(cx - w / 2, topY, w, h);
  ctx.fillStyle = r > 0.4 ? "#81c995" : "#f28b82";
  ctx.fillRect(cx - w / 2, topY, w * r, h);
}
