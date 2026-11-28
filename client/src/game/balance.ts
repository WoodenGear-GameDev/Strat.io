import balanceJson from "../../../config/balance.json";

export interface BuildingLevelConfig {
  cost: number;
  trophy_cost?: number;
  energy_cap?: number;
  hp: number;
  income_per_sec?: number;
  fire_rate?: number;
  damage?: number;
  range?: number;
  projectile_speed?: number;
  spawn_interval_sec?: number;
  unit_cap_bonus?: number;
  shot_cost?: number;
  splash_radius?: number;
}

export interface BuildingEffectsConfig {
  node_hex_radius?: number;
  node_cost_mult?: number;
}

export interface BuildingConfig {
  kind_id: number;
  name: string;
  radius: number;
  max_per_player?: number;
  levels: BuildingLevelConfig[];
  effects?: BuildingEffectsConfig;
}

export interface UnitConfig {
  kind_id: number;
  name: string;
  cost: number;
  radius: number;
  hp: number;
  damage: number;
  attack_cooldown_sec: number;
  speed: number;
  range?: number;
  aggro_range: number;
  supply_cost?: number;
}

export interface WorldBalanceConfig {
  map_radius: number;
  tick_rate: number;
  spawn_protection_sec: number;
  zone_base_radius: number;
  zone_repulsion_strength: number;
  sell_refund_ratio: number;
  starting_energy: number;
  ring3_radius: number;
  ring2_radius: number;
  gravity_pull_strength: number;
  base_regen_hp_per_sec_pct: number;
  base_regen_hq_mult: number;
  base_regen_energy_per_hp: number;
  teleport_base_cost: number;
  teleport_dist_factor: number;
  teleport_ring_penalty: number;
  teleport_charge_sec: number;
  teleport_patrol_pull_speed: number;
  teleport_zone_clearance: number;
  teleport_node_clearance: number;
  camera_wasd_speed?: number;
  hex_size: number;
  bot_count: number;
}

export interface PveBalanceConfig {
  hive_count: number;
  hive_hq_level: number;
  hive_target_nodes: number;
  hive_target_barracks: number;
  hive_target_generators: number;
  hive_regrow_interval_sec: number;
  hive_respawn_sec: number;
  hive_aggro_range: number;
  hive_gen_trigger_count: number;
  hive_wave_size: number;
  hive_hq_trophy_reward: number;
  hive_hq_energy_reward: number;
  attractor_max_active: number;
  attractor_initial_delay_sec: number;
  attractor_spawn_interval_sec: number;
  attractor_hp: number;
  attractor_radius: number;
  attractor_trophy_reward: number;
  attractor_energy_reward: number;
}

export interface BalanceConfig {
  world: WorldBalanceConfig;
  buildings: Record<string, BuildingConfig>;
  units?: Record<string, UnitConfig>;
  pve?: PveBalanceConfig;
}

export const DEFAULT_BALANCE: BalanceConfig = balanceJson as BalanceConfig;

export function getBuildingConfigByKind(
  balance: BalanceConfig,
  kindId: number,
): BuildingConfig | undefined {
  return Object.values(balance.buildings).find((b) => b.kind_id === kindId);
}

export function getUnitConfigByKind(
  balance: BalanceConfig,
  kindId: number,
): UnitConfig | undefined {
  const units = balance.units ?? DEFAULT_BALANCE.units;
  return units ? Object.values(units).find((u) => u.kind_id === kindId) : undefined;
}

export function getBuildingRadius(balance: BalanceConfig, kindId: number): number {
  const cfg = getBuildingConfigByKind(balance, kindId);
  if (cfg) return cfg.radius;
  const fallback = getBuildingConfigByKind(DEFAULT_BALANCE, kindId);
  return fallback ? fallback.radius : 20;
}

export function getNodeHexRadius(balance: BalanceConfig): number {
  const cfg = getBuildingConfigByKind(balance, 5);
  return cfg?.effects?.node_hex_radius ?? DEFAULT_BALANCE.buildings.beacon.effects?.node_hex_radius ?? 2;
}

export function getNodeTrophyCost(balance: BalanceConfig, existingNodes: number): number {
  const cfg = getBuildingConfigByKind(balance, 5) ?? DEFAULT_BALANCE.buildings.beacon;
  const base = cfg.levels[0]?.trophy_cost ?? 0;
  const mult = cfg.effects?.node_cost_mult ?? 1.65;
  return Math.round(base * Math.pow(mult, existingNodes));
}

export async function loadBalanceConfig(): Promise<BalanceConfig> {
  try {
    const res = await fetch("/api/config");
    if (!res.ok) return DEFAULT_BALANCE;
    const raw = (await res.json()) as Partial<BalanceConfig>;
    return {
      world: { ...DEFAULT_BALANCE.world, ...(raw.world ?? {}) },
      buildings: raw.buildings ?? DEFAULT_BALANCE.buildings,
      units: raw.units ?? DEFAULT_BALANCE.units,
      pve: { ...DEFAULT_BALANCE.pve!, ...(raw.pve ?? {}) },
    };
  } catch {
    return DEFAULT_BALANCE;
  }
}
