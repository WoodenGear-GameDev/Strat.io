use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::spatial::SpatialGrid;
use crate::telemetry::{DST_BUILDING_OFFSET, SRC_RAD};
use crate::types::{EntityId, PlayerId, PlayerState, Projectile, SquadOrder, Unit};

use super::hex::{RING1_RADIUS, RING2_RADIUS};
use super::{BuildingSnap, UnitSnap, World};

#[inline]
fn formation_offset(slot: u16) -> (f32, f32) {
    static TABLE: OnceLock<[(f32, f32); 64]> = OnceLock::new();
    let tbl = TABLE.get_or_init(|| {
        std::array::from_fn(|s| {
            if s == 0 {
                (0.0, 0.0)
            } else {
                let angle = (s as f32) * 2.399963_f32;
                let r = 28.0 * (s as f32).sqrt();
                (angle.cos() * r, angle.sin() * r)
            }
        })
    });
    let idx = usize::from(slot);
    if idx < tbl.len() {
        tbl[idx]
    } else {
        let angle = (slot as f32) * 2.399963_f32;
        let r = 28.0 * (slot as f32).sqrt();
        (angle.cos() * r, angle.sin() * r)
    }
}

struct UnitStepCtx<'a> {
    aggro_r: f32,
    players: &'a HashMap<PlayerId, PlayerState>,
    grid: &'a SpatialGrid,
    b_grid: &'a SpatialGrid,
    unit_snapshot: &'a [UnitSnap],
    building_snapshot: &'a [BuildingSnap],
}

impl UnitStepCtx<'_> {
    #[inline]
    fn is_protected(&self, pid: PlayerId) -> bool {
        self.players
            .get(&pid)
            .is_some_and(|p| p.spawn_protection_left > 0.0)
    }
}

struct UnitTarget {
    unit: Option<(EntityId, usize, f32, f32)>,
    building: Option<(EntityId, usize, f32, f32, f32, u8)>,
    hold_course: bool,
}

impl World {
    pub(super) fn step_core_push(&mut self, dt: f32) {
        let core_r = self.config.world.core_radius;
        let base_push = self.config.world.core_push_strength;
        let players = &self.players;
        for (&pid, z) in &mut self.zones {
            let hq_lvl = players.get(&pid).map(|p| p.hq_level).unwrap_or(1);
            let core_min = core_r + z.radius;
            let ring_min = match hq_lvl {
                0 | 1 => core_min,
                2 => RING1_RADIUS + z.radius + 140.0,
                _ => RING2_RADIUS + z.radius + 80.0,
            };
            let d2 = z.x * z.x + z.y * z.y;
            if d2 < core_min * core_min {
                let d = d2.sqrt();
                let (nx, ny) = if d > 0.001 {
                    (z.x / d, z.y / d)
                } else {
                    (1.0, 0.0)
                };
                z.x = nx * core_min;
                z.y = ny * core_min;
            } else if d2 < ring_min * ring_min {
                let d = d2.sqrt();
                if d > 0.001 {
                    let deficit = (ring_min - d).max(0.0);
                    let step = deficit.min(base_push * 1.8 * dt);
                    z.x += (z.x / d) * step;
                    z.y += (z.y / d) * step;
                }
            }
        }
    }

    pub(super) fn step_zone_repulsion(&mut self, dt: f32) {
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
                let min_dist = ar + br + 150.0;
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

    /// Dynamically splits lagging unit clusters (e.g., slow Titans) into separate squads and merges nearby ones.
    pub(super) fn step_squad_clustering(&mut self) {
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
                    if let Some(rsid) = pl.reinforce_squad {
                        pl.squads.retain(|&sid, _| sid == rsid);
                    } else {
                        pl.squads.clear();
                    }
                }
                continue;
            };
            let Some(mut orders) = self.players.get(&pid).map(|p| p.squads.clone()) else {
                continue;
            };
            let mut next_id = self
                .players
                .get(&pid)
                .map(|p| p.next_squad_id)
                .unwrap_or(1);
            let mut reinforce_sid = self.players.get(&pid).and_then(|p| p.reinforce_squad);

