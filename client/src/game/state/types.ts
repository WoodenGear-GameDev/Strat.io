import type { Point2D } from "../hex.ts";

export interface VisualEffect {
  x: number;
  y: number;
  radius: number;
  maxRadius: number;
  color: string;
  alpha: number;
}

export interface VisionCircle {
  x: number;
  y: number;
  r: number;
}

export interface MinimapFlash {
  x: number;
  y: number;
  startMs: number;
  untilMs: number;
}

export interface CombatHeat {
  x: number;
  y: number;
  power: number;
  showAtMs: number;
  untilMs: number;
}

export interface BuildPreview {
  isLarge: boolean;
  centerRel: Point2D;
  rhombusVerts: [Point2D, Point2D, Point2D, Point2D] | null;
  insideTerritory: boolean;
  overlapsExisting: boolean;
  footprintValid: boolean;
  withinLimit: boolean;
  affordable: boolean;
  valid: boolean;
}
