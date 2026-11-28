use std::collections::HashMap;
use std::f32::consts::TAU;

use crate::telemetry::{SRC_TURRET_1, SRC_TURRET_2, SRC_TURRET_3};
use crate::types::{EntityId, PlayerId, Projectile};

use super::super::World;

impl World {
    pub(in super::super) fn step_turrets(&mut self, dt: f32) {
        let mut new_projectiles: Vec<Projectile> = Vec::new();
        let mut player_energy: HashMap<PlayerId, f32> = self
            .players
            .iter()
            .map(|(&id, p)| (id, p.energy))
            .collect();
        // Track in-flight projectile damage per unit to prevent overkill energy waste
        let mut incoming_dmg: HashMap<EntityId, u32> = HashMap::new();
        for p in &self.projectiles {
            if !p.target_is_building && p.target_id != 0 {
                *incoming_dmg.entry(p.target_id).or_insert(0) += u32::from(p.damage);
            }
        }

        {
            let config = &self.config;
            let zones = &self.zones;
            let players = &self.players;
            let units = &self.units;
            let unit_grid = &self.unit_grid;
            let building_grid = &self.building_grid;
            let building_snap = &self.building_world_snap;
            let turret_cfg = config.building_by_kind(2);

            for b in &mut self.buildings {
                if b.kind != 2 {
                    continue;
                }
                let Some(cfg) = turret_cfg else {
                    continue;
                };
                let Some(lvl) = cfg.levels.get((b.level as usize).saturating_sub(1)) else {
                    continue;
                };
                if lvl.fire_rate <= 0.0 || lvl.range <= 0.0 {
                    continue;
                }
                let Some(zone) = zones.get(&b.owner_id) else {
                    continue;
                };

                b.cooldown_left -= dt;
                if b.cooldown_left > 0.0 {
                    continue;
                }

                let owner_id = b.owner_id;
                let base_range = lvl.range;
                let bx = zone.x + b.rel_x;
                let by = zone.y + b.rel_y;

                let mut best_hp = u16::MAX;
                let mut best_dist2 = base_range * base_range;
                let mut target_info: Option<(EntityId, usize, bool, f32, f32)> = None;

                // 1. Immediate self-defense: enemy units right on our base perimeter or point-blank to this turret
                let close_zone_r2 = (zone.radius + 55.0) * (zone.radius + 55.0);
                let close_turret_r2 = 105.0 * 105.0;
                let mut urgent_hp = u16::MAX;
                let mut urgent_d2 = base_range * base_range;
                unit_grid.for_each_in_radius(bx, by, base_range, |ui, ux, uy, d2| {
                    let u = &units[ui];
                    if u.owner_id == owner_id || u.hp == 0 {
                        return;
                    }
                    if u32::from(u.hp) <= incoming_dmg.get(&u.id).copied().unwrap_or(0) {
                        return;
                    }
                    let zdx = ux - zone.x;
                    let zdy = uy - zone.y;
                    if d2 > close_turret_r2 && (zdx * zdx + zdy * zdy) > close_zone_r2 {
                        return;
                    }
                    if !(u.hp < urgent_hp || (u.hp == urgent_hp && d2 < urgent_d2)) {
                        return;
                    }
                    if u.owner_id != 0
                        && players
                            .get(&u.owner_id)
                            .is_some_and(|p| p.spawn_protection_left > 0.0)
                    {
                        return;
                    }
                    urgent_hp = u.hp;
                    urgent_d2 = d2;
                    target_info = Some((u.id, ui, false, ux, uy));
                });

                // 2. Siege priority: enemy HQ (kind=0) or enemy Turret (kind=2) in range
                if target_info.is_none() {
                    let mut best_siege_prio: u8 = u8::MAX;
                    let mut best_siege_d2 = base_range * base_range;
                    building_grid.for_each_in_radius(bx, by, base_range, |bi, tx, ty, d2| {
                        let Some((bid, b_owner, _, _, _, bhp, bkind)) = building_snap[bi] else {
                            return;
                        };
                        if b_owner == owner_id || bhp == 0 || (bkind != 0 && bkind != 2) {
                            return;
                        }
                        let prio: u8 = if bkind == 0 { 0 } else { 1 };
                        if prio > best_siege_prio || (prio == best_siege_prio && d2 >= best_siege_d2)
                        {
                            return;
                        }
                        if players
                            .get(&b_owner)
                            .is_some_and(|p| p.spawn_protection_left > 0.0)
                        {
                            return;
                        }
                        best_siege_prio = prio;
                        best_siege_d2 = d2;
                        target_info = Some((bid, bi, true, tx, ty));
                    });
                }

                // 3. Standard anti-unit targeting (lowest HP with overkill protection)
                if target_info.is_none() {
                    unit_grid.for_each_in_radius(bx, by, base_range, |ui, ux, uy, d2| {
                        let u = &units[ui];
                        if u.owner_id == owner_id || u.hp == 0 {
                            return;
                        }
                        if u32::from(u.hp) <= incoming_dmg.get(&u.id).copied().unwrap_or(0) {
                            return;
                        }
                        if !(u.hp < best_hp || (u.hp == best_hp && d2 < best_dist2)) {
                            return;
                        }
                        if u.owner_id != 0
                            && players
                                .get(&u.owner_id)
                                .is_some_and(|p| p.spawn_protection_left > 0.0)
                        {
                            return;
                        }
                        best_hp = u.hp;
                        best_dist2 = d2;
                        target_info = Some((u.id, ui, false, ux, uy));
                    });
                }

                // 4. Fallback: any remaining enemy building in range
                if target_info.is_none() {
                    building_grid.for_each_in_radius(bx, by, base_range, |bi, tx, ty, d2| {
                        if d2 > best_dist2 {
                            return;
                        }
                        let Some((bid, b_owner, _, _, _, bhp, _)) = building_snap[bi] else {
                            return;
                        };
                        if b_owner == owner_id
                            || bhp == 0
                            || players
                                .get(&b_owner)
                                .is_some_and(|p| p.spawn_protection_left > 0.0)
                        {
                            return;
                        }
                        best_dist2 = d2;
                        target_info = Some((bid, bi, true, tx, ty));
                    });
                }

                if let Some((target_id, target_idx, target_is_building, tx, ty)) = target_info {
                    let shot_cost = lvl.shot_cost;
                    if shot_cost > 0.0 {
                        let cur_e = player_energy.get(&owner_id).copied().unwrap_or(0.0);
                        if cur_e < shot_cost {
                            b.cooldown_left = 0.0;
                            continue;
                        }
                        player_energy.insert(owner_id, cur_e - shot_cost);
                    }
                    let fire_rate = lvl.fire_rate;
                    let base_dmg = lvl.damage;
                    let proj_speed = lvl.projectile_speed.max(120.0);
                    let splash_r = if lvl.splash_radius > 0.0 {
                        lvl.splash_radius
                    } else {
                        match b.level {
                            1 => 40.0_f32,
                            2 => 36.0_f32,
                            _ => 32.0_f32,
                        }
                    };
                    let src_tag = match b.level {
                        1 => SRC_TURRET_1,
                        2 => SRC_TURRET_2,
                        _ => SRC_TURRET_3,
                    };

                    let dx = tx - bx;
                    let dy = ty - by;
                    let dist = (dx * dx + dy * dy).sqrt().max(0.001);
                    let flight_sec = (dist / proj_speed).clamp(0.02, 1.5);
                    let rad = dy.atan2(dx).rem_euclid(TAU);
                    b.angle = ((rad / TAU) * 255.0).round() as u8;
                    b.cooldown_left = 1.0 / fire_rate;

                    let dmg_u16 = base_dmg.round() as u16;
                    if !target_is_building && target_id != 0 {
                        *incoming_dmg.entry(target_id).or_insert(0) += u32::from(dmg_u16);
                    }

                    new_projectiles.push(Projectile {
                        id: 0,
                        owner_id,
                        kind: 0,
                        src_tag,
                        x: bx,
                        y: by,
                        vx: dx / flight_sec,
                        vy: dy / flight_sec,
                        target_x: tx,
                        target_y: ty,
                        target_id,
                        target_idx,
                        target_is_building,
                        damage: dmg_u16,
                        ttl: flight_sec,
                        splash_radius: splash_r,
                    });
                }
            }
        }

        for (pid, e) in player_energy {
            if let Some(p) = self.players.get_mut(&pid) {
                p.energy = e;
            }
        }

        if !new_projectiles.is_empty() {
            for p in &mut new_projectiles {
                p.id = self.alloc_entity_id();
            }
            self.projectiles.extend(new_projectiles);
        }
    }
}
