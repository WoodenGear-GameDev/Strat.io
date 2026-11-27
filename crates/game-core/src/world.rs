use crate::config::BalanceConfig;
use crate::protocol::{
    ClientMessage, LeaderboardEntry, PlayerMetaEntry, ServerMessage, SnapshotBuilding,
    SnapshotProjectile, SnapshotUnit, SnapshotZone,
};
use crate::spatial::SpatialGrid;
use crate::telemetry::{
    tag_name, TelemetryLogger, DST_BUILDING_OFFSET, SRC_RAD, SRC_TURRET_1, SRC_TURRET_2,
    SRC_TURRET_3, SRC_UNKNOWN,
};
use crate::types::{
    ArmyMode, BuildCombatLock, Building, EntityId, GameOverEvent, PlayerId, PlayerState,
    Projectile, Unit, Zone,
};
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

pub const RING1_RADIUS: f32 = 1100.0;
pub const RING2_RADIUS: f32 = 2150.0;
pub const HEX_SIZE: f32 = 34.0;
pub type TriCoord = (i32, i32, u8);

pub const HEX_EDGE_DIRS: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

pub fn is_large_building(kind: u8) -> bool {
    kind == 0 || kind == 2 || kind == 3
}

pub fn hex_to_rel(q: i32, r: i32) -> (f32, f32) {
    let sqrt3 = 3.0_f32.sqrt();
    let qf = q as f32;
    let rf = r as f32;
    let x = HEX_SIZE * (sqrt3 * qf + (sqrt3 / 2.0) * rf);
    let y = HEX_SIZE * 1.5 * rf;
    (x, y)
}

pub fn rel_to_hex_round(x: f32, y: f32) -> (i32, i32) {
    let sqrt3 = 3.0_f32.sqrt();
    let qf = ((sqrt3 / 3.0) * x - (1.0 / 3.0) * y) / HEX_SIZE;
    let rf = ((2.0 / 3.0) * y) / HEX_SIZE;
    let sf = -qf - rf;

    let mut rq = qf.round();
    let mut rr = rf.round();
    let rs = sf.round();

    let q_diff = (rq - qf).abs();
    let r_diff = (rr - rf).abs();
    let s_diff = (rs - sf).abs();

    if q_diff > r_diff && q_diff > s_diff {
        rq = -rr - rs;
    } else if r_diff > s_diff {
        rr = -rq - rs;
    }

    (rq as i32, rr as i32)
}

pub fn hex_dist(q1: i32, r1: i32, q2: i32, r2: i32) -> i32 {
    ((q1 - q2).abs() + (q1 + r1 - q2 - r2).abs() + (r1 - r2).abs()) / 2
}

pub fn hex_vertex(q: i32, r: i32, i: u8) -> (f32, f32) {
    let (cx, cy) = hex_to_rel(q, r);
    let ang = f32::from(i) * (TAU / 6.0) - TAU / 12.0;
    (cx + ang.cos() * HEX_SIZE, cy + ang.sin() * HEX_SIZE)
}

pub fn collect_rhombuses(open_hexes: &HashSet<(i32, i32)>) -> Vec<([TriCoord; 2], (f32, f32))> {
    let mut out = Vec::with_capacity(open_hexes.len() * 8);
    for &(q, r) in open_hexes {
        let (cx, cy) = hex_to_rel(q, r);
        for s in 0..6_u8 {
            let s_next = (s + 1) % 6;
            let (vx1, vy1) = hex_vertex(q, r, s_next);
            out.push((
                [(q, r, s), (q, r, s_next)],
                ((cx + vx1) * 0.5, (cy + vy1) * 0.5),
            ));

            let (dq, dr) = HEX_EDGE_DIRS[s as usize];
            let nq = q + dq;
            let nr = r + dr;
            if open_hexes.contains(&(nq, nr)) && (q, r) < (nq, nr) {
                let (vx0, vy0) = hex_vertex(q, r, s);
                let s_opp = (s + 3) % 6;
                out.push((
                    [(q, r, s), (nq, nr, s_opp)],
                    ((vx0 + vx1) * 0.5, (vy0 + vy1) * 0.5),
                ));
            }
        }
    }
    out
}

pub fn core_hex_radius(level: u8) -> i32 {
    match level {
        0 | 1 => 1,
        2 => 2,
        _ => 3,
    }
}

fn insert_hex_disk(center_q: i32, center_r: i32, hex_r: i32, out: &mut HashSet<(i32, i32)>) {
    for dq in -hex_r..=hex_r {
        let min_dr = (-hex_r).max(-dq - hex_r);
        let max_dr = hex_r.min(-dq + hex_r);
        for dr in min_dr..=max_dr {
            out.insert((center_q + dq, center_r + dr));
        }
    }
}

pub fn hq_zone_radius(_base: f32, level: u8) -> f32 {
    match level {
        0 | 1 => 95.0,
        2 => 145.0,
        _ => 200.0,
    }
}

pub fn beacon_ext_radius(level: u8) -> f32 {
    match level {
        0 | 1 => 68.0,
        2 => 85.0,
        _ => 105.0,
    }
}

#[derive(Default)]
struct BotPlayerIntel {
    hq_id: EntityId,
    hq_lvl: u8,
    hq_hp: u16,
    hq_max_hp: u16,
    gens: usize,
    turrets: usize,
    barracks: usize,
    beacons: usize,
    total_buildings: usize,
    turret_power: f32,
    gen_positions: Vec<(f32, f32)>,
    turret_positions: Vec<(f32, f32)>,
    beacon_positions: Vec<(f32, f32)>,
    barracks_info: Vec<(EntityId, u8, u8)>,
    non_hq_buildings: Vec<(EntityId, u8, u8)>,
    own_units: usize,
    own_combat_power: f32,
    defending_unit_power: f32,
    enemy_threat_power: f32,
}

pub struct World {
    pub config: BalanceConfig,
    pub tick: u32,
    seed: u64,
    next_entity_id: EntityId,
    next_player_id: PlayerId,
    rng: SmallRng,
    pub players: HashMap<PlayerId, PlayerState>,
    pub zones: HashMap<PlayerId, Zone>,
    pub buildings: Vec<Building>,
    pub units: Vec<Unit>,
    pub projectiles: Vec<Projectile>,
    pub telemetry: Option<TelemetryLogger>,
}

impl World {
    pub fn new(config: BalanceConfig, seed: u64) -> Self {
        Self {
            config,
            tick: 0,
            seed,
            next_entity_id: 1,
            next_player_id: 1,
            rng: SmallRng::seed_from_u64(seed),
            players: HashMap::new(),
            zones: HashMap::new(),
            buildings: Vec::new(),
            units: Vec::new(),
            projectiles: Vec::new(),
            telemetry: None,
        }
    }

    pub fn enable_telemetry(&mut self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let mut logger = TelemetryLogger::new(path.as_ref(), self.config.world.tick_rate)?;
        logger.log_meta(
            self.seed,
            self.config.world.tick_rate,
            self.config.world.spawn_protection_sec,
        );
        self.telemetry = Some(logger);
        Ok(())
    }

    fn alloc_entity_id(&mut self) -> EntityId {
        let id = self.next_entity_id;
        self.next_entity_id = self.next_entity_id.wrapping_add(1).max(1);
        id
    }

    pub fn add_player(
        &mut self,
        name: String,
        user_id: Option<i64>,
        _team_id: u8,
        skin_id: u16,
    ) -> PlayerId {
        self.add_player_internal(name, user_id, skin_id, 0, 0)
    }

    fn add_player_internal(
        &mut self,
        name: String,
        user_id: Option<i64>,
        skin_id: u16,
        bot_tier: u8,
        bot_archetype: u8,
    ) -> PlayerId {
        let player_id = self.next_player_id;
        self.next_player_id = self.next_player_id.wrapping_add(1).max(1);

        let (min_r, max_r) = match bot_tier {
            1 => (520.0_f32, 720.0_f32),
            2 => (1440.0_f32, 1720.0_f32),
            3 => (2420.0_f32, 2760.0_f32),
            _ => (520.0_f32, 720.0_f32),
        };
        let new_r = hq_zone_radius(self.config.world.zone_base_radius, 1);
        let (mut spawn_x, mut spawn_y) = (min_r, 0.0_f32);
        let mut best_clearance = f32::NEG_INFINITY;
        for _ in 0..16 {
            let angle = self.rng.gen_range(0.0..TAU);
            let dist = self.rng.gen_range(min_r..=max_r);
            let cx = angle.cos() * dist;
            let cy = angle.sin() * dist;
            let mut min_clear = f32::MAX;
            for z in self.zones.values() {
                let clear = (cx - z.x).hypot(cy - z.y) - (new_r + z.radius);
                if clear < min_clear {
                    min_clear = clear;
                }
            }
            if min_clear > best_clearance {
                best_clearance = min_clear;
                spawn_x = cx;
                spawn_y = cy;
                if best_clearance >= 170.0 {
                    break;
                }
            }
        }

        let start_hq_lvl: u8 = match bot_tier {
            2 => 2,
            3 => 3,
            _ => 1,
        };
        let hq_cfg = self.config.buildings.get("hq");
        let hq_kind = hq_cfg.map(|c| c.kind_id).unwrap_or(0);
        let hq_lvl_cfg = hq_cfg.and_then(|c| c.levels.get(usize::from(start_hq_lvl - 1)));
        let hq_hp = hq_lvl_cfg.map(|l| l.hp).unwrap_or(1800);
        let base_cap = hq_lvl_cfg.map(|l| l.unit_cap_bonus).unwrap_or(6);

        let seed_earned = match bot_tier {
            2 => 600.0,
            3 => 1800.0,
            _ => 0.0,
        };
        let seed_trophies = match bot_tier {
            2 => 70.0,
            3 => 160.0,
            _ => 0.0,
        };

        let player = PlayerState {
            id: player_id,
            user_id,
            name,
            skin_id,
            energy: self.config.world.starting_energy,
            trophies: seed_trophies,
            total_energy_earned: seed_earned,
            unit_cap: base_cap,
            army_mode: ArmyMode::Defend,
            rally_x: spawn_x,
            rally_y: spawn_y,
            spawn_protection_left: self.config.world.spawn_protection_sec,
            score: 0,
            kills: 0,
            buildings_destroyed: 0,
            survival_sec: 0.0,
            is_bot: bot_tier > 0,
            bot_tier,
            bot_think_cooldown: 1.0,
            teleport_cooldown_left: 0.0,
            spawn_counter: 0,
            last_damaged_sec: -100.0,
            bot_archetype,
            regen_accum: 0.0,
            build_combat_locks: Vec::new(),
            bot_known_intel: HashMap::new(),
            bot_scout_angle: self.rng.gen_range(0.0..TAU),
            last_teleport_sec: -100.0,
            squads: HashMap::new(),
            next_squad_id: 1,
            base_rally: None,
            reinforce_squad: None,
        };

        let zone = Zone {
            player_id,
            x: spawn_x,
            y: spawn_y,
            radius: hq_zone_radius(self.config.world.zone_base_radius, start_hq_lvl),
            team_id: 0,
            skin_id,
        };

        let hq_id = self.alloc_entity_id();
        let hq = Building {
            id: hq_id,
            owner_id: player_id,
            kind: hq_kind,
            level: start_hq_lvl,
            meta: 0,
            rel_x: 0.0,
            rel_y: 0.0,
            hp: hq_hp,
            max_hp: hq_hp,
            angle: 0,
            cooldown_left: 0.0,
            invested_cost: 0.0,
            invested_trophies: 0.0,
            last_hit_by: None,
            last_hit_src: SRC_UNKNOWN,
        };

        self.players.insert(player_id, player);
        self.zones.insert(player_id, zone);
        self.buildings.push(hq);

        player_id
    }

    pub fn remove_player(&mut self, player_id: PlayerId) {
        self.players.remove(&player_id);
        self.zones.remove(&player_id);
        self.buildings.retain(|b| b.owner_id != player_id);
        self.units.retain(|u| u.owner_id != player_id);
        self.projectiles.retain(|p| p.owner_id != player_id);
    }

    fn ring_for_radius(r: f32) -> u8 {
        if r < RING1_RADIUS {
            1
        } else if r < RING2_RADIUS {
            2
        } else {
            3
        }
    }

    /// Computes movement direction from `(ux, uy)` to `(tx, ty)` in polar coordinates `(r, theta)`
    /// so long routes curve along arena rings instead of cutting through inner radiation rings.
    fn polar_move_dir(ux: f32, uy: f32, tx: f32, ty: f32) -> (f32, f32) {
        let dx = tx - ux;
        let dy = ty - uy;
        let dist = dx.hypot(dy);
        if dist <= 0.001 {
            return (0.0, 0.0);
        }
        let ur = ux.hypot(uy);
        let tr = tx.hypot(ty);
        if dist < 180.0 || ur < 90.0 || tr < 90.0 {
            return (dx / dist, dy / dist);
        }
        let ut = uy.atan2(ux);
        let tt = ty.atan2(tx);
        let mut dtheta = tt - ut;
        while dtheta > std::f32::consts::PI {
            dtheta -= TAU;
        }
        while dtheta < -std::f32::consts::PI {
            dtheta += TAU;
        }
        if dtheta.abs() < 0.14 {
            return (dx / dist, dy / dist);
        }
        let rx = ux / ur;
        let ry = uy / ur;
        let tx_hat = -ry;
        let ty_hat = rx;
        let dr = tr - ur;
        let arc = ((ur + tr) * 0.5) * dtheta;
        let vx = rx * dr + tx_hat * arc;
        let vy = ry * dr + ty_hat * arc;
        let len = vx.hypot(vy);
        if len > 0.001 {
            (vx / len, vy / len)
        } else {
            (dx / dist, dy / dist)
        }
    }

    fn recompute_unit_cap(&mut self, player_id: PlayerId) {
        let mut cap: u16 = 0;
        for b in &self.buildings {
            if b.owner_id == player_id {
                if let Some(cfg) = self.config.building_by_kind(b.kind) {
                    if let Some(lvl) = cfg.levels.get((b.level as usize).saturating_sub(1)) {
                        cap = cap.saturating_add(lvl.unit_cap_bonus);
                    }
                }
            }
        }
        if let Some(p) = self.players.get_mut(&player_id) {
            p.unit_cap = cap;
        }
    }

    fn hq_level_of(&self, player_id: PlayerId) -> u8 {
        self.buildings
            .iter()
            .find(|b| b.owner_id == player_id && b.kind == 0)
            .map(|b| b.level)
            .unwrap_or(1)
    }

    fn hq_level_map(&self) -> HashMap<PlayerId, u8> {
        let mut map = HashMap::new();
        for b in &self.buildings {
            if b.kind == 0 {
                map.insert(b.owner_id, b.level);
            }
        }
        map
    }

    fn energy_cap_for_hq(&self, hq_lvl: u8) -> f32 {
        let cfg_cap = self
            .config
            .building_by_kind(0)
            .and_then(|c| c.levels.get(usize::from(hq_lvl.saturating_sub(1))))
            .map(|l| l.energy_cap)
            .unwrap_or(0.0);
        if cfg_cap > 0.0 {
            cfg_cap
        } else {
            match hq_lvl {
                0 | 1 => 400.0,
                2 => 950.0,
                _ => 2000.0,
            }
        }
    }

    fn trophy_cap_for_hq(&self, hq_lvl: u8) -> f32 {
        let cfg_cap = self
            .config
            .building_by_kind(0)
            .and_then(|c| c.levels.get(usize::from(hq_lvl.saturating_sub(1))))
            .map(|l| l.trophy_cap)
            .unwrap_or(0.0);
        if cfg_cap > 0.0 {
            cfg_cap
        } else {
            match hq_lvl {
                0 | 1 => 200.0,
                2 => 420.0,
                _ => 600.0,
            }
        }
    }

    fn unit_supply_cost(&self, ukind: u8) -> u16 {
        self.config
            .unit_by_kind(ukind)
            .map(|c| c.supply_cost.max(1))
            .unwrap_or_else(|| if ukind == 2 { 3 } else { 1 })
    }

    fn unit_energy_cost(&self, ukind: u8) -> f32 {
        self.config
            .unit_by_kind(ukind)
            .map(|c| c.cost.max(0.0))
            .unwrap_or_else(|| match ukind {
                1 => 24.0,
                2 => 65.0,
                _ => 14.0,
            })
    }

    fn tier_dmg_mult(attacker_hq: u8, victim_hq: u8, is_hq_building: bool) -> f32 {
        if attacker_hq > victim_hq {
            let diff = attacker_hq - victim_hq;
            if is_hq_building {
                match diff {
                    1 => 0.25,
                    _ => 0.15,
                }
            } else {
                match diff {
                    1 => 0.50,
                    _ => 0.30,
                }
            }
        } else if attacker_hq < victim_hq {
            let diff = victim_hq - attacker_hq;
            match diff {
                1 => 1.35,
                _ => 1.75,
            }
        } else {
            1.0
        }
    }

    fn tier_reward_mult(killer_hq: u8, victim_hq: u8) -> (f32, f32) {
        if killer_hq > victim_hq {
            let diff = killer_hq - victim_hq;
            match diff {
                1 => (0.50, 0.50),
                _ => (0.20, 0.25),
            }
        } else if killer_hq < victim_hq {
            let diff = victim_hq - killer_hq;
            match diff {
                1 => (1.45, 1.35),
                _ => (1.90, 1.65),
            }
        } else {
            (1.0, 1.0)
        }
    }

    fn has_vision_at(&self, player_id: PlayerId, tx: f32, ty: f32) -> bool {
        if let Some(z) = self.zones.get(&player_id) {
            if (tx - z.x).hypot(ty - z.y) <= z.radius + 360.0 {
                return true;
            }
            for b in &self.buildings {
                if b.owner_id == player_id && b.kind == 5 {
                    let vr = beacon_ext_radius(b.level) + 220.0;
                    if (tx - (z.x + b.rel_x)).hypot(ty - (z.y + b.rel_y)) <= vr {
                        return true;
                    }
                }
            }
        }
        for u in &self.units {
            if u.owner_id == player_id && u.hp > 0 && u.kind != 4 {
                if (tx - u.x).hypot(ty - u.y) <= 290.0 {
                    return true;
                }
            }
        }
        false
    }

