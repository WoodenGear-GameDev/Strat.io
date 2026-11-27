export interface BuildingLevelConfig {
  cost: number;
  trophy_cost?: number;
  energy_cap?: number;
  trophy_cap?: number;
  hp: number;
  income_per_sec?: number;
  fire_rate?: number;
  damage?: number;
  range?: number;
  spawn_interval_sec?: number;
  unit_cap_bonus?: number;
  local_supply_cap?: number;
}

export interface BuildingEffectsConfig {
  aura_radius?: number;
  fire_rate_mult?: number;
  zone_radius_bonus?: number;
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

export interface BalanceConfig {
  world: {
    map_radius: number;
    tick_rate: number;
    spawn_protection_sec?: number;
    sell_refund_ratio: number;
    zone_base_radius?: number;
    core_radius?: number;
    teleport_base_cost?: number;
    teleport_dist_factor?: number;
    teleport_ring_penalty?: number;
  };
  buildings: Record<string, BuildingConfig>;
  units?: Record<string, UnitConfig>;
}

export const DEFAULT_BALANCE: BalanceConfig = {
  world: {
    map_radius: 3200,
    tick_rate: 20,
    spawn_protection_sec: 40,
    sell_refund_ratio: 0.75,
    zone_base_radius: 95,
    core_radius: 280,
    teleport_base_cost: 150,
    teleport_dist_factor: 0.11,
    teleport_ring_penalty: 0.30,
  },
  buildings: {
    hq: {
      kind_id: 0,
      name: "Ядро",
      radius: 28,
      max_per_player: 1,
      levels: [
        { cost: 0, trophy_cost: 0, energy_cap: 400, trophy_cap: 200, hp: 1800, income_per_sec: 3.0, unit_cap_bonus: 6 },
        { cost: 0, trophy_cost: 85, energy_cap: 950, trophy_cap: 420, hp: 3200, income_per_sec: 5.0, unit_cap_bonus: 14 },
        { cost: 0, trophy_cost: 320, energy_cap: 2000, trophy_cap: 600, hp: 3800, income_per_sec: 8.0, unit_cap_bonus: 24 },
      ],
    },
    generator: {
      kind_id: 1,
      name: "Генератор",
      radius: 20,
      levels: [
        { cost: 20, hp: 120, income_per_sec: 1.8 },
        { cost: 45, hp: 200, income_per_sec: 3.5 },
        { cost: 95, hp: 300, income_per_sec: 6.0 },
      ],
      effects: {},
    },
    turret: {
      kind_id: 2,
      name: "Турель",
      radius: 28,
      levels: [
        { cost: 70, trophy_cost: 0, hp: 520, range: 250 },
        { cost: 160, trophy_cost: 0, hp: 800, range: 285 },
        { cost: 360, trophy_cost: 0, hp: 1200, range: 325 },
      ],
    },
    barracks: {
      kind_id: 3,
      name: "Дроны",
      radius: 28,
      levels: [
        { cost: 90, trophy_cost: 0, hp: 420, spawn_interval_sec: 4.2, unit_cap_bonus: 1, local_supply_cap: 3 },
        { cost: 210, trophy_cost: 0, hp: 700, spawn_interval_sec: 3.4, unit_cap_bonus: 2, local_supply_cap: 4 },
        { cost: 450, trophy_cost: 0, hp: 1100, spawn_interval_sec: 2.6, unit_cap_bonus: 3, local_supply_cap: 6 },
      ],
    },
    beacon: {
      kind_id: 5,
      name: "Маяк",
      radius: 20,
      levels: [
        { cost: 130, trophy_cost: 90, hp: 450 },
        { cost: 280, trophy_cost: 180, hp: 750 },
        { cost: 520, trophy_cost: 340, hp: 1100 },
      ],
      effects: { zone_radius_bonus: 56 },
    },
  },
  units: {
    raider: {
      kind_id: 0,
      name: "Рейдер",
      cost: 14,
      radius: 8.5,
      hp: 78,
      damage: 13,
      attack_cooldown_sec: 0.55,
      speed: 235,
      range: 0,
      aggro_range: 290,
      supply_cost: 1,
    },
    archer: {
      kind_id: 1,
      name: "Стрелок",
      cost: 24,
      radius: 9.5,
      hp: 44,
      damage: 15,
      attack_cooldown_sec: 0.9,
      speed: 158,
      range: 175,
      aggro_range: 300,
      supply_cost: 1,
    },
    titan: {
      kind_id: 2,
      name: "Титан",
      cost: 65,
      radius: 14,
      hp: 540,
      damage: 40,
      attack_cooldown_sec: 0.95,
      speed: 128,
      range: 0,
      aggro_range: 280,
      supply_cost: 3,
    },
  },
};

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
  if (kindId === 0 || kindId === 2 || kindId === 3) return 28;
  return 20;
}

export async function loadBalanceConfig(): Promise<BalanceConfig> {
  try {
    const res = await fetch("/api/config");
    if (!res.ok) return DEFAULT_BALANCE;
    return (await res.json()) as BalanceConfig;
  } catch {
    return DEFAULT_BALANCE;
  }
}
