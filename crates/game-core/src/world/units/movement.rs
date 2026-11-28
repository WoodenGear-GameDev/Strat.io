use std::collections::HashMap;

use crate::types::{PlayerId, Projectile, SquadOrder};

use super::super::World;
use super::{formation_offset, UnitStepCtx};

impl World {
    /// Mass gravity: bases with HQ Lv 2+ are smoothly pulled inward into their target ring
    /// (`r <= max_radius_for_hq`). Light bases (HQ Lv 1) can voluntarily enter inner rings.
    pub(in super::super) fn step_mass_gravity(&mut self, dt: f32) {
        let pull_speed = self.config.world.gravity_pull_strength;
        let ring3_r = self.config.world.ring3_radius;
        let ring2_r = self.config.world.ring2_radius;
        let map_r = self.config.world.map_radius;
        let players = &self.players;
        for (&pid, z) in &mut self.zones {
            let hq_lvl = players.get(&pid).map(|p| p.hq_level).unwrap_or(1);
            let max_r = match hq_lvl {
                0 | 1 => (map_r - z.radius).max(0.0),
                2 => (ring2_r - z.radius - 60.0).max(0.0),
                _ => (ring3_r - z.radius - 60.0).max(0.0),
            };
            let d2 = z.x * z.x + z.y * z.y;
            if d2 > max_r * max_r {
                let d = d2.sqrt();
                if d > 0.001 {
                    let excess = d - max_r;
                    let step = excess.min(pull_speed * dt);
                    z.x -= (z.x / d) * step;
                    z.y -= (z.y / d) * step;
                }
            }
        }
    }

    pub(in super::super) fn step_zone_repulsion(&mut self, dt: f32) {
        let mut keys: Vec<PlayerId> = self.zones.keys().copied().collect();
        keys.sort_unstable();
        let max_step = self.config.world.zone_repulsion_strength * dt;
        let map_r = self.config.world.map_radius;
        let clearance = self.config.world.teleport_zone_clearance;

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
                let min_dist = ar + br + clearance;
                let d2 = dx * dx + dy * dy;
                if d2 < min_dist * min_dist {
                    let dist = d2.sqrt();
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
            let d2 = z.x * z.x + z.y * z.y;
            if d2 > max_r * max_r {
                let d = d2.sqrt();
                if d > 0.001 {
                    z.x = z.x / d * max_r;
                    z.y = z.y / d * max_r;
                }
            }
        }
    }

