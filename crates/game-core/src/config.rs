use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_KIND_SLOTS: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct UnitFastStats {
    pub radius: f32,
    pub hp: f32,
    pub damage: f32,
    pub attack_cooldown_sec: f32,
    pub speed: f32,
    pub range: f32,
    pub aggro_range: f32,
    pub cost: f32,
    pub supply_cost: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceConfig {
    pub world: WorldConfig,
    pub buildings: HashMap<String, BuildingConfig>,
    pub units: HashMap<String, UnitConfig>,
    #[serde(default)]
    pub pve: PveConfig,
    #[serde(skip)]
    building_by_kind_cache: [Option<BuildingConfig>; MAX_KIND_SLOTS],
    #[serde(skip)]
    unit_by_kind_cache: [Option<UnitConfig>; MAX_KIND_SLOTS],
    #[serde(skip)]
    caches_built: bool,
}

impl BalanceConfig {
    pub fn from_json(raw: &str) -> Result<Self, serde_json::Error> {
        let mut cfg: Self = serde_json::from_str(raw)?;
        cfg.rebuild_caches();
        Ok(cfg)
    }

    pub fn rebuild_caches(&mut self) {
        self.building_by_kind_cache = std::array::from_fn(|_| None);
        for b in self.buildings.values() {
            let idx = usize::from(b.kind_id);
            if idx < MAX_KIND_SLOTS {
                self.building_by_kind_cache[idx] = Some(b.clone());
            }
        }
        self.unit_by_kind_cache = std::array::from_fn(|_| None);
        for u in self.units.values() {
            let idx = usize::from(u.kind_id);
            if idx < MAX_KIND_SLOTS {
                self.unit_by_kind_cache[idx] = Some(u.clone());
            }
        }
        self.caches_built = true;
    }

    #[inline]
    pub fn building_by_kind(&self, kind_id: u8) -> Option<&BuildingConfig> {
        if self.caches_built {
            let idx = usize::from(kind_id);
            if idx < MAX_KIND_SLOTS {
                return self.building_by_kind_cache[idx].as_ref();
            }
        }
        self.buildings.values().find(|b| b.kind_id == kind_id)
    }

    #[inline]
    pub fn unit_by_kind(&self, kind_id: u8) -> Option<&UnitConfig> {
        if self.caches_built {
            let idx = usize::from(kind_id);
            if idx < MAX_KIND_SLOTS {
                return self.unit_by_kind_cache[idx].as_ref();
            }
        }
        self.units.values().find(|u| u.kind_id == kind_id)
    }

    #[inline]
    pub fn unit_fast_stats(&self, kind_id: u8) -> UnitFastStats {
        if let Some(c) = self.unit_by_kind(kind_id) {
            UnitFastStats {
                radius: c.radius,
                hp: c.hp,
                damage: c.damage,
                attack_cooldown_sec: c.attack_cooldown_sec,
                speed: c.speed,
                range: if kind_id == 1 && c.range <= 0.0 {
                    175.0
                } else {
                    c.range
                },
                aggro_range: c.aggro_range,
                cost: c.cost.max(0.0),
                supply_cost: c.supply_cost.max(1),
            }
        } else {
            match kind_id {
                1 => UnitFastStats {
                    radius: 9.5,
                    hp: 44.0,
                    damage: 15.0,
                    attack_cooldown_sec: 0.90,
                    speed: 158.0,
                    range: 175.0,
                    aggro_range: 300.0,
                    cost: 24.0,
                    supply_cost: 1,
                },
                2 => UnitFastStats {
                    radius: 14.0,
                    hp: 540.0,
                    damage: 40.0,
                    attack_cooldown_sec: 0.95,
                    speed: 128.0,
                    range: 0.0,
                    aggro_range: 290.0,
                    cost: 65.0,
                    supply_cost: 3,
                },
                _ => UnitFastStats {
                    radius: 8.5,
                    hp: 78.0,
                    damage: 13.0,
                    attack_cooldown_sec: 0.55,
                    speed: 235.0,
                    range: 0.0,
                    aggro_range: 290.0,
                    cost: 14.0,
                    supply_cost: 1,
                },
            }
        }
    }

    #[inline]
    pub fn node_trophy_cost(&self, existing_nodes: i32) -> f32 {
        let cfg = self.building_by_kind(5);
        let base = cfg
            .and_then(|c| c.levels.first())
            .map(|l| l.trophy_cost)
            .filter(|&v| v > 0.0)
            .unwrap_or(90.0);
        let mult = cfg
            .map(|c| c.effects.node_cost_mult)
            .filter(|&v| v > 0.0)
            .unwrap_or(1.65);
        (base * mult.powi(existing_nodes)).round()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldConfig {
    pub map_radius: f32,
    pub tick_rate: u8,
    pub spawn_protection_sec: f32,
    pub zone_base_radius: f32,
    pub zone_repulsion_strength: f32,
    pub sell_refund_ratio: f32,
    pub starting_energy: f32,
    #[serde(default = "default_ring3_radius")]
    pub ring3_radius: f32,
    #[serde(default = "default_ring2_radius")]
    pub ring2_radius: f32,
    #[serde(default = "default_gravity_pull")]
    pub gravity_pull_strength: f32,
    #[serde(default = "default_base_regen_pct")]
    pub base_regen_hp_per_sec_pct: f32,
    #[serde(default = "default_base_regen_hq_mult")]
    pub base_regen_hq_mult: f32,
    #[serde(default = "default_base_regen_energy_per_hp")]
    pub base_regen_energy_per_hp: f32,
    #[serde(default = "default_tp_base")]
    pub teleport_base_cost: f32,
    #[serde(default = "default_tp_dist")]
    pub teleport_dist_factor: f32,
    #[serde(default = "default_tp_ring")]
    pub teleport_ring_penalty: f32,
    #[serde(default = "default_tp_charge_sec")]
    pub teleport_charge_sec: f32,
    #[serde(default = "default_tp_patrol_pull_speed")]
    pub teleport_patrol_pull_speed: f32,
    #[serde(default = "default_tp_zone_clearance")]
    pub teleport_zone_clearance: f32,
    #[serde(default = "default_tp_node_clearance")]
    pub teleport_node_clearance: f32,
    #[serde(default = "default_camera_wasd_speed")]
    pub camera_wasd_speed: f32,
    #[serde(default = "default_hex_size")]
    pub hex_size: f32,
    #[serde(default)]
    pub bot_count: usize,
}

fn default_ring3_radius() -> f32 {
    1050.0
}
fn default_ring2_radius() -> f32 {
    2150.0
}
fn default_gravity_pull() -> f32 {
    65.0
}
fn default_base_regen_pct() -> f32 {
    0.04
}
fn default_base_regen_hq_mult() -> f32 {
    0.4
}
fn default_base_regen_energy_per_hp() -> f32 {
    0.09
}
fn default_tp_base() -> f32 {
    150.0
}
fn default_tp_dist() -> f32 {
    0.11
}
fn default_tp_ring() -> f32 {
    0.30
}
fn default_tp_charge_sec() -> f32 {
    1.8
}
fn default_tp_patrol_pull_speed() -> f32 {
    320.0
}
fn default_tp_zone_clearance() -> f32 {
    150.0
}
fn default_tp_node_clearance() -> f32 {
    120.0
}
fn default_camera_wasd_speed() -> f32 {
    650.0
}
fn default_hex_size() -> f32 {
    34.0
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildingConfig {
    pub kind_id: u8,
    pub name: String,
    pub radius: f32,
    #[serde(default)]
    pub max_per_player: Option<u32>,
    pub levels: Vec<BuildingLevelConfig>,
    #[serde(default)]
    pub effects: BuildingEffectsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildingLevelConfig {
    pub cost: f32,
    #[serde(default)]
    pub trophy_cost: f32,
    #[serde(default)]
    pub energy_cap: f32,
    pub hp: u16,
    #[serde(default)]
    pub income_per_sec: f32,
    #[serde(default)]
    pub fire_rate: f32,
    #[serde(default)]
    pub damage: f32,
    #[serde(default)]
    pub range: f32,
    #[serde(default)]
    pub projectile_speed: f32,
    #[serde(default)]
    pub spawn_interval_sec: f32,
    #[serde(default)]
    pub unit_cap_bonus: u16,
    #[serde(default)]
    pub shot_cost: f32,
    #[serde(default)]
    pub splash_radius: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildingEffectsConfig {
    #[serde(default = "default_node_hex_radius")]
    pub node_hex_radius: i32,
    #[serde(default = "default_node_cost_mult")]
    pub node_cost_mult: f32,
}

fn default_node_hex_radius() -> i32 {
    2
}

fn default_node_cost_mult() -> f32 {
    1.65
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitConfig {
    pub kind_id: u8,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cost: f32,
    pub radius: f32,
    pub hp: f32,
    pub damage: f32,
    pub attack_cooldown_sec: f32,
    pub speed: f32,
    #[serde(default)]
    pub range: f32,
    pub aggro_range: f32,
    #[serde(default = "default_supply_cost")]
    pub supply_cost: u16,
}

fn default_supply_cost() -> u16 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PveConfig {
    pub hive_count: usize,
    pub hive_hq_level: u8,
    pub hive_target_nodes: usize,
    pub hive_target_barracks: usize,
    pub hive_target_generators: usize,
    pub hive_regrow_interval_sec: f32,
    pub hive_respawn_sec: f32,
    pub hive_aggro_range: f32,
    pub hive_gen_trigger_count: usize,
    pub hive_wave_size: usize,
    pub hive_hq_trophy_reward: f32,
    pub hive_hq_energy_reward: f32,
    pub attractor_max_active: usize,
    pub attractor_initial_delay_sec: f32,
    pub attractor_spawn_interval_sec: f32,
    pub attractor_hp: u16,
    pub attractor_radius: f32,
    pub attractor_trophy_reward: f32,
    pub attractor_energy_reward: f32,
}

impl Default for PveConfig {
    fn default() -> Self {
        Self {
            hive_count: 3,
            hive_hq_level: 2,
            hive_target_nodes: 2,
            hive_target_barracks: 4,
            hive_target_generators: 12,
            hive_regrow_interval_sec: 3.0,
            hive_respawn_sec: 25.0,
            hive_aggro_range: 1150.0,
            hive_gen_trigger_count: 6,
            hive_wave_size: 7,
            hive_hq_trophy_reward: 120.0,
            hive_hq_energy_reward: 240.0,
            attractor_max_active: 2,
            attractor_initial_delay_sec: 18.0,
            attractor_spawn_interval_sec: 45.0,
            attractor_hp: 2600,
            attractor_radius: 36.0,
            attractor_trophy_reward: 150.0,
            attractor_energy_reward: 220.0,
        }
    }
}