    fn find_safe_orbit_spot(&self, player_id: PlayerId, target_r: f32, zone_r: f32, cur_angle: f32) -> (f32, f32) {
        let mut best_x = cur_angle.cos() * target_r;
        let mut best_y = cur_angle.sin() * target_r;
        let mut best_clear = f32::NEG_INFINITY;
        let steps = 24_i32;
        for step in 0..steps {
            let signed = if step == 0 {
                0
            } else if step % 2 == 1 {
                (step + 1) / 2
            } else {
                -(step / 2)
            };
            let ang = cur_angle + (signed as f32) * (TAU / steps as f32);
            let cx = ang.cos() * target_r;
            let cy = ang.sin() * target_r;
            let mut min_clear = f32::MAX;
            for other in self.zones.values() {
                if other.player_id == player_id {
                    continue;
                }
                let clear = (cx - other.x).hypot(cy - other.y) - (zone_r + other.radius);
                if clear < min_clear {
                    min_clear = clear;
                }
            }
            for b in &self.buildings {
                if b.owner_id != player_id && b.kind == 5 {
                    if let Some(other) = self.zones.get(&b.owner_id) {
                        let ext_r = beacon_ext_radius(b.level);
                        let clear = (cx - (other.x + b.rel_x)).hypot(cy - (other.y + b.rel_y)) - (zone_r + ext_r);
                        if clear < min_clear {
                            min_clear = clear;
                        }
                    }
                }
            }
            if min_clear > best_clear {
                best_clear = min_clear;
                best_x = cx;
                best_y = cy;
                if best_clear >= 220.0 {
                    break;
                }
            }
        }
        (best_x, best_y)
    }

    fn grant_energy(&mut self, pid: PlayerId, amount: f32, cap_mult: f32, hq_lvl: u8) {
        if amount <= 0.0 {
            return;
        }
        let max_e = self.energy_cap_for_hq(hq_lvl) * cap_mult;
        if let Some(p) = self.players.get_mut(&pid) {
            p.total_energy_earned += amount;
            if p.energy < max_e {
                p.energy = (p.energy + amount).min(max_e);
            }
        }
    }

    fn grant_trophies(&mut self, pid: PlayerId, amount: f32, hq_lvl: u8) {
        if amount <= 0.0 {
            return;
        }
        let max_t = self.trophy_cap_for_hq(hq_lvl);
        if let Some(p) = self.players.get_mut(&pid) {
            p.trophies = (p.trophies + amount).min(max_t);
        }
    }

    fn collect_player_open_large_hexes(&self, player_id: PlayerId) -> HashSet<(i32, i32)> {
        let mut open = HashSet::new();
        let hq_lvl = self.hq_level_of(player_id);
        insert_hex_disk(0, 0, core_hex_radius(hq_lvl), &mut open);

        if hq_lvl >= 3 {
            for b in &self.buildings {
                if b.owner_id == player_id && b.kind == 5 {
                    let rad = beacon_ext_radius(b.level);
                    let hex_span = ((rad / (HEX_SIZE * 1.5)).ceil() as i32) + 1;
                    let (cq, cr) = rel_to_hex_round(b.rel_x, b.rel_y);
                    for dq in -hex_span..=hex_span {
                        for dr in -hex_span..=hex_span {
                            let lq = cq + dq;
                            let lr = cr + dr;
                            let (lx, ly) = hex_to_rel(lq, lr);
                            if (lx - b.rel_x).hypot(ly - b.rel_y) <= rad {
                                open.insert((lq, lr));
                            }
                        }
                    }
                }
            }
        }
        open
    }

    fn collect_player_occupied_tris(
        &self,
        player_id: PlayerId,
        rhombuses: &[([TriCoord; 2], (f32, f32))],
    ) -> HashSet<TriCoord> {
        let mut occupied = HashSet::new();
        for b in &self.buildings {
            if b.owner_id != player_id {
                continue;
            }
            if is_large_building(b.kind) {
                let (aq, ar) = rel_to_hex_round(b.rel_x, b.rel_y);
                for s in 0..6_u8 {
                    occupied.insert((aq, ar, s));
                }
            } else if let Some((tris, _)) = rhombuses.iter().min_by(|(_, (ax, ay)), (_, (bx, by))| {
                let da = (ax - b.rel_x).hypot(ay - b.rel_y);
                let db = (bx - b.rel_x).hypot(by - b.rel_y);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            }) {
                occupied.insert(tris[0]);
                occupied.insert(tris[1]);
            }
        }
        occupied
    }

    fn find_valid_placement(
        &self,
        player_id: PlayerId,
        building_kind: u8,
        rel_x: f32,
        rel_y: f32,
        max_dist: f32,
    ) -> Option<(f32, f32)> {
        if building_kind == 0 {
            return None;
        }
        let open_hexes = self.collect_player_open_large_hexes(player_id);
        let rhombuses = collect_rhombuses(&open_hexes);
        let occupied = self.collect_player_occupied_tris(player_id, &rhombuses);

        let mut best: Option<(f32, f32)> = None;
        let mut best_d = max_dist;

        if is_large_building(building_kind) {
            for &(q, r) in &open_hexes {
                if (0..6_u8).any(|s| occupied.contains(&(q, r, s))) {
                    continue;
                }
                let (cx, cy) = hex_to_rel(q, r);
                let d = (cx - rel_x).hypot(cy - rel_y);
                if d <= best_d {
                    best_d = d;
                    best = Some((cx, cy));
                }
            }
        } else {
            for &(tris, (mx, my)) in &rhombuses {
                if occupied.contains(&tris[0]) || occupied.contains(&tris[1]) {
                    continue;
                }
                let d = (mx - rel_x).hypot(my - rel_y);
                if d <= best_d {
                    best_d = d;
                    best = Some((mx, my));
                }
            }
        }
        best
    }

    fn prune_orphaned_buildings(&mut self, player_id: PlayerId) {
        if !self.zones.contains_key(&player_id) {
            return;
        }
        let open_hexes = self.collect_player_open_large_hexes(player_id);
        let mut reachable: HashSet<(i32, i32)> = HashSet::new();
        let mut queue: Vec<(i32, i32)> = Vec::new();
        if open_hexes.contains(&(0, 0)) {
            reachable.insert((0, 0));
            queue.push((0, 0));
        }

        let mut head = 0;
        while head < queue.len() {
            let (cq, cr) = queue[head];
            head += 1;
            for (dq, dr) in HEX_EDGE_DIRS {
                let next = (cq + dq, cr + dr);
                if open_hexes.contains(&next) && reachable.insert(next) {
                    queue.push(next);
                }
            }
        }

        let valid_rhombuses = collect_rhombuses(&reachable);
        let mut removed_station_ids: HashSet<EntityId> = HashSet::new();
        self.buildings.retain(|b| {
            if b.owner_id != player_id || b.kind == 0 {
                return true;
            }
            let keep = if is_large_building(b.kind) {
                let (aq, ar) = rel_to_hex_round(b.rel_x, b.rel_y);
                reachable.contains(&(aq, ar))
            } else {
                valid_rhombuses
                    .iter()
                    .any(|&(_, (mx, my))| (mx - b.rel_x).hypot(my - b.rel_y) <= 6.0)
            };
            if !keep && b.kind == 1 {
                removed_station_ids.insert(b.id);
            }
            keep
        });

        if !removed_station_ids.is_empty() {
            self.units
                .retain(|u| !(u.kind == 4 && removed_station_ids.contains(&u.home_id)));
        }

        self.recompute_unit_cap(player_id);
    }

