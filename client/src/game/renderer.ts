import { getBuildingConfigByKind, getBuildingRadius } from "./balance.ts";
import {
  HEX_EDGE_DIRS,
  HEX_SIZE,
  beaconExtRadius,
  beaconExtensionCenter,
  findLocalRhombusVerts,
  hexKey,
  hexToRel,
  hexVertex,
  isLargeBuilding,
  relToHexRound,
  triKey,
  type ClientGameState,
  type Point2D,
  type VisionCircle,
} from "./state.ts";

export class CanvasRenderer {
  private ctx: CanvasRenderingContext2D;
  private fogCanvas: HTMLCanvasElement;
  private fogCtx: CanvasRenderingContext2D;
  private width = 0;
  private height = 0;
  camX = 0;
  camY = 0;
  zoom = 1;
  private minZoom = 0.55;
  private lastFrameMs: number | null = null;
  private unitAngles = new Map<number, number>();
  private camInitialized = false;
  private lastPlayerId: number | null = null;
  private lastOwnZonePos: Point2D | null = null;
  private camFlyTarget: Point2D | null = null;
  private camFollowPaused = false;
  private lastTrackedSquad = 0;

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
    this.lastFrameMs = null;
    this.minZoom = 0.55;
    this.zoom = 1;
    this.camInitialized = false;
    this.lastPlayerId = null;
    this.lastOwnZonePos = null;
    this.camFlyTarget = null;
    this.camFollowPaused = false;
    this.lastTrackedSquad = 0;
  }

  zoomAt(factor: number, _screenX: number, _screenY: number): void {
    this.zoom = Math.max(this.minZoom, Math.min(1.85, this.zoom * factor));
  }

  centerOn(wx: number, wy: number): void {
    this.camX = wx;
    this.camY = wy;
    this.camFlyTarget = null;
    this.camFollowPaused = false;
    this.camInitialized = true;
  }

  flyTo(wx: number, wy: number): void {
    this.camFlyTarget = { x: wx, y: wy };
    this.camFollowPaused = false;
  }

  resumeFollow(): void {
    this.camFollowPaused = false;
  }

  panBy(screenDx: number, screenDy: number): void {
    this.camFlyTarget = null;
    this.camFollowPaused = true;
    this.camX -= screenDx / this.zoom;
    this.camY -= screenDy / this.zoom;
  }

  screenToWorld(sx: number, sy: number): { x: number; y: number } {
    return {
      x: (sx - this.width / 2) / this.zoom + this.camX,
      y: (sy - this.height / 2) / this.zoom + this.camY,
    };
  }

  private getMinimapGeometry(): { cx: number; cy: number; r: number } {
    const r = 58;
    const cx = this.width - r - 16;
    const cy = this.height - r - 86;
    return { cx, cy, r };
  }

  hitTestMinimap(sx: number, sy: number, mapRadius: number): { x: number; y: number } | null {
    const { cx, cy, r } = this.getMinimapGeometry();
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

  render(state: ClientGameState, now: number): void {
    const ctx = this.ctx;
    ctx.fillStyle = "#121316";
    ctx.fillRect(0, 0, this.width, this.height);

    const alpha = Math.max(
      0,
      Math.min(1, (now - state.snapshotTime) / Math.max(1, state.tickIntervalMs)),
    );

    const dt =
      this.lastFrameMs === null ? 0.016 : Math.min(0.05, Math.max(0.001, (now - this.lastFrameMs) / 1000));
    this.lastFrameMs = now;

    const zonePositions = this.collectZonePositions(state, alpha);
    const visionCircles = state.getVisionCircles(alpha, zonePositions);

    if (state.playerId !== this.lastPlayerId) {
      this.lastPlayerId = state.playerId;
      this.camInitialized = false;
      this.lastOwnZonePos = null;
      this.camFlyTarget = null;
    }

    if (state.playerId !== null) {
      const ownZone = state.zones.get(state.playerId);
      if (ownZone) {
        const prev = state.prevZones.get(state.playerId);
        const ownX = prev ? prev.x + (ownZone.x - prev.x) * alpha : ownZone.x;
        const ownY = prev ? prev.y + (ownZone.y - prev.y) * alpha : ownZone.y;

        if (!this.camInitialized) {
          this.camX = ownX;
          this.camY = ownY;
          this.camInitialized = true;
        } else if (
          this.lastOwnZonePos &&
          Math.hypot(ownX - this.lastOwnZonePos.x, ownY - this.lastOwnZonePos.y) > 420
        ) {
          // Zone teleported — snap/fly camera to new HQ location
          this.camX = ownX;
          this.camY = ownY;
          this.camFlyTarget = null;
        }
        this.lastOwnZonePos = { x: ownX, y: ownY };

        this.minZoom = 0.55;
        if (this.zoom < this.minZoom) {
          this.zoom = this.minZoom;
        }

        if (state.activeSquad !== this.lastTrackedSquad) {
          this.lastTrackedSquad = state.activeSquad;
          if (state.activeSquad > 0) {
            this.camFollowPaused = false;
            this.camFlyTarget = null;
          }
        }

        if (!this.camFollowPaused && state.cameraAnchor === "drones" && state.activeSquad > 0) {
          const droneFocus = state.getDroneFocusTarget(alpha);
          if (droneFocus) {
            const dx = droneFocus.x - this.camX;
            const dy = droneFocus.y - this.camY;
            const d = Math.hypot(dx, dy);
            if (d > 2200) {
              this.camX = droneFocus.x;
              this.camY = droneFocus.y;
            } else {
              const followRate = 1 - Math.exp(-12 * dt);
              this.camX += dx * followRate;
              this.camY += dy * followRate;
            }
          }
        } else if (this.camFlyTarget) {
          const dx = this.camFlyTarget.x - this.camX;
          const dy = this.camFlyTarget.y - this.camY;
          const d = Math.hypot(dx, dy);
          if (d <= 6 || d > 2200) {
            this.camX = this.camFlyTarget.x;
            this.camY = this.camFlyTarget.y;
            this.camFlyTarget = null;
          } else {
            const followRate = 1 - Math.exp(-14 * dt);
            this.camX += dx * followRate;
            this.camY += dy * followRate;
          }
        }

        // Clamp camera smoothly inside the union of all active vision circles,
        // unless holding camera at a reinforced squad's target marker while waiting for reinforcements.
        const holdingReinforceTarget =
          state.cameraAnchor === "drones" &&
          state.activeSquad > 0 &&
          state.reinforceSquad === state.activeSquad;
        if (!holdingReinforceTarget && visionCircles.length > 0) {
          let inside = false;
          let bestDist = Infinity;
          let projX = ownX;
          let projY = ownY;
          for (const c of visionCircles) {
            const dx = this.camX - c.x;
            const dy = this.camY - c.y;
            const d = Math.hypot(dx, dy);
            if (d <= c.r) {
              inside = true;
              break;
            }
            const excess = d - c.r;
            if (excess < bestDist) {
              bestDist = excess;
              projX = c.x + (dx / d) * c.r;
              projY = c.y + (dy / d) * c.r;
            }
          }
          if (!inside) {
            const pull = bestDist > 600 ? 1 : 1 - Math.exp(-16 * dt);
            this.camX += (projX - this.camX) * pull;
            this.camY += (projY - this.camY) * pull;
          }
        }
      }
    }

    ctx.save();
    ctx.translate(this.width / 2, this.height / 2);
    ctx.scale(this.zoom, this.zoom);
    ctx.translate(-this.camX, -this.camY);

    this.drawGrid(state.mapRadius);
    this.drawArenaBorder(state.mapRadius);
    this.drawCoreAndRings(state);
    this.drawZones(state, zonePositions, visionCircles, now);
    this.drawOwnHexGrid(state, zonePositions);
    this.drawOverlays(state, zonePositions, visionCircles);
    this.drawBuildings(state, zonePositions, now, visionCircles);
    this.drawUnits(state, alpha, visionCircles);
    this.drawProjectiles(state, alpha, visionCircles);
    this.drawHpBars(state, zonePositions, alpha, visionCircles);
    this.drawEffects(state);

    ctx.restore();

    if (state.playerId !== null) {
      this.drawVisionFog(state, visionCircles);
      this.drawMinimap(state, zonePositions, visionCircles, now);
    }
  }

  private collectZonePositions(
    state: ClientGameState,
    alpha: number,
  ): Map<number, { x: number; y: number; radius: number }> {
    const positions = new Map<number, { x: number; y: number; radius: number }>();
    for (const z of state.zones.values()) {
      const prev = state.prevZones.get(z.playerId);
      const x = prev ? prev.x + (z.x - prev.x) * alpha : z.x;
      const y = prev ? prev.y + (z.y - prev.y) * alpha : z.y;
      positions.set(z.playerId, { x, y, radius: z.radius });
    }
    return positions;
  }

  private isCircleInVision(
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

  private isPointInVision(x: number, y: number, visionCircles: VisionCircle[]): boolean {
    if (visionCircles.length === 0) return true;
    for (let i = 0; i < visionCircles.length; i++) {
      const c = visionCircles[i];
      const dx = x - c.x;
      const dy = y - c.y;
      if (dx * dx + dy * dy <= c.r * c.r) return true;
    }
    return false;
  }

  private drawVisionFog(state: ClientGameState, visionCircles: VisionCircle[]): void {
    if (state.playerId === null || visionCircles.length === 0) return;
    const fctx = this.fogCtx;
    fctx.globalCompositeOperation = "source-over";
    fctx.clearRect(0, 0, this.width, this.height);
    fctx.fillStyle = "rgba(8, 10, 15, 0.56)";
    fctx.fillRect(0, 0, this.width, this.height);

    fctx.globalCompositeOperation = "destination-out";
    for (let i = 0; i < visionCircles.length; i++) {
      const c = visionCircles[i];
      const sx = (c.x - this.camX) * this.zoom + this.width / 2;
      const sy = (c.y - this.camY) * this.zoom + this.height / 2;
      const sr = c.r * this.zoom;
      if (sx + sr < 0 || sx - sr > this.width || sy + sr < 0 || sy - sr > this.height) {
        continue;
      }
      const grad = fctx.createRadialGradient(sx, sy, sr * 0.72, sx, sy, sr);
      grad.addColorStop(0, "rgba(0, 0, 0, 1)");
      grad.addColorStop(1, "rgba(0, 0, 0, 0)");
      fctx.fillStyle = grad;
      fctx.beginPath();
      fctx.arc(sx, sy, sr, 0, Math.PI * 2);
      fctx.fill();
    }

    this.ctx.drawImage(this.fogCanvas, 0, 0, this.width, this.height);
  }

  private drawGrid(mapRadius: number): void {
    const ctx = this.ctx;
    const step = 100;
    const halfW = this.width / (2 * this.zoom);
    const halfH = this.height / (2 * this.zoom);

    const minX = Math.max(-mapRadius, Math.floor((this.camX - halfW) / step) * step);
    const maxX = Math.min(mapRadius, Math.ceil((this.camX + halfW) / step) * step);
    const minY = Math.max(-mapRadius, Math.floor((this.camY - halfH) / step) * step);
    const maxY = Math.min(mapRadius, Math.ceil((this.camY + halfH) / step) * step);

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

  private drawArenaBorder(mapRadius: number): void {
    const ctx = this.ctx;
    ctx.strokeStyle = "#c95b62";
    ctx.lineWidth = 5;
    ctx.beginPath();
    ctx.arc(0, 0, mapRadius, 0, Math.PI * 2);
    ctx.stroke();
  }

  private drawCoreAndRings(state: ClientGameState): void {
    const ctx = this.ctx;
    const coreR = state.balance.world.core_radius ?? 280;

    // Central repelling core (flat matte Material 3 Expressive rings)
    ctx.fillStyle = "rgba(168, 199, 250, 0.08)";
    ctx.beginPath();
    ctx.arc(0, 0, coreR, 0, Math.PI * 2);
    ctx.fill();

    ctx.fillStyle = "rgba(168, 199, 250, 0.12)";
    ctx.beginPath();
    ctx.arc(0, 0, coreR * 0.45, 0, Math.PI * 2);
    ctx.fill();

    ctx.strokeStyle = "rgba(168, 199, 250, 0.45)";
    ctx.lineWidth = 2.5;
    ctx.beginPath();
    ctx.arc(0, 0, coreR, 0, Math.PI * 2);
    ctx.stroke();

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

    // Ring boundaries (Ring 1 Cradle: 1100, Ring 2/3 Storm Periphery: 2150)
    ctx.strokeStyle = "rgba(168, 199, 250, 0.18)";
    ctx.lineWidth = 2;
    ctx.setLineDash([16, 12]);
    ctx.beginPath();
    ctx.arc(0, 0, 1100, 0, Math.PI * 2);
    ctx.stroke();

    ctx.strokeStyle = "rgba(186, 165, 224, 0.22)";
    ctx.beginPath();
    ctx.arc(0, 0, 2150, 0, Math.PI * 2);
    ctx.stroke();
    ctx.setLineDash([]);

    // Ring labels
    ctx.fillStyle = "rgba(194, 199, 207, 0.44)";
    ctx.font = "600 13px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.fillText("Кольцо 2 · Кристаллы ×2", 0, -1115);
    ctx.fillText("Кольцо 3 · Эндгейм (Маяки)", 0, -2165);
  }

  private drawZones(
    state: ClientGameState,
    positions: Map<number, { x: number; y: number; radius: number }>,
    visionCircles: VisionCircle[],
    now: number,
  ): void {
    const ctx = this.ctx;

    for (const z of state.zones.values()) {
      const pos = positions.get(z.playerId);
      if (!pos) continue;
      const { x, y } = pos;

      const isOwn = z.playerId === state.playerId;
      if (!isOwn && !this.isCircleInVision(x, y, z.radius, visionCircles)) {
        continue;
      }
      const openLarge = state.getOpenLargeHexes(z.playerId);

      // Fill polygonal territory from open large hexes (no circular zone!)
      ctx.fillStyle = isOwn ? "rgba(168, 199, 250, 0.08)" : "rgba(242, 139, 130, 0.08)";
      for (const hex of openLarge.values()) {
        const p = hexToRel(hex.q, hex.r);
        this.drawHexPolygon(x + p.x, y + p.y, HEX_SIZE, ctx.fillStyle, undefined, 0);
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
      if (!isOwn && !this.isCircleInVision(zone.x + b.relX, zone.y + b.relY, 24, visionCircles)) {
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

  private drawOwnHexGrid(
    state: ClientGameState,
    zonePositions: Map<number, { x: number; y: number; radius: number }>,
  ): void {
    if (state.playerId === null) return;
    const zone = zonePositions.get(state.playerId);
    if (!zone) return;

    const openLarge = state.getOpenLargeHexes();
    const occupiedTris = state.getOccupiedTris(state.playerId);
    const ctx = this.ctx;
    const buildKind = state.buildKind;

    if (buildKind === null) {
      for (const hex of openLarge.values()) {
        const pos = hexToRel(hex.q, hex.r);
        this.drawHexPolygon(
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
          this.drawHexPolygon(
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
          this.drawHexPolygon(
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
      // Draw triangular sub-sectors so player clearly sees all 6 sectors per hex for rhombuses
      for (const hex of openLarge.values()) {
        const c = hexToRel(hex.q, hex.r);
        const cx = zone.x + c.x;
        const cy = zone.y + c.y;
        for (let s = 0; s < 6; s++) {
          const v0 = hexVertex(hex.q, hex.r, s);
          const v1 = hexVertex(hex.q, hex.r, (s + 1) % 6);
          const busy = occupiedTris.has(triKey(hex.q, hex.r, s));
          ctx.beginPath();
          ctx.moveTo(cx, cy);
          ctx.lineTo(zone.x + v0.x, zone.y + v0.y);
          ctx.lineTo(zone.x + v1.x, zone.y + v1.y);
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

  private drawOverlays(
    state: ClientGameState,
    zonePositions: Map<number, { x: number; y: number; radius: number }>,
    visionCircles: VisionCircle[],
  ): void {
    const ctx = this.ctx;
    const now = performance.now();

    // Building ruins (20s build lock indicator after destruction)
    for (const ruin of state.buildingRuins) {
      const z = zonePositions.get(ruin.ownerId);
      if (!z) continue;
      const rx = z.x + ruin.relX;
      const ry = z.y + ruin.relY;
      if (ruin.ownerId !== state.playerId && !this.isPointInVision(rx, ry, visionCircles)) {
        continue;
      }
      const rem = Math.max(0, (ruin.expiresAtMs - now) / Math.max(1, ruin.expiresAtMs - ruin.createdAtMs));
      ctx.save();
      ctx.fillStyle = `rgba(32, 34, 40, ${0.45 + 0.3 * rem})`;
      ctx.strokeStyle = `rgba(180, 120, 90, ${0.25 + 0.45 * rem})`;
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(rx, ry, ruin.radius * 0.82, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      // Jagged rubble fragments
      ctx.fillStyle = `rgba(92, 96, 108, ${0.45 + 0.35 * rem})`;
      for (let k = 0; k < 4; k++) {
        const a = k * 1.57 + 0.4;
        const dist = ruin.radius * 0.38;
        ctx.fillRect(rx + Math.cos(a) * dist - 3, ry + Math.sin(a) * dist - 2, 6, 4);
      }
      // Countdown arc
      ctx.strokeStyle = `rgba(235, 160, 100, ${0.5 + 0.4 * rem})`;
      ctx.lineWidth = 2.2;
      ctx.beginPath();
      ctx.arc(rx, ry, ruin.radius * 0.94, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * rem);
      ctx.stroke();
      ctx.restore();
    }

    // Squad target markers & lines
    if (state.playerId !== null) {
      const squadPts = new Map<number, { sx: number; sy: number; cnt: number }>();
      for (const u of state.units.values()) {
        if (u.ownerId !== state.playerId || u.kind === 4 || u.squad <= 0) continue;
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

    const tpTarget =
      state.pendingTeleport ??
      (state.hasPointerHover ? { x: state.hoverWorldX, y: state.hoverWorldY } : null);
    if (state.teleportSelectMode && tpTarget && state.playerId !== null) {
      const zone = zonePositions.get(state.playerId);
      if (zone) {
        const tx = tpTarget.x;
        const ty = tpTarget.y;
        const r = Math.hypot(tx, ty);
        const coreR = state.balance.world.core_radius ?? 280;
        const cost = state.computeTeleportCost(tx, ty);
        const RING1_RADIUS = 1100;
        const minR = state.rEq >= RING1_RADIUS ? RING1_RADIUS + zone.radius + 200 : coreR + zone.radius;
        const inVision = this.isPointInVision(tx, ty, visionCircles);
        let valid =
          inVision && r >= minR && r <= state.mapRadius - zone.radius && state.energy >= cost;
        if (valid) {
          for (const [pid, other] of zonePositions) {
            if (pid === state.playerId) continue;
            if (Math.hypot(tx - other.x, ty - other.y) < zone.radius + other.radius + 150) {
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
        this.drawBeaconNewHexContour(state, zone, preview.centerRel, preview.valid);
      }

      // Highlight exact hex (large) or rhombus (2 triangles for small)
      if (preview.isLarge || !preview.rhombusVerts) {
        this.drawHexPolygon(
          zone.x + preview.centerRel.x,
          zone.y + preview.centerRel.y,
          HEX_SIZE - 0.8,
          previewFill,
          previewStroke,
          2.2,
        );
      } else {
        this.drawRhombusPoly(
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

  private drawBeaconNewHexContour(
    state: ClientGameState,
    zone: { x: number; y: number },
    centerRel: { x: number; y: number },
    valid: boolean,
  ): void {
    const ctx = this.ctx;
    const openLarge = state.getOpenLargeHexes();
    const newHexes = new Map<string, { q: number; r: number }>();
    const rad = beaconExtRadius(1);
    const hexSpan = Math.ceil(rad / (HEX_SIZE * 1.5)) + 1;
    const base = relToHexRound(centerRel.x, centerRel.y);

    for (let dq = -hexSpan; dq <= hexSpan; dq++) {
      for (let dr = -hexSpan; dr <= hexSpan; dr++) {
        const q = base.q + dq;
        const r = base.r + dr;
        const p = hexToRel(q, r);
        if (Math.hypot(p.x - centerRel.x, p.y - centerRel.y) <= rad) {
          const key = hexKey(q, r);
          if (!openLarge.has(key)) {
            newHexes.set(key, { q, r });
          }
        }
      }
    }

    if (newHexes.size === 0) return;

    const cellFill = valid ? "rgba(168, 199, 250, 0.08)" : "rgba(242, 139, 130, 0.06)";
    const cellStroke = valid ? "rgba(168, 199, 250, 0.22)" : "rgba(242, 139, 130, 0.18)";
    for (const hex of newHexes.values()) {
      const p = hexToRel(hex.q, hex.r);
      this.drawHexPolygon(zone.x + p.x, zone.y + p.y, HEX_SIZE, cellFill, cellStroke, 1);
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

  private drawRhombusPoly(
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
    const ctx = this.ctx;
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

  private drawBuildings(
    state: ClientGameState,
    zonePositions: Map<number, { x: number; y: number; radius: number }>,
    now: number,
    visionCircles: VisionCircle[],
  ): void {
    const ctx = this.ctx;
    for (const b of state.buildings.values()) {
      const zone = zonePositions.get(b.ownerId);
      if (!zone) continue;

      const isOwn = b.ownerId === state.playerId;
      const radius = getBuildingRadius(state.balance, b.kind);
      if (!isOwn && !this.isCircleInVision(zone.x + b.relX, zone.y + b.relY, radius, visionCircles)) {
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
          this.drawHexPolygon(x, y, HEX_SIZE + 2, undefined, "#e8c584", 2);
        } else {
          this.drawRhombusPoly(zx, zy, b.relX, b.relY, rhVerts, 1.04, undefined, "#e8c584", 2);
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
            this.drawHexPolygon(
              x,
              y,
              HEX_SIZE + 3.5 + pulse * 2.5,
              undefined,
              `rgba(255, 209, 102, ${(0.55 + 0.4 * pulse).toFixed(2)})`,
              2.6,
            );
          }
          this.drawPolygon(x, y, 30, 6);
          ctx.fillStyle = isOwn ? "#a8c7fa" : "#f2b8b5";
          this.drawPolygon(x, y, 10, 6);
          break;
        case 1: // Генератор (2-triangle Rhombus)
          if (rhVerts) {
            this.drawRhombusPoly(
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
            this.drawPolygon(x, y, 18.5, 6);
          }
          ctx.save();
          ctx.translate(x, y);
          ctx.rotate(now * 0.002);
          ctx.fillStyle = "#e8c584";
          this.drawPolygon(0, 0, 8.5, 4);
          ctx.restore();
          break;
        case 2: {
          // Турель (Large hex)
          const rad = (b.angle / 255) * Math.PI * 2;
          this.drawPolygon(x, y, 30, 6);
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
          this.drawPolygon(x, y, 30, 6);
          ctx.fillStyle = isOwn ? "#284777" : "#87373d";
          this.drawPolygon(x, y, 16, 4);
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
        case 5: // Маяк (2-triangle Rhombus)
          if (rhVerts) {
            this.drawRhombusPoly(
              zx,
              zy,
              b.relX,
              b.relY,
              rhVerts,
              0.88,
              isOwn ? "#3f8ca8" : "#d4556b",
              "#0e1013",
              2,
            );
          } else {
            ctx.fillStyle = isOwn ? "#3f8ca8" : "#d4556b";
            this.drawPolygon(x, y, 18.5, 6);
          }
          ctx.fillStyle = isOwn ? "#a8c7fa" : "#f2b8b5";
          this.drawPolygon(x, y, 7.5, 3);
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
            this.drawHexPolygon(x, y, 30 - 5 * l, undefined, "#e8c584", 1.3);
          }
        } else {
          for (let l = 1; l < b.level; l++) {
            this.drawRhombusPoly(
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
          this.drawHexPolygon(x, y, 30, hitFill, hitStroke, 2.5);
        } else {
          this.drawRhombusPoly(zx, zy, b.relX, b.relY, rhVerts, 0.88, hitFill, hitStroke, 2.2);
        }
      }
    }
  }

  private drawUnits(
    state: ClientGameState,
    alpha: number,
    visionCircles: VisionCircle[],
  ): void {
    const ctx = this.ctx;
    for (const id of this.unitAngles.keys()) {
      if (!state.units.has(id)) {
        this.unitAngles.delete(id);
      }
    }

    const squadCenters = new Map<number, { sx: number; sy: number; cnt: number }>();

    for (const u of state.units.values()) {
      if (u.ownerId === 0) continue;
      const prev = state.prevUnits.get(u.id);
      const x = prev ? prev.x + (u.x - prev.x) * alpha : u.x;
      const y = prev ? prev.y + (u.y - prev.y) * alpha : u.y;
      const isOwn = u.ownerId === state.playerId;
      if (!isOwn && !this.isPointInVision(x, y, visionCircles)) continue;
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
          this.unitAngles.set(u.id, Math.atan2(dy, dx));
        }
      }
      const angle = this.unitAngles.get(u.id) ?? -Math.PI / 2;

      if (u.kind === 2) {
        // Титан — тяжёлый гекс с контрастным броне-кольцом и ядром
        ctx.strokeStyle = "#090b0e";
        ctx.lineWidth = 3.5;
        ctx.fillStyle = isOwn ? "#ffd166" : "#ff6b4a";
        this.drawPolygon(x, y, 17, 6);
        ctx.strokeStyle = "#181c24";
        ctx.lineWidth = 2;
        ctx.fillStyle = isOwn ? "#ffe8a3" : "#ffb09c";
        this.drawPolygon(x, y, 8.5, 6);
      } else if (u.kind === 1) {
        // Стрелок — яркий ромб с контрастным контуром и светлой сердцевиной
        ctx.strokeStyle = "#090b0e";
        ctx.lineWidth = 3;
        ctx.fillStyle = isOwn ? "#5ce68a" : "#ff9248";
        this.drawPolygon(x, y, 12, 4);
        ctx.strokeStyle = "#090b0e";
        ctx.lineWidth = 1.2;
        ctx.fillStyle = isOwn ? "#e6fff0" : "#fff0e0";
        this.drawPolygon(x, y, 5, 4);
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

  private drawProjectiles(
    state: ClientGameState,
    alpha: number,
    visionCircles: VisionCircle[],
  ): void {
    const ctx = this.ctx;
    for (const p of state.projectiles.values()) {
      const prev = state.prevProjectiles.get(p.id);
      const x = prev ? prev.x + (p.x - prev.x) * alpha : p.x;
      const y = prev ? prev.y + (p.y - prev.y) * alpha : p.y;
      const isOwn = p.ownerId === state.playerId;
      if (!isOwn && !this.isPointInVision(x, y, visionCircles)) continue;

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

  private drawHpBars(
    state: ClientGameState,
    zonePositions: Map<number, { x: number; y: number; radius: number }>,
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
      if (b.ownerId !== state.playerId && !this.isCircleInVision(bx, by, r, visionCircles)) {
        continue;
      }
      this.drawBar(bx, by - r - 8, 32, 4, b.hp / Math.max(1, b.maxHp));
    }

    for (const u of state.units.values()) {
      if (u.hp >= u.maxHp) continue;
      const prev = state.prevUnits.get(u.id);
      const x = prev ? prev.x + (u.x - prev.x) * alpha : u.x;
      const y = prev ? prev.y + (u.y - prev.y) * alpha : u.y;
      if (u.ownerId !== state.playerId && !this.isPointInVision(x, y, visionCircles)) continue;
      this.drawBar(x, y - 16, 20, 3, u.hp / Math.max(1, u.maxHp));
    }
  }

  private drawBar(cx: number, topY: number, w: number, h: number, ratio: number): void {
    const ctx = this.ctx;
    const r = Math.max(0, Math.min(1, ratio));
    ctx.fillStyle = "rgba(0, 0, 0, 0.65)";
    ctx.fillRect(cx - w / 2, topY, w, h);
    ctx.fillStyle = r > 0.4 ? "#81c995" : "#f28b82";
    ctx.fillRect(cx - w / 2, topY, w * r, h);
  }

  private drawEffects(state: ClientGameState): void {
    const ctx = this.ctx;
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

  private drawMinimap(
    state: ClientGameState,
    zonePositions: Map<number, { x: number; y: number; radius: number }>,
    visionCircles: VisionCircle[],
    now: number,
  ): void {
    const ctx = this.ctx;
    const { cx, cy, r } = this.getMinimapGeometry();
    const scale = r / Math.max(1, state.mapRadius);

    ctx.save();
    ctx.fillStyle = "rgba(18, 19, 22, 0.88)";
    ctx.strokeStyle = "#434954";
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();

    // Core on minimap
    const coreR = (state.balance.world.core_radius ?? 280) * scale;
    ctx.fillStyle = "rgba(168, 199, 250, 0.30)";
    ctx.beginPath();
    ctx.arc(cx, cy, Math.max(3, coreR), 0, Math.PI * 2);
    ctx.fill();

    // Ring 1 & 2 on minimap
    ctx.strokeStyle = "rgba(168, 199, 250, 0.22)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(cx, cy, 1100 * scale, 0, Math.PI * 2);
    ctx.stroke();
    ctx.strokeStyle = "rgba(186, 165, 224, 0.24)";
    ctx.beginPath();
    ctx.arc(cx, cy, 2150 * scale, 0, Math.PI * 2);
    ctx.stroke();

    // Heat zones of major combat (delayed slightly)
    for (const h of state.combatHeats) {
      if (now < h.showAtMs || h.power < 2.5) continue;
      const hx = cx + h.x * scale;
      const hy = cy + h.y * scale;
      const fade = Math.min(1, (h.untilMs - now) / 2500);
      const hr = Math.min(11, 4 + h.power * 0.7);
      const grad = ctx.createRadialGradient(hx, hy, 1, hx, hy, hr);
      grad.addColorStop(0, `rgba(255, 118, 87, ${(0.72 * fade).toFixed(2)})`);
      grad.addColorStop(1, "rgba(255, 118, 87, 0)");
      ctx.fillStyle = grad;
      ctx.beginPath();
      ctx.arc(hx, hy, hr, 0, Math.PI * 2);
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
      const inVision = isOwn || this.isCircleInVision(z.x, z.y, z.radius, visionCircles);
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

      ctx.fillStyle = isOwn ? "#a8c7fa" : rank === 1 ? "#ffd166" : "#f28b82";
      ctx.beginPath();
      ctx.arc(zx, zy, dotR, 0, Math.PI * 2);
      ctx.fill();
    }

    // Camera crosshair
    const camMx = cx + this.camX * scale;
    const camMy = cy + this.camY * scale;
    ctx.strokeStyle = "#e3e5e8";
    ctx.lineWidth = 1;
    ctx.strokeRect(camMx - 5, camMy - 4, 10, 8);
    ctx.restore();
  }

  private drawHexPolygon(
    cx: number,
    cy: number,
    size: number,
    fillStyle?: string,
    strokeStyle?: string,
    lineWidth = 1,
  ): void {
    const ctx = this.ctx;
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

  private drawPolygon(cx: number, cy: number, radius: number, sides: number): void {
    const ctx = this.ctx;
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
}