            orders.retain(|sid, _| squad_map.contains_key(sid) || reinforce_sid == Some(*sid));
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
                            let dx = self.units[vi].x - ux;
                            let dy = self.units[vi].y - uy;
                            if dx * dx + dy * dy <= 250.0 * 250.0 {
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
                        .map(|&idx| {
                            let dx = self.units[idx].x - order.target_x;
                            let dy = self.units[idx].y - order.target_y;
                            dx * dx + dy * dy
                        })
                        .fold(f32::MAX, f32::min);
                    let db = b
                        .iter()
                        .map(|&idx| {
                            let dx = self.units[idx].x - order.target_x;
                            let dy = self.units[idx].y - order.target_y;
                            dx * dx + dy * dy
                        })
                        .fold(f32::MAX, f32::min);
                    da.total_cmp(&db)
                });

                let mut comp_iter = components.into_iter();
                let mut kept_indices = comp_iter.next().unwrap_or_default();
                let mut split_full = false;
                for comp in comp_iter {
                    if split_full {
                        kept_indices.extend(comp);
                        continue;
                    }
                    let Some(cand) = (1..=5_u8).find(|id| !orders.contains_key(id)) else {
                        split_full = true;
                        kept_indices.extend(comp);
                        continue;
                    };
                    next_id = cand;
                    orders.insert(cand, order);
                    for &ui in &comp {
                        self.units[ui].squad = cand;
                    }
                    squad_map.entry(cand).or_default().extend(comp);
                }
                squad_map.insert(sid, kept_indices);
            }

            // 2. Auto-merge two active squads if they converge closely (<165 px) toward similar targets
            let mut active_sids: Vec<u8> = orders.keys().copied().collect();
            active_sids.sort_unstable();
            let mut centroids: HashMap<u8, (f32, f32, usize)> = HashMap::new();
            for &sid in &active_sids {
                if let Some(indices) = squad_map.get(&sid) {
                    if !indices.is_empty() {
                        let mut sx = 0.0_f32;
                        let mut sy = 0.0_f32;
                        for &ui in indices {
                            let u = &self.units[ui];
                            sx += u.x;
                            sy += u.y;
                        }
                        let cnt = indices.len();
                        centroids.insert(sid, (sx / cnt as f32, sy / cnt as f32, cnt));
                    }
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
                    let cdx = ax - bx;
                    let cdy = ay - by;
                    let tdx = oa.target_x - ob.target_x;
                    let tdy = oa.target_y - ob.target_y;
                    if cdx * cdx + cdy * cdy <= 165.0 * 165.0
                        && tdx * tdx + tdy * tdy <= 260.0 * 260.0
                    {
                        let (keep, drop) = if acnt >= bcnt { (a, b) } else { (b, a) };
                        if let Some(dropped) = squad_map.remove(&drop) {
                            for &ui in &dropped {
                                self.units[ui].squad = keep;
                            }
                            squad_map.entry(keep).or_default().extend(dropped);
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
                let zx = z.x;
                let zy = z.y;
                let z_rad = z.radius;
                let vision_r = z_rad + 360.0;
                let vision_r2 = vision_r * vision_r;
                let z_rad2 = z_rad * z_rad;
                let mut return_sids: Vec<u8> = Vec::new();
                for (&sid, &ord) in &orders {
                    let odx = ord.target_x - zx;
                    let ody = ord.target_y - zy;
                    if odx * odx + ody * ody <= vision_r2 {
                        let all_inside = squad_map.get(&sid).map_or(true, |indices| {
                            indices.iter().all(|&ui| {
                                let u = &self.units[ui];
                                let udx = u.x - zx;
                                let udy = u.y - zy;
                                udx * udx + udy * udy <= vision_r2
                            })
                        });
                        if all_inside {
                            return_sids.push(sid);
                        }
                    }
                }
                for sid in return_sids {
                    if let Some(ord) = orders.remove(&sid) {
                        if let Some(indices) = squad_map.remove(&sid) {
                            for ui in indices {
                                self.units[ui].squad = 0;
                            }
                        }
                        if let Some(pl) = self.players.get_mut(&pid) {
                            let odx = ord.target_x - zx;
                            let ody = ord.target_y - zy;
                            pl.base_rally = if odx * odx + ody * ody > z_rad2 {
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

    pub(super) fn step_units(&mut self, dt: f32) {
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
        let mut rad_damage_events: Vec<(u8, u8, u16)> = Vec::new();
        let mut new_projectiles: Vec<Projectile> = Vec::new();
        let max_sep_query = titan_r * 2.0 + 6.0;
        let max_b_coll_r = 28.0 + titan_r + 4.0;
        let map_r = self.config.world.map_radius;
        let cur_tick = self.tick;
        let kb_damp = (-6.5 * dt).exp();

        // Only allocate/count formation slots for units that actually have an active order.
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

                let (owner_hq, owner_is_bot, raw_order) = if u.owner_id != 0 {
                    ctx.players
                        .get(&u.owner_id)
                        .map(|p| {
                            let ord = if u.squad > 0 {
                                p.squads.get(&u.squad).copied()
                            } else {
                                p.base_rally
                            };
                            (p.hq_level, p.is_bot, ord)
                        })
                        .unwrap_or((1, false, None))
                } else {
                    (1, false, None)
                };

                let Some(grav_speed_mult) =
                    Self::apply_unit_ring_radiation(u, owner_hq, dt, &mut rad_damage_events)
                else {
                    continue;
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

                let speed = unit_base_speed * grav_speed_mult;
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
                // Human-controlled squads NEVER auto-chase on march (>150px from target);
                // at the target point they close in to attack enemies within 110px of raw_order,
                // and otherwise hold formation slots without bumping.
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

        self.apply_unit_damage_events(
            unit_damage_events,
            building_damage_events,
            rad_damage_events,
        );
    }

    fn apply_unit_ring_radiation(
        u: &mut Unit,
        owner_hq: u8,
        dt: f32,
        rad_damage_events: &mut Vec<(u8, u8, u16)>,
    ) -> Option<f32> {
        if u.owner_id == 0 || owner_hq <= 1 {
            u.in_lower_ring_sec = 0.0;
            u.rad_accum = 0.0;
            return Some(1.0);
        }
        let u_r2 = u.x * u.x + u.y * u.y;
        let max_rad_r = if owner_hq == 2 {
            RING1_RADIUS
        } else {
            RING2_RADIUS
        };
        if u_r2 >= max_rad_r * max_rad_r {
            u.in_lower_ring_sec = 0.0;
            u.rad_accum = 0.0;
            return Some(1.0);
        }
        let u_r = u_r2.sqrt();
        let unit_ring = Self::ring_for_radius(u_r);
        if unit_ring < owner_hq {
            u.in_lower_ring_sec += dt;
            let diff = owner_hq - unit_ring;
            let base_rate = if diff == 1 { 0.045_f32 } else { 0.10_f32 };
            let ring_edge = if unit_ring == 1 {
                RING1_RADIUS
            } else {
                RING2_RADIUS
            };
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
                    return None;
                }
            }
            Some(match diff {
                1 => 0.58,
                _ => 0.40,
            })
        } else {
            u.in_lower_ring_sec = 0.0;
            u.rad_accum = 0.0;
            Some(1.0)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn find_unit_target_cached(
        cur_tick: u32,
        i: usize,
        u: &mut Unit,
        ctx: &UnitStepCtx<'_>,
        raw_order: Option<SquadOrder>,
        squad_order: Option<SquadOrder>,
        strict_manual: bool,
        at_manual_target: bool,
        hold_course: bool,
        is_ranged: bool,
        attack_range: f32,
        my_r: f32,
        unit_hit_r: f32,
    ) -> UnitTarget {
        let should_rescan = ((cur_tick.wrapping_add(u.id)) & 3) == 0;

        // 1. O(1) validation of cached target (avoids 75%+ of spatial queries!)
        if u.target_id != 0 {
            if !u.target_is_building && !at_manual_target {
                if let Some(&(other_id, other_owner, ox, oy, other_hp, _, _other_r)) =
                    ctx.unit_snapshot.get(u.target_idx)
                {
                    if other_id == u.target_id
                        && other_hp > 0
                        && other_owner != u.owner_id
                        && (other_owner == 0 || !ctx.is_protected(other_owner))
                    {
                        let search_r = if hold_course { unit_hit_r } else { ctx.aggro_r };
                        let dx = ox - u.x;
                        let dy = oy - u.y;
                        let in_range = dx * dx + dy * dy <= search_r * search_r;
                        let in_leash = hold_course
                            || squad_order.map_or(true, |o| {
                                let odx = ox - o.target_x;
                                let ody = oy - o.target_y;
                                odx * odx + ody * ody <= 210.0 * 210.0
                            });
                        if in_range && in_leash {
                            if !should_rescan {
                                return UnitTarget {
                                    unit: Some((other_id, u.target_idx, ox, oy)),
                                    building: None,
                                    hold_course,
                                };
                            }
                        } else {
                            u.target_id = 0;
                        }
                    } else {
                        u.target_id = 0;
                    }
                } else {
                    u.target_id = 0;
                }
            } else if u.target_is_building {
                if let Some(Some((bid, b_owner, bx, by, br, bhp, bkind))) =
                    ctx.building_snapshot.get(u.target_idx).copied()
                {
                    if bid == u.target_id
                        && bhp > 0
                        && b_owner != u.owner_id
                        && !ctx.is_protected(b_owner)
                    {
                        let valid = if at_manual_target {
                            raw_order.is_some_and(|ro| {
                                let dx = bx - ro.target_x;
                                let dy = by - ro.target_y;
                                let lim = 110.0 + br;
                                dx * dx + dy * dy <= lim * lim
                            })
                        } else if strict_manual {
                            let hit_d = if is_ranged {
                                attack_range + br
                            } else {
                                my_r + br + 6.0
                            };
                            let dx = bx - u.x;
                            let dy = by - u.y;
                            dx * dx + dy * dy <= hit_d * hit_d
                        } else if !hold_course {
                            let dx = bx - u.x;
                            let dy = by - u.y;
                            let lim = ctx.aggro_r + br;
                            let in_r = dx * dx + dy * dy <= lim * lim;
                            let in_leash = squad_order.map_or(true, |o| {
                                let odx = bx - o.target_x;
                                let ody = by - o.target_y;
                                let l2 = 210.0 + br;
                                odx * odx + ody * ody <= l2 * l2
                            });
                            in_r && in_leash
                        } else {
                            false
                        };
                        if valid {
                            if !should_rescan {
                                return UnitTarget {
                                    unit: None,
                                    building: Some((bid, u.target_idx, bx, by, br, bkind)),
                                    hold_course: if at_manual_target {
                                        false
                                    } else {
                                        hold_course
                                    },
                                };
                            }
                        } else {
                            u.target_id = 0;
                        }
                    } else {
                        u.target_id = 0;
                    }
                } else {
                    u.target_id = 0;
                }
            } else {
                u.target_id = 0;
            }
        } else if !should_rescan && !at_manual_target {
            // Idle/marching unit with no target: staggered 4-tick scan (0.2s max delay, 75% fewer queries)
            return UnitTarget {
                unit: None,
                building: None,
                hold_course,
            };
        }

        let target = Self::find_unit_target(
            i,
            u,
            ctx,
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

        if let Some((uid, idx, _, _)) = target.unit {
            u.target_id = uid;
            u.target_idx = idx;
            u.target_is_building = false;
        } else if let Some((bid, idx, _, _, _, _)) = target.building {
            u.target_id = bid;
            u.target_idx = idx;
            u.target_is_building = true;
        } else {
            u.target_id = 0;
        }
        target
    }

    #[allow(clippy::too_many_arguments)]
    fn find_unit_target(
        i: usize,
        u: &Unit,
        ctx: &UnitStepCtx<'_>,
        raw_order: Option<SquadOrder>,
        squad_order: Option<SquadOrder>,
        strict_manual: bool,
        at_manual_target: bool,
        mut hold_course: bool,
        is_ranged: bool,
        attack_range: f32,
        my_r: f32,
        unit_hit_r: f32,
    ) -> UnitTarget {
        let mut best_edge_dist = ctx.aggro_r;
        let mut target_unit: Option<(EntityId, usize, f32, f32)> = None;
        let mut target_building: Option<(EntityId, usize, f32, f32, f32, u8)> = None;

        if at_manual_target {
            if let Some(ro) = raw_order {
                // Attack only enemy buildings in the ordered target area (no auto-attack on enemy units)
                let mut best_b_d = f32::MAX;
                ctx.b_grid
                    .for_each_in_radius(ro.target_x, ro.target_y, 130.0, |bi, bx, by, d2| {
                        let Some((bid, b_owner, _, _, br, bhp, bkind)) = ctx.building_snapshot[bi]
                        else {
                            return;
                        };
                        if b_owner == u.owner_id || bhp == 0 || ctx.is_protected(b_owner) {
                            return;
                        }
                        let d_raw_edge = (d2.sqrt() - br).max(0.0);
                        if d_raw_edge <= 110.0 && d_raw_edge < best_b_d {
                            best_b_d = d_raw_edge;
                            target_building = Some((bid, bi, bx, by, br, bkind));
                        }
                    });
                if target_building.is_some() {
                    hold_course = false;
                }
            }
        } else {
            let search_r = if hold_course { unit_hit_r } else { ctx.aggro_r };

            ctx.grid
                .for_each_in_radius(u.x, u.y, search_r, |other_i, ox, oy, d2| {
                    if other_i == i {
                        return;
                    }
                    let (other_id, other_owner, _, _, other_hp, _, other_r) =
                        ctx.unit_snapshot[other_i];
                    if other_owner == u.owner_id || other_hp == 0 {
                        return;
                    }
                    let max_d = best_edge_dist + other_r + 0.01;
                    if d2 > max_d * max_d {
                        return;
                    }
                    if !hold_course {
                        if let Some(o) = squad_order {
                            let odx = ox - o.target_x;
                            let ody = oy - o.target_y;
                            if odx * odx + ody * ody > 210.0 * 210.0 {
                                return;
                            }
                        }
                    }
                    if other_owner != 0 && ctx.is_protected(other_owner) {
                        return;
                    }
                    let edge_d = (d2.sqrt() - other_r).max(0.0);
                    if edge_d <= best_edge_dist {
                        best_edge_dist = edge_d;
                        target_unit = Some((other_id, other_i, ox, oy));
                    }
                });

            if u.owner_id != 0 && target_unit.is_none() && (strict_manual || !hold_course) {
                let b_search_r = if strict_manual {
                    (if is_ranged { attack_range } else { my_r + 6.0 }) + 32.0
                } else {
                    ctx.aggro_r
                };
                ctx.b_grid
                    .for_each_in_radius(u.x, u.y, b_search_r, |bi, bx, by, d2| {
                        let Some((bid, b_owner, _, _, br, bhp, bkind)) = ctx.building_snapshot[bi]
                        else {
                            return;
                        };
                        if b_owner == u.owner_id || bhp == 0 {
                            return;
                        }
                        let max_d = best_edge_dist + br + 0.01;
                        if d2 > max_d * max_d {
                            return;
                        }
                        if strict_manual {
                            let hit_d = if is_ranged {
                                attack_range + br
                            } else {
                                my_r + br + 6.0
                            };
                            if d2 > hit_d * hit_d {
                                return;
                            }
                        } else if let Some(o) = squad_order {
                            let odx = bx - o.target_x;
                            let ody = by - o.target_y;
                            let lim = 210.0 + br;
                            if odx * odx + ody * ody > lim * lim {
                                return;
                            }
                        }
                        if ctx.is_protected(b_owner) {
                            return;
                        }
                        let edge_d = (d2.sqrt() - br).max(0.0);
                        if edge_d <= best_edge_dist {
                            best_edge_dist = edge_d;
                            target_building = Some((bid, bi, bx, by, br, bkind));
                        }
                    });
            }
        }

        UnitTarget {
            unit: target_unit,
            building: target_building,
            hold_course,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_unit_combat(
        u: &mut Unit,
        target: &UnitTarget,
        speed: f32,
        damage: u16,
        unit_cd: f32,
        attack_range: f32,
        is_ranged: bool,
        my_r: f32,
        unit_hit_r: f32,
        new_projectiles: &mut Vec<Projectile>,
        unit_damage_events: &mut Vec<(usize, u16, PlayerId, u8)>,
        building_damage_events: &mut Vec<(usize, u16, PlayerId, u8)>,
    ) -> (f32, f32) {
        let mut vx = 0.0_f32;
        let mut vy = 0.0_f32;

        if let Some((tid, ti, tx, ty)) = target.unit {
            let dx = tx - u.x;
            let dy = ty - u.y;
            let d2 = dx * dx + dy * dy;
            let hit_dist = unit_hit_r;
            if d2 <= hit_dist * hit_dist {
                if u.attack_cooldown_left <= 0.0 {
                    if is_ranged && d2 > 0.000001 {
                        let dist = d2.sqrt();
                        let p_speed = 540.0;
                        let flight_sec = (dist / p_speed).clamp(0.02, 1.0);
                        new_projectiles.push(Projectile {
                            id: 0,
                            owner_id: u.owner_id,
                            kind: 2,
                            src_tag: u.kind,
                            x: u.x,
                            y: u.y,
                            vx: dx / flight_sec,
                            vy: dy / flight_sec,
                            target_x: tx,
                            target_y: ty,
                            target_id: tid,
                            target_idx: ti,
                            target_is_building: false,
                            damage,
                            ttl: flight_sec,
                            splash_radius: 0.0,
                        });
                    } else {
                        unit_damage_events.push((ti, damage, u.owner_id, u.kind));
                        if !target.hold_course && d2 > 0.000001 {
                            let dist = d2.sqrt();
                            let recoil = if u.kind == 2 { 110.0 } else { 195.0 };
                            u.knockback_vx = -(dx / dist) * recoil;
                            u.knockback_vy = -(dy / dist) * recoil;
                        }
                    }
                    u.attack_cooldown_left = unit_cd;
                }
            } else if !target.hold_course && d2 > 0.000001 {
                let dist = d2.sqrt();
                vx = (dx / dist) * speed;
                vy = (dy / dist) * speed;
            }
        } else if let Some((bid, bi, bx, by, br, bkind)) = target.building {
            let dx = bx - u.x;
            let dy = by - u.y;
            let d2 = dx * dx + dy * dy;
            let hit_dist = if is_ranged {
                attack_range + br
            } else {
                my_r + br + 6.0
            };
            if d2 <= hit_dist * hit_dist {
                if u.attack_cooldown_left <= 0.0 {
                    let b_dmg = if u.kind == 0 && (bkind == 1 || bkind == 5) {
                        (f32::from(damage) * 1.3).round() as u16
                    } else if u.kind == 0 && bkind == 0 {
                        (f32::from(damage) * 0.85).round().max(1.0) as u16
                    } else {
                        damage
                    };
                    if is_ranged && d2 > 0.000001 {
                        let dist = d2.sqrt();
                        let p_speed = 540.0;
                        let flight_sec = (dist / p_speed).clamp(0.02, 1.0);
                        new_projectiles.push(Projectile {
                            id: 0,
                            owner_id: u.owner_id,
                            kind: 2,
                            src_tag: u.kind,
                            x: u.x,
                            y: u.y,
                            vx: dx / flight_sec,
                            vy: dy / flight_sec,
                            target_x: bx,
                            target_y: by,
                            target_id: bid,
                            target_idx: bi,
                            target_is_building: true,
                            damage: b_dmg,
                            ttl: flight_sec,
                            splash_radius: 0.0,
                        });
                    } else {
                        building_damage_events.push((bi, b_dmg, u.owner_id, u.kind));
                        if !target.hold_course && d2 > 0.000001 {
                            let dist = d2.sqrt();
                            let bounce = if u.kind == 2 { 175.0 } else { 290.0 };
                            u.knockback_vx = -(dx / dist) * bounce;
                            u.knockback_vy = -(dy / dist) * bounce;
                        }
                    }
                    u.attack_cooldown_left = unit_cd;
                }
            } else if !target.hold_course && d2 > 0.000001 {
                let dist = d2.sqrt();
                vx = (dx / dist) * speed;
                vy = (dy / dist) * speed;
            }
        }

        (vx, vy)
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_unit_collisions(
        i: usize,
        u: &mut Unit,
        ctx: &UnitStepCtx<'_>,
        my_r: f32,
        max_sep_query: f32,
        max_b_coll_r: f32,
        strict_manual: bool,
    ) {
        // Unit <-> Unit collision separation + elastic knockback on collision (capped at 6 overlaps/tick to prevent O(K^2) in huge clumps)
        let ux0 = u.x;
        let uy0 = u.y;
        let mut sep_hits = 0_u8;
        ctx.grid
            .for_each_in_radius_until(ux0, uy0, max_sep_query, |other_i, ox, oy, _d2| {
                if other_i == i {
                    return true;
                }
                let (_, other_owner, _, _, other_hp, other_kind, other_r) =
                    ctx.unit_snapshot[other_i];
                if other_hp == 0 {
                    return true;
                }
                let min_pair = my_r + other_r + 2.5;
                let dx = u.x - ox;
                let dy = u.y - oy;
                let d2 = dx * dx + dy * dy;
                if d2 < min_pair * min_pair {
                    let dist = d2.sqrt();
                    let (nx, ny) = if dist > 0.001 {
                        (dx / dist, dy / dist)
                    } else {
                        (1.0, 0.0)
                    };
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
                    sep_hits += 1;
                    if sep_hits >= 6 {
                        return false;
                    }
                }
                true
            });

        // Unit <-> Enemy Building collision pushout + bounce (allies pass freely through own buildings!)
        let ux1 = u.x;
        let uy1 = u.y;
        ctx.b_grid
            .for_each_in_radius(ux1, uy1, max_b_coll_r, |bi, bx, by, _d2| {
                let Some((_, b_owner, _, _, br, bhp, _)) = ctx.building_snapshot[bi] else {
                    return;
                };
                if bhp == 0 || b_owner == u.owner_id {
                    return;
                }
                let min_b_dist = br + my_r + 2.0;
                let dx = u.x - bx;
                let dy = u.y - by;
                let d2 = dx * dx + dy * dy;
                if d2 < min_b_dist * min_b_dist {
                    let dist = d2.sqrt();
                    let (nx, ny) = if dist > 0.001 {
                        (dx / dist, dy / dist)
                    } else {
                        (1.0, 0.0)
                    };
                    u.x = bx + nx * min_b_dist;
                    u.y = by + ny * min_b_dist;

                    if !strict_manual {
                        let bounce = if u.kind == 2 { 155.0 } else { 240.0 };
                        let radial_kb = u.knockback_vx * nx + u.knockback_vy * ny;
                        if radial_kb < bounce {
                            let add = bounce - radial_kb;
                            u.knockback_vx += nx * add;
                            u.knockback_vy += ny * add;
                        }
                    }
                }
            });
    }

    fn apply_unit_damage_events(
        &mut self,
        unit_damage_events: Vec<(usize, u16, PlayerId, u8)>,
        building_damage_events: Vec<(usize, u16, PlayerId, u8)>,
        rad_damage_events: Vec<(u8, u8, u16)>,
    ) {
        if unit_damage_events.is_empty()
            && building_damage_events.is_empty()
            && rad_damage_events.is_empty()
        {
            return;
        }

        let mut tlog = self.telemetry.take();
        let owner_rings: HashMap<PlayerId, u8> =
            if tlog.is_some() && (!unit_damage_events.is_empty() || !building_damage_events.is_empty())
            {
                self.zones
                    .iter()
                    .map(|(&pid, z)| (pid, Self::ring_for_radius((z.x * z.x + z.y * z.y).sqrt())))
                    .collect()
            } else {
                HashMap::new()
            };

        let mut damaged_owners: HashSet<PlayerId> = HashSet::new();
        {
            let players = &self.players;
            let units = &mut self.units;
            let buildings = &mut self.buildings;

            for (ti, dmg, attacker, src_kind) in unit_damage_events {
                if let Some(target) = units.get_mut(ti) {
                    let adj_dmg = if attacker != 0 && target.owner_id != 0 {
                        let att_hq = players.get(&attacker).map(|p| p.hq_level).unwrap_or(1);
                        let def_hq = players
                            .get(&target.owner_id)
                            .map(|p| p.hq_level)
                            .unwrap_or(1);
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
                        let dr =
                            Self::ring_for_radius((target.x * target.x + target.y * target.y).sqrt());
                        tl.log_damage(src_kind, target.kind, sr, dr, dealt);
                    }
                }
            }
            for (bi, dmg, attacker, src_kind) in building_damage_events {
                if let Some(target) = buildings.get_mut(bi) {
                    let att_hq = players.get(&attacker).map(|p| p.hq_level).unwrap_or(1);
                    let def_hq = players
                        .get(&target.owner_id)
                        .map(|p| p.hq_level)
                        .unwrap_or(1);
                    let dmg_amt =
                        (f32::from(dmg) * Self::tier_dmg_mult(att_hq, def_hq, target.kind == 0))
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
}
