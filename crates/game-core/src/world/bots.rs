use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

use rand::Rng;
use serde_json::json;

use crate::protocol::ClientMessage;
use crate::types::{BotKnownTarget, EntityId, PlayerId, SquadOrder, Zone};

use super::hex::{
    beacon_ext_radius, hex_to_rel, is_large_building, RING1_RADIUS, RING2_RADIUS,
};
use super::World;

#[derive(Default)]
pub(super) struct BotPlayerIntel {
    pub hq_id: EntityId,
    pub hq_lvl: u8,
    pub hq_hp: u16,
    pub hq_max_hp: u16,
    pub gens: usize,
    pub turrets: usize,
    pub barracks: usize,
    pub beacons: usize,
    pub total_buildings: usize,
    pub turret_power: f32,
    pub gen_positions: Vec<(f32, f32)>,
    pub turret_positions: Vec<(f32, f32)>,
    pub beacon_positions: Vec<(f32, f32)>,
    pub beacon_vision: Vec<(f32, f32, f32)>,
    pub barracks_info: Vec<(EntityId, u8, u8)>,
    pub non_hq_buildings: Vec<(EntityId, u8, u8)>,
    pub own_units: usize,
    pub own_combat_power: f32,
    pub defending_unit_power: f32,
    pub enemy_threat_power: f32,
    pub own_unit_positions: Vec<(f32, f32)>,
    pub squad_unit_positions: Vec<(f32, f32)>,
    pub garrison_count: usize,
    pub live_squads: Vec<(u8, usize, f32, f32)>,
}

#[inline]
fn bot_has_vision_fast(own_z: &Zone, intel: &BotPlayerIntel, tx: f32, ty: f32) -> bool {
    let dx = tx - own_z.x;
    let dy = ty - own_z.y;
    let zr = own_z.radius + 360.0;
    if dx * dx + dy * dy <= zr * zr {
        return true;
    }
    for &(bx, by, vr2) in &intel.beacon_vision {
        let bdx = tx - bx;
        let bdy = ty - by;
        if bdx * bdx + bdy * bdy <= vr2 {
            return true;
        }
    }
    const UNIT_VR2: f32 = 290.0 * 290.0;
    for &(ux, uy) in &intel.own_unit_positions {
        let udx = tx - ux;
        let udy = ty - uy;
        if udx * udx + udy * udy <= UNIT_VR2 {
            return true;
        }
    }
    false
}

struct ArchetypeProfile {
    wave_min: usize,
    wave_cap_pct: usize,
    scout_min: usize,
    min_win_ratio: f32,
    crown_mult: f32,
    finisher_mult: f32,
    exposed_mult: f32,
    gen_loot_weight: f32,
    dist_bias: f32,
    uses_reinforce: bool,
    uses_tactical_tp: bool,
    extra_gens: usize,
    extra_turrets: bool,
    desired_beacons: usize,
}

fn archetype_profile(arch: u8, tier: u8) -> ArchetypeProfile {
    match arch {
        // 0: Старатель — осторожный экономист
        0 => ArchetypeProfile {
            wave_min: 7,
            wave_cap_pct: 85,
            scout_min: 3,
            min_win_ratio: 1.05,
            crown_mult: if tier == 3 { 1.5 } else { 1.2 },
            finisher_mult: 2.0,
            exposed_mult: 1.55,
            gen_loot_weight: 0.30,
            dist_bias: 140.0,
            uses_reinforce: false,
            uses_tactical_tp: false,
            extra_gens: 1,
            extra_turrets: false,
            desired_beacons: 3,
        },
        // 1: Дредноут — тяжёлый танк, охотник на лидера
        1 => ArchetypeProfile {
            wave_min: 8,
            wave_cap_pct: 75,
            scout_min: 3,
            min_win_ratio: 0.78,
            crown_mult: 2.8,
            finisher_mult: 2.1,
            exposed_mult: 1.55,
            gen_loot_weight: 0.28,
            dist_bias: 240.0,
            uses_reinforce: true,
            uses_tactical_tp: true,
            extra_gens: 0,
            extra_turrets: true,
            desired_beacons: 3,
        },
        // 2: Агрессор / Охотник / Рейдер — быстрые частые волны, высокий риск
        2 => ArchetypeProfile {
            wave_min: 4,
            wave_cap_pct: 45,
            scout_min: 2,
            min_win_ratio: 0.62,
            crown_mult: if tier == 3 { 1.65 } else { 1.25 },
            finisher_mult: 2.6,
            exposed_mult: 1.65,
            gen_loot_weight: 0.28,
            dist_bias: 260.0,
            uses_reinforce: true,
            uses_tactical_tp: true,
            extra_gens: 0,
            extra_turrets: false,
            desired_beacons: 3,
        },
        // 3: Мародёр — охотник за оголёнными тылами и генераторами
        3 => ArchetypeProfile {
            wave_min: 5,
            wave_cap_pct: 55,
            scout_min: 2,
            min_win_ratio: 0.74,
            crown_mult: if tier == 3 { 1.55 } else { 1.25 },
            finisher_mult: 2.6,
            exposed_mult: 2.6,
            gen_loot_weight: 0.42,
            dist_bias: 240.0,
            uses_reinforce: true,
            uses_tactical_tp: true,
            extra_gens: 0,
            extra_turrets: false,
            desired_beacons: 3,
        },
        // 4: Архитектор — застройщик, маяки, крепкая база
        _ => ArchetypeProfile {
            wave_min: 7,
            wave_cap_pct: 85,
            scout_min: 3,
            min_win_ratio: 0.85,
            crown_mult: if tier == 3 { 1.65 } else { 1.25 },
            finisher_mult: 2.1,
            exposed_mult: 1.55,
            gen_loot_weight: 0.28,
            dist_bias: 150.0,
            uses_reinforce: true,
            uses_tactical_tp: false,
            extra_gens: 1,
            extra_turrets: true,
            desired_beacons: 4,
        },
    }
}