    pub fn apply_command(&mut self, player_id: PlayerId, cmd: ClientMessage) -> Option<u8> {
        if !self.players.contains_key(&player_id) {
            return Some(1);
        }

        match cmd {
            ClientMessage::Build {
                building_kind,
                rel_x,
                rel_y,
            } => {
                if !rel_x.is_finite() || !rel_y.is_finite() {
                    return Some(3);
                }
                let Some(cfg) = self.config.building_by_kind(building_kind).cloned() else {
                    return Some(6);
                };
                if cfg.kind_id == 0 {
                    return Some(6);
                }
                let hq_lvl = self.hq_level_of(player_id);
                if building_kind == 5 && hq_lvl < 3 {
                    return Some(14);
                }
                let Some(lvl0) = cfg.levels.first() else {
                    return Some(6);
                };

                if let Some(max_count) = cfg.max_per_player {
                    let count = self
                        .buildings
                        .iter()
                        .filter(|b| b.owner_id == player_id && b.kind == building_kind)
                        .count() as u32;
                    if count >= max_count {
                        return Some(5);
                    }
                }

                let existing_beacons = if building_kind == 5 {
                    self.buildings
                        .iter()
                        .filter(|b| b.owner_id == player_id && b.kind == 5)
                        .count() as i32
                } else {
                    0
                };
                let trophy_cost = if building_kind == 5 {
                    (90.0 * 1.65_f32.powi(existing_beacons)).round()
                } else {
                    lvl0.trophy_cost
                };

                let Some((final_x, final_y)) =
                    self.find_valid_placement(player_id, building_kind, rel_x, rel_y, 75.0)
                else {
                    return Some(3);
                };

                let cost = lvl0.cost;
                let scaled_hp = lvl0.hp;

                let is_bot;
                {
                    let Some(player) = self.players.get_mut(&player_id) else {
                        return Some(1);
                    };
                    let now_sec = player.survival_sec;
                    player.build_combat_locks.retain(|lk| lk.expires_at_sec > now_sec);
                    if building_kind == 5 && player.is_in_combat() {
                        return Some(17);
                    }
                    if player
                        .build_combat_locks
                        .iter()
                        .any(|lk| (final_x - lk.rel_x).hypot(final_y - lk.rel_y) < lk.radius)
                    {
                        return Some(16);
                    }
                    if player.trophies < trophy_cost {
                        return Some(15);
                    }
                    if player.energy < cost {
                        return Some(2);
                    }
                    player.trophies -= trophy_cost;
                    player.energy -= cost;
                    is_bot = player.is_bot;
                }

                let id = self.alloc_entity_id();
                self.buildings.push(Building {
                    id,
                    owner_id: player_id,
                    kind: building_kind,
                    level: 1,
                    meta: 0,
                    rel_x: final_x,
                    rel_y: final_y,
                    hp: scaled_hp,
                    max_hp: scaled_hp,
                    angle: 0,
                    cooldown_left: 0.0,
                    invested_cost: cost,
                    invested_trophies: trophy_cost,
                    last_hit_by: None,
                    last_hit_src: SRC_UNKNOWN,
                });
                if let Some(tlog) = self.telemetry.as_mut() {
                    let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                    let ring = self
                        .zones
                        .get(&player_id)
                        .map(|z| Self::ring_for_radius(z.x.hypot(z.y)))
                        .unwrap_or(1);
                    tlog.log_action(
                        self.tick,
                        sec,
                        player_id,
                        is_bot,
                        "build",
                        ring,
                        json!({"bkind": building_kind, "cost": cost}),
                    );
                }
                self.recompute_unit_cap(player_id);
                None
            }
            ClientMessage::UpgradeBuilding { building_id } => {
                let Some(idx) = self
                    .buildings
                    .iter()
                    .position(|b| b.id == building_id && b.owner_id == player_id)
                else {
                    return Some(7);
                };

                let kind = self.buildings[idx].kind;
                let cur_level = self.buildings[idx].level as usize;
                if kind != 0 {
                    let hq_lvl = usize::from(self.hq_level_of(player_id));
                    if cur_level >= hq_lvl {
                        return Some(9);
                    }
                }

                let Some(cfg) = self.config.building_by_kind(kind).cloned() else {
                    return Some(6);
                };
                let Some(next_lvl) = cfg.levels.get(cur_level) else {
                    return Some(6);
                };

                let cost = next_lvl.cost;
                let trophy_cost = next_lvl.trophy_cost;
                let scaled_hp = next_lvl.hp;

                let is_bot;
                {
                    let Some(player) = self.players.get_mut(&player_id) else {
                        return Some(1);
                    };
                    if kind != 0 && cur_level + 1 >= 3 && player.is_in_combat() {
                        return Some(17);
                    }
                    if player.trophies < trophy_cost {
                        return Some(15);
                    }
                    if player.energy < cost {
                        return Some(2);
                    }
                    player.trophies -= trophy_cost;
                    player.energy -= cost;
                    is_bot = player.is_bot;
                }

                let b = &mut self.buildings[idx];
                let hp_gain = scaled_hp.saturating_sub(b.max_hp);
                b.level += 1;
                b.max_hp = scaled_hp;
                b.hp = b.hp.saturating_add(hp_gain).min(b.max_hp);
                b.invested_cost += cost;
                b.invested_trophies += trophy_cost;
                let new_level = b.level;

                if kind == 0 {
                    let base_r = self.config.world.zone_base_radius;
                    let new_zone_r = hq_zone_radius(base_r, new_level);
                    let target_r = self.equilibrium_radius(player_id);
                    let (old_x, old_y, cur_ang) = self
                        .zones
                        .get(&player_id)
                        .map(|z| (z.x, z.y, z.y.atan2(z.x)))
                        .unwrap_or((target_r, 0.0, 0.0));
                    let (dest_x, dest_y) =
                        self.find_safe_orbit_spot(player_id, target_r, new_zone_r, cur_ang);
                    if let Some(z) = self.zones.get_mut(&player_id) {
                        z.radius = new_zone_r;
                        z.x = dest_x;
                        z.y = dest_y;
                    }
                    let shift_x = dest_x - old_x;
                    let shift_y = dest_y - old_y;
                    if let Some(pl) = self.players.get_mut(&player_id) {
                        pl.spawn_protection_left = pl.spawn_protection_left.max(3.0);
                        if pl.army_mode == ArmyMode::Defend {
                            pl.rally_x = dest_x;
                            pl.rally_y = dest_y;
                        }
                    }
                    let wave_r = new_zone_r + 160.0;
                    for u in &mut self.units {
                        if u.owner_id == player_id && u.kind != 4 && u.squad == 0 {
                            u.x += shift_x;
                            u.y += shift_y;
                        } else if u.owner_id != player_id && u.hp > 0 {
                            let dx = u.x - dest_x;
                            let dy = u.y - dest_y;
                            let d = dx.hypot(dy);
                            if d < wave_r {
                                let (nx, ny) = if d > 0.001 { (dx / d, dy / d) } else { (1.0, 0.0) };
                                u.knockback_vx += nx * 380.0;
                                u.knockback_vy += ny * 380.0;
                            }
                        }
                    }
                }

                if let Some(tlog) = self.telemetry.as_mut() {
                    let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                    let ring = self
                        .zones
                        .get(&player_id)
                        .map(|z| Self::ring_for_radius(z.x.hypot(z.y)))
                        .unwrap_or(1);
                    tlog.log_action(
                        self.tick,
                        sec,
                        player_id,
                        is_bot,
                        "upgrade",
                        ring,
                        json!({"bkind": kind, "level": new_level, "cost": cost}),
                    );
                }

                self.recompute_unit_cap(player_id);
                None
            }
            ClientMessage::SellBuilding { building_id } => {
                let Some(idx) = self
                    .buildings
                    .iter()
                    .position(|b| b.id == building_id && b.owner_id == player_id)
                else {
                    return Some(7);
                };
                if self.buildings[idx].kind == 0 {
                    return Some(8);
                }
                let sold_id = self.buildings[idx].id;
                let sold_kind = self.buildings[idx].kind;
                let energy_refund =
                    self.buildings[idx].invested_cost * self.config.world.sell_refund_ratio;
                let trophy_refund = (self.buildings[idx].invested_trophies * 0.5).round();
                self.buildings.swap_remove(idx);
                if sold_kind == 1 {
                    self.units.retain(|u| !(u.kind == 4 && u.home_id == sold_id));
                }
                let hq_lvl = self.hq_level_of(player_id);
                self.grant_energy(player_id, energy_refund, 1.25, hq_lvl);
                self.grant_trophies(player_id, trophy_refund, hq_lvl);
                if sold_kind == 5 {
                    self.prune_orphaned_buildings(player_id);
                } else {
                    self.recompute_unit_cap(player_id);
                }
                None
            }
            ClientMessage::ArmyCommand {
                squad,
                mode,
                target_x,
                target_y,
            } => {
                if !target_x.is_finite() || !target_y.is_finite() {
                    return None;
                }
                if mode == 2 {
                    if let Some(player) = self.players.get_mut(&player_id) {
                        if squad == 0 || player.reinforce_squad == Some(squad) {
                            player.reinforce_squad = None;
                        } else if player.squads.contains_key(&squad) {
                            player.reinforce_squad = Some(squad);
                        }
                    }
                    return None;
                }
                if let Some(m) = ArmyMode::from_u8(mode) {
                    let (outside_zone, in_base_vision, zone_ring) = self
                        .zones
                        .get(&player_id)
                        .map(|z| {
                            let d = (target_x - z.x).hypot(target_y - z.y);
                            (
                                d > z.radius,
                                d <= z.radius + 360.0,
                                Self::ring_for_radius(z.x.hypot(z.y)),
                            )
                        })
                        .unwrap_or((false, false, 1));
                    let mut changed = false;
                    let mut is_bot = false;
                    let mut assigned_squad: Option<u8> = None;

                    if m == ArmyMode::Defend {
                        if let Some(player) = self.players.get_mut(&player_id) {
                            is_bot = player.is_bot;
                            if squad == 0 {
                                if player.base_rally.is_some() || player.army_mode != ArmyMode::Defend {
                                    changed = true;
                                }
                                player.base_rally = None;
                                if player.squads.is_empty() {
                                    player.army_mode = ArmyMode::Defend;
                                }
                            } else {
                                if player.squads.remove(&squad).is_some() {
                                    changed = true;
                                }
                            }
                        }
                        if squad > 0 {
                            for u in &mut self.units {
                                if u.owner_id == player_id && u.kind != 4 && u.squad == squad {
                                    u.squad = 0;
                                }
                            }
                        }
                    } else if squad == 0 {
                        let has_base_units = self
                            .units
                            .iter()
                            .any(|u| u.owner_id == player_id && u.kind != 4 && u.hp > 0 && u.squad == 0);
                        if has_base_units {
                            if in_base_vision {
                                if let Some(player) = self.players.get_mut(&player_id) {
                                    is_bot = player.is_bot;
                                    changed = true;
                                    player.army_mode = ArmyMode::Rally;
                                    player.rally_x = target_x;
                                    player.rally_y = target_y;
                                    player.base_rally = if outside_zone {
                                        Some(crate::types::SquadOrder {
                                            target_x,
                                            target_y,
                                        })
                                    } else {
                                        None
                                    };
                                }
                            } else {
                                if let Some(player) = self.players.get_mut(&player_id) {
                                    is_bot = player.is_bot;
                                    changed = player.army_mode != ArmyMode::Rally;
                                    player.army_mode = ArmyMode::Rally;
                                    player.rally_x = target_x;
                                    player.rally_y = target_y;
                                    player.base_rally = None;
                                    if outside_zone {
                                        player.spawn_protection_left = 0.0;
                                    }
                                    let new_id = if player.squads.len() < 5 {
                                        let mut cand = player.next_squad_id.max(1);
                                        for _ in 0..250 {
                                            if !player.squads.contains_key(&cand) {
                                                break;
                                            }
                                            cand = if cand >= 250 { 1 } else { cand + 1 };
                                        }
                                        player.next_squad_id = if cand >= 250 { 1 } else { cand + 1 };
                                        cand
                                    } else {
                                        *player.squads.keys().min().unwrap_or(&1)
                                    };
                                    player.squads.insert(
                                        new_id,
                                        crate::types::SquadOrder {
                                            target_x,
                                            target_y,
                                        },
                                    );
                                    assigned_squad = Some(new_id);
                                }
                                if let Some(sid) = assigned_squad {
                                    for u in &mut self.units {
                                        if u.owner_id == player_id && u.kind != 4 && u.hp > 0 && u.squad == 0 {
                                            u.squad = sid;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        let has_squad_units = self
                            .units
                            .iter()
                            .any(|u| u.owner_id == player_id && u.kind != 4 && u.hp > 0 && u.squad == squad);
                        if has_squad_units {
                            if let Some(player) = self.players.get_mut(&player_id) {
                                is_bot = player.is_bot;
                                changed = true;
                                player.army_mode = ArmyMode::Rally;
                                player.rally_x = target_x;
                                player.rally_y = target_y;
                                if outside_zone {
                                    player.spawn_protection_left = 0.0;
                                }
                                player.squads.insert(
                                    squad,
                                    crate::types::SquadOrder {
                                        target_x,
                                        target_y,
                                    },
                                );
                            }
                        }
                    }
                    if changed {
                        if let Some(tlog) = self.telemetry.as_mut() {
                            let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                            let cmd = if m == ArmyMode::Rally { "rally" } else { "defend" };
                            tlog.log_action(
                                self.tick,
                                sec,
                                player_id,
                                is_bot,
                                cmd,
                                zone_ring,
                                json!({"to_ring": Self::ring_for_radius(target_x.hypot(target_y))}),
                            );
                        }
                    }
                }
                None
            }
            ClientMessage::ConfigureBuilding { building_id, meta } => {
                let Some(b) = self
                    .buildings
                    .iter_mut()
                    .find(|b| b.id == building_id && b.owner_id == player_id)
                else {
                    return Some(7);
                };
                if b.kind == 3 {
                    let unit_type = (meta >> 2) & 0x07;
                    let allowed = match unit_type {
                        0 => true,         // Raider (Lv1)
                        1 => b.level >= 2, // Archer (Lv2)
                        2 => b.level >= 3, // Titan (Lv3)
                        _ => false,
                    };
                    if !allowed {
                        return Some(9);
                    }
                    b.meta = meta & 0x1F;
                }
                None
            }
            ClientMessage::TeleportZone { target_x, target_y } => {
                if !target_x.is_finite() || !target_y.is_finite() {
                    return Some(12);
                }
                let cd_left = self
                    .players
                    .get(&player_id)
                    .map(|p| p.teleport_cooldown_left)
                    .unwrap_or(0.0);
                if cd_left > 0.0 {
                    return Some(11);
                }
                let Some(zone) = self.zones.get(&player_id).cloned() else {
                    return Some(1);
                };
                let is_bot_caller = self
                    .players
                    .get(&player_id)
                    .map(|p| p.is_bot)
                    .unwrap_or(false);
                let target_r = target_x.hypot(target_y);
                let r_eq = self.equilibrium_radius(player_id);
                let min_r = if r_eq >= RING1_RADIUS {
                    RING1_RADIUS + zone.radius + 200.0
                } else {
                    self.config.world.core_radius + zone.radius
                };
                let max_r = self.config.world.map_radius - zone.radius;
                if target_r < min_r || target_r > max_r {
                    return Some(12);
                }
                if !is_bot_caller && !self.has_vision_at(player_id, target_x, target_y) {
                    return Some(18);
                }

                for other in self.zones.values() {
                    if other.player_id == player_id {
                        continue;
                    }
                    if (target_x - other.x).hypot(target_y - other.y)
                        < zone.radius + other.radius + 150.0
                    {
                        return Some(13);
                    }
                }
                for b in &self.buildings {
                    if b.owner_id != player_id && b.kind == 5 {
                        if let Some(other) = self.zones.get(&b.owner_id) {
                            let ext_r = beacon_ext_radius(b.level);
                            if (target_x - (other.x + b.rel_x)).hypot(target_y - (other.y + b.rel_y))
                                < zone.radius + ext_r + 120.0
                            {
                                return Some(13);
                            }
                        }
                    }
                }

                let dist = (target_x - zone.x).hypot(target_y - zone.y);
                let ring_pen = if target_r < r_eq {
                    self.config.world.teleport_ring_penalty * 3.0
                } else {
                    self.config.world.teleport_ring_penalty
                };
                let cost = (self.config.world.teleport_base_cost
                    + dist * self.config.world.teleport_dist_factor
                    + (target_r - r_eq).abs() * ring_pen)
                    .round();
                let cd_sec = self.config.world.teleport_cooldown_sec;

                let Some(player) = self.players.get_mut(&player_id) else {
                    return Some(1);
                };
                if player.energy < cost {
                    return Some(2);
                }
                let from_r = zone.x.hypot(zone.y);
                player.energy -= cost;
                player.teleport_cooldown_left = cd_sec;
                player.spawn_protection_left = 0.0;
                player.last_teleport_sec = player.survival_sec;
                let is_bot = player.is_bot;
                if let Some(z) = self.zones.get_mut(&player_id) {
                    z.x = target_x;
                    z.y = target_y;
                }
                if let Some(tlog) = self.telemetry.as_mut() {
                    let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                    let ring = Self::ring_for_radius(target_r);
                    tlog.log_action(
                        self.tick,
                        sec,
                        player_id,
                        is_bot,
                        "tp",
                        ring,
                        json!({"cost": cost}),
                    );
                    tlog.log_milestone(
                        self.tick,
                        sec,
                        player_id,
                        "teleport",
                        json!({"from_r": from_r.round(), "to_r": target_r.round(), "cost": cost}),
                    );
                }
                None
            }
            _ => None,
        }
    }

    /// Discrete HQ-locked orbital ring radius: Ring 1 (620), Ring 2 (1580), Ring 3 (2600).
    pub fn equilibrium_radius(&self, player_id: PlayerId) -> f32 {
        let hq_lvl = self.hq_level_of(player_id);
        match hq_lvl {
            0 | 1 => 620.0,
            2 => 1580.0,
            _ => 2600.0,
        }
    }

    pub fn step(&mut self, dt: f32) -> Vec<GameOverEvent> {
        self.tick = self.tick.wrapping_add(1);
        for player in self.players.values_mut() {
            player.survival_sec += dt;
            if player.spawn_protection_left > 0.0 {
                player.spawn_protection_left = (player.spawn_protection_left - dt).max(0.0);
            }
            if player.teleport_cooldown_left > 0.0 {
                player.teleport_cooldown_left = (player.teleport_cooldown_left - dt).max(0.0);
            }
        }

        self.step_bots(dt);
        self.step_core_push(dt);
        self.step_zone_repulsion(dt);
        self.step_ring_scoring(dt);
        self.step_ooc_regen(dt);
        self.step_income_and_spawning(dt);
        self.step_turrets(dt);
        self.step_projectiles(dt);
        self.step_units(dt);
        if self.tick % 10 == 0 {
            self.step_squad_clustering();
        }
        let game_overs = self.step_cleanup();
        self.record_telemetry_tick();
        game_overs
    }

    /// Dynamically splits lagging unit clusters (e.g., slow Titans) into separate squads and merges nearby ones.
    fn step_squad_clustering(&mut self) {
        let mut per_player: HashMap<PlayerId, HashMap<u8, Vec<usize>>> = HashMap::new();
        for (i, u) in self.units.iter().enumerate() {
            if u.owner_id != 0 && u.hp > 0 && u.kind != 4 && u.squad > 0 {
                per_player
                    .entry(u.owner_id)
                    .or_default()
                    .entry(u.squad)
                    .or_default()
                    .push(i);
            }
        }

        let player_ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for pid in player_ids {
            let Some(mut squad_map) = per_player.remove(&pid) else {
                if let Some(pl) = self.players.get_mut(&pid) {
                    pl.squads.clear();
                }
                continue;
            };
            let Some(mut orders) = self.players.get(&pid).map(|p| p.squads.clone()) else {
                continue;
            };
            let mut next_id = self.players.get(&pid).map(|p| p.next_squad_id).unwrap_or(1);
            let mut reinforce_sid = self.players.get(&pid).and_then(|p| p.reinforce_squad);

            orders.retain(|sid, _| squad_map.contains_key(sid));
            if let Some(rsid) = reinforce_sid {
                if !orders.contains_key(&rsid) {
                    reinforce_sid = None;
                }
            }

            // 1. Split spatially disconnected clusters within each squad (>250 px gap)
            // Skip splitting the active reinforce_squad so newly spawned reinforcements streaming from base stay in it.
            let sids: Vec<u8> = squad_map.keys().copied().collect();
            for sid in sids {
                if reinforce_sid == Some(sid) {
                    continue;
                }
                let Some(order) = orders.get(&sid).copied() else {
                    continue;
                };
                let indices = match squad_map.get(&sid) {
                    Some(v) if v.len() >= 2 => v.clone(),
                    _ => continue,
                };
                let n = indices.len();
                let mut visited = vec![false; n];
                let mut components: Vec<Vec<usize>> = Vec::new();
                for start in 0..n {
                    if visited[start] {
                        continue;
                    }
                    visited[start] = true;
                    let mut comp = vec![indices[start]];
                    let mut queue = vec![start];
                    let mut qi = 0;
                    while qi < queue.len() {
                        let cur = queue[qi];
                        qi += 1;
                        let ui = indices[cur];
                        let (ux, uy) = (self.units[ui].x, self.units[ui].y);
                        for nxt in 0..n {
                            if visited[nxt] {
                                continue;
                            }
                            let vi = indices[nxt];
                            if (self.units[vi].x - ux).hypot(self.units[vi].y - uy) <= 250.0 {
                                visited[nxt] = true;
                                queue.push(nxt);
                                comp.push(vi);
                            }
                        }
                    }
                    components.push(comp);
                }

                if components.len() <= 1 {
                    continue;
                }

                components.sort_by(|a, b| {
                    let da = a
                        .iter()
                        .map(|&idx| (self.units[idx].x - order.target_x).hypot(self.units[idx].y - order.target_y))
                        .fold(f32::MAX, f32::min);
                    let db = b
                        .iter()
                        .map(|&idx| (self.units[idx].x - order.target_x).hypot(self.units[idx].y - order.target_y))
                        .fold(f32::MAX, f32::min);
                    da.total_cmp(&db)
                });

                for comp in components.into_iter().skip(1) {
                    if orders.len() >= 5 {
                        break;
                    }
                    let mut cand = next_id.max(1);
                    for _ in 0..250 {
                        if !orders.contains_key(&cand) {
                            break;
                        }
                        cand = if cand >= 250 { 1 } else { cand + 1 };
                    }
                    next_id = if cand >= 250 { 1 } else { cand + 1 };
                    orders.insert(cand, order);
                    for &ui in &comp {
                        self.units[ui].squad = cand;
                    }
                    squad_map.entry(cand).or_default().extend(comp);
                }
            }

            // 2. Auto-merge two active squads if they converge closely (<165 px) toward similar targets
            let mut active_sids: Vec<u8> = orders.keys().copied().collect();
            active_sids.sort_unstable();
            let mut centroids: HashMap<u8, (f32, f32, usize)> = HashMap::new();
            for &sid in &active_sids {
                let mut sx = 0.0_f32;
                let mut sy = 0.0_f32;
                let mut cnt = 0_usize;
                for u in &self.units {
                    if u.owner_id == pid && u.hp > 0 && u.kind != 4 && u.squad == sid {
                        sx += u.x;
                        sy += u.y;
                        cnt += 1;
                    }
                }
                if cnt > 0 {
                    centroids.insert(sid, (sx / cnt as f32, sy / cnt as f32, cnt));
                }
            }
            for i in 0..active_sids.len() {
                let a = active_sids[i];
                let Some(&(ax, ay, acnt)) = centroids.get(&a) else {
                    continue;
                };
                let Some(&oa) = orders.get(&a) else {
                    continue;
                };
                for j in (i + 1)..active_sids.len() {
                    let b = active_sids[j];
                    let Some(&(bx, by, bcnt)) = centroids.get(&b) else {
                        continue;
                    };
                    let Some(&ob) = orders.get(&b) else {
                        continue;
                    };
                    if (ax - bx).hypot(ay - by) <= 165.0
                        && (oa.target_x - ob.target_x).hypot(oa.target_y - ob.target_y) <= 260.0
                    {
                        let (keep, drop) = if acnt >= bcnt { (a, b) } else { (b, a) };
                        for u in &mut self.units {
                            if u.owner_id == pid && u.squad == drop {
                                u.squad = keep;
                            }
                        }
                        orders.remove(&drop);
                        centroids.remove(&drop);
                        if reinforce_sid == Some(drop) {
                            reinforce_sid = Some(keep);
                        }
                        break;
                    }
                }
            }

            // 3. Re-attach any combat squad whose order and all units are back inside base vision
            if let Some(z) = self.zones.get(&pid) {
                let vision_r = z.radius + 360.0;
                let mut return_sids: Vec<u8> = Vec::new();
                for (&sid, &ord) in &orders {
                    if (ord.target_x - z.x).hypot(ord.target_y - z.y) <= vision_r {
                        let all_inside = self.units.iter().all(|u| {
                            u.owner_id != pid
                                || u.hp == 0
                                || u.kind == 4
                                || u.squad != sid
                                || (u.x - z.x).hypot(u.y - z.y) <= vision_r
                        });
                        if all_inside {
                            return_sids.push(sid);
                        }
                    }
                }
                for sid in return_sids {
                    if let Some(ord) = orders.remove(&sid) {
                        for u in &mut self.units {
                            if u.owner_id == pid && u.hp > 0 && u.kind != 4 && u.squad == sid {
                                u.squad = 0;
                            }
                        }
                        if let Some(pl) = self.players.get_mut(&pid) {
                            pl.base_rally = if (ord.target_x - z.x).hypot(ord.target_y - z.y) > z.radius {
                                Some(ord)
                            } else {
                                None
                            };
                        }
                    }
                }
            }

            if let Some(rsid) = reinforce_sid {
                if !orders.contains_key(&rsid) {
                    reinforce_sid = None;
                }
            }

            if let Some(pl) = self.players.get_mut(&pid) {
                pl.squads = orders;
                pl.next_squad_id = next_id;
                pl.reinforce_squad = reinforce_sid;
            }
        }
    }

    fn record_telemetry_tick(&mut self) {
        if self.telemetry.is_none() {
            return;
        }
        let tick_rate = f32::from(self.config.world.tick_rate.max(1));
        let sec = self.tick as f32 / tick_rate;

        let mut ring_updates: Vec<(PlayerId, u8, f32)> = Vec::new();
        for (&pid, z) in &self.zones {
            let r = z.x.hypot(z.y);
            let ring = Self::ring_for_radius(r);
            ring_updates.push((pid, ring, self.equilibrium_radius(pid)));
        }

        let Some(mut tlog) = self.telemetry.take() else {
            return;
        };
        for (pid, ring, r_eq) in ring_updates {
            let prev = tlog.last_rings.insert(pid, ring);
            if let Some(from_ring) = prev {
                if from_ring != ring {
                    tlog.log_milestone(
                        self.tick,
                        sec,
                        pid,
                        "ring_change",
                        json!({"from_ring": from_ring, "to_ring": ring, "r_eq": r_eq.round()}),
                    );
                }
            }
        }

        if self.tick % tlog.snapshot_interval_ticks == 0 {
            tlog.flush_combat_window(self.tick, sec);

            let mut b_counts: HashMap<PlayerId, ([u16; 5], u8, f32)> = HashMap::new();
            for b in &self.buildings {
                let entry = b_counts.entry(b.owner_id).or_insert(([0; 5], 1, 0.0));
                entry.2 += b.invested_cost;
                match b.kind {
                    0 => {
                        entry.0[0] += 1;
                        entry.1 = b.level;
                    }
                    1 => entry.0[1] += 1,
                    2 => entry.0[2] += 1,
                    3 => entry.0[3] += 1,
                    5 => entry.0[4] += 1,
                    _ => {}
                }
            }
            let mut u_counts: HashMap<PlayerId, [u16; 5]> = HashMap::new();
            for u in &self.units {
                if u.owner_id == 0 || u.hp == 0 {
                    continue;
                }
                let entry = u_counts.entry(u.owner_id).or_insert([0; 5]);
                match u.kind {
                    0 => entry[0] += 1,
                    1 => entry[1] += 1,
                    2 => entry[2] += 1,
                    4 => entry[4] += 1,
                    _ => {}
                }
            }

            let mut pids: Vec<PlayerId> = self.players.keys().copied().collect();
            pids.sort_unstable();
            let players_json: Vec<serde_json::Value> = pids
                .into_iter()
                .filter_map(|pid| {
                    let p = self.players.get(&pid)?;
                    let z = self.zones.get(&pid)?;
                    let r = z.x.hypot(z.y);
                    let r_eq = self.equilibrium_radius(pid);
                    let (bc, hq_lvl, invested) = b_counts.get(&pid).copied().unwrap_or(([0; 5], 1, 0.0));
                    let uc = u_counts.get(&pid).copied().unwrap_or([0; 5]);
                    Some(json!({
                        "id": pid,
                        "name": p.name,
                        "is_bot": p.is_bot,
                        "tier": p.bot_tier,
                        "arch": p.bot_archetype,
                        "spec": 0,
                        "hq_lvl": hq_lvl,
                        "level": hq_lvl,
                        "score": p.score,
                        "kills": p.kills,
                        "bd": p.buildings_destroyed,
                        "energy": p.energy.round(),
                        "trophies": p.trophies.round(),
                        "r": r.round(),
                        "r_eq": r_eq.round(),
                        "ring": Self::ring_for_radius(r),
                        "zone_r": z.radius.round(),
                        "b": bc,
                        "u": uc,
                        "cap": p.unit_cap,
                        "inv": invested.round(),
                    }))
                })
                .collect();
            tlog.log_snapshot(self.tick, sec, players_json);
        }
        self.telemetry = Some(tlog);
    }

    fn step_core_push(&mut self, dt: f32) {
        let core_r = self.config.world.core_radius;
        let base_push = self.config.world.core_push_strength;
        let hq_levels = self.hq_level_map();
        let player_ids: Vec<PlayerId> = self.zones.keys().copied().collect();
        for pid in player_ids {
            let hq_lvl = hq_levels.get(&pid).copied().unwrap_or(1);
            if let Some(z) = self.zones.get_mut(&pid) {
                let d = z.x.hypot(z.y);
                let core_min = core_r + z.radius;
                let ring_min = match hq_lvl {
                    0 | 1 => core_min,
                    2 => RING1_RADIUS + z.radius + 140.0,
                    _ => RING2_RADIUS + z.radius + 80.0,
                };
                if d < core_min {
                    let (nx, ny) = if d > 0.001 { (z.x / d, z.y / d) } else { (1.0, 0.0) };
                    z.x = nx * core_min;
                    z.y = ny * core_min;
                } else if d < ring_min && d > 0.001 {
                    let deficit = (ring_min - d).max(0.0);
                    let step = deficit.min(base_push * 1.8 * dt);
                    z.x += (z.x / d) * step;
                    z.y += (z.y / d) * step;
                }
            }
        }
    }

    fn step_ring_scoring(&mut self, _dt: f32) {
        let tick_rate = u32::from(self.config.world.tick_rate.max(1));
        if self.tick % tick_rate != 0 {
            return;
        }
        let zone_rings: Vec<(PlayerId, u8)> = self
            .zones
            .values()
            .map(|z| (z.player_id, Self::ring_for_radius(z.x.hypot(z.y))))
            .collect();
        for (pid, ring) in zone_rings {
            let pts = match ring {
                2 => 1,
                3 => 4,
                _ => 0,
            };
            if pts > 0 {
                if let Some(p) = self.players.get_mut(&pid) {
                    p.score += pts;
                }
            }
        }
    }

    fn step_ooc_regen(&mut self, dt: f32) {
        let mut regen_rates: HashMap<PlayerId, f32> = HashMap::new();
        for p in self.players.values_mut() {
            if (p.survival_sec - p.last_damaged_sec) >= 6.0 {
                p.regen_accum += dt;
                if p.regen_accum >= 0.5 {
                    p.regen_accum -= 0.5;
                    regen_rates.insert(p.id, 0.015);
                }
            } else {
                p.regen_accum = 0.0;
            }
        }
        if regen_rates.is_empty() {
            return;
        }
        for b in &mut self.buildings {
            if b.hp > 0 && b.hp < b.max_hp {
                if let Some(&rate) = regen_rates.get(&b.owner_id) {
                    let eff_rate = if b.kind == 0 { rate * 0.35 } else { rate };
                    let heal = (f32::from(b.max_hp) * eff_rate * 0.5).round().max(1.0) as u16;
                    b.hp = b.hp.saturating_add(heal).min(b.max_hp);
                }
            }
        }
    }

    fn building_build_cost(&self, kind: u8) -> f32 {
        self.config
            .building_by_kind(kind)
            .and_then(|c| c.levels.first())
            .map(|l| l.cost)
            .unwrap_or(9999.0)
    }

    fn building_upgrade_cost(&self, kind: u8, cur_level: u8) -> Option<(f32, f32)> {
        let cfg = self.config.building_by_kind(kind)?;
        let next = cfg.levels.get(usize::from(cur_level))?;
        Some((next.cost, next.trophy_cost))
    }

    /// Aggregates per-player base composition, turret coverage, army strength, and active threats.
    fn gather_bot_intel(&self) -> HashMap<PlayerId, BotPlayerIntel> {
        let mut map: HashMap<PlayerId, BotPlayerIntel> = self
            .zones
            .keys()
            .map(|&pid| (pid, BotPlayerIntel::default()))
            .collect();

        for b in &self.buildings {
            let entry = map.entry(b.owner_id).or_default();
            entry.total_buildings += 1;
            match b.kind {
                0 => {
                    entry.hq_id = b.id;
                    entry.hq_lvl = b.level;
                    entry.hq_hp = b.hp;
                    entry.hq_max_hp = b.max_hp.max(1);
                }
                1 => {
                    entry.gens += 1;
                    entry.gen_positions.push((b.rel_x, b.rel_y));
                }
                2 => {
                    entry.turrets += 1;
                    entry.turret_positions.push((b.rel_x, b.rel_y));
                    entry.turret_power += match b.level {
                        1 => 4.2,
                        2 => 7.2,
                        _ => 11.5,
                    };
                }
                3 => {
                    entry.barracks += 1;
                    entry.barracks_info.push((b.id, b.level, b.meta));
                }
                5 => {
                    entry.beacons += 1;
                    entry.beacon_positions.push((b.rel_x, b.rel_y));
                }
                _ => {}
            }
            if b.kind != 0 {
                entry.non_hq_buildings.push((b.id, b.kind, b.level));
            }
        }

        let bot_zones: Vec<(PlayerId, f32, f32, f32)> = self
            .players
            .values()
            .filter(|p| p.is_bot)
            .filter_map(|p| self.zones.get(&p.id).map(|z| (p.id, z.x, z.y, z.radius + 230.0)))
            .collect();

        for u in &self.units {
            if u.owner_id == 0 || u.hp == 0 {
                continue;
            }
            let power = match u.kind {
                2 => 4.2_f32,
                1 => 1.35,
                _ => 1.0,
            };
            if let Some(entry) = map.get_mut(&u.owner_id) {
                entry.own_units += 1;
                entry.own_combat_power += power;
                if let Some(oz) = self.zones.get(&u.owner_id) {
                    if (u.x - oz.x).hypot(u.y - oz.y) <= oz.radius + 260.0 {
                        entry.defending_unit_power += power;
                    }
                }
            }
            for &(bpid, zx, zy, perim_r) in &bot_zones {
                if u.owner_id != bpid && (u.x - zx).hypot(u.y - zy) <= perim_r {
                    if let Some(entry) = map.get_mut(&bpid) {
                        entry.enemy_threat_power += power;
                    }
                }
            }
        }

        map
    }

    /// Refreshes local building counts/positions for one bot after mutating commands mid-think.
    fn refresh_bot_building_intel(&self, pid: PlayerId, intel: &mut BotPlayerIntel) {
        intel.hq_id = 0;
        intel.hq_lvl = 1;
        intel.hq_hp = 0;
        intel.hq_max_hp = 1;
        intel.gens = 0;
        intel.turrets = 0;
        intel.barracks = 0;
        intel.beacons = 0;
        intel.total_buildings = 0;
        intel.turret_power = 0.0;
        intel.gen_positions.clear();
        intel.turret_positions.clear();
        intel.beacon_positions.clear();
        intel.barracks_info.clear();
        intel.non_hq_buildings.clear();

        for b in &self.buildings {
            if b.owner_id != pid {
                continue;
            }
            intel.total_buildings += 1;
            match b.kind {
                0 => {
                    intel.hq_id = b.id;
                    intel.hq_lvl = b.level;
                    intel.hq_hp = b.hp;
                    intel.hq_max_hp = b.max_hp.max(1);
                }
                1 => {
                    intel.gens += 1;
                    intel.gen_positions.push((b.rel_x, b.rel_y));
                }
                2 => {
                    intel.turrets += 1;
                    intel.turret_positions.push((b.rel_x, b.rel_y));
                    intel.turret_power += match b.level {
                        1 => 4.2,
                        2 => 7.2,
                        _ => 11.5,
                    };
                }
                3 => {
                    intel.barracks += 1;
                    intel.barracks_info.push((b.id, b.level, b.meta));
                }
                5 => {
                    intel.beacons += 1;
                    intel.beacon_positions.push((b.rel_x, b.rel_y));
                }
                _ => {}
            }
            if b.kind != 0 {
                intel.non_hq_buildings.push((b.id, b.kind, b.level));
            }
        }
    }

    /// Teleports a bot to a safe clear position in `target_ring` if affordable and off cooldown.
    fn bot_try_teleport_to_ring(&mut self, pid: PlayerId, target_ring: u8) -> bool {
        let Some(p) = self.players.get(&pid) else {
            return false;
        };
        if p.teleport_cooldown_left > 0.0 {
            return false;
        }
        let energy = p.energy;
        let Some(zone) = self.zones.get(&pid).cloned() else {
            return false;
        };
        let map_r = self.config.world.map_radius;
        let (min_r, max_r) = match target_ring {
            3 => (RING2_RADIUS + zone.radius + 110.0, (map_r - zone.radius - 40.0).max(RING2_RADIUS + 200.0)),
            _ => (RING1_RADIUS + zone.radius + 200.0, (RING2_RADIUS - zone.radius - 120.0).max(RING1_RADIUS + 300.0)),
        };
        let r_eq = self.equilibrium_radius(pid);
        let ring_mid = ((min_r + max_r) * 0.5).clamp(min_r, max_r);
        let target_r = r_eq.clamp(min_r, max_r).max(ring_mid * 0.92);

        for i in 0..16 {
            let angle = (i as f32) * (TAU / 16.0) + self.rng.gen_range(-0.12..0.12);
            let tx = angle.cos() * target_r;
            let ty = angle.sin() * target_r;

            let clear_zones = self.zones.values().all(|other| {
                other.player_id == pid
                    || (tx - other.x).hypot(ty - other.y) >= zone.radius + other.radius + 160.0
            });
            if !clear_zones {
                continue;
            }
            let clear_beacons = self.buildings.iter().all(|b| {
                if b.owner_id == pid || b.kind != 5 {
                    return true;
                }
                let Some(other) = self.zones.get(&b.owner_id) else {
                    return true;
                };
                let ext_r = beacon_ext_radius(b.level);
                (tx - (other.x + b.rel_x)).hypot(ty - (other.y + b.rel_y)) >= zone.radius + ext_r + 125.0
            });
            if !clear_beacons {
                continue;
            }

            let dist = (tx - zone.x).hypot(ty - zone.y);
            let ring_pen = if target_r < r_eq {
                self.config.world.teleport_ring_penalty * 3.0
            } else {
                self.config.world.teleport_ring_penalty
            };
            let est_cost = self.config.world.teleport_base_cost
                + dist * self.config.world.teleport_dist_factor
                + (target_r - r_eq).abs() * ring_pen;
            if energy < est_cost + 20.0 {
                continue;
            }

            if self
                .apply_command(
                    pid,
                    ClientMessage::TeleportZone {
                        target_x: tx,
                        target_y: ty,
                    },
                )
                .is_none()
            {
                return true;
            }
        }
        false
    }

    /// Configures barrack unit types by archetype for a balanced Raider/Archer/Titan mix.
    fn bot_configure_barracks(&mut self, pid: PlayerId, arch: u8, barracks_info: &[(EntityId, u8, u8)]) {
        for (idx, &(bid, blvl, bmeta)) in barracks_info.iter().enumerate() {
            let slot = idx % 4;
            let desired_utype = match arch {
                1 => match blvl {
                    // Dreadnought: Titan frontline + Archer backline + Raider finishers
                    3.. => [2_u8, 1, 2, 0][slot],
                    2 => [1_u8, 1, 0, 1][slot],
                    _ => 0,
                },
                2 => match blvl {
                    // Hunter: fast Raider pressure backed by Archers & occasional Titan
                    3.. => [0_u8, 1, 0, 2][slot],
                    2 => [0_u8, 1, 0, 0][slot],
                    _ => 0,
                },
                _ => match blvl {
                    // Balanced / Architect: heavy Archer + Titan core + fast Raiders
                    3.. => [1_u8, 2, 0, 1][slot],
                    2 => [1_u8, 0, 1, 1][slot],
                    _ => 0,
                },
            };
            if ((bmeta >> 2) & 0x07) != desired_utype {
                let _ = self.apply_command(
                    pid,
                    ClientMessage::ConfigureBuilding {
                        building_id: bid,
                        meta: desired_utype << 2,
                    },
                );
            }
        }
    }

    /// Picks a tactically optimal free hex or rhombus coordinate for `build_kind`.
    fn bot_pick_build_slot(
        &mut self,
        pid: PlayerId,
        build_kind: u8,
        gen_aura_r: f32,
        intel: &BotPlayerIntel,
    ) -> (Option<(f32, f32)>, usize, usize) {
        let open_hexes = self.collect_player_open_large_hexes(pid);
        let rhombuses = collect_rhombuses(&open_hexes);
        let occupied = self.collect_player_occupied_tris(pid, &rhombuses);

        let free_hex_list: Vec<(i32, i32)> = open_hexes
            .iter()
            .copied()
            .filter(|&(q, r)| !(0..6_u8).any(|s| occupied.contains(&(q, r, s))))
            .collect();
        let free_rhombus_list: Vec<([TriCoord; 2], (f32, f32))> = rhombuses
            .iter()
            .copied()
            .filter(|&(tris, _)| !occupied.contains(&tris[0]) && !occupied.contains(&tris[1]))
            .collect();

        let free_hex_count = free_hex_list.len();
        let free_rhom_count = free_rhombus_list.len();
        let aura_r = (gen_aura_r - 4.0).max(40.0);

        let picked = if is_large_building(build_kind) {
            let mut best: Option<((f32, f32), f32)> = None;
            for (q, r) in free_hex_list {
                let (cx, cy) = hex_to_rel(q, r);
                let dist_hq = cx.hypot(cy);
                let jitter = self.rng.gen_range(0.0..6.0);
                let score = if build_kind == 2 {
                    let aura_bonus = if intel
                        .gen_positions
                        .iter()
                        .any(|&(gx, gy)| (cx - gx).hypot(cy - gy) <= aura_r)
                    {
                        120.0
                    } else {
                        0.0
                    };
                    let min_t_dist = intel
                        .turret_positions
                        .iter()
                        .map(|&(tx, ty)| (cx - tx).hypot(cy - ty))
                        .fold(140.0_f32, f32::min);
                    let ring_pref = -((dist_hq - 86.0).abs() * 0.45);
                    aura_bonus + ring_pref + min_t_dist.min(140.0) * 0.3 + jitter
                } else {
                    // Barracks: slightly prefer mid/outer hexes so core inner ring stays open for Turrets
                    let outer_pref = if dist_hq >= 55.0 { 24.0 } else { 0.0 };
                    outer_pref + jitter
                };
                if best.as_ref().is_none_or(|&(_, bs)| score > bs) {
                    best = Some(((cx, cy), score));
                }
            }
            best.map(|(pos, _)| pos)
        } else {
            let mut best: Option<((f32, f32), f32)> = None;
            for (tris, (mx, my)) in free_rhombus_list {
                let dist_hq = mx.hypot(my);
                let jitter = self.rng.gen_range(0.0..5.0);
                let score = if build_kind == 5 {
                    let min_b_dist = intel
                        .beacon_positions
                        .iter()
                        .map(|&(bx, by)| (mx - bx).hypot(my - by))
                        .fold(f32::MAX, f32::min);
                    let spacing_bonus = if min_b_dist >= 82.0 { 350.0 } else { min_b_dist * 2.0 };
                    dist_hq * 1.4 + spacing_bonus + jitter
                } else {
                    // Generator: buff uncovered Turrets & pack onto already-used hexes to save full hexes
                    let turret_buff = intel
                        .turret_positions
                        .iter()
                        .filter(|&&(tx, ty)| {
                            (mx - tx).hypot(my - ty) <= aura_r
                                && !intel
                                    .gen_positions
                                    .iter()
                                    .any(|&(gx, gy)| (gx - tx).hypot(gy - ty) <= aura_r)
                        })
                        .count() as f32
                        * 115.0;
                    let mut shared_hex_bonus = 0.0_f32;
                    for (hq_q, hq_r, _) in tris {
                        if (0..6_u8).any(|s| occupied.contains(&(hq_q, hq_r, s))) {
                            shared_hex_bonus += 65.0;
                        }
                    }
                    turret_buff + shared_hex_bonus - dist_hq * 0.15 + jitter
                };
                if best.as_ref().is_none_or(|&(_, bs)| score > bs) {
                    best = Some(((mx, my), score));
                }
            }
            best.map(|(pos, _)| pos)
        };

        (picked, free_hex_count, free_rhom_count)
    }

    /// Executes up to `max_actions` prioritized upgrades and builds for a single bot think tick.
    fn bot_step_economy(
        &mut self,
        pid: PlayerId,
        tier: u8,
        arch: u8,
        gen_aura_r: f32,
        intel: &mut BotPlayerIntel,
    ) {
        let max_actions = match tier {
            1 => 2,
            2 => 3,
            _ => 5,
        };
        let max_b_lvl = tier.clamp(1, 3);

        for _ in 0..max_actions {
            let (energy, trophies, in_combat) = self
                .players
                .get(&pid)
                .map(|p| (p.energy, p.trophies, p.is_in_combat()))
                .unwrap_or((0.0, 0.0, false));

            // 1. Always upgrade HQ first once affordable (unlocks slots, levels, cap, beacons)
            if intel.hq_id != 0 && intel.hq_lvl < max_b_lvl && intel.total_buildings >= 2 {
                if let Some((e_cost, t_cost)) = self.building_upgrade_cost(0, intel.hq_lvl) {
                    if energy >= e_cost && trophies >= t_cost {
                        if self
                            .apply_command(
                                pid,
                                ClientMessage::UpgradeBuilding {
                                    building_id: intel.hq_id,
                                },
                            )
                            .is_none()
                        {
                            self.refresh_bot_building_intel(pid, intel);
                            continue;
                        }
                    }
                }
            }

            let threatened = intel.enemy_threat_power >= 1.5;
            let (_, free_hexes, free_rhombuses) = self.bot_pick_build_slot(pid, 3, gen_aura_r, intel);

            // 2. Hard opener essentials before branching out
            let opener_kind = if intel.gens < 2 && free_rhombuses > 0 {
                Some(1_u8)
            } else if intel.turrets < 1 && free_hexes > 0 {
                Some(2_u8)
            } else if intel.barracks < 1 && free_hexes > 0 {
                Some(3_u8)
            } else {
                None
            };

            if let Some(bk) = opener_kind {
                let cost = self.building_build_cost(bk);
                if energy >= cost {
                    if let (Some((bx, by)), _, _) = self.bot_pick_build_slot(pid, bk, gen_aura_r, intel) {
                        if self
                            .apply_command(
                                pid,
                                ClientMessage::Build {
                                    building_kind: bk,
                                    rel_x: bx,
                                    rel_y: by,
                                },
                            )
                            .is_none()
                        {
                            self.refresh_bot_building_intel(pid, intel);
                            continue;
                        }
                    }
                }
                break;
            }

            // 3. Evaluate best affordable upgrade candidate
            let mut best_upgrade: Option<(EntityId, f32)> = None;
            for &(bid, kind, lvl) in &intel.non_hq_buildings {
                if lvl >= max_b_lvl || lvl >= intel.hq_lvl {
                    continue;
                }
                if in_combat && lvl + 1 >= 3 {
                    continue;
                }
                let Some((u_cost, t_cost)) = self.building_upgrade_cost(kind, lvl) else {
                    continue;
                };
                if energy < u_cost || trophies < t_cost {
                    continue;
                }
                let score = match kind {
                    2 => {
                        if threatened {
                            135.0
                        } else if lvl == 1 {
                            105.0
                        } else {
                            82.0
                        }
                    }
                    3 => {
                        if lvl == 1 {
                            115.0
                        } else {
                            96.0
                        }
                    }
                    1 => {
                        if !threatened && energy < 700.0 {
                            112.0
                        } else {
                            76.0
                        }
                    }
                    5 => {
                        if free_hexes <= 2 || energy >= 600.0 {
                            70.0
                        } else {
                            25.0
                        }
                    }
                    _ => 50.0,
                };
                if best_upgrade.as_ref().is_none_or(|&(_, bs)| score > bs) {
                    best_upgrade = Some((bid, score));
                }
            }

            // 4. Economy-aware expansion (keep Generators ahead of Barracks so paid drone spawns stay funded)
            let min_turrets = match intel.hq_lvl {
                1 => if threatened || arch == 1 { 2 } else { 1 },
                2 => if threatened || arch == 1 { 3 } else { 2 },
                _ => if threatened || arch == 1 || arch == 4 { 4 } else { 3 },
            };
            let desired_beacons = if intel.hq_lvl >= 3 && !in_combat {
                if arch == 4 { 4 } else { 3 }
            } else {
                0
            };
            let max_bot_barracks = match intel.hq_lvl {
                1 => 4_usize,
                2 => 7_usize,
                _ => 12_usize,
            };
            let target_gens = 2 + usize::from(intel.hq_lvl) * 2 + usize::from(arch == 0 || arch == 4);
            let turret_ratio = if arch == 1 { 2 } else { 3 };
            let need_eco_for_barracks = intel.gens * 5 < intel.barracks * 4 && free_rhombuses > 0;

            let build_choice: Option<(u8, f32)> = if intel.turrets < min_turrets && free_hexes > 0 {
                Some((2, if threatened { 145.0 } else { 122.0 }))
            } else if need_eco_for_barracks && intel.gens < target_gens + 2 {
                Some((1, 118.0))
            } else if intel.hq_lvl >= 3
                && !in_combat
                && intel.beacons < desired_beacons
                && (intel.barracks >= 2 || free_hexes <= 3)
                && free_rhombuses > 0
                && trophies >= (90.0 * 1.65_f32.powi(intel.beacons as i32)).round()
            {
                Some((5, if free_hexes <= 2 { 114.0 } else { 96.0 }))
            } else if intel.gens < target_gens && free_rhombuses > 0 && (free_hexes >= 2 || intel.barracks >= 2) {
                Some((1, if intel.gens < usize::from(intel.hq_lvl) * 2 { 104.0 } else { 78.0 }))
            } else if free_hexes > 0 && intel.barracks < max_bot_barracks {
                let want_turret = intel.turrets * turret_ratio < intel.barracks;
                Some((if want_turret { 2 } else { 3 }, 94.0))
            } else if free_rhombuses > 0 && intel.gens < target_gens + 3 {
                Some((1, 68.0))
            } else {
                None
            };

            let affordable_build = build_choice.and_then(|(bk, sc)| {
                let c = self.building_build_cost(bk);
                let tc = if bk == 5 {
                    (90.0 * 1.65_f32.powi(intel.beacons as i32)).round()
                } else {
                    0.0
                };
                if energy >= c && trophies >= tc {
                    Some((bk, sc))
                } else {
                    None
                }
            });

            let do_upgrade = match (best_upgrade, affordable_build) {
                (Some((bid, us)), Some((_, bs))) => {
                    if us >= bs {
                        Some(bid)
                    } else {
                        None
                    }
                }
                (Some((bid, _)), None) => Some(bid),
                _ => None,
            };

            if let Some(bid) = do_upgrade {
                if self
                    .apply_command(pid, ClientMessage::UpgradeBuilding { building_id: bid })
                    .is_none()
                {
                    self.refresh_bot_building_intel(pid, intel);
                    continue;
                }
            } else if let Some((bk, _)) = affordable_build {
                if let (Some((bx, by)), _, _) = self.bot_pick_build_slot(pid, bk, gen_aura_r, intel) {
                    if self
                        .apply_command(
                            pid,
                            ClientMessage::Build {
                                building_kind: bk,
                                rel_x: bx,
                                rel_y: by,
                            },
                        )
                        .is_none()
                    {
                        self.refresh_bot_building_intel(pid, intel);
                        continue;
                    }
                }
            }
            break;
        }
    }

    fn step_bots(&mut self, dt: f32) {
        // 3 active starters in Ring 1, 3 active in Ring 2, 2 strong in Ring 3
        let desired_bots = [
            (1u8, 2u8, "Малек-1"),
            (1u8, 3u8, "Малек-2"),
            (1u8, 0u8, "Старатель-1"),
            (2u8, 2u8, "Рейдер-1"),
            (2u8, 3u8, "Рейдер-2"),
            (2u8, 1u8, "Дредноут-1"),
            (3u8, 4u8, "Архитектор-1"),
            (3u8, 2u8, "Охотник-1"),
        ];
        for (tier, arch, name) in desired_bots {
            if !self.players.values().any(|p| p.is_bot && p.name == name) {
                self.add_player_internal(name.to_string(), None, 0, tier, arch);
            }
        }

        let bot_ids: Vec<(PlayerId, u8, u8)> = self
            .players
            .values()
            .filter(|p| p.is_bot)
            .map(|p| (p.id, p.bot_tier, p.bot_archetype))
            .collect();

        let mut ready_bots: Vec<(PlayerId, u8, u8)> = Vec::new();
        for (pid, tier, arch) in bot_ids {
            let think_cd = match tier {
                1 => 1.2,
                3 => 0.45,
                _ => 0.75,
            };
            if let Some(p) = self.players.get_mut(&pid) {
                p.bot_think_cooldown -= dt;
                if p.bot_think_cooldown <= 0.0 {
                    p.bot_think_cooldown = think_cd;
                    ready_bots.push((pid, tier, arch));
                }
            }
        }
        if ready_bots.is_empty() {
            return;
        }

        let mut sorted_scores: Vec<(PlayerId, u32)> = self
            .players
            .values()
            .filter(|p| p.score >= 400)
            .map(|p| (p.id, p.score))
            .collect();
        sorted_scores.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        let top3_leaders: HashSet<PlayerId> =
            sorted_scores.iter().take(3).map(|&(id, _)| id).collect();
        let leader_id = sorted_scores.first().map(|&(id, _)| id);

        let gen_aura_r = self
            .config
            .building_by_kind(1)
            .map(|c| c.effects.aura_radius)
            .unwrap_or(140.0);

        let mut intel_map = self.gather_bot_intel();

        for (pid, tier, arch) in ready_bots {
            let Some(mut bot_intel) = intel_map.remove(&pid) else {
                continue;
            };

            let (energy, unit_cap, bot_now_sec, active_squad_opt) = self
                .players
                .get(&pid)
                .map(|p| {
                    let primary_sq = p
                        .squads
                        .keys()
                        .copied()
                        .min()
                        .filter(|&sq| {
                            self.units
                                .iter()
                                .any(|u| u.owner_id == pid && u.hp > 0 && u.kind != 4 && u.squad == sq)
                        });
                    (p.energy, p.unit_cap, p.survival_sec, primary_sq)
                })
                .unwrap_or((0.0, 4, 0.0, None));
            let old_rally_info = active_squad_opt.and_then(|sq| {
                self.players
                    .get(&pid)
                    .and_then(|p| p.squads.get(&sq).map(|o| (sq, o.target_x, o.target_y)))
            });
            let garrison_count = self
                .units
                .iter()
                .filter(|u| u.owner_id == pid && u.hp > 0 && u.kind != 4 && u.squad == 0)
                .count();
            let mut active_squad_units = 0_usize;
            if let Some(sq) = active_squad_opt {
                active_squad_units = self
                    .units
                    .iter()
                    .filter(|u| u.owner_id == pid && u.hp > 0 && u.kind != 4 && u.squad == sq)
                    .count();
            }

            if let Some(own_z) = self.zones.get(&pid).cloned() {
                let own_r = own_z.x.hypot(own_z.y);
                let own_r_eq = self.equilibrium_radius(pid);
                let cur_ring = Self::ring_for_radius(own_r);
                if own_r_eq >= RING1_RADIUS && own_r < RING1_RADIUS + own_z.radius + 20.0 {
                    let _ = self.bot_try_teleport_to_ring(pid, if tier == 3 { 3 } else { 2 });
                } else if tier == 3
                    && cur_ring < 3
                    && own_r_eq >= RING2_RADIUS - 120.0
                    && energy >= 450.0
                    && bot_intel.enemy_threat_power < 1.5
                {
                    let _ = self.bot_try_teleport_to_ring(pid, 3);
                }
            }

            // 2. Configure Barracks mix & execute multi-action builds/upgrades
            if !bot_intel.barracks_info.is_empty() {
                self.bot_configure_barracks(pid, arch, &bot_intel.barracks_info);
            }
            self.bot_step_economy(pid, tier, arch, gen_aura_r, &mut bot_intel);

            // 3. Archetype-driven Tactical AI with Honest Fog-of-War & Squad Control
            if bot_intel.enemy_threat_power >= 2.0 && garrison_count == 0 && active_squad_opt.is_none() {
                intel_map.insert(pid, bot_intel);
                continue;
            }

            let wave_threshold = match arch {
                2 => 4,
                3 => 5,
                1 => ((usize::from(unit_cap) * 3) / 4).max(8),
                4 | 0 => ((usize::from(unit_cap) * 85) / 100).max(7),
                _ => ((usize::from(unit_cap) * 2) / 3).max(6),
            };

            let garrison_ready = garrison_count >= wave_threshold;
            let squad_ready = active_squad_units >= 2;
            let full_cap_push = garrison_count >= (usize::from(unit_cap) * 8) / 10
                || active_squad_units >= (usize::from(unit_cap) * 7) / 10;

            if garrison_ready || squad_ready {
                let own_r_eq = self.equilibrium_radius(pid);
                if let Some(own_z) = self.zones.get(&pid).cloned() {
                    let own_r = own_z.x.hypot(own_z.y);
                    let mut known = self
                        .players
                        .get_mut(&pid)
                        .map(|p| std::mem::take(&mut p.bot_known_intel))
                        .unwrap_or_default();
                    let mut scout_ang = self
                        .players
                        .get(&pid)
                        .map(|p| p.bot_scout_angle)
                        .unwrap_or(0.0);

                    for z in self.zones.values() {
                        if z.player_id == pid {
                            continue;
                        }
                        let Some(tp) = self.players.get(&z.player_id) else {
                            continue;
                        };
                        let in_vision = self.has_vision_at(pid, z.x, z.y)
                            || (z.x - own_z.x).hypot(z.y - own_z.y) <= own_z.radius + z.radius + 360.0;
                        let ez_intel = intel_map.get(&z.player_id);
                        let ez_hq_lvl = ez_intel.map(|e| e.hq_lvl).unwrap_or(1);

                        if in_vision {
                            let ez_hq_max_hp = ez_intel.map(|e| e.hq_max_hp).unwrap_or(1800);
                            known.insert(
                                z.player_id,
                                crate::types::BotKnownTarget {
                                    x: z.x,
                                    y: z.y,
                                    hq_lvl: ez_hq_lvl,
                                    hq_hp: ez_intel.map(|e| e.hq_hp).unwrap_or(ez_hq_max_hp),
                                    hq_max_hp: ez_hq_max_hp,
                                    gens: ez_intel.map(|e| e.gens as u16).unwrap_or(0),
                                    turret_power: ez_intel.map(|e| e.turret_power).unwrap_or(0.0),
                                    defending_unit_power: ez_intel
                                        .map(|e| e.defending_unit_power)
                                        .unwrap_or(0.0),
                                    trophies: tp.trophies,
                                    observed_at_sec: bot_now_sec,
                                    is_full_intel: true,
                                },
                            );
                        } else {
                            let is_leader = top3_leaders.contains(&z.player_id);
                            let recent_tp = (tp.survival_sec - tp.last_teleport_sec) <= 8.0;
                            if is_leader || recent_tp {
                                let keep_fresh_full = known
                                    .get(&z.player_id)
                                    .map(|k| k.is_full_intel && (bot_now_sec - k.observed_at_sec) <= 20.0)
                                    .unwrap_or(false);
                                if !keep_fresh_full {
                                    let est_hp = match ez_hq_lvl {
                                        1 => 1800,
                                        2 => 3200,
                                        _ => 5000,
                                    };
                                    known.insert(
                                        z.player_id,
                                        crate::types::BotKnownTarget {
                                            x: z.x,
                                            y: z.y,
                                            hq_lvl: ez_hq_lvl,
                                            hq_hp: est_hp,
                                            hq_max_hp: est_hp,
                                            gens: u16::from(ez_hq_lvl) * 3,
                                            turret_power: f32::from(ez_hq_lvl) * 4.2,
                                            defending_unit_power: f32::from(ez_hq_lvl) * 3.0,
                                            trophies: tp.trophies,
                                            observed_at_sec: bot_now_sec,
                                            is_full_intel: false,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    known.retain(|target_id, kt| {
                        self.zones.contains_key(target_id)
                            && (bot_now_sec - kt.observed_at_sec) <= 45.0
                    });

                    let base_min_win = match arch {
                        2 => 0.62_f32,
                        3 => 0.74,
                        1 => 0.78,
                        0 => 1.05,
                        _ => 0.85,
                    };
                    let min_win_ratio = if full_cap_push {
                        (base_min_win * 0.85).max(0.55)
                    } else {
                        base_min_win
                    };
                    let active_combat_power = if garrison_ready {
                        bot_intel.defending_unit_power.max(bot_intel.own_combat_power * 0.65)
                    } else {
                        bot_intel.own_combat_power
                    };

                    let mut best_target: Option<(f32, f32)> = None;
                    let mut best_utility = f32::NEG_INFINITY;

                    for (&target_id, kt) in &known {
                        let Some(tp) = self.players.get(&target_id) else {
                            continue;
                        };
                        let Some(cur_z) = self.zones.get(&target_id) else {
                            continue;
                        };
                        let currently_visible = self.has_vision_at(pid, cur_z.x, cur_z.y);
                        if currently_visible && tp.spawn_protection_left > 0.0 {
                            continue;
                        }

                        let tier_diff = bot_intel.hq_lvl.saturating_sub(kt.hq_lvl);
                        let (surviving_frac, dmg_mult, def_mult) = match tier_diff {
                            0 => (1.0_f32, 1.0_f32, 1.0_f32),
                            1 => (0.75, 0.50, 1.35),
                            _ => (0.35, 0.30, 1.75),
                        };

                        let hq_dmg_mult = Self::tier_dmg_mult(bot_intel.hq_lvl, kt.hq_lvl, true);
                        let hq_base_def = if tier_diff == 0 {
                            0.75_f32
                        } else {
                            (f32::from(kt.hq_hp) / 700.0) / hq_dmg_mult.max(0.15)
                        };
                        let effective_attack = active_combat_power * surviving_frac * dmg_mult;
                        let effective_defense =
                            (kt.turret_power + kt.defending_unit_power * 1.1) * def_mult
                                + hq_base_def;
                        let win_ratio = effective_attack / effective_defense;
                        if win_ratio < min_win_ratio {
                            continue;
                        }

                        let ez_hq_ratio = f32::from(kt.hq_hp) / f32::from(kt.hq_max_hp.max(1));
                        let (raw_reward_mult, _) = Self::tier_reward_mult(bot_intel.hq_lvl, kt.hq_lvl);
                        let reward_weight = if arch == 2 { 0.85_f32.max(raw_reward_mult) } else { raw_reward_mult };
                        let crown_mult = if leader_id == Some(target_id) {
                            if arch == 1 { 2.8 } else if tier == 3 { 1.65 } else { 1.25 }
                        } else {
                            1.0
                        };
                        let finisher_bonus = if ez_hq_ratio < 0.60 {
                            if arch == 2 || arch == 3 { 2.6 } else { 2.1 }
                        } else {
                            1.0
                        };
                        let exposed_bonus = if kt.is_full_intel && kt.defending_unit_power < 1.2 {
                            if arch == 3 { 2.6 } else { 1.55 }
                        } else {
                            1.0
                        };
                        let loot_value = 1.0
                            + f32::from(kt.gens) * (if arch == 3 { 0.42 } else { 0.28 })
                            + (kt.trophies.min(400.0) / 400.0) * 0.45;
                        let dist = (kt.x - own_z.x).hypot(kt.y - own_z.y);
                        let dist_denom = if arch == 0 || arch == 4 { dist + 140.0 } else { dist + 240.0 };
                        let utility = (reward_weight
                            * crown_mult
                            * finisher_bonus
                            * exposed_bonus
                            * loot_value
                            * win_ratio.min(2.6))
                            / dist_denom;
                        if utility > best_utility {
                            best_utility = utility;
                            best_target = Some((kt.x, kt.y));
                        }
                    }

                    if best_target.is_none() && (full_cap_push || old_rally_info.is_some()) {
                        let any_near_rally = old_rally_info
                            .map(|(sq, rx, ry)| {
                                self.units.iter().any(|u| {
                                    u.owner_id == pid
                                        && u.hp > 0
                                        && u.kind != 4
                                        && u.squad == sq
                                        && (u.x - rx).hypot(u.y - ry) <= 150.0
                                })
                            })
                            .unwrap_or(true);
                        if old_rally_info.is_none() || any_near_rally {
                            scout_ang = (scout_ang + 1.15).rem_euclid(TAU);
                            best_target = Some((
                                scout_ang.cos() * own_r_eq,
                                scout_ang.sin() * own_r_eq,
                            ));
                        } else if let Some((_, rx, ry)) = old_rally_info {
                            best_target = Some((rx, ry));
                        }
                    }

                    if let Some(p) = self.players.get_mut(&pid) {
                        p.bot_known_intel = known;
                        p.bot_scout_angle = scout_ang;
                    }

                    if let Some((tx, ty)) = best_target {
                        let from_ring = Self::ring_for_radius(own_r);
                        let to_ring = Self::ring_for_radius(tx.hypot(ty));
                        let cmd_squad = if garrison_ready {
                            0
                        } else {
                            active_squad_opt.unwrap_or(0)
                        };
                        let should_log_rally = cmd_squad == 0 && garrison_ready;
                        let _ = self.apply_command(
                            pid,
                            ClientMessage::ArmyCommand {
                                squad: cmd_squad,
                                mode: 1,
                                target_x: tx,
                                target_y: ty,
                            },
                        );
                        if should_log_rally {
                            if let Some(tlog) = self.telemetry.as_mut() {
                                let sec =
                                    self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                                tlog.log_milestone(
                                    self.tick,
                                    sec,
                                    pid,
                                    "bot_rally",
                                    json!({"tier": tier, "arch": arch, "from_ring": from_ring, "to_ring": to_ring, "units": garrison_count}),
                                );
                            }
                        }
                    }
                }
            }

            intel_map.insert(pid, bot_intel);
        }
    }

    fn step_zone_repulsion(&mut self, dt: f32) {
        let mut keys: Vec<PlayerId> = self.zones.keys().copied().collect();
        keys.sort_unstable();
        let max_step = self.config.world.zone_repulsion_strength * dt;
        let map_r = self.config.world.map_radius;

        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                let id_a = keys[i];
                let id_b = keys[j];
                let (ax, ay, ar) = {
                    let za = &self.zones[&id_a];
                    (za.x, za.y, za.radius)
                };
                let (bx, by, br) = {
                    let zb = &self.zones[&id_b];
                    (zb.x, zb.y, zb.radius)
                };

                let dx = bx - ax;
                let dy = by - ay;
                let dist = dx.hypot(dy);
                let min_dist = ar + br + 150.0;
                if dist < min_dist {
                    let overlap = min_dist - dist;
                    let push = (overlap * 0.5).min(max_step);
                    let (nx, ny) = if dist > 0.001 {
                        (dx / dist, dy / dist)
                    } else {
                        (1.0, 0.0)
                    };
                    if let Some(za) = self.zones.get_mut(&id_a) {
                        za.x -= nx * push;
                        za.y -= ny * push;
                    }
                    if let Some(zb) = self.zones.get_mut(&id_b) {
                        zb.x += nx * push;
                        zb.y += ny * push;
                    }
                }
            }
        }

        for z in self.zones.values_mut() {
            let max_r = (map_r - z.radius).max(0.0);
            let d = z.x.hypot(z.y);
            if d > max_r && d > 0.001 {
                z.x = z.x / d * max_r;
                z.y = z.y / d * max_r;
            }
        }
    }

    fn step_income_and_spawning(&mut self, dt: f32) {
        let hq_levels = self.hq_level_map();
        let mut player_supply: HashMap<PlayerId, u16> = HashMap::new();
        let mut barrack_supply: HashMap<EntityId, u16> = HashMap::new();
        for u in &self.units {
            if u.owner_id != 0 && u.hp > 0 && u.kind != 4 {
                let sup = self.unit_supply_cost(u.kind);
                *player_supply.entry(u.owner_id).or_insert(0) += sup;
                *barrack_supply.entry(u.home_id).or_insert(0) += sup;
            }
        }

        // 1. Passive energy generation (clamped to HQ energy_cap)
        let mut passive_income: Vec<(PlayerId, u8, f32)> = Vec::new();
        for b in &self.buildings {
            if b.kind != 0 && b.kind != 1 {
                continue;
            }
            if let Some(cfg) = self.config.building_by_kind(b.kind) {
                if let Some(lvl) = cfg.levels.get((b.level as usize).saturating_sub(1)) {
                    if lvl.income_per_sec > 0.0 {
                        passive_income.push((
                            b.owner_id,
                            if b.kind == 0 { 0 } else { 1 },
                            lvl.income_per_sec * dt,
                        ));
                    }
                }
            }
        }

        for (pid, src_slot, amount) in passive_income {
            let hq_lvl = hq_levels.get(&pid).copied().unwrap_or(1);
            self.grant_energy(pid, amount, 1.0, hq_lvl);
            if let Some(tl) = self.telemetry.as_mut() {
                tl.log_income(pid, usize::from(src_slot), amount);
            }
        }

        // 2. Paid Barracks spawning (per-barrack local supply cap + global supply cap + unit energy cost)
        let raider_cfg = self.config.unit_by_kind(0).cloned();
        let archer_cfg = self.config.unit_by_kind(1).cloned();
        let titan_cfg = self.config.unit_by_kind(2).cloned();
        let mut player_energy: HashMap<PlayerId, f32> = self
            .players
            .iter()
            .map(|(&pid, p)| (pid, p.energy))
            .collect();
        let player_caps: HashMap<PlayerId, u16> = self
            .players
            .iter()
            .map(|(&pid, p)| (pid, p.unit_cap))
            .collect();

        let mut spawned_units: Vec<Unit> = Vec::new();
        for b in &mut self.buildings {
            if b.kind != 3 || (b.meta & 0x01) != 0 {
                continue;
            }
            let Some(cfg) = self.config.building_by_kind(3) else {
                continue;
            };
            let Some(lvl) = cfg.levels.get((b.level as usize).saturating_sub(1)) else {
                continue;
            };
            if lvl.spawn_interval_sec <= 0.0 {
                continue;
            }

            b.cooldown_left -= dt;
            if b.cooldown_left > 0.0 {
                continue;
            }

            let requested_type = (b.meta >> 2) & 0x07;
            let (ukind, raw_hp, u_cost, u_sup, cd_mult) = match requested_type {
                1 if b.level >= 2 => (
                    1_u8,
                    archer_cfg.as_ref().map(|c| c.hp).unwrap_or(44.0),
                    archer_cfg.as_ref().map(|c| c.cost).unwrap_or(24.0),
                    archer_cfg.as_ref().map(|c| c.supply_cost.max(1)).unwrap_or(1),
                    1.25_f32,
                ),
                2 if b.level >= 3 => (
                    2_u8,
                    titan_cfg.as_ref().map(|c| c.hp).unwrap_or(540.0),
                    titan_cfg.as_ref().map(|c| c.cost).unwrap_or(65.0),
                    titan_cfg.as_ref().map(|c| c.supply_cost.max(1)).unwrap_or(3),
                    2.1_f32,
                ),
                _ => (
                    0_u8,
                    raider_cfg.as_ref().map(|c| c.hp).unwrap_or(78.0),
                    raider_cfg.as_ref().map(|c| c.cost).unwrap_or(14.0),
                    raider_cfg.as_ref().map(|c| c.supply_cost.max(1)).unwrap_or(1),
                    1.0_f32,
                ),
            };

            let local_cap = if lvl.local_supply_cap > 0 {
                lvl.local_supply_cap
            } else {
                match b.level {
                    1 => 3,
                    2 => 4,
                    _ => 6,
                }
            };
            let global_cap = player_caps.get(&b.owner_id).copied().unwrap_or(0);
            let cur_global = player_supply.get(&b.owner_id).copied().unwrap_or(0);
            let cur_local = barrack_supply.get(&b.id).copied().unwrap_or(0);
            let cur_energy = player_energy.get(&b.owner_id).copied().unwrap_or(0.0);

            if cur_local + u_sup <= local_cap
                && cur_global + u_sup <= global_cap
                && cur_energy >= u_cost
            {
                if let Some(zone) = self.zones.get(&b.owner_id) {
                    let angle = self.rng.gen_range(0.0..TAU);
                    let offset = cfg.radius + 14.0;
                    let ux = zone.x + b.rel_x + angle.cos() * offset;
                    let uy = zone.y + b.rel_y + angle.sin() * offset;
                    let max_hp = raw_hp.round() as u16;
                    let spawn_squad = self
                        .players
                        .get(&b.owner_id)
                        .and_then(|p| p.reinforce_squad.filter(|sid| p.squads.contains_key(sid)))
                        .unwrap_or(0);
                    spawned_units.push(Unit {
                        id: 0,
                        owner_id: b.owner_id,
                        kind: ukind,
                        squad: spawn_squad,
                        x: ux,
                        y: uy,
                        vx: 0.0,
                        vy: 0.0,
                        hp: max_hp,
                        max_hp,
                        attack_cooldown_left: 0.0,
                        last_hit_by: None,
                        last_hit_src: SRC_UNKNOWN,
                        rad_accum: 0.0,
                        in_lower_ring_sec: 0.0,
                        regen_accum: 0.0,
                        home_id: b.id,
                        knockback_vx: 0.0,
                        knockback_vy: 0.0,
                    });
                    *player_supply.entry(b.owner_id).or_insert(0) += u_sup;
                    *barrack_supply.entry(b.id).or_insert(0) += u_sup;
                    player_energy.insert(b.owner_id, cur_energy - u_cost);
                    b.cooldown_left = lvl.spawn_interval_sec * cd_mult;
                    continue;
                }
            }
            b.cooldown_left = 0.0;
        }

        for (pid, e_left) in player_energy {
            if let Some(p) = self.players.get_mut(&pid) {
                p.energy = e_left;
            }
        }

        if !spawned_units.is_empty() {
            for u in &mut spawned_units {
                u.id = self.alloc_entity_id();
            }
            self.units.extend(spawned_units);
        }
    }

    fn step_turrets(&mut self, dt: f32) {
        let protected: HashSet<PlayerId> = self
            .players
            .values()
            .filter(|p| p.spawn_protection_left > 0.0)
            .map(|p| p.id)
            .collect();

        let gen_cfg = self.config.building_by_kind(1);
        let gen_aura_r = gen_cfg
            .map(|c| c.effects.aura_radius)
            .filter(|&v| v > 0.0)
            .unwrap_or(140.0);
        let gen_fr_mult = gen_cfg
            .map(|c| c.effects.fire_rate_mult)
            .filter(|&v| v > 1.0)
            .unwrap_or(1.18);

        let building_world: Vec<(PlayerId, u8, f32, f32)> = self
            .buildings
            .iter()
            .filter_map(|b| {
                self.zones
                    .get(&b.owner_id)
                    .map(|z| (b.owner_id, b.kind, z.x + b.rel_x, z.y + b.rel_y))
            })
            .collect();

        let mut b_grid = SpatialGrid::new(96.0);
        for (bi, &(_, _, bx, by)) in building_world.iter().enumerate() {
            b_grid.insert(bi, bx, by);
        }

        let mut u_grid = SpatialGrid::new(96.0);
        for (ui, u) in self.units.iter().enumerate() {
            if u.hp > 0 && !protected.contains(&u.owner_id) {
                u_grid.insert(ui, u.x, u.y);
            }
        }

        let mut new_projectiles: Vec<Projectile> = Vec::new();
        let mut neighbors: Vec<(usize, f32, f32)> = Vec::new();

        for i in 0..self.buildings.len() {
            let (owner_id, kind, level, rel_x, rel_y) = {
                let b = &self.buildings[i];
                (b.owner_id, b.kind, b.level, b.rel_x, b.rel_y)
            };
            if kind != 2 {
                continue;
            }
            let Some(cfg) = self.config.building_by_kind(kind) else {
                continue;
            };
            let Some(lvl) = cfg.levels.get((level as usize).saturating_sub(1)) else {
                continue;
            };
            if lvl.fire_rate <= 0.0 || lvl.range <= 0.0 {
                continue;
            }

            let mut fire_rate = lvl.fire_rate;
            let base_dmg = lvl.damage;
            let base_range = lvl.range;
            let proj_speed = lvl.projectile_speed.max(120.0);
            let splash_r = match level {
                1 => 44.0_f32,
                2 => 48.0_f32,
                _ => 54.0_f32,
            };

            let Some(zone) = self.zones.get(&owner_id) else {
                continue;
            };
            let bx = zone.x + rel_x;
            let by = zone.y + rel_y;

            let gen_aura_r2 = gen_aura_r * gen_aura_r;
            b_grid.query_radius(bx, by, gen_aura_r, &mut neighbors);
            if neighbors.iter().any(|&(bi, gx, gy)| {
                let (bo, bk, _, _) = building_world[bi];
                bo == owner_id && bk == 1 && (gx - bx).powi(2) + (gy - by).powi(2) <= gen_aura_r2
            }) {
                fire_rate *= gen_fr_mult;
            }

            self.buildings[i].cooldown_left -= dt;
            if self.buildings[i].cooldown_left > 0.0 {
                continue;
            }

            let mut best_hp = u16::MAX;
            let mut best_dist2 = base_range * base_range;
            let mut target_info: Option<(f32, f32, f32, f32)> = None;

            u_grid.query_radius(bx, by, base_range, &mut neighbors);
            for &(ui, ux, uy) in &neighbors {
                let u = &self.units[ui];
                if u.owner_id == owner_id || u.hp == 0 {
                    continue;
                }
                let dx = ux - bx;
                let dy = uy - by;
                let d2 = dx * dx + dy * dy;
                if d2 <= base_range * base_range
                    && (u.hp < best_hp || (u.hp == best_hp && d2 < best_dist2))
                {
                    best_hp = u.hp;
                    best_dist2 = d2;
                    target_info = Some((ux, uy, u.vx, u.vy));
                }
            }

            if target_info.is_none() {
                b_grid.query_radius(bx, by, base_range, &mut neighbors);
                for &(bi, tx, ty) in &neighbors {
                    let (b_owner, _, _, _) = building_world[bi];
                    if b_owner == owner_id || protected.contains(&b_owner) {
                        continue;
                    }
                    let dx = tx - bx;
                    let dy = ty - by;
                    let d2 = dx * dx + dy * dy;
                    if d2 <= best_dist2 {
                        best_dist2 = d2;
                        target_info = Some((tx, ty, 0.0, 0.0));
                    }
                }
            }

            let src_tag = match level {
                1 => SRC_TURRET_1,
                2 => SRC_TURRET_2,
                _ => SRC_TURRET_3,
            };

            if let Some((tx, ty, tvx, tvy)) = target_info {
                let raw_dist = (tx - bx).hypot(ty - by).max(0.001);
                let t_flight = (raw_dist / proj_speed).min(1.5);
                let lead_x = tx + tvx * t_flight * 0.85;
                let lead_y = ty + tvy * t_flight * 0.85;
                let dx = lead_x - bx;
                let dy = lead_y - by;
                let dist = dx.hypot(dy).max(0.001);
                let rad = dy.atan2(dx).rem_euclid(TAU);
                self.buildings[i].angle = ((rad / TAU) * 255.0).round() as u8;
                self.buildings[i].cooldown_left = 1.0 / fire_rate;

                new_projectiles.push(Projectile {
                    id: 0,
                    owner_id,
                    kind: 0,
                    src_tag,
                    x: bx,
                    y: by,
                    vx: (dx / dist) * proj_speed,
                    vy: (dy / dist) * proj_speed,
                    target_x: lead_x,
                    target_y: lead_y,
                    damage: base_dmg.round() as u16,
                    ttl: (base_range / proj_speed) * 1.15,
                    splash_radius: splash_r,
                });
            }
        }

        for p in &mut new_projectiles {
            p.id = self.alloc_entity_id();
        }
        self.projectiles.extend(new_projectiles);
    }

    fn step_projectiles(&mut self, dt: f32) {
        let protected: HashSet<PlayerId> = self
            .players
            .values()
            .filter(|p| p.spawn_protection_left > 0.0)
            .map(|p| p.id)
            .collect();

        let hq_levels = self.hq_level_map();
        let mut tlog = self.telemetry.take();
        let owner_rings: HashMap<PlayerId, u8> = if tlog.is_some() {
            self.zones
                .iter()
                .map(|(&pid, z)| (pid, Self::ring_for_radius(z.x.hypot(z.y))))
                .collect()
        } else {
            HashMap::new()
        };

        let unit_r2 = 15.0_f32 * 15.0_f32;
        let mut alive = vec![true; self.projectiles.len()];
        let mut damaged_owners: HashSet<PlayerId> = HashSet::new();

        let mut u_grid = SpatialGrid::new(64.0);
        for (ui, u) in self.units.iter().enumerate() {
            if u.hp > 0 && !protected.contains(&u.owner_id) {
                u_grid.insert(ui, u.x, u.y);
            }
        }

        let b_pos: Vec<Option<(PlayerId, f32, f32, f32, u8)>> = self
            .buildings
            .iter()
            .map(|b| {
                if b.hp == 0 || protected.contains(&b.owner_id) {
                    return None;
                }
                let z = self.zones.get(&b.owner_id)?;
                let br = self
                    .config
                    .building_by_kind(b.kind)
                    .map(|c| c.radius)
                    .unwrap_or(22.0)
                    + 4.0;
                Some((b.owner_id, z.x + b.rel_x, z.y + b.rel_y, br, b.kind))
            })
            .collect();
        let mut b_grid = SpatialGrid::new(64.0);
        for (bi, entry) in b_pos.iter().enumerate() {
            if let Some(&(_, bx, by, _, _)) = entry.as_ref() {
                b_grid.insert(bi, bx, by);
            }
        }
        let mut neighbors: Vec<(usize, f32, f32)> = Vec::new();

        for (pi, p) in self.projectiles.iter_mut().enumerate() {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.ttl -= dt;

            if p.ttl <= 0.0 {
                alive[pi] = false;
                continue;
            }

            let mut hit = false;
            let mut direct_hit_unit_idx: Option<usize> = None;
            let att_hq = hq_levels.get(&p.owner_id).copied().unwrap_or(1);

            u_grid.query_radius(p.x, p.y, 16.0, &mut neighbors);
            for &(ui, ux, uy) in &neighbors {
                let u = &mut self.units[ui];
                if u.owner_id == p.owner_id || u.hp == 0 {
                    continue;
                }
                let dx = ux - p.x;
                let dy = uy - p.y;
                if dx * dx + dy * dy <= unit_r2 {
                    let dmg_amt = if u.owner_id != 0 {
                        let def_hq = hq_levels.get(&u.owner_id).copied().unwrap_or(1);
                        (f32::from(p.damage) * Self::tier_dmg_mult(att_hq, def_hq, false))
                            .round()
                            .max(1.0) as u16
                    } else {
                        p.damage
                    };
                    let dealt = u.hp.min(dmg_amt);
                    u.hp = u.hp.saturating_sub(dmg_amt);
                    u.last_hit_by = Some(p.owner_id);
                    u.last_hit_src = p.src_tag;
                    if u.owner_id != 0 {
                        damaged_owners.insert(u.owner_id);
                    }
                    if let Some(tl) = tlog.as_mut() {
                        let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                        let dr = Self::ring_for_radius(ux.hypot(uy));
                        tl.log_damage(p.src_tag, u.kind, sr, dr, dealt);
                    }
                    direct_hit_unit_idx = Some(ui);
                    hit = true;
                    break;
                }
            }

            if !hit {
                b_grid.query_radius(p.x, p.y, 34.0, &mut neighbors);
                for &(bi, bx, by) in &neighbors {
                    let Some((b_owner, _, _, br, bkind)) = b_pos[bi] else {
                        continue;
                    };
                    if b_owner == p.owner_id {
                        continue;
                    }
                    let b = &mut self.buildings[bi];
                    if b.hp == 0 {
                        continue;
                    }
                    let dx = bx - p.x;
                    let dy = by - p.y;
                    if dx * dx + dy * dy <= br * br {
                        let def_hq = hq_levels.get(&b.owner_id).copied().unwrap_or(1);
                        let dmg_amt = (f32::from(p.damage) * Self::tier_dmg_mult(att_hq, def_hq, b.kind == 0))
                            .round()
                            .max(1.0) as u16;
                        let dealt = b.hp.min(dmg_amt);
                        b.hp = b.hp.saturating_sub(dmg_amt);
                        b.last_hit_by = Some(p.owner_id);
                        b.last_hit_src = p.src_tag;
                        damaged_owners.insert(b.owner_id);
                        if let Some(tl) = tlog.as_mut() {
                            let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                            let dr = Self::ring_for_radius(bx.hypot(by));
                            tl.log_damage(p.src_tag, DST_BUILDING_OFFSET + bkind, sr, dr, dealt);
                        }
                        hit = true;
                        break;
                    }
                }
            }

            if hit && p.splash_radius > 0.0 {
                let sr2 = p.splash_radius.powi(2);
                let splash_base = (p.damage / 2).max(1);
                u_grid.query_radius(p.x, p.y, p.splash_radius + 12.0, &mut neighbors);
                for &(ui, ux, uy) in &neighbors {
                    if Some(ui) == direct_hit_unit_idx {
                        continue;
                    }
                    let u = &mut self.units[ui];
                    if u.owner_id == p.owner_id || u.hp == 0 {
                        continue;
                    }
                    let dx = ux - p.x;
                    let dy = uy - p.y;
                    if dx * dx + dy * dy <= sr2 {
                        let splash_dmg = if u.owner_id != 0 {
                            let def_hq = hq_levels.get(&u.owner_id).copied().unwrap_or(1);
                            (f32::from(splash_base) * Self::tier_dmg_mult(att_hq, def_hq, false))
                                .round()
                                .max(1.0) as u16
                        } else {
                            splash_base
                        };
                        let dealt = u.hp.min(splash_dmg);
                        u.hp = u.hp.saturating_sub(splash_dmg);
                        u.last_hit_by = Some(p.owner_id);
                        u.last_hit_src = p.src_tag;
                        if u.owner_id != 0 {
                            damaged_owners.insert(u.owner_id);
                        }
                        if let Some(tl) = tlog.as_mut() {
                            let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                            let dr = Self::ring_for_radius(ux.hypot(uy));
                            tl.log_damage(p.src_tag, u.kind, sr, dr, dealt);
                        }
                    }
                }
            }
            if hit {
                alive[pi] = false;
            }
        }

        for pid in damaged_owners {
            if let Some(pl) = self.players.get_mut(&pid) {
                pl.last_damaged_sec = pl.survival_sec;
            }
        }
        self.telemetry = tlog;

        let mut idx = 0;
        self.projectiles.retain(|_| {
            let keep = alive[idx];
            idx += 1;
            keep
        });
    }

    fn step_units(&mut self, dt: f32) {
        if self.units.is_empty() {
            return;
        }
        let raider_cfg = self.config.unit_by_kind(0).cloned();
        let archer_cfg = self.config.unit_by_kind(1).cloned();
        let titan_cfg = self.config.unit_by_kind(2).cloned();
        let aggro_r = raider_cfg.as_ref().map(|u| u.aggro_range).unwrap_or(290.0);
        let aggro_r2 = aggro_r * aggro_r;
        let archer_range = archer_cfg
            .as_ref()
            .map(|c| if c.range > 0.0 { c.range } else { 175.0 })
            .unwrap_or(175.0);

        let hq_levels = self.hq_level_map();

        let protected: HashSet<PlayerId> = self
            .players
            .values()
            .filter(|p| p.spawn_protection_left > 0.0)
            .map(|p| p.id)
            .collect();

        let mut grid = SpatialGrid::new(64.0);
        for (i, u) in self.units.iter().enumerate() {
            if u.hp > 0 {
                grid.insert(i, u.x, u.y);
            }
        }

        let raider_r = raider_cfg.as_ref().map(|c| c.radius).unwrap_or(8.5);
        let archer_r = archer_cfg.as_ref().map(|c| c.radius).unwrap_or(9.5);
        let titan_r = titan_cfg.as_ref().map(|c| c.radius).unwrap_or(14.0);

        let unit_snapshot: Vec<(PlayerId, f32, f32, u16, u8, f32)> = self
            .units
            .iter()
            .map(|u| {
                let r = match u.kind {
                    1 => archer_r,
                    2 => titan_r,
                    _ => raider_r,
                };
                (u.owner_id, u.x, u.y, u.hp, u.kind, r)
            })
            .collect();

        let building_snapshot: Vec<Option<(PlayerId, f32, f32, f32, u16, u8)>> = self
            .buildings
            .iter()
            .map(|b| {
                let z = self.zones.get(&b.owner_id)?;
                let r = self
                    .config
                    .building_by_kind(b.kind)
                    .map(|c| c.radius)
                    .unwrap_or(22.0);
                Some((b.owner_id, z.x + b.rel_x, z.y + b.rel_y, r, b.hp, b.kind))
            })
            .collect();

        let mut b_grid = SpatialGrid::new(96.0);
        for (bi, entry) in building_snapshot.iter().enumerate() {
            if let Some(&(_, bx, by, _, bhp, _)) = entry.as_ref() {
                if bhp > 0 {
                    b_grid.insert(bi, bx, by);
                }
            }
        }

        let mut unit_damage_events: Vec<(usize, u16, PlayerId, u8)> = Vec::new();
        let mut building_damage_events: Vec<(usize, u16, PlayerId, u8)> = Vec::new();
        let mut rad_damage_events: Vec<(u8, u8, u16)> = Vec::new();
        let mut new_projectiles: Vec<Projectile> = Vec::new();
        let mut neighbors: Vec<(usize, f32, f32)> = Vec::new();
        let mut b_neighbors: Vec<(usize, f32, f32)> = Vec::new();
        let max_sep_query = titan_r * 2.0 + 6.0;
        let max_b_coll_r = 28.0 + titan_r + 4.0;
        let map_r = self.config.world.map_radius;

        // Deterministic formation slot within each (owner_id, squad) so arriving drones keep distance.
        let mut squad_counters: HashMap<(PlayerId, u8), u16> = HashMap::new();
        let mut squad_slots: Vec<u16> = vec![0; self.units.len()];
        for (i, u) in self.units.iter().enumerate() {
            if u.hp > 0 && u.owner_id != 0 && u.kind != 4 {
                let c = squad_counters.entry((u.owner_id, u.squad)).or_insert(0);
                squad_slots[i] = *c;
                *c += 1;
            }
        }

        for (i, u) in self.units.iter_mut().enumerate() {
            if u.hp == 0 {
                continue;
            }
            if u.attack_cooldown_left > 0.0 {
                u.attack_cooldown_left = (u.attack_cooldown_left - dt).max(0.0);
            }

            // Ring Gravity Slowdown & Progressive % Radiation in lower rings
            let owner_hq = hq_levels.get(&u.owner_id).copied().unwrap_or(1);
            let u_r = u.x.hypot(u.y);
            let unit_ring = Self::ring_for_radius(u_r);
            let grav_speed_mult = if u.owner_id != 0 && unit_ring < owner_hq {
                u.in_lower_ring_sec += dt;
                let diff = owner_hq - unit_ring;
                let base_rate = if diff == 1 { 0.045_f32 } else { 0.10_f32 };
                let ring_edge = if unit_ring == 1 { RING1_RADIUS } else { RING2_RADIUS };
                let depth = ((ring_edge - u_r) / ring_edge).clamp(0.0, 1.0);
                let rate = base_rate * (1.0 + 0.85 * depth) * (1.0 + 0.08 * u.in_lower_ring_sec);
                u.rad_accum += f32::from(u.max_hp) * rate * dt;
                if u.rad_accum >= 1.0 {
                    let rad_dmg = u.rad_accum.floor() as u16;
                    u.rad_accum -= f32::from(rad_dmg);
                    let dealt = u.hp.min(rad_dmg);
                    u.hp = u.hp.saturating_sub(rad_dmg);
                    if dealt > 0 {
                        rad_damage_events.push((u.kind, unit_ring, dealt));
                    }
                    if u.hp == 0 {
                        if u.last_hit_by.is_none() {
                            u.last_hit_src = SRC_RAD;
                        }
                        continue;
                    }
                }
                match diff {
                    1 => 0.58,
                    _ => 0.40,
                }
            } else {
                u.in_lower_ring_sec = 0.0;
                u.rad_accum = 0.0;
                1.0
            };

            let (unit_base_speed, unit_base_dmg, unit_cd, attack_range, is_ranged, my_r) =
                match u.kind {
                    1 => (
                        archer_cfg.as_ref().map(|c| c.speed).unwrap_or(158.0),
                        archer_cfg.as_ref().map(|c| c.damage).unwrap_or(15.0),
                        archer_cfg.as_ref().map(|c| c.attack_cooldown_sec).unwrap_or(0.90),
                        archer_range,
                        true,
                        archer_r,
                    ),
                    2 => (
                        titan_cfg.as_ref().map(|c| c.speed).unwrap_or(128.0),
                        titan_cfg.as_ref().map(|c| c.damage).unwrap_or(40.0),
                        titan_cfg.as_ref().map(|c| c.attack_cooldown_sec).unwrap_or(0.95),
                        0.0,
                        false,
                        titan_r,
                    ),
                    _ => (
                        raider_cfg.as_ref().map(|c| c.speed).unwrap_or(235.0),
                        raider_cfg.as_ref().map(|c| c.damage).unwrap_or(13.0),
                        raider_cfg.as_ref().map(|c| c.attack_cooldown_sec).unwrap_or(0.55),
                        0.0,
                        false,
                        raider_r,
                    ),
                };

            let speed = unit_base_speed * grav_speed_mult;
            let damage = unit_base_dmg.round() as u16;

            let (owner_is_bot, squad_order) = if u.owner_id != 0 {
                self.players
                    .get(&u.owner_id)
                    .map(|p| {
                        let ord = if u.squad > 0 {
                            p.squads.get(&u.squad).copied()
                        } else {
                            p.base_rally
                        };
                        (p.is_bot, ord)
                    })
                    .unwrap_or((false, None))
            } else {
                (false, None)
            };
            let slot = squad_slots[i];
            let (slot_ox, slot_oy) = if slot == 0 {
                (0.0, 0.0)
            } else {
                let angle = (slot as f32) * 2.399963_f32;
                let r = 28.0 * (slot as f32).sqrt();
                (angle.cos() * r, angle.sin() * r)
            };
            let raw_order = squad_order;
            let squad_order = squad_order.map(|o| crate::types::SquadOrder {
                target_x: o.target_x + slot_ox,
                target_y: o.target_y + slot_oy,
            });
            let dist_to_raw = raw_order
                .map(|o| (o.target_x - u.x).hypot(o.target_y - u.y))
                .unwrap_or(0.0);
            let dist_to_order = squad_order
                .map(|o| (o.target_x - u.x).hypot(o.target_y - u.y))
                .unwrap_or(0.0);
            // Human-controlled squads NEVER auto-chase on march (>150px from target);
            // at the target point they close in to attack enemies within 110px of raw_order,
            // and otherwise hold formation slots without bumping.
            let strict_manual = squad_order.is_some() && !owner_is_bot;
            let at_manual_target = strict_manual && dist_to_raw <= 150.0;
            let mut hold_course = strict_manual || (squad_order.is_some() && dist_to_order > 85.0);

            let mut best_edge_dist = aggro_r;
            let mut target_unit: Option<(usize, f32, f32)> = None;
            let mut target_building: Option<(usize, f32, f32, f32, u8)> = None;

            let unit_hit_r = if is_ranged { attack_range } else { my_r * 2.0 + 4.5 };

            if at_manual_target {
                if let Some(ro) = raw_order {
                    // Attack only enemy buildings in the ordered target area (no auto-attack on enemy units)
                    let mut best_b_d = f32::MAX;
                    b_grid.query_radius(ro.target_x, ro.target_y, 130.0, &mut b_neighbors);
                    for &(bi, bx, by) in &b_neighbors {
                        let Some((b_owner, _, _, br, bhp, bkind)) = building_snapshot[bi] else {
                            continue;
                        };
                        if b_owner == u.owner_id || bhp == 0 || protected.contains(&b_owner) {
                            continue;
                        }
                        let d_raw_edge = ((bx - ro.target_x).hypot(by - ro.target_y) - br).max(0.0);
                        if d_raw_edge <= 110.0 && d_raw_edge < best_b_d {
                            best_b_d = d_raw_edge;
                            target_building = Some((bi, bx, by, br, bkind));
                        }
                    }
                    if target_building.is_some() {
                        hold_course = false;
                    }
                }
            } else {
                let search_r = if hold_course { unit_hit_r } else { aggro_r };
                let search_r2 = search_r * search_r;

                grid.query_radius(u.x, u.y, search_r + 16.0, &mut neighbors);
                for &(other_i, ox, oy) in &neighbors {
                    if other_i == i {
                        continue;
                    }
                    let (other_owner, _, _, other_hp, _, other_r) = unit_snapshot[other_i];
                    if other_owner == u.owner_id
                        || other_hp == 0
                        || (other_owner != 0 && protected.contains(&other_owner))
                    {
                        continue;
                    }
                    if !hold_course {
                        if let Some(o) = squad_order {
                            if (ox - o.target_x).hypot(oy - o.target_y) > 210.0 {
                                continue;
                            }
                        }
                    }
                    let d2 = (ox - u.x).powi(2) + (oy - u.y).powi(2);
                    if d2 <= search_r2 {
                        let edge_d = (d2.sqrt() - other_r).max(0.0);
                        if edge_d <= best_edge_dist {
                            best_edge_dist = edge_d;
                            target_unit = Some((other_i, ox, oy));
                        }
                    }
                }

                if u.owner_id != 0 && target_unit.is_none() && (strict_manual || !hold_course) {
                    let b_search_r = if strict_manual {
                        (if is_ranged { attack_range } else { my_r + 3.0 }) + 28.0
                    } else {
                        aggro_r
                    };
                    let b_search_r2 = b_search_r * b_search_r;
                    b_grid.query_radius(u.x, u.y, b_search_r, &mut b_neighbors);
                    for &(bi, bx, by) in &b_neighbors {
                        let Some((b_owner, _, _, br, bhp, bkind)) = building_snapshot[bi] else {
                            continue;
                        };
                        if b_owner == u.owner_id || bhp == 0 || protected.contains(&b_owner) {
                            continue;
                        }
                        if strict_manual {
                            let d_edge = (bx - u.x).hypot(by - u.y);
                            let hit_d = if is_ranged { attack_range + br } else { my_r + br + 3.0 };
                            if d_edge > hit_d {
                                continue;
                            }
                        } else if let Some(o) = squad_order {
                            if (bx - o.target_x).hypot(by - o.target_y) > 210.0 + br {
                                continue;
                            }
                        }
                        let d2 = (bx - u.x).powi(2) + (by - u.y).powi(2);
                        if d2 <= b_search_r2 {
                            let edge_d = (d2.sqrt() - br).max(0.0);
                            if edge_d <= best_edge_dist {
                                best_edge_dist = edge_d;
                                target_building = Some((bi, bx, by, br, bkind));
                            }
                        }
                    }
                }
            }

            let mut vx = 0.0_f32;
            let mut vy = 0.0_f32;

            if let Some((ti, tx, ty)) = target_unit {
                let dx = tx - u.x;
                let dy = ty - u.y;
                let dist = dx.hypot(dy);
                let hit_dist = unit_hit_r;
                if dist <= hit_dist {
                    if u.attack_cooldown_left <= 0.0 {
                        if is_ranged && dist > 0.001 {
                            let p_speed = 540.0;
                            new_projectiles.push(Projectile {
                                id: 0,
                                owner_id: u.owner_id,
                                kind: 2,
                                src_tag: u.kind,
                                x: u.x,
                                y: u.y,
                                vx: (dx / dist) * p_speed,
                                vy: (dy / dist) * p_speed,
                                target_x: tx,
                                target_y: ty,
                                damage,
                                ttl: (attack_range / p_speed) + 0.15,
                                splash_radius: 0.0,
                            });
                        } else {
                            unit_damage_events.push((ti, damage, u.owner_id, u.kind));
                            if !hold_course && dist > 0.001 {
                                let recoil = if u.kind == 2 { 110.0 } else { 195.0 };
                                u.knockback_vx = -(dx / dist) * recoil;
                                u.knockback_vy = -(dy / dist) * recoil;
                            }
                        }
                        u.attack_cooldown_left = unit_cd;
                    }
                } else if !hold_course && dist > 0.001 {
                    vx = (dx / dist) * speed;
                    vy = (dy / dist) * speed;
                }
            } else if let Some((bi, bx, by, br, bkind)) = target_building {
                let dx = bx - u.x;
                let dy = by - u.y;
                let dist = dx.hypot(dy);
                let hit_dist = if is_ranged { attack_range + br } else { my_r + br + 3.0 };
                if dist <= hit_dist {
                    if u.attack_cooldown_left <= 0.0 {
                        let b_dmg = if u.kind == 0 && (bkind == 1 || bkind == 5) {
                            (f32::from(damage) * 1.3).round() as u16
                        } else if u.kind == 0 && bkind == 0 {
                            (f32::from(damage) * 0.85).round().max(1.0) as u16
                        } else {
                            damage
                        };
                        if is_ranged && dist > 0.001 {
                            let p_speed = 540.0;
                            new_projectiles.push(Projectile {
                                id: 0,
                                owner_id: u.owner_id,
                                kind: 2,
                                src_tag: u.kind,
                                x: u.x,
                                y: u.y,
                                vx: (dx / dist) * p_speed,
                                vy: (dy / dist) * p_speed,
                                target_x: bx,
                                target_y: by,
                                damage: b_dmg,
                                ttl: ((attack_range + br) / p_speed) + 0.15,
                                splash_radius: 0.0,
                            });
                        } else {
                            building_damage_events.push((bi, b_dmg, u.owner_id, u.kind));
                            if !hold_course && dist > 0.001 {
                                let bounce = if u.kind == 2 { 175.0 } else { 290.0 };
                                u.knockback_vx = -(dx / dist) * bounce;
                                u.knockback_vy = -(dy / dist) * bounce;
                            }
                        }
                        u.attack_cooldown_left = unit_cd;
                    }
                } else if !hold_course && dist > 0.001 {
                    vx = (dx / dist) * speed;
                    vy = (dy / dist) * speed;
                }
            }

            if hold_course {
                if let Some(order) = squad_order {
                    let dist = (order.target_x - u.x).hypot(order.target_y - u.y);
                    if dist > 8.0 && dist > 0.001 {
                        let slow = (dist / 24.0).clamp(0.25, 1.0);
                        let (dir_x, dir_y) = Self::polar_move_dir(u.x, u.y, order.target_x, order.target_y);
                        vx = dir_x * speed * slow;
                        vy = dir_y * speed * slow;
                    }
                }
            } else if target_unit.is_none() && target_building.is_none() && u.owner_id != 0 {
                if let Some(order) = squad_order {
                    let dist = (order.target_x - u.x).hypot(order.target_y - u.y);
                    if dist > 8.0 && dist > 0.001 {
                        let slow = (dist / 24.0).clamp(0.25, 1.0);
                        let (dir_x, dir_y) = Self::polar_move_dir(u.x, u.y, order.target_x, order.target_y);
                        vx = dir_x * speed * slow;
                        vy = dir_y * speed * slow;
                    }
                } else if let Some(z) = self.zones.get(&u.owner_id) {
                    let orbit_r = z.radius + 14.0 + ((u.id % 3) as f32) * 10.0;
                    let rx = u.x - z.x;
                    let ry = u.y - z.y;
                    let dist = rx.hypot(ry);
                    let (nx, ny) = if dist > 0.001 {
                        (rx / dist, ry / dist)
                    } else {
                        let a = (u.id as f32) * 1.1;
                        (a.cos(), a.sin())
                    };
                    let tx = -ny;
                    let ty = nx;
                    let radial_err = ((orbit_r - dist) / 48.0).clamp(-1.0, 1.0);
                    let tang_weight =
                        if (dist - orbit_r).abs() > 75.0 { 0.25 } else { 1.0 };
                    let dir_x = nx * radial_err + tx * tang_weight;
                    let dir_y = ny * radial_err + ty * tang_weight;
                    let dlen = dir_x.hypot(dir_y);
                    let patrol_speed = speed * 0.78;
                    if dlen > 0.001 {
                        vx = (dir_x / dlen) * patrol_speed;
                        vy = (dir_y / dlen) * patrol_speed;
                    }
                }
            }

            // Unit <-> Unit collision separation + elastic knockback on collision
            grid.query_radius(u.x, u.y, max_sep_query, &mut neighbors);
            for &(other_i, ox, oy) in &neighbors {
                if other_i == i {
                    continue;
                }
                let (other_owner, _, _, other_hp, other_kind, other_r) = unit_snapshot[other_i];
                if other_hp == 0 {
                    continue;
                }
                let min_pair = my_r + other_r + 2.5;
                let dx = u.x - ox;
                let dy = u.y - oy;
                let dist = dx.hypot(dy);
                if dist < min_pair {
                    let (nx, ny) = if dist > 0.001 { (dx / dist, dy / dist) } else { (1.0, 0.0) };
                    let overlap = min_pair - dist;
                    u.x += nx * overlap * 0.52;
                    u.y += ny * overlap * 0.52;

                    if other_owner != u.owner_id {
                        let mass_factor = if other_kind == 2 && u.kind != 2 {
                            1.45
                        } else if u.kind == 2 && other_kind != 2 {
                            0.45
                        } else {
                            1.0
                        };
                        let imp = 190.0 * mass_factor;
                        u.knockback_vx += nx * imp * 0.45;
                        u.knockback_vy += ny * imp * 0.45;
                    }
                }
            }

            // Unit <-> Enemy Building collision pushout + bounce (allies pass freely through own buildings!)
            b_grid.query_radius(u.x, u.y, max_b_coll_r, &mut b_neighbors);
            for &(bi, bx, by) in &b_neighbors {
                let Some((b_owner, _, _, br, bhp, _)) = building_snapshot[bi] else {
                    continue;
                };
                if bhp == 0 || b_owner == u.owner_id {
                    continue;
                }
                let min_b_dist = br + my_r + 2.0;
                let dx = u.x - bx;
                let dy = u.y - by;
                let dist = dx.hypot(dy);
                if dist < min_b_dist {
                    let (nx, ny) = if dist > 0.001 { (dx / dist, dy / dist) } else { (1.0, 0.0) };
                    u.x = bx + nx * min_b_dist;
                    u.y = by + ny * min_b_dist;

                    let bounce = if u.kind == 2 { 155.0 } else { 240.0 };
                    let radial_kb = u.knockback_vx * nx + u.knockback_vy * ny;
                    if radial_kb < bounce {
                        let add = bounce - radial_kb;
                        u.knockback_vx += nx * add;
                        u.knockback_vy += ny * add;
                    }
                }
            }

            let kb_len = u.knockback_vx.hypot(u.knockback_vy);
            if kb_len > 360.0 && kb_len > 0.001 {
                u.knockback_vx = (u.knockback_vx / kb_len) * 360.0;
                u.knockback_vy = (u.knockback_vy / kb_len) * 360.0;
            }

            u.vx = vx + u.knockback_vx;
            u.vy = vy + u.knockback_vy;
            u.x += u.vx * dt;
            u.y += u.vy * dt;

            let damp = (1.0 - 6.5 * dt).max(0.0);
            u.knockback_vx *= damp;
            u.knockback_vy *= damp;

            let max_r = (map_r - my_r).max(0.0);
            let d = u.x.hypot(u.y);
            if d > max_r && d > 0.001 {
                u.x = u.x / d * max_r;
                u.y = u.y / d * max_r;
            }
        }

        for p in &mut new_projectiles {
            p.id = self.alloc_entity_id();
        }
        self.projectiles.extend(new_projectiles);

        let mut tlog = self.telemetry.take();
        let owner_rings: HashMap<PlayerId, u8> = if tlog.is_some() {
            self.zones
                .iter()
                .map(|(&pid, z)| (pid, Self::ring_for_radius(z.x.hypot(z.y))))
                .collect()
        } else {
            HashMap::new()
        };

        let mut damaged_owners: HashSet<PlayerId> = HashSet::new();
        for (ti, dmg, attacker, src_kind) in unit_damage_events {
            if let Some(target) = self.units.get_mut(ti) {
                let adj_dmg = if attacker != 0 && target.owner_id != 0 {
                    let att_hq = hq_levels.get(&attacker).copied().unwrap_or(1);
                    let def_hq = hq_levels.get(&target.owner_id).copied().unwrap_or(1);
                    (f32::from(dmg) * Self::tier_dmg_mult(att_hq, def_hq, false))
                        .round()
                        .max(1.0) as u16
                } else {
                    dmg
                };
                let dealt = target.hp.min(adj_dmg);
                target.hp = target.hp.saturating_sub(adj_dmg);
                target.last_hit_by = Some(attacker);
                target.last_hit_src = src_kind;
                if target.owner_id != 0 {
                    damaged_owners.insert(target.owner_id);
                }
                if let Some(tl) = tlog.as_mut() {
                    let sr = owner_rings.get(&attacker).copied().unwrap_or(0);
                    let dr = Self::ring_for_radius(target.x.hypot(target.y));
                    tl.log_damage(src_kind, target.kind, sr, dr, dealt);
                }
            }
        }
        for (bi, dmg, attacker, src_kind) in building_damage_events {
            if let Some(target) = self.buildings.get_mut(bi) {
                let att_hq = hq_levels.get(&attacker).copied().unwrap_or(1);
                let def_hq = hq_levels.get(&target.owner_id).copied().unwrap_or(1);
                let dmg_amt = (f32::from(dmg) * Self::tier_dmg_mult(att_hq, def_hq, target.kind == 0))
                    .round()
                    .max(1.0) as u16;
                let dealt = target.hp.min(dmg_amt);
                target.hp = target.hp.saturating_sub(dmg_amt);
                target.last_hit_by = Some(attacker);
                target.last_hit_src = src_kind;
                damaged_owners.insert(target.owner_id);
                if let Some(tl) = tlog.as_mut() {
                    let sr = owner_rings.get(&attacker).copied().unwrap_or(0);
                    let dr = owner_rings.get(&target.owner_id).copied().unwrap_or(1);
                    tl.log_damage(src_kind, DST_BUILDING_OFFSET + target.kind, sr, dr, dealt);
                }
            }
        }
        for pid in damaged_owners {
            if let Some(pl) = self.players.get_mut(&pid) {
                pl.last_damaged_sec = pl.survival_sec;
            }
        }
        if let Some(tl) = tlog.as_mut() {
            for (ukind, dr, dealt) in rad_damage_events {
                tl.log_damage(SRC_RAD, ukind, 1, dr, dealt);
            }
        }
        self.telemetry = tlog;
    }

    fn step_cleanup(&mut self) -> Vec<GameOverEvent> {
        let mut tlog = self.telemetry.take();
        let owner_rings: HashMap<PlayerId, u8> = if tlog.is_some() {
            self.zones
                .iter()
                .map(|(&pid, z)| (pid, Self::ring_for_radius(z.x.hypot(z.y))))
                .collect()
        } else {
            HashMap::new()
        };

        let mut unit_killers: Vec<(PlayerId, PlayerId, u8, u8, u8)> = Vec::new();
        self.units.retain(|u| {
            if u.hp == 0 {
                let dr = Self::ring_for_radius(u.x.hypot(u.y));
                if let Some(tl) = tlog.as_mut() {
                    let sr = if u.last_hit_src == SRC_RAD {
                        1
                    } else {
                        u.last_hit_by
                            .and_then(|k| owner_rings.get(&k).copied())
                            .unwrap_or(0)
                    };
                    tl.log_kill(u.last_hit_src, u.kind, sr, dr);
                }
                if u.last_hit_src != SRC_RAD {
                    if let Some(k) = u.last_hit_by {
                        unit_killers.push((k, u.owner_id, u.kind, u.last_hit_src, dr));
                    }
                }
                false
            } else {
                true
            }
        });

        let hq_levels = self.hq_level_map();
        for (k, victim_id, ukind, killer_src, death_ring) in unit_killers {
            let turret_kill = (20..=22).contains(&killer_src);
            let unit_cost = self.unit_energy_cost(ukind);
            let killer_hq = hq_levels.get(&k).copied().unwrap_or(1);
            let victim_hq = if victim_id == 0 {
                1
            } else {
                hq_levels.get(&victim_id).copied().unwrap_or(1)
            };
            let (t_tier_mult, e_tier_mult) = Self::tier_reward_mult(killer_hq, victim_hq);
            let ring_bonus = if death_ring >= 3 { 1.2 } else { 1.0 };

            let base_e = unit_cost * if turret_kill { 0.55 } else { 0.60 };
            let base_t = unit_cost * if turret_kill { 0.12 } else { 0.32 };
            let base_s = if turret_kill { 8.0 } else { unit_cost * 0.9 };

            let gained_t = (base_t * t_tier_mult * ring_bonus).round();
            let gained_e = (base_e * e_tier_mult * ring_bonus).round();
            if let Some(p) = self.players.get_mut(&k) {
                p.score += (base_s * t_tier_mult * ring_bonus).round().max(1.0) as u32;
                p.kills += 1;
            }
            self.grant_trophies(k, gained_t, killer_hq);
            self.grant_energy(k, gained_e, 1.25, killer_hq);
            if gained_t > 0.0 {
                if let Some(tl) = tlog.as_mut() {
                    tl.log_income(k, 4, gained_t);
                }
            }
        }

        let mut building_destroyers: Vec<(PlayerId, PlayerId, u8, bool, f32)> = Vec::new();
        let mut hq_destroyed_owners: Vec<(PlayerId, Option<PlayerId>, u8)> = Vec::new();
        let mut affected_cap_owners: HashSet<PlayerId> = HashSet::new();
        let mut beacon_destroyed_owners: HashSet<PlayerId> = HashSet::new();
        let mut new_combat_locks: Vec<(PlayerId, BuildCombatLock)> = Vec::new();

        let zone_rings: HashMap<PlayerId, u8> = self
            .zones
            .iter()
            .map(|(&pid, z)| (pid, Self::ring_for_radius(z.x.hypot(z.y))))
            .collect();

        let b_radii: HashMap<u8, f32> = self
            .config
            .buildings
            .values()
            .map(|c| (c.kind_id, c.radius))
            .collect();
        let player_survivals: HashMap<PlayerId, f32> = self
            .players
            .iter()
            .map(|(&pid, p)| (pid, p.survival_sec))
            .collect();
        let player_trophies: HashMap<PlayerId, f32> = self
            .players
            .iter()
            .map(|(&pid, p)| (pid, p.trophies))
            .collect();

        self.buildings.retain(|b| {
            if b.hp == 0 {
                let ring = zone_rings.get(&b.owner_id).copied().unwrap_or(2);
                if let Some(tl) = tlog.as_mut() {
                    let sr = b
                        .last_hit_by
                        .and_then(|k| owner_rings.get(&k).copied())
                        .unwrap_or(0);
                    tl.log_kill(b.last_hit_src, DST_BUILDING_OFFSET + b.kind, sr, ring);
                }
                if let Some(k) = b.last_hit_by {
                    building_destroyers.push((k, b.owner_id, ring, b.kind == 0, b.invested_cost));
                }
                if b.kind == 0 {
                    hq_destroyed_owners.push((b.owner_id, b.last_hit_by, b.last_hit_src));
                } else {
                    let br = b_radii.get(&b.kind).copied().unwrap_or(22.0);
                    let now_sec = player_survivals.get(&b.owner_id).copied().unwrap_or(0.0);
                    new_combat_locks.push((
                        b.owner_id,
                        BuildCombatLock {
                            rel_x: b.rel_x,
                            rel_y: b.rel_y,
                            radius: br + 14.0,
                            expires_at_sec: now_sec + 8.0,
                        },
                    ));
                    affected_cap_owners.insert(b.owner_id);
                    if b.kind == 5 {
                        beacon_destroyed_owners.insert(b.owner_id);
                    }
                }
                false
            } else {
                true
            }
        });

        for (owner_id, lock) in new_combat_locks {
            if let Some(p) = self.players.get_mut(&owner_id) {
                let now_sec = p.survival_sec;
                p.build_combat_locks.retain(|lk| lk.expires_at_sec > now_sec);
                p.build_combat_locks.push(lock);
            }
        }

        for (k, victim_id, ring, is_hq, invested_cost) in building_destroyers {
            let killer_hq = hq_levels.get(&k).copied().unwrap_or(1);
            let victim_hq = hq_levels.get(&victim_id).copied().unwrap_or(1);
            let (t_tier_mult, e_tier_mult) = Self::tier_reward_mult(killer_hq, victim_hq);
            let ring_bonus = if ring >= 3 { 1.2 } else { 1.0 };

            let base_e = if is_hq {
                (invested_cost * 0.60).max(140.0)
            } else {
                (invested_cost * 0.60).max(30.0)
            };
            let base_t = if is_hq {
                let victim_t = player_trophies.get(&victim_id).copied().unwrap_or(0.0);
                80.0 + victim_t * 0.35
            } else {
                24.0 + invested_cost * 0.08
            };
            let base_s = if is_hq { 250.0 } else { 75.0 };

            let gained_t = (base_t * t_tier_mult * ring_bonus).round();
            let gained_e = (base_e * e_tier_mult * ring_bonus).round();
            if let Some(p) = self.players.get_mut(&k) {
                p.score += (base_s * t_tier_mult * ring_bonus).round().max(1.0) as u32;
                p.buildings_destroyed += 1;
            }
            self.grant_trophies(k, gained_t, killer_hq);
            self.grant_energy(k, gained_e, 1.25, killer_hq);
            if let Some(tl) = tlog.as_mut() {
                tl.log_income(k, 4, gained_t);
            }
        }
        for owner_id in beacon_destroyed_owners {
            self.prune_orphaned_buildings(owner_id);
        }
        for owner_id in affected_cap_owners {
            self.recompute_unit_cap(owner_id);
        }

        let mut game_overs = Vec::new();
        for (owner_id, killer_id, fatal_src) in hq_destroyed_owners {
            if let Some(tl) = tlog.as_mut() {
                if let Some(p) = self.players.get(&owner_id) {
                    let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                    let v_ring = zone_rings.get(&owner_id).copied().unwrap_or(1);
                    let v_req = self.equilibrium_radius(owner_id);
                    let k_info = killer_id.and_then(|kid| {
                        self.players.get(&kid).map(|kp| {
                            let k_ring = zone_rings.get(&kid).copied().unwrap_or(0);
                            (kid, kp.name.clone(), kp.is_bot, kp.bot_tier, k_ring)
                        })
                    });
                    tl.log_death(
                        self.tick,
                        sec,
                        json!({
                            "pid": owner_id,
                            "name": p.name,
                            "is_bot": p.is_bot,
                            "tier": p.bot_tier,
                            "arch": p.bot_archetype,
                            "spec": 0,
                            "survival_sec": (p.survival_sec * 10.0).round() / 10.0,
                            "score": p.score,
                            "kills": p.kills,
                            "ring": v_ring,
                            "r_eq": v_req.round(),
                            "killer_id": k_info.as_ref().map(|x| x.0),
                            "killer_name": k_info.as_ref().map(|x| x.1.as_str()),
                            "killer_is_bot": k_info.as_ref().map(|x| x.2),
                            "killer_tier": k_info.as_ref().map(|x| x.3),
                            "killer_spec": 0,
                            "killer_ring": k_info.as_ref().map(|x| x.4),
                            "fatal_src": tag_name(fatal_src),
                        }),
                    );
                }
            }
            if let Some(p) = self.players.get(&owner_id) {
                game_overs.push(GameOverEvent {
                    player_id: p.id,
                    user_id: p.user_id,
                    score: p.score,
                    survival_sec: p.survival_sec.round() as u32,
                    kills: p.kills,
                    buildings_destroyed: p.buildings_destroyed,
                    reason: 1,
                });
            }
            self.remove_player(owner_id);
        }

        self.telemetry = tlog;
        game_overs
    }

    pub fn build_snapshot(&self, for_player: PlayerId) -> Option<ServerMessage> {
        let player = self.players.get(&for_player)?;
        let unit_count = self
            .units
            .iter()
            .filter(|u| u.owner_id == for_player && u.hp > 0 && u.kind != 4)
            .map(|u| self.unit_supply_cost(u.kind))
            .sum::<u16>();
        let r_eq = self.equilibrium_radius(for_player);
        let leader_id = self
            .players
            .values()
            .filter(|p| p.score >= 400)
            .max_by_key(|p| p.score)
            .map(|p| p.id);

        let zones = self
            .zones
            .values()
            .map(|z| {
                let p = self.players.get(&z.player_id);
                let protected = p.map(|pl| pl.spawn_protection_left > 0.0).unwrap_or(false);
                let is_leader = leader_id == Some(z.player_id);
                let mut flags = 0u8;
                if protected {
                    flags |= 1;
                }
                if is_leader {
                    flags |= 2;
                }
                SnapshotZone {
                    player_id: z.player_id,
                    x: z.x,
                    y: z.y,
                    radius: z.radius,
                    team_id: z.team_id,
                    skin_id: z.skin_id,
                    flags,
                }
            })
            .collect();

        let buildings = self
            .buildings
            .iter()
            .map(|b| SnapshotBuilding {
                id: b.id,
                owner_id: b.owner_id,
                kind: b.kind,
                level: b.level,
                mutation: 0,
                meta: b.meta,
                rel_x: b.rel_x,
                rel_y: b.rel_y,
                hp: b.hp,
                max_hp: b.max_hp,
                angle: b.angle,
            })
            .collect();

        let units = self
            .units
            .iter()
            .map(|u| SnapshotUnit {
                id: u.id,
                owner_id: u.owner_id,
                kind: u.kind,
                squad: u.squad,
                x: u.x,
                y: u.y,
                hp: u.hp,
                max_hp: u.max_hp,
            })
            .collect();

        let projectiles = self
            .projectiles
            .iter()
            .map(|p| SnapshotProjectile {
                id: p.id,
                owner_id: p.owner_id,
                kind: p.kind,
                x: p.x,
                y: p.y,
            })
            .collect();

        Some(ServerMessage::Snapshot {
            tick: self.tick,
            score: player.score,
            r_eq,
            energy: player.energy,
            trophies: player.trophies,
            unit_count,
            unit_cap: player.unit_cap,
            teleport_cd_sec: player.teleport_cooldown_left.ceil().clamp(0.0, 255.0) as u8,
            zones,
            buildings,
            units,
            projectiles,
        })
    }

    pub fn build_player_meta(&self) -> ServerMessage {
        let players = self
            .players
            .values()
            .map(|p| PlayerMetaEntry {
                player_id: p.id,
                name: p.name.clone(),
                team_id: 0,
                skin_id: p.skin_id,
            })
            .collect();
        ServerMessage::PlayerMeta { players }
    }

    pub fn build_leaderboard(&self) -> ServerMessage {
        let mut entries: Vec<LeaderboardEntry> = self
            .players
            .values()
            .map(|p| LeaderboardEntry {
                player_id: p.id,
                score: p.score,
            })
            .collect();
        entries.sort_by(|a, b| b.score.cmp(&a.score));
        entries.truncate(10);
        ServerMessage::MatchLeaderboard { entries }
    }
}
