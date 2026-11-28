use std::collections::HashMap;

pub type PlayerId = u32;
pub type EntityId = u32;

#[derive(Debug, Clone, Copy)]
pub struct SquadOrder {
    pub target_x: f32,
    pub target_y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct BotKnownTarget {
    pub x: f32,
    pub y: f32,
    pub hq_lvl: u8,
    pub hq_hp: u16,
    pub hq_max_hp: u16,
    pub gens: u16,
    pub turret_power: f32,
    pub defending_unit_power: f32,
    pub trophies: f32,
    pub observed_at_sec: f32,
    pub is_full_intel: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmyMode {
    Defend = 0,
    Rally = 1,
}

impl ArmyMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Defend),
            1 => Some(Self::Rally),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub id: PlayerId,
    pub user_id: Option<i64>,
    pub name: String,
    pub skin_id: u16,
    pub energy: f32,
    pub trophies: f32,
    pub total_energy_earned: f32,
    pub unit_cap: u16,
    pub hq_level: u8,
    pub army_mode: ArmyMode,
    pub rally_x: f32,
    pub rally_y: f32,
    pub spawn_protection_left: f32,
    pub score: u32,
    pub kills: u32,
    pub buildings_destroyed: u32,
    pub survival_sec: f32,
    pub is_bot: bool,
    pub bot_tier: u8,
    pub bot_think_cooldown: f32,
    pub teleport_cooldown_left: f32,
    pub spawn_counter: u32,
    pub last_damaged_sec: f32,
    pub bot_archetype: u8,
    pub regen_accum: f32,
    pub regen_enabled: bool,
    pub bot_known_intel: HashMap<PlayerId, BotKnownTarget>,
    pub bot_scout_angle: f32,
    pub last_teleport_sec: f32,
    pub squads: HashMap<u8, SquadOrder>,
    pub next_squad_id: u8,
    pub base_rally: Option<SquadOrder>,
    pub reinforce_squad: Option<u8>,
}

impl PlayerState {
    pub fn is_in_combat(&self) -> bool {
        (self.survival_sec - self.last_damaged_sec) < 6.0
    }
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub player_id: PlayerId,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub team_id: u8,
    pub skin_id: u16,
}

#[derive(Debug, Clone)]
pub struct Building {
    pub id: EntityId,
    pub owner_id: PlayerId,
    pub kind: u8,
    pub level: u8,
    pub meta: u8,
    pub rel_x: f32,
    pub rel_y: f32,
    pub hp: u16,
    pub max_hp: u16,
    pub angle: u8,
    pub cooldown_left: f32,
    pub invested_cost: f32,
    pub invested_trophies: f32,
    pub last_hit_by: Option<PlayerId>,
    pub last_hit_src: u8,
}

#[derive(Debug, Clone)]
pub struct Unit {
    pub id: EntityId,
    pub owner_id: PlayerId,
    pub kind: u8,
    pub squad: u8,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub hp: u16,
    pub max_hp: u16,
    pub attack_cooldown_left: f32,
    pub last_hit_by: Option<PlayerId>,
    pub last_hit_src: u8,
    pub regen_accum: f32,
    pub home_id: EntityId,
    pub knockback_vx: f32,
    pub knockback_vy: f32,
    pub target_id: EntityId,
    pub target_idx: usize,
    pub target_is_building: bool,
}

#[derive(Debug, Clone)]
pub struct Projectile {
    pub id: EntityId,
    pub owner_id: PlayerId,
    pub kind: u8,
    pub src_tag: u8,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub target_x: f32,
    pub target_y: f32,
    pub target_id: EntityId,
    pub target_idx: usize,
    pub target_is_building: bool,
    pub damage: u16,
    pub ttl: f32,
    pub splash_radius: f32,
}

#[derive(Debug, Clone)]
pub struct GameOverEvent {
    pub player_id: PlayerId,
    pub user_id: Option<i64>,
    pub score: u32,
    pub survival_sec: u32,
    pub kills: u32,
    pub buildings_destroyed: u32,
    pub reason: u8,
}
