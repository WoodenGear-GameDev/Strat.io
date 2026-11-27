use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceConfig {
    pub world: WorldConfig,
    pub buildings: HashMap<String, BuildingConfig>,
    pub units: HashMap<String, UnitConfig>,
}

impl BalanceConfig {
    pub fn from_json(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(raw)
    }

    pub fn building_by_kind(&self, kind_id: u8) -> Option<&BuildingConfig> {
        self.buildings.values().find(|b| b.kind_id == kind_id)
    }

    pub fn unit_by_kind(&self, kind_id: u8) -> Option<&UnitConfig> {
        self.units.values().find(|u| u.kind_id == kind_id)
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
    #[serde(default = "default_core_radius")]
    pub core_radius: f32,
    #[serde(default = "default_core_push")]
    pub core_push_strength: f32,
    #[serde(default = "default_tp_base")]
    pub teleport_base_cost: f32,
    #[serde(default = "default_tp_dist")]
    pub teleport_dist_factor: f32,
    #[serde(default = "default_tp_ring")]
    pub teleport_ring_penalty: f32,
    #[serde(default = "default_tp_cd")]
    pub teleport_cooldown_sec: f32,
    #[serde(default)]
    pub bot_count: usize,
}

fn default_core_radius() -> f32 {
    260.0
}
fn default_core_push() -> f32 {
    55.0
}
fn default_tp_base() -> f32 {
    120.0
}
fn default_tp_dist() -> f32 {
    0.08
}
fn default_tp_ring() -> f32 {
    0.18
}
fn default_tp_cd() -> f32 {
    25.0
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
    #[serde(default)]
    pub trophy_cap: f32,
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
    pub local_supply_cap: u16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildingEffectsConfig {
    #[serde(default)]
    pub aura_radius: f32,
    #[serde(default)]
    pub fire_rate_mult: f32,
    #[serde(default)]
    pub zone_radius_bonus: f32,
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
