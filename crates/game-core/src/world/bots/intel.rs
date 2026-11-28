use std::collections::{HashMap, HashSet};

use crate::types::{BotKnownTarget, EntityId, PlayerId, Zone};

use super::super::hex::beacon_ext_radius;
use super::super::World;

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
pub(super) fn bot_has_vision_fast(own_z: &Zone, intel: &BotPlayerIntel, tx: f32, ty: f32) -> bool {
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

impl World {
    /// Aggregates per-player base composition, turret coverage, army strength, and active threats.
    pub(super) fn gather_bot_intel(&self) -> HashMap<PlayerId, BotPlayerIntel> {
        let mut map: HashMap<PlayerId, BotPlayerIntel> = self
            .zones
            .keys()
            .map(|&pid| (pid, BotPlayerIntel::default()))
            .collect();

        for b in &self.buildings {
            if b.owner_id == 0 {
                continue;
            }
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
    pub(super) fn refresh_bot_building_intel(&self, pid: PlayerId, intel: &mut BotPlayerIntel) {
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

    /// Updates honest Fog-of-War intel (own vision + Top-3 leaders + teleport flashes).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bot_update_fog_intel(
        &self,
        pid: PlayerId,
        own_z: &Zone,
        bot_intel: &BotPlayerIntel,
        bot_now_sec: f32,
        top3_leaders: &HashSet<PlayerId>,
        intel_map: &HashMap<PlayerId, BotPlayerIntel>,
        known: &mut HashMap<PlayerId, BotKnownTarget>,
    ) {
        let hq_cfg = self.config.building_by_kind(0);
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
                        let est_hp = hq_cfg
                            .and_then(|c| c.levels.get(usize::from(ez_hq_lvl.saturating_sub(1))))
                            .map(|l| l.hp)
                            .unwrap_or(1800);
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
}
