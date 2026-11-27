use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

use rand::Rng;
use serde_json::json;

use crate::protocol::ClientMessage;
use crate::types::{EntityId, PlayerId};

use super::{
    beacon_ext_radius, collect_rhombuses, hex_to_rel, is_large_building, TriCoord, World,
    RING1_RADIUS, RING2_RADIUS,
};

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
    pub barracks_info: Vec<(EntityId, u8, u8)>,
    pub non_hq_buildings: Vec<(EntityId, u8, u8)>,
    pub own_units: usize,
    pub own_combat_power: f32,
    pub defending_unit_power: f32,
    pub enemy_threat_power: f32,
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

    /// Attempts a legal vision-checked teleport to a candidate position `(tx, ty)`.
    fn bot_try_teleport_to_point(&mut self, pid: PlayerId, tx: f32, ty: f32, min_ring_r: f32) -> bool {
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
        let tr = tx.hypot(ty);
        if tr < min_ring_r || tr > (map_r - zone.radius - 30.0).max(min_ring_r) {
            return false;
        }
        if !self.has_vision_at(pid, tx, ty) {
            return false;
        }
        let clear_zones = self.zones.values().all(|other| {
            other.player_id == pid
                || (tx - other.x).hypot(ty - other.y) >= zone.radius + other.radius + 165.0
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
            (tx - (other.x + b.rel_x)).hypot(ty - (other.y + b.rel_y)) >= zone.radius + ext_r + 125.0
        });
        if !clear_beacons {
            return false;
        }
        let r_eq = self.equilibrium_radius(pid);
        let dist = (tx - zone.x).hypot(ty - zone.y);
        if dist < 240.0 {
            return false;
        }
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
        hq_lvl: u8,
        desired_target: Option<(f32, f32)>,
        allow_tactical: bool,
    ) -> bool {
        let Some(p) = self.players.get(&pid) else {
            return false;
        };
        if p.teleport_cooldown_left > 0.0 || p.energy < 220.0 {
            return false;
        }
        let Some(own_z) = self.zones.get(&pid).cloned() else {
            return false;
        };
        let min_ring_r = match hq_lvl {
            3.. => RING2_RADIUS + own_z.radius + 90.0,
            2 => RING1_RADIUS + own_z.radius + 90.0,
            _ => own_z.radius + 120.0,
        };

        // Gather candidate vision anchors from own combat drones
        let drone_pts: Vec<(f32, f32)> = self
            .units
            .iter()
            .filter(|u| u.owner_id == pid && u.hp > 0 && u.kind != 4 && u.squad > 0)
            .map(|u| (u.x, u.y))
            .collect();

        // 1. Tactical forward jump toward a distant target (>850px away) if a drone gives vision near it
        if allow_tactical {
            if let Some((tx, ty)) = desired_target {
                let cur_dist = (tx - own_z.x).hypot(ty - own_z.y);
                if cur_dist > 850.0 {
                    for &(dx, dy) in drone_pts.iter().step_by(2.max(1)) {
                        let d_to_enemy = (dx - tx).hypot(dy - ty);
                        if d_to_enemy >= 280.0 && d_to_enemy <= 560.0 {
                            if self.bot_try_teleport_to_point(pid, dx, dy, min_ring_r) {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        // 2. Ring migration jump if HQ leveled up or bot is stuck in a lower ring than its equilibrium
        let own_r = own_z.x.hypot(own_z.y);
        let own_r_eq = self.equilibrium_radius(pid);
        if own_r_eq >= RING1_RADIUS && own_r < min_ring_r {
            for &(dx, dy) in &drone_pts {
                if dx.hypot(dy) >= min_ring_r {
                    if self.bot_try_teleport_to_point(pid, dx, dy, min_ring_r) {
                        return true;
                    }
                }
            }
            // Fallback: step outward along own vision edge
            let ang = own_z.y.atan2(own_z.x);
            let edge_r = (own_r + 280.0).max(min_ring_r);
            if self.bot_try_teleport_to_point(pid, ang.cos() * edge_r, ang.sin() * edge_r, min_ring_r) {
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
                1 => if threatened || profile.extra_turrets { 2 } else { 1 },
                2 => if threatened || profile.extra_turrets { 3 } else { 2 },
                _ => if threatened || profile.extra_turrets { 4 } else { 3 },
            };
            let target_gens = 2 + usize::from(intel.hq_lvl) * 2 + profile.extra_gens;
            let turret_ratio = if profile.extra_turrets { 2 } else { 3 };
            let need_eco_for_barracks = intel.gens * 5 < intel.barracks * 4 && free_rhombuses > 0;
            let beacon_trophy_cost = (90.0 * 1.65_f32.powi(intel.beacons as i32)).round();

            let build_choice: Option<(u8, f32)> = if intel.turrets < min_turrets && free_hexes > 0 {
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
                Some((1, if intel.gens < usize::from(intel.hq_lvl) * 2 { 104.0 } else { 78.0 }))
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

    pub(super) fn step_bots(&mut self, dt: f32) {
        // Maintain `config.world.bot_count` bots + ring ecosystem quotas (so Ring 1/2 never stay empty
        // when earlier bots graduate to Ring 3).
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
                    match Self::ring_for_radius(z.x.hypot(z.y)) {
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
                .map(|p| (p.unit_cap, p.survival_sec, p.reinforce_squad, p.squads.clone()))
                .unwrap_or((4, 0.0, None, HashMap::new()));

            // Collect per-squad counts & centroids
            let mut garrison_count = 0_usize;
            let mut squad_stats: HashMap<u8, (usize, f32, f32)> = HashMap::new();
            for u in &self.units {
                if u.owner_id != pid || u.hp == 0 || u.kind == 4 {
                    continue;
                }
                if u.squad == 0 {
                    garrison_count += 1;
                } else {
                    let e = squad_stats.entry(u.squad).or_insert((0, 0.0, 0.0));
                    e.0 += 1;
                    e.1 += u.x;
                    e.2 += u.y;
                }
            }
            let mut live_squads: Vec<(u8, usize, f32, f32)> = squad_stats
                .into_iter()
                .map(|(sq, (cnt, sx, sy))| (sq, cnt, sx / cnt as f32, sy / cnt as f32))
                .collect();
            live_squads.sort_unstable_by_key(|&(sq, _, _, _)| sq);

            // 2. Priority Defense: if base is under real attack and local defenders aren't enough
            let under_attack = bot_intel.enemy_threat_power >= 2.5
                && bot_intel.defending_unit_power < bot_intel.enemy_threat_power * 0.9;
            if under_attack {
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
                    for &(sq, _, cx, cy) in &live_squads {
                        let d = (cx - own_z.x).hypot(cy - own_z.y);
                        if d <= 750.0 && closest_sq.is_none_or(|(_, bd)| d < bd) {
                            closest_sq = Some((sq, d));
                        }
                    }
                    if let Some((sq, _)) = closest_sq {
                        let already_heading_home = squad_orders
                            .get(&sq)
                            .map(|o| (o.target_x - own_z.x).hypot(o.target_y - own_z.y) <= own_z.radius + 80.0)
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
                intel_map.insert(pid, bot_intel);
                continue;
            }

            // 3. Update honest Fog-of-War intel (own vision + Top-3 leaders + teleport flashes)
            let own_r = own_z.x.hypot(own_z.y);
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

            // 4. Pick best target by archetype utility
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
                    (kt.turret_power + kt.defending_unit_power * 1.1) * def_mult + hq_base_def;
                let win_ratio = effective_attack / effective_defense;
                let current_primary_order = live_squads
                    .first()
                    .and_then(|&(sq, _, _, _)| squad_orders.get(&sq).copied());
                let is_current_target = current_primary_order
                    .map(|o| (kt.x - o.target_x).hypot(kt.y - o.target_y) <= 240.0)
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
                let dist = (kt.x - own_z.x).hypot(kt.y - own_z.y);
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

            // 5. Honest vision-based tactical teleport (ring upgrade or forward jump)
            let _ = self.bot_try_tactical_teleport(
                pid,
                bot_intel.hq_lvl,
                best_target,
                profile.uses_tactical_tp && bot_intel.enemy_threat_power < 1.0,
            );

            // 6. Multi-squad command dispatch, early scouting, and auto-reinforce
            if let Some((tx, ty)) = best_target {
                let target_dist = (tx - own_z.x).hypot(ty - own_z.y);
                // Direct ALL live combat squads only if the target actually shifted (>120px)
                for &(sq, _, _, _) in &live_squads {
                    let already_on_target = squad_orders
                        .get(&sq)
                        .map(|o| (o.target_x - tx).hypot(o.target_y - ty) <= 120.0)
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
                    let to_ring = Self::ring_for_radius(tx.hypot(ty));
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
                if profile.uses_reinforce && target_dist > 550.0 {
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
                    for &(sq, _, cx, cy) in &live_squads {
                        let need_next_wp = squad_orders
                            .get(&sq)
                            .map(|o| (cx - o.target_x).hypot(cy - o.target_y) <= 170.0)
                            .unwrap_or(true);
                        if need_next_wp {
                            scout_ang = (scout_ang + 1.15).rem_euclid(TAU);
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
                    scout_ang = (scout_ang + 1.15).rem_euclid(TAU);
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

            if let Some(p) = self.players.get_mut(&pid) {
                p.bot_known_intel = known;
                p.bot_scout_angle = scout_ang;
            }

            intel_map.insert(pid, bot_intel);
        }
    }
}
