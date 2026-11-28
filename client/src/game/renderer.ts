import { CameraController } from "./render/camera.ts";
import {
  drawAttractorsOverFog,
  drawBuildings,
  drawEffects,
  drawHpBars,
  drawProjectiles,
  drawUnits,
} from "./render/entities.ts";
import { drawMinimap, hitTestMinimap } from "./render/minimap.ts";
import { drawOverlays } from "./render/overlays.ts";
import type { ZonePosition } from "./render/primitives.ts";
import {
  drawArenaBorder,
  drawCoreAndRings,
  drawGrid,
  drawOwnHexGrid,
  drawVisionFog,
  drawZones,
} from "./render/worldLayers.ts";
import type { ClientGameState } from "./state.ts";

export class CanvasRenderer {
  private ctx: CanvasRenderingContext2D;
  private fogCanvas: HTMLCanvasElement;
  private fogCtx: CanvasRenderingContext2D;
  private width = 0;
  private height = 0;
  private camera = new CameraController();
  private unitAngles = new Map<number, number>();
  private zonePositionsBuf = new Map<number, ZonePosition>();

  get camX(): number {
    return this.camera.camX;
  }

  set camX(v: number) {
    this.camera.camX = v;
  }

  get camY(): number {
    return this.camera.camY;
  }

  set camY(v: number) {
    this.camera.camY = v;
  }

  get zoom(): number {
    return this.camera.zoom;
  }

  set zoom(v: number) {
    this.camera.zoom = v;
  }

  constructor(private canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      throw new Error("Canvas2D context is not available");
    }
    this.ctx = ctx;
    this.fogCanvas = document.createElement("canvas");
    const fctx = this.fogCanvas.getContext("2d");
    if (!fctx) {
      throw new Error("Fog Canvas2D context is not available");
    }
    this.fogCtx = fctx;
    this.resize();
  }

  resize(): void {
    const dpr = window.devicePixelRatio || 1;
    this.width = window.innerWidth;
    this.height = window.innerHeight;
    this.canvas.width = Math.floor(this.width * dpr);
    this.canvas.height = Math.floor(this.height * dpr);
    this.fogCanvas.width = Math.max(1, this.width);
    this.fogCanvas.height = Math.max(1, this.height);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  resetCamera(): void {
    this.camera.reset();
  }

  zoomAt(factor: number, _screenX: number, _screenY: number): void {
    this.camera.zoomAt(factor);
  }

  centerOn(wx: number, wy: number): void {
    this.camera.centerOn(wx, wy);
  }

  flyTo(wx: number, wy: number): void {
    this.camera.flyTo(wx, wy);
  }

  resumeFollow(): void {
    this.camera.resumeFollow();
  }

  panBy(screenDx: number, screenDy: number): void {
    this.camera.panBy(screenDx, screenDy);
  }

  screenToWorld(sx: number, sy: number): { x: number; y: number } {
    return this.camera.screenToWorld(sx, sy, this.width, this.height);
  }

  hitTestMinimap(sx: number, sy: number, mapRadius: number): { x: number; y: number } | null {
    return hitTestMinimap(sx, sy, this.width, this.height, mapRadius);
  }

  render(state: ClientGameState, now: number): void {
    const ctx = this.ctx;
    ctx.fillStyle = "#121316";
    ctx.fillRect(0, 0, this.width, this.height);

    const alpha = Math.max(
      0,
      Math.min(1, (now - state.snapshotTime) / Math.max(1, state.tickIntervalMs)),
    );

    const zonePositions = this.collectZonePositions(state, alpha);
    const visionCircles = state.getVisionCircles(alpha, zonePositions);
    this.camera.update(state, alpha, visionCircles, now);

    ctx.save();
    ctx.translate(this.width / 2, this.height / 2);
    ctx.scale(this.camera.zoom, this.camera.zoom);
    ctx.translate(-this.camera.camX, -this.camera.camY);

    drawGrid(
      ctx,
      this.camera.camX,
      this.camera.camY,
      this.camera.zoom,
      this.width,
      this.height,
      state.mapRadius,
    );
    drawArenaBorder(ctx, state.mapRadius);
    drawCoreAndRings(ctx, state);
    drawZones(ctx, state, zonePositions, visionCircles, now);
    drawOwnHexGrid(ctx, state, zonePositions);
    drawOverlays(ctx, state, zonePositions, visionCircles);
    drawBuildings(ctx, state, zonePositions, now, visionCircles);
    drawUnits(ctx, state, alpha, visionCircles, this.unitAngles);
    drawProjectiles(ctx, state, alpha, visionCircles);
    drawHpBars(ctx, state, zonePositions, alpha, visionCircles);
    drawEffects(ctx, state);

    ctx.restore();

    if (state.playerId !== null) {
      drawVisionFog(
        ctx,
        this.fogCtx,
        this.fogCanvas,
        this.width,
        this.height,
        this.camera.camX,
        this.camera.camY,
        this.camera.zoom,
        state,
        visionCircles,
      );
      drawAttractorsOverFog(
        ctx,
        state,
        this.camera.camX,
        this.camera.camY,
        this.camera.zoom,
        this.width,
        this.height,
        now,
      );
      drawMinimap(
        ctx,
        this.width,
        this.height,
        this.camera.camX,
        this.camera.camY,
        state,
        zonePositions,
        visionCircles,
        now,
      );
    }
  }

  private collectZonePositions(
    state: ClientGameState,
    alpha: number,
  ): Map<number, ZonePosition> {
    const positions = this.zonePositionsBuf;
    positions.clear();
    for (const z of state.zones.values()) {
      const prev = state.prevZones.get(z.playerId);
      const x = prev ? prev.x + (z.x - prev.x) * alpha : z.x;
      const y = prev ? prev.y + (z.y - prev.y) * alpha : z.y;
      positions.set(z.playerId, { x, y, radius: z.radius });
    }
    return positions;
  }
}