    pub(in super::super) fn step_units(&mut self, dt: f32) {
        if self.units.is_empty() {
            return;
        }
        let raider = self.config.unit_fast_stats(0);
        let archer = self.config.unit_fast_stats(1);
        let titan = self.config.unit_fast_stats(2);
        let aggro_r = raider.aggro_range;
        let archer_range = archer.range;
        let raider_r = raider.radius;
        let archer_r = archer.radius;
        let titan_r = titan.radius;

        let mut unit_damage_events: Vec<(usize, u16, PlayerId, u8)> = Vec::new();
        let mut building_damage_events: Vec<(usize, u16, PlayerId, u8)> = Vec::new();
        let mut new_projectiles: Vec<Projectile> = Vec::new();
        let max_sep_query = titan_r * 2.0 + 6.0;
        let max_b_coll_r = 28.0 + titan_r + 4.0;
        let map_r = self.config.world.map_radius;
        let cur_tick = self.tick;
        let kb_damp = (-6.5 * dt).exp();

        let mut squad_counters: HashMap<(PlayerId, u8), u16> = HashMap::new();

        {
            let ctx = UnitStepCtx {
                aggro_r,
                players: &self.players,
                grid: &self.unit_grid,
                b_grid: &self.building_grid,
                unit_snapshot: &self.unit_world_snap,
                building_snapshot: &self.building_world_snap,
            };
            let zones = &self.zones;
            let units = &mut self.units;

            for (i, u) in units.iter_mut().enumerate() {
                if u.hp == 0 {
                    continue;
                }
                if u.attack_cooldown_left > 0.0 {
                    u.attack_cooldown_left = (u.attack_cooldown_left - dt).max(0.0);
                }

                let (owner_is_bot, raw_order) = if u.owner_id != 0 {
                    ctx.players
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

                let (unit_base_speed, unit_base_dmg, unit_cd, attack_range, is_ranged, my_r) =
                    match u.kind {
                        1 => (
                            archer.speed,
                            archer.damage,
                            archer.attack_cooldown_sec,
                            archer_range,
                            true,
                            archer_r,
                        ),
                        2 => (
                            titan.speed,
                            titan.damage,
                            titan.attack_cooldown_sec,
                            0.0,
                            false,
                            titan_r,
                        ),
                        _ => (
                            raider.speed,
                            raider.damage,
                            raider.attack_cooldown_sec,
                            0.0,
                            false,
                            raider_r,
                        ),
                    };

                let speed = unit_base_speed;
                let damage = unit_base_dmg.round() as u16;

                let squad_order = raw_order.map(|o| {
                    let c = squad_counters.entry((u.owner_id, u.squad)).or_insert(0);
                    let slot = *c;
                    *c += 1;
                    let (slot_ox, slot_oy) = formation_offset(slot);
                    SquadOrder {
                        target_x: o.target_x + slot_ox,
                        target_y: o.target_y + slot_oy,
                    }
                });
                let strict_manual = squad_order.is_some() && !owner_is_bot;
                let at_manual_target = strict_manual
                    && raw_order.is_some_and(|o| {
                        let dx = o.target_x - u.x;
                        let dy = o.target_y - u.y;
                        dx * dx + dy * dy <= 150.0 * 150.0
                    });
                let dist_to_order_sq = squad_order.map(|o| {
                    let dx = o.target_x - u.x;
                    let dy = o.target_y - u.y;
                    dx * dx + dy * dy
                });
                let hold_course =
                    strict_manual || dist_to_order_sq.is_some_and(|d2| d2 > 85.0 * 85.0);

                let unit_hit_r = if is_ranged {
                    attack_range
                } else {
                    my_r * 2.0 + 4.5
                };

                let target = Self::find_unit_target_cached(
                    cur_tick,
                    i,
                    u,
                    &ctx,
                    raw_order,
                    squad_order,
                    strict_manual,
                    at_manual_target,
                    hold_course,
                    is_ranged,
                    attack_range,
                    my_r,
                    unit_hit_r,
                );

                let (mut vx, mut vy) = Self::resolve_unit_combat(
                    u,
                    &target,
                    speed,
                    damage,
                    unit_cd,
                    attack_range,
                    is_ranged,
                    my_r,
                    unit_hit_r,
                    &mut new_projectiles,
                    &mut unit_damage_events,
                    &mut building_damage_events,
                );

                if target.hold_course {
                    if let Some(order) = squad_order {
                        let d2 = dist_to_order_sq.unwrap_or(0.0);
                        if d2 > 64.0 {
                            let dist = d2.sqrt();
                            let slow = (dist / 24.0).clamp(0.25, 1.0);
                            let (dir_x, dir_y) =
                                Self::polar_move_dir(u.x, u.y, order.target_x, order.target_y);
                            vx = dir_x * speed * slow;
                            vy = dir_y * speed * slow;
                        }
                    }
                } else if target.unit.is_none() && target.building.is_none() && u.owner_id != 0 {
                    if let Some(order) = squad_order {
                        let d2 = dist_to_order_sq.unwrap_or(0.0);
                        if d2 > 64.0 {
                            let dist = d2.sqrt();
                            let slow = (dist / 24.0).clamp(0.25, 1.0);
                            let (dir_x, dir_y) =
                                Self::polar_move_dir(u.x, u.y, order.target_x, order.target_y);
                            vx = dir_x * speed * slow;
                            vy = dir_y * speed * slow;
                        }
                    } else if let Some(z) = zones.get(&u.owner_id) {
                        let orbit_r = z.radius + 14.0 + ((u.id % 3) as f32) * 10.0;
                        let rx = u.x - z.x;
                        let ry = u.y - z.y;
                        let dist = (rx * rx + ry * ry).sqrt();
                        let (nx, ny) = if dist > 0.001 {
                            (rx / dist, ry / dist)
                        } else {
                            let a = (u.id as f32) * 1.1;
                            (a.cos(), a.sin())
                        };
                        let tx = -ny;
                        let ty = nx;
                        let radial_err = ((orbit_r - dist) / 48.0).clamp(-1.0, 1.0);
                        let tang_weight = if (dist - orbit_r).abs() > 75.0 {
                            0.25
                        } else {
                            1.0
                        };
                        let dir_x = nx * radial_err + tx * tang_weight;
                        let dir_y = ny * radial_err + ty * tang_weight;
                        let dlen = (dir_x * dir_x + dir_y * dir_y).sqrt();
                        let patrol_speed = speed * 0.78;
                        if dlen > 0.001 {
                            vx = (dir_x / dlen) * patrol_speed;
                            vy = (dir_y / dlen) * patrol_speed;
                        }
                    }
                }

                Self::resolve_unit_collisions(
                    i,
                    u,
                    &ctx,
                    my_r,
                    max_sep_query,
                    max_b_coll_r,
                    strict_manual,
                );

                let kb_len2 =
                    u.knockback_vx * u.knockback_vx + u.knockback_vy * u.knockback_vy;
                if kb_len2 > 360.0 * 360.0 {
                    let kb_len = kb_len2.sqrt();
                    u.knockback_vx = (u.knockback_vx / kb_len) * 360.0;
                    u.knockback_vy = (u.knockback_vy / kb_len) * 360.0;
                }

                u.vx = vx + u.knockback_vx;
                u.vy = vy + u.knockback_vy;
                u.x += u.vx * dt;
                u.y += u.vy * dt;

                u.knockback_vx *= kb_damp;
                u.knockback_vy *= kb_damp;

                let max_r = (map_r - my_r).max(0.0);
                let d2 = u.x * u.x + u.y * u.y;
                if d2 > max_r * max_r {
                    let d = d2.sqrt();
                    if d > 0.001 {
                        u.x = u.x / d * max_r;
                        u.y = u.y / d * max_r;
                    }
                }
            }
        }

        if !new_projectiles.is_empty() {
            for p in &mut new_projectiles {
                p.id = self.alloc_entity_id();
            }
            self.projectiles.extend(new_projectiles);
        }

        self.apply_unit_damage_events(unit_damage_events, building_damage_events);
    }
}
