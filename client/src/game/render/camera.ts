import type { ClientGameState, Point2D, VisionCircle } from "../state.ts";

export class CameraController {
  camX = 0;
  camY = 0;
  zoom = 1;
  private minZoom = 0.55;
  private lastFrameMs: number | null = null;
  private camInitialized = false;
  private lastPlayerId: number | null = null;
  private lastOwnZonePos: Point2D | null = null;
  private camFlyTarget: Point2D | null = null;
  private camFollowPaused = false;
  private lastTrackedSquad = 0;

  reset(): void {
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

  zoomAt(factor: number): void {
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

  screenToWorld(sx: number, sy: number, width: number, height: number): { x: number; y: number } {
    return {
      x: (sx - width / 2) / this.zoom + this.camX,
      y: (sy - height / 2) / this.zoom + this.camY,
    };
  }

  update(
    state: ClientGameState,
    alpha: number,
    visionCircles: VisionCircle[],
    now: number,
  ): void {
    const dt =
      this.lastFrameMs === null ? 0.016 : Math.min(0.05, Math.max(0.001, (now - this.lastFrameMs) / 1000));
    this.lastFrameMs = now;

    if (state.playerId !== this.lastPlayerId) {
      this.lastPlayerId = state.playerId;
      this.camInitialized = false;
      this.lastOwnZonePos = null;
      this.camFlyTarget = null;
    }

    if (state.playerId === null) return;
    const ownZone = state.zones.get(state.playerId);
    if (!ownZone) return;

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
    // unless holding camera at a reinforced squad or frozen on a lead drone's death spot.
    const holdingDroneFreeze =
      state.cameraAnchor === "drones" &&
      state.activeSquad > 0 &&
      (state.reinforceSquad === state.activeSquad ||
        state.droneFreezeHold?.squad === state.activeSquad);
    if (!holdingDroneFreeze && visionCircles.length > 0) {
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