const BOT_ROSTER: [(u8, u8, &str); 15] = [
    (1, 2, "Малек-1"),
    (1, 3, "Малек-2"),
    (1, 0, "Старатель-1"),
    (1, 2, "Малек-3"),
    (2, 2, "Рейдер-1"),
    (2, 3, "Рейдер-2"),
    (2, 1, "Дредноут-1"),
    (2, 0, "Старатель-2"),
    (2, 3, "Рейдер-3"),
    (3, 4, "Архитектор-1"),
    (3, 2, "Охотник-1"),
    (3, 1, "Дредноут-2"),
    (3, 3, "Мародёр-1"),
    (3, 4, "Архитектор-2"),
    (3, 2, "Охотник-2"),
];

impl World {
    /// Aggregates per-player base composition, turret coverage, army strength, and active threats.
    pub(super) fn gather_bot_intel(&self) -> HashMap<PlayerId, BotPlayerIntel> {
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
                    if let Some(z) = self.zones.get(&b.owner_id) {
                        let vr = beacon_ext_radius(b.level) + 220.0;
                        entry
                            .beacon_vision
                            .push((z.x + b.rel_x, z.y + b.rel_y, vr * vr));
                    }
                }
                _ => {}
            }
            if b.kind != 0 {
                entry.non_hq_buildings.push((b.id, b.kind, b.level));
            }
        }

        let bot_zones: Vec<(PlayerId, f32, f32, f32, f32)> = self
            .players
            .values()
            .filter(|p| p.is_bot)
            .filter_map(|p| {
                self.zones.get(&p.id).map(|z| {
                    let perim_r = z.radius + 230.0;
                    (p.id, z.x, z.y, perim_r, perim_r * perim_r)
                })
            })
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
                    let dx = u.x - oz.x;
                    let dy = u.y - oz.y;
                    let def_r = oz.radius + 260.0;
                    if dx * dx + dy * dy <= def_r * def_r {
                        entry.defending_unit_power += power;
                    }
                }
                if u.kind != 4 {
                    entry.own_unit_positions.push((u.x, u.y));
                    if u.squad == 0 {
                        entry.garrison_count += 1;
                    } else {
                        entry.squad_unit_positions.push((u.x, u.y));
                        if let Some(sq) = entry.live_squads.iter_mut().find(|s| s.0 == u.squad) {
                            sq.1 += 1;
                            sq.2 += u.x;
                            sq.3 += u.y;
                        } else {
                            entry.live_squads.push((u.squad, 1, u.x, u.y));
                        }
                    }
                }
            }
            for &(bpid, zx, zy, perim_r, perim_r2) in &bot_zones {
                if u.owner_id != bpid {
                    let dx = u.x - zx;
                    let dy = u.y - zy;
                    if dx.abs() <= perim_r
                        && dy.abs() <= perim_r
                        && dx * dx + dy * dy <= perim_r2
                    {
                        if let Some(entry) = map.get_mut(&bpid) {
                            entry.enemy_threat_power += power;
                        }
                    }
                }
            }
        }

        for entry in map.values_mut() {
            if !entry.live_squads.is_empty() {
                for sq in &mut entry.live_squads {
                    let inv = 1.0 / sq.1 as f32;
                    sq.2 *= inv;
                    sq.3 *= inv;
                }
                entry.live_squads.sort_unstable_by_key(|&(sq, _, _, _)| sq);
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
        intel.beacon_vision.clear();
        intel.barracks_info.clear();
        intel.non_hq_buildings.clear();

        let own_zone_pos = self.zones.get(&pid).map(|z| (z.x, z.y));

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
                    if let Some((zx, zy)) = own_zone_pos {
                        let vr = beacon_ext_radius(b.level) + 220.0;
                        intel
                            .beacon_vision
                            .push((zx + b.rel_x, zy + b.rel_y, vr * vr));
                    }
                }
                _ => {}
            }
            if b.kind != 0 {
                intel.non_hq_buildings.push((b.id, b.kind, b.level));
            }
        }
    }

    /// Attempts a legal vision-checked teleport to a candidate position `(tx, ty)`.
    fn bot_try_teleport_to_point(
        &mut self,
        pid: PlayerId,
        zone: &Zone,
        intel: &BotPlayerIntel,
        tx: f32,
        ty: f32,
        min_ring_r: f32,
    ) -> bool {
        let Some(p) = self.players.get(&pid) else {
            return false;
        };
        if p.teleport_cooldown_left > 0.0 {
            return false;
        }
        let energy = p.energy;
        let map_r = self.config.world.map_radius;
        let tr = (tx * tx + ty * ty).sqrt();
        if tr < min_ring_r || tr > (map_r - zone.radius - 30.0).max(min_ring_r) {
            return false;
        }
        if !bot_has_vision_fast(zone, intel, tx, ty) {
            return false;
        }
        let clear_zones = self.zones.values().all(|other| {
            if other.player_id == pid {
                return true;
            }
            let dx = tx - other.x;
            let dy = ty - other.y;
            let min_d = zone.radius + other.radius + 165.0;
            dx * dx + dy * dy >= min_d * min_d
        });
        if !clear_zones {
            return false;
        }
        let clear_beacons = self.buildings.iter().all(|b| {
            if b.owner_id == pid || b.kind != 5 {
                return true;
            }
            let Some(other) = self.zones.get(&b.owner_id) else {
                return true;
            };
            let ext_r = beacon_ext_radius(b.level);
            let dx = tx - (other.x + b.rel_x);
            let dy = ty - (other.y + b.rel_y);
            let min_d = zone.radius + ext_r + 125.0;
            dx * dx + dy * dy >= min_d * min_d
        });
        if !clear_beacons {
            return false;
        }
        let r_eq = self.equilibrium_radius(pid);
        let dx = tx - zone.x;
        let dy = ty - zone.y;
        let dist2 = dx * dx + dy * dy;
        if dist2 < 240.0 * 240.0 {
            return false;
        }
        let dist = dist2.sqrt();
        let ring_pen = if tr < r_eq {
            self.config.world.teleport_ring_penalty * 3.0
        } else {
            self.config.world.teleport_ring_penalty
        };
        let est_cost = self.config.world.teleport_base_cost
            + dist * self.config.world.teleport_dist_factor
            + (tr - r_eq).abs() * ring_pen;
        if energy < est_cost + 35.0 {
            return false;
        }
        self.apply_command(
            pid,
            ClientMessage::TeleportZone {
                target_x: tx,
                target_y: ty,
            },
        )
        .is_none()
    }

    /// Tries to teleport the bot using its own drones' vision: either to a higher ring or closer to a distant target.
    fn bot_try_tactical_teleport(
        &mut self,
        pid: PlayerId,
        own_z: &Zone,
        own_r: f32,
        intel: &BotPlayerIntel,
        desired_target: Option<(f32, f32)>,
        allow_tactical: bool,
    ) -> bool {
        let Some(p) = self.players.get(&pid) else {
            return false;
        };
        if p.teleport_cooldown_left > 0.0 || p.energy < 220.0 {
            return false;
        }
        let min_ring_r = match intel.hq_lvl {
            3.. => RING2_RADIUS + own_z.radius + 90.0,
            2 => RING1_RADIUS + own_z.radius + 90.0,
            _ => own_z.radius + 120.0,
        };

        // 1. Tactical forward jump toward a distant target (>850px away) if a drone gives vision near it
        if allow_tactical {
            if let Some((tx, ty)) = desired_target {
                let dtx = tx - own_z.x;
                let dty = ty - own_z.y;
                if dtx * dtx + dty * dty > 850.0 * 850.0 {
                    for &(dx, dy) in intel.squad_unit_positions.iter().step_by(2) {
                        let ex = dx - tx;
                        let ey = dy - ty;
                        let e2 = ex * ex + ey * ey;
                        if (280.0 * 280.0..=560.0 * 560.0).contains(&e2)
                            && self.bot_try_teleport_to_point(pid, own_z, intel, dx, dy, min_ring_r)
                        {
                            return true;
                        }
                    }
                }
            }
        }

        // 2. Ring migration jump if HQ leveled up or bot is stuck in a lower ring than its equilibrium
        let own_r_eq = self.equilibrium_radius(pid);
        if own_r_eq >= RING1_RADIUS && own_r < min_ring_r {
            let min_ring_r2 = min_ring_r * min_ring_r;
            for &(dx, dy) in &intel.squad_unit_positions {
                if dx * dx + dy * dy >= min_ring_r2
                    && self.bot_try_teleport_to_point(pid, own_z, intel, dx, dy, min_ring_r)
                {
                    return true;
                }
            }
            // Fallback: step outward along own vision edge
            let ang = own_z.y.atan2(own_z.x);
            let edge_r = (own_r + 280.0).max(min_ring_r);
            if self.bot_try_teleport_to_point(
                pid,
                own_z,
                intel,
                ang.cos() * edge_r,
                ang.sin() * edge_r,
                min_ring_r,
            ) {
                return true;
            }
        }
        false
    }

    /// Configures barrack unit types by archetype for a balanced Raider/Archer/Titan mix.
    fn bot_configure_barracks(
        &mut self,
        pid: PlayerId,
        arch: u8,
        barracks_info: &[(EntityId, u8, u8)],
    ) {
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

    #[inline]
    fn bot_free_slot_counts(&mut self, pid: PlayerId) -> (usize, usize) {
        let cache = self.ensure_base_grid(pid);
        (cache.free_hexes.len(), cache.free_rhombuses.len())
    }

    /// Picks a tactically optimal free hex or rhombus coordinate for `build_kind`.
    fn bot_pick_build_slot(
        &mut self,
        pid: PlayerId,
        build_kind: u8,
        gen_aura_r: f32,
        intel: &BotPlayerIntel,
    ) -> Option<(f32, f32)> {
        let _ = self.ensure_base_grid(pid);
        let Some(cache) = self.base_grid_cache.get(&pid) else {
            return None;
        };
        let rng = &mut self.rng;

        let aura_r = (gen_aura_r - 4.0).max(40.0);
        let aura_r2 = aura_r * aura_r;

        if is_large_building(build_kind) {
            let mut best: Option<((f32, f32), f32)> = None;
            for &(q, r) in &cache.free_hexes {
                let (cx, cy) = hex_to_rel(q, r);
                let dist_hq2 = cx * cx + cy * cy;
                let jitter = rng.gen_range(0.0..6.0);
                let score = if build_kind == 2 {
                    let dist_hq = dist_hq2.sqrt();
                    let aura_bonus = if intel.gen_positions.iter().any(|&(gx, gy)| {
                        let dx = cx - gx;
                        let dy = cy - gy;
                        dx * dx + dy * dy <= aura_r2
                    }) {
                        120.0
                    } else {
                        0.0
                    };
                    let min_t_dist2 = intel
                        .turret_positions
                        .iter()
                        .map(|&(tx, ty)| {
                            let dx = cx - tx;
                            let dy = cy - ty;
                            dx * dx + dy * dy
                        })
                        .fold(140.0_f32 * 140.0, f32::min);
                    let ring_pref = -((dist_hq - 86.0).abs() * 0.45);
                    aura_bonus + ring_pref + min_t_dist2.sqrt() * 0.3 + jitter
                } else {
                    let outer_pref = if dist_hq2 >= 55.0 * 55.0 { 24.0 } else { 0.0 };
                    outer_pref + jitter
                };
                if best.as_ref().is_none_or(|&(_, bs)| score > bs) {
                    best = Some(((cx, cy), score));
                }
            }
            best.map(|(pos, _)| pos)
        } else {
            let unbuffed_turrets: Vec<(f32, f32)> = if build_kind != 5 {
                intel
                    .turret_positions
                    .iter()
                    .copied()
                    .filter(|&(tx, ty)| {
                        !intel.gen_positions.iter().any(|&(gx, gy)| {
                            let dx = gx - tx;
                            let dy = gy - ty;
                            dx * dx + dy * dy <= aura_r2
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };

            let mut best: Option<((f32, f32), f32)> = None;
            for &(tris, (mx, my)) in &cache.free_rhombuses {
                let dist_hq = (mx * mx + my * my).sqrt();
                let jitter = rng.gen_range(0.0..5.0);
                let score = if build_kind == 5 {
                    let min_b_dist2 = intel
                        .beacon_positions
                        .iter()
                        .map(|&(bx, by)| {
                            let dx = mx - bx;
                            let dy = my - by;
                            dx * dx + dy * dy
                        })
                        .fold(f32::MAX, f32::min);
                    let spacing_bonus = if min_b_dist2 >= 82.0 * 82.0 {
                        350.0
                    } else {
                        min_b_dist2.sqrt() * 2.0
                    };
                    dist_hq * 1.4 + spacing_bonus + jitter
                } else {
                    let turret_buff = unbuffed_turrets
                        .iter()
                        .filter(|&&(tx, ty)| {
                            let dx = mx - tx;
                            let dy = my - ty;
                            dx * dx + dy * dy <= aura_r2
                        })
                        .count() as f32
                        * 115.0;
                    let mut shared_hex_bonus = 0.0_f32;
                    for (hq_q, hq_r, _) in tris {
                        if (0..6_u8).any(|s| cache.occupied_tris.contains(&(hq_q, hq_r, s))) {
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
        }
    }

    /// Executes up to `max_actions` prioritized upgrades and builds for a single bot think tick.
    /// All bots can naturally grow from HQ L1 -> L2 -> L3 just like human players.
    fn bot_step_economy(
        &mut self,
        pid: PlayerId,
        tier: u8,
        arch: u8,
        gen_aura_r: f32,
        intel: &mut BotPlayerIntel,
    ) {
        let profile = archetype_profile(arch, tier);
        let max_actions = match tier {
            1 => 2,
            2 => 3,
            _ => 5,
        };
        let max_b_lvl: u8 = 3;

        for _ in 0..max_actions {
            let (energy, trophies, _in_combat) = self
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
            let (free_hexes, free_rhombuses) = self.bot_free_slot_counts(pid);

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
                    if let Some((bx, by)) = self.bot_pick_build_slot(pid, bk, gen_aura_r, intel) {
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

            // 3. Evaluate best affordable upgrade candidate (same rules as human players — no combat lock)
            let mut best_upgrade: Option<(EntityId, f32)> = None;
            for &(bid, kind, lvl) in &intel.non_hq_buildings {
                if lvl >= max_b_lvl || lvl >= intel.hq_lvl {
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
                            88.0
                        }
                    }
                    3 => {
                        if lvl == 1 {
                            115.0
                        } else {
                            98.0
                        }
                    }
                    1 => {
                        if !threatened && energy < 700.0 {
                            112.0
                        } else {
                            78.0
                        }
                    }
                    5 => {
                        if free_hexes <= 2 || energy >= 600.0 {
                            75.0
                        } else {
                            30.0
                        }
                    }
                    _ => 50.0,
                };
                if best_upgrade.as_ref().is_none_or(|&(_, bs)| score > bs) {
                    best_upgrade = Some((bid, score));
                }
            }

            // 4. Full-base expansion: ZERO artificial hard caps — bots fill all free hexes & rhombuses
            //    using only soft archetype ratios/priorities.
            let min_turrets = match intel.hq_lvl {
                1 => {
                    if threatened || profile.extra_turrets {
                        2
                    } else {
                        1
                    }
                }
                2 => {
                    if threatened || profile.extra_turrets {
                        3
                    } else {
                        2
                    }
                }
                _ => {
                    if threatened || profile.extra_turrets {
                        4
                    } else {
                        3
                    }
                }
            };
            let target_gens = 2 + usize::from(intel.hq_lvl) * 2 + profile.extra_gens;
            let turret_ratio = if profile.extra_turrets { 2 } else { 3 };
            let need_eco_for_barracks = intel.gens * 5 < intel.barracks * 4 && free_rhombuses > 0;
            let beacon_trophy_cost = (90.0 * 1.65_f32.powi(intel.beacons as i32)).round();

            let build_choice: Option<(u8, f32)> = if intel.turrets < min_turrets && free_hexes > 0
            {
                Some((2, if threatened { 145.0 } else { 122.0 }))
            } else if need_eco_for_barracks {
                Some((1, 118.0))
            } else if intel.hq_lvl >= 3
                && free_rhombuses > 0
                && (free_hexes <= 3 || intel.beacons < profile.desired_beacons)
                && (intel.barracks >= 2 || free_hexes <= 3)
                && trophies >= beacon_trophy_cost
            {
                Some((5, if free_hexes <= 2 { 116.0 } else { 96.0 }))
            } else if intel.gens < target_gens
                && free_rhombuses > 0
                && (free_hexes >= 2 || intel.barracks >= 2)
            {
                Some((
                    1,
                    if intel.gens < usize::from(intel.hq_lvl) * 2 {
                        104.0
                    } else {
                        78.0
                    },
                ))
            } else if free_hexes > 0 {
                let want_turret = intel.turrets * turret_ratio < intel.barracks;
                Some((if want_turret { 2 } else { 3 }, 94.0))
            } else if free_rhombuses > 0 {
                Some((1, 72.0))
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
                if let Some((bx, by)) = self.bot_pick_build_slot(pid, bk, gen_aura_r, intel) {
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

    /// Maintains `config.world.bot_count` bots + Ring 1/Ring 2 ecosystem quotas.
    fn bot_maintain_roster(&mut self) {
        let target_bot_count = usize::from(self.config.world.bot_count);
        let cur_bot_count = self.players.values().filter(|p| p.is_bot).count();
        if cur_bot_count < target_bot_count {
            for &(tier, arch, name) in BOT_ROSTER.iter().take(target_bot_count) {
                if self.players.values().filter(|p| p.is_bot).count() >= target_bot_count {
                    break;
                }
                if !self.players.values().any(|p| p.is_bot && p.name == name) {
                    self.add_player_internal(name.to_string(), None, 0, tier, arch);
                }
            }
        }
        // Keep at least 3 bots in Ring 1 and 3 in Ring 2 even if all starters grew to Ring 3 (up to +4 overflow)
        let max_with_overflow = target_bot_count + 4;
        if self.players.values().filter(|p| p.is_bot).count() < max_with_overflow {
            let mut r1_bots = 0_usize;
            let mut r2_bots = 0_usize;
            for p in self.players.values().filter(|p| p.is_bot) {
                if let Some(z) = self.zones.get(&p.id) {
                    match Self::ring_for_radius((z.x * z.x + z.y * z.y).sqrt()) {
                        1 => r1_bots += 1,
                        2 => r2_bots += 1,
                        _ => {}
                    }
                }
            }
            if r1_bots < 3 {
                let arch = [2_u8, 3, 0][self.rng.gen_range(0..3)];
                let num = self.next_player_id;
                self.add_player_internal(format!("Новичок-{num}"), None, 0, 1, arch);
            } else if r2_bots < 3 {
                let arch = [2_u8, 3, 1][self.rng.gen_range(0..3)];
                let num = self.next_player_id;
                self.add_player_internal(format!("Корсар-{num}"), None, 0, 2, arch);
            }
        }
    }

    /// Priority defense: recalls nearby combat squad and cancels auto-reinforce if base is under threat.
    fn bot_handle_defense(
        &mut self,
        pid: PlayerId,
        bot_intel: &BotPlayerIntel,
        own_z: &Zone,
        reinforce_sq: Option<u8>,
        live_squads: &[(u8, usize, f32, f32)],
        squad_orders: &HashMap<u8, SquadOrder>,
    ) -> bool {
        let under_attack = bot_intel.enemy_threat_power >= 2.5
            && bot_intel.defending_unit_power < bot_intel.enemy_threat_power * 0.9;
        if !under_attack {
            return false;
        }
        if reinforce_sq.is_some() {
            let _ = self.apply_command(
                pid,
                ClientMessage::ArmyCommand {
                    squad: 0,
                    mode: 2,
                    target_x: 0.0,
                    target_y: 0.0,
                },
            );
        }
        // Ensure garrison stays on patrol around base
        let _ = self.apply_command(
            pid,
            ClientMessage::ArmyCommand {
                squad: 0,
                mode: 0,
                target_x: 0.0,
                target_y: 0.0,
            },
        );
        // Only recall a nearby combat squad if the threat is serious (>= 3.5 power)
        if bot_intel.enemy_threat_power >= 3.5 {
            let mut closest_sq: Option<(u8, f32)> = None;
            for &(sq, _, cx, cy) in live_squads {
                let dx = cx - own_z.x;
                let dy = cy - own_z.y;
                let d2 = dx * dx + dy * dy;
                if d2 <= 750.0 * 750.0 && closest_sq.is_none_or(|(_, bd2)| d2 < bd2) {
                    closest_sq = Some((sq, d2));
                }
            }
            if let Some((sq, _)) = closest_sq {
                let home_r = own_z.radius + 80.0;
                let home_r2 = home_r * home_r;
                let already_heading_home = squad_orders
                    .get(&sq)
                    .map(|o| {
                        let dx = o.target_x - own_z.x;
                        let dy = o.target_y - own_z.y;
                        dx * dx + dy * dy <= home_r2
                    })
                    .unwrap_or(false);
                if !already_heading_home {
                    let _ = self.apply_command(
                        pid,
                        ClientMessage::ArmyCommand {
                            squad: sq,
                            mode: 1,
                            target_x: own_z.x,
                            target_y: own_z.y,
                        },
                    );
                }
            }
        }
        true
    }

    /// Updates honest Fog-of-War intel (own vision + Top-3 leaders + teleport flashes).
    #[allow(clippy::too_many_arguments)]
    fn bot_update_fog_intel(
        &self,
        pid: PlayerId,
        own_z: &Zone,
        bot_intel: &BotPlayerIntel,
        bot_now_sec: f32,
        top3_leaders: &HashSet<PlayerId>,
        intel_map: &HashMap<PlayerId, BotPlayerIntel>,
        known: &mut HashMap<PlayerId, BotKnownTarget>,
    ) {
        for z in self.zones.values() {
            if z.player_id == pid {
                continue;
            }
            let Some(tp) = self.players.get(&z.player_id) else {
                continue;
            };
            let zdx = z.x - own_z.x;
            let zdy = z.y - own_z.y;
            let combined_r = own_z.radius + z.radius + 360.0;
            let in_vision = zdx * zdx + zdy * zdy <= combined_r * combined_r
                || bot_has_vision_fast(own_z, bot_intel, z.x, z.y);
            let ez_intel = intel_map.get(&z.player_id);
            let ez_hq_lvl = ez_intel.map(|e| e.hq_lvl).unwrap_or(1);

            if in_vision {
                let ez_hq_max_hp = ez_intel.map(|e| e.hq_max_hp).unwrap_or(1800);
                known.insert(
                    z.player_id,
                    BotKnownTarget {
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
                            BotKnownTarget {
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
            self.zones.contains_key(target_id) && (bot_now_sec - kt.observed_at_sec) <= 45.0
        });
    }

    /// Picks best attack target by archetype utility and returns `(garrison_ready, best_target)`.
    #[allow(clippy::too_many_arguments)]
    fn bot_select_target(
        &self,
        arch: u8,
        profile: &ArchetypeProfile,
        own_z: &Zone,
        unit_cap: u16,
        garrison_count: usize,
        bot_intel: &BotPlayerIntel,
        known: &HashMap<PlayerId, BotKnownTarget>,
        live_squads: &[(u8, usize, f32, f32)],
        squad_orders: &HashMap<u8, SquadOrder>,
        leader_id: Option<PlayerId>,
    ) -> (bool, Option<(f32, f32)>) {
        let wave_threshold = ((usize::from(unit_cap) * profile.wave_cap_pct) / 100)
            .max(profile.wave_min)
            .min(usize::from(unit_cap).max(1));
        let garrison_ready = garrison_count >= wave_threshold;
        let full_cap_push = garrison_count >= (usize::from(unit_cap) * 8) / 10
            || bot_intel.own_units >= (usize::from(unit_cap) * 8) / 10;
        let min_win_ratio = if full_cap_push {
            (profile.min_win_ratio * 0.85).max(0.55)
        } else {
            profile.min_win_ratio
        };
        let active_combat_power = if garrison_ready {
            bot_intel
                .defending_unit_power
                .max(bot_intel.own_combat_power * 0.65)
        } else {
            bot_intel.own_combat_power
        };

        let mut best_target: Option<(f32, f32)> = None;
        let mut best_utility = f32::NEG_INFINITY;

        for (&target_id, kt) in known {
            let Some(tp) = self.players.get(&target_id) else {
                continue;
            };
            let Some(cur_z) = self.zones.get(&target_id) else {
                continue;
            };
            let currently_visible = bot_has_vision_fast(own_z, bot_intel, cur_z.x, cur_z.y);
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
                (kt.turret_power + kt.defending_unit_power * 1.1) * def_mult + hq_base_def;
            let win_ratio = effective_attack / effective_defense;
            let current_primary_order = live_squads
                .first()
                .and_then(|&(sq, _, _, _)| squad_orders.get(&sq).copied());
            let is_current_target = current_primary_order
                .map(|o| {
                    let dx = kt.x - o.target_x;
                    let dy = kt.y - o.target_y;
                    dx * dx + dy * dy <= 240.0 * 240.0
                })
                .unwrap_or(false);
            let eff_min_win = if is_current_target {
                (min_win_ratio * 0.72).max(0.45)
            } else {
                min_win_ratio
            };
            if win_ratio < eff_min_win {
                continue;
            }

            let ez_hq_ratio = f32::from(kt.hq_hp) / f32::from(kt.hq_max_hp.max(1));
            let (raw_reward_mult, _) = Self::tier_reward_mult(bot_intel.hq_lvl, kt.hq_lvl);
            let reward_weight = if arch == 2 {
                0.85_f32.max(raw_reward_mult)
            } else {
                raw_reward_mult
            };
            let crown_mult = if leader_id == Some(target_id) {
                profile.crown_mult
            } else {
                1.0
            };
            let finisher_bonus = if ez_hq_ratio < 0.60 {
                profile.finisher_mult
            } else {
                1.0
            };
            let exposed_bonus = if kt.is_full_intel && kt.defending_unit_power < 1.2 {
                profile.exposed_mult
            } else {
                1.0
            };
            let sticky_bonus = if is_current_target { 1.55 } else { 1.0 };
            let loot_value = 1.0
                + f32::from(kt.gens) * profile.gen_loot_weight
                + (kt.trophies.min(400.0) / 400.0) * 0.45;
            let dx = kt.x - own_z.x;
            let dy = kt.y - own_z.y;
            let dist = (dx * dx + dy * dy).sqrt();
            let utility = (reward_weight
                * crown_mult
                * finisher_bonus
                * exposed_bonus
                * sticky_bonus
                * loot_value
                * win_ratio.min(2.6))
                / (dist + profile.dist_bias);
            if utility > best_utility {
                best_utility = utility;
                best_target = Some((kt.x, kt.y));
            }
        }

        (garrison_ready, best_target)
    }

    /// Dispatches multi-squad attack orders, early scouting, and auto-reinforce.
    #[allow(clippy::too_many_arguments)]
    fn bot_dispatch_orders(
        &mut self,
        pid: PlayerId,
        tier: u8,
        arch: u8,
        profile: &ArchetypeProfile,
        own_z: &Zone,
        own_r: f32,
        own_r_eq: f32,
        garrison_count: usize,
        garrison_ready: bool,
        reinforce_sq: Option<u8>,
        live_squads: &[(u8, usize, f32, f32)],
        squad_orders: &HashMap<u8, SquadOrder>,
        best_target: Option<(f32, f32)>,
        scout_ang: &mut f32,
    ) {
        if let Some((tx, ty)) = best_target {
            let tdx = tx - own_z.x;
            let tdy = ty - own_z.y;
            let target_dist2 = tdx * tdx + tdy * tdy;
            // Direct ALL live combat squads only if the target actually shifted (>120px)
            for &(sq, _, _, _) in live_squads {
                let already_on_target = squad_orders
                    .get(&sq)
                    .map(|o| {
                        let dx = o.target_x - tx;
                        let dy = o.target_y - ty;
                        dx * dx + dy * dy <= 120.0 * 120.0
                    })
                    .unwrap_or(false);
                if !already_on_target {
                    let _ = self.apply_command(
                        pid,
                        ClientMessage::ArmyCommand {
                            squad: sq,
                            mode: 1,
                            target_x: tx,
                            target_y: ty,
                        },
                    );
                }
            }
            // Launch garrison if ready
            if garrison_ready {
                let from_ring = Self::ring_for_radius(own_r);
                let to_ring = Self::ring_for_radius((tx * tx + ty * ty).sqrt());
                let _ = self.apply_command(
                    pid,
                    ClientMessage::ArmyCommand {
                        squad: 0,
                        mode: 1,
                        target_x: tx,
                        target_y: ty,
                    },
                );
                if let Some(tlog) = self.telemetry.as_mut() {
                    let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                    tlog.log_milestone(
                        self.tick,
                        sec,
                        pid,
                        "bot_rally",
                        json!({"tier": tier, "arch": arch, "from_ring": from_ring, "to_ring": to_ring, "units": garrison_count}),
                    );
                }
            }
            // Manage auto-reinforce for long-range assaults
            let main_sq = self
                .players
                .get(&pid)
                .and_then(|p| p.squads.keys().copied().min());
            if profile.uses_reinforce && target_dist2 > 550.0 * 550.0 {
                if let Some(msq) = main_sq {
                    if reinforce_sq != Some(msq) {
                        let _ = self.apply_command(
                            pid,
                            ClientMessage::ArmyCommand {
                                squad: msq,
                                mode: 2,
                                target_x: 0.0,
                                target_y: 0.0,
                            },
                        );
                    }
                }
            } else if reinforce_sq.is_some() {
                let _ = self.apply_command(
                    pid,
                    ClientMessage::ArmyCommand {
                        squad: 0,
                        mode: 2,
                        target_x: 0.0,
                        target_y: 0.0,
                    },
                );
            }
        } else {
            // No valid attack target: disable auto-reinforce and run ring scouting
            if reinforce_sq.is_some() {
                let _ = self.apply_command(
                    pid,
                    ClientMessage::ArmyCommand {
                        squad: 0,
                        mode: 2,
                        target_x: 0.0,
                        target_y: 0.0,
                    },
                );
            }
            let scout_r = own_r_eq.max(own_z.radius + 320.0);
            if !live_squads.is_empty() {
                for &(sq, _, cx, cy) in live_squads {
                    let need_next_wp = squad_orders
                        .get(&sq)
                        .map(|o| {
                            let dx = cx - o.target_x;
                            let dy = cy - o.target_y;
                            dx * dx + dy * dy <= 170.0 * 170.0
                        })
                        .unwrap_or(true);
                    if need_next_wp {
                        *scout_ang = (*scout_ang + 1.15).rem_euclid(TAU);
                        let _ = self.apply_command(
                            pid,
                            ClientMessage::ArmyCommand {
                                squad: sq,
                                mode: 1,
                                target_x: scout_ang.cos() * scout_r,
                                target_y: scout_ang.sin() * scout_r,
                            },
                        );
                    }
                }
            } else if garrison_count >= profile.scout_min {
                *scout_ang = (*scout_ang + 1.15).rem_euclid(TAU);
                let _ = self.apply_command(
                    pid,
                    ClientMessage::ArmyCommand {
                        squad: 0,
                        mode: 1,
                        target_x: scout_ang.cos() * scout_r,
                        target_y: scout_ang.sin() * scout_r,
                    },
                );
            }
        }
    }

    pub(super) fn step_bots(&mut self, dt: f32) {
        self.bot_maintain_roster();

        let bot_ids: Vec<(PlayerId, u8, u8)> = self
            .players
            .values()
            .filter(|p| p.is_bot)
            .map(|p| (p.id, p.bot_tier, p.bot_archetype))
            .collect();

        let mut ready_bots: Vec<(PlayerId, u8, u8)> = Vec::new();
        for (pid, tier, arch) in bot_ids {
            let think_cd = match tier {
                1 => 1.1,
                3 => 0.45,
                _ => 0.72,
            };
            if let Some(p) = self.players.get_mut(&pid) {
                p.bot_think_cooldown -= dt;
                if p.bot_think_cooldown <= 0.0 {
                    p.bot_think_cooldown = think_cd * self.rng.gen_range(0.92..1.08);
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
            let profile = archetype_profile(arch, tier);

            // 1. Configure barracks mix & run economy/upgrades (bots naturally grow L1->L2->L3)
            if !bot_intel.barracks_info.is_empty() {
                self.bot_configure_barracks(pid, arch, &bot_intel.barracks_info);
            }
            self.bot_step_economy(pid, tier, arch, gen_aura_r, &mut bot_intel);

            let Some(own_z) = self.zones.get(&pid).cloned() else {
                intel_map.insert(pid, bot_intel);
                continue;
            };
            let (unit_cap, bot_now_sec, reinforce_sq, squad_orders) = self
                .players
                .get(&pid)
                .map(|p| {
                    (
                        p.unit_cap,
                        p.survival_sec,
                        p.reinforce_squad,
                        p.squads.clone(),
                    )
                })
                .unwrap_or((4, 0.0, None, HashMap::new()));

            // 2. Priority Defense
            if self.bot_handle_defense(
                pid,
                &bot_intel,
                &own_z,
                reinforce_sq,
                &bot_intel.live_squads,
                &squad_orders,
            ) {
                intel_map.insert(pid, bot_intel);
                continue;
            }

            // 3. Update honest Fog-of-War intel
            let own_r = (own_z.x * own_z.x + own_z.y * own_z.y).sqrt();
            let own_r_eq = self.equilibrium_radius(pid);
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

            self.bot_update_fog_intel(
                pid,
                &own_z,
                &bot_intel,
                bot_now_sec,
                &top3_leaders,
                &intel_map,
                &mut known,
            );

            // 4. Pick best target by archetype utility
            let (garrison_ready, best_target) = self.bot_select_target(
                arch,
                &profile,
                &own_z,
                unit_cap,
                bot_intel.garrison_count,
                &bot_intel,
                &known,
                &bot_intel.live_squads,
                &squad_orders,
                leader_id,
            );

            // 5. Honest vision-based tactical teleport (ring upgrade or forward jump)
            let _ = self.bot_try_tactical_teleport(
                pid,
                &own_z,
                own_r,
                &bot_intel,
                best_target,
                profile.uses_tactical_tp && bot_intel.enemy_threat_power < 1.0,
            );

            // 6. Multi-squad command dispatch, early scouting, and auto-reinforce
            self.bot_dispatch_orders(
                pid,
                tier,
                arch,
                &profile,
                &own_z,
                own_r,
                own_r_eq,
                bot_intel.garrison_count,
                garrison_ready,
                reinforce_sq,
                &bot_intel.live_squads,
                &squad_orders,
                best_target,
                &mut scout_ang,
            );

            if let Some(p) = self.players.get_mut(&pid) {
                p.bot_known_intel = known;
                p.bot_scout_angle = scout_ang;
            }

            intel_map.insert(pid, bot_intel);
        }
    }
}
