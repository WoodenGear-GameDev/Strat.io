use std::collections::{HashMap, HashSet};

use crate::spatial::SpatialGrid;
use crate::telemetry::{DST_BUILDING_OFFSET, SRC_RAD};
use crate::types::{PlayerId, Projectile, SquadOrder, Unit};

use super::hex::{RING1_RADIUS, RING2_RADIUS};
use super::World;

type UnitSnap = (PlayerId, f32, f32, u16, u8, f32);
type BuildingSnap = Option<(PlayerId, f32, f32, f32, u16, u8)>;

struct UnitStepCtx<'a> {
    aggro_r: f32,
    protected: &'a HashSet<PlayerId>,
    grid: &'a SpatialGrid,
    b_grid: &'a SpatialGrid,
    unit_snapshot: &'a [UnitSnap],
    building_snapshot: &'a [BuildingSnap],
}

struct UnitTarget {
    unit: Option<(usize, f32, f32)>,
    building: Option<(usize, f32, f32, f32, u8)>,
    hold_course: bool,
}

impl World {
    pub(super) fn step_core_push(&mut self, dt: f32) {
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
                    let (nx, ny) = if d > 0.001 {
                        (z.x / d, z.y / d)
                    } else {
                        (1.0, 0.0)
                    };
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
                        .map(|&idx| {
                            (self.units[idx].x - order.target_x)
                                .hypot(self.units[idx].y - order.target_y)
                        })
                        .fold(f32::MAX, f32::min);
                    let db = b
                        .iter()
                        .map(|&idx| {
                            (self.units[idx].x - order.target_x)
                                .hypot(self.units[idx].y - order.target_y)
                        })
                        .fold(f32::MAX, f32::min);
                    da.total_cmp(&db)
                });

                for comp in components.into_iter().skip(1) {
                    let Some(cand) = (1..=5_u8).find(|id| !orders.contains_key(id)) else {
                        break;
                    };
                    next_id = cand;
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
                            pl.base_rally =
                                if (ord.target_x - z.x).hypot(ord.target_y - z.y) > z.radius {
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
        let raider_cfg = self.config.unit_by_kind(0).cloned();
        let archer_cfg = self.config.unit_by_kind(1).cloned();
        let titan_cfg = self.config.unit_by_kind(2).cloned();
        let aggro_r = raider_cfg.as_ref().map(|u| u.aggro_range).unwrap_or(290.0);
        let _aggro_r2 = aggro_r * aggro_r;
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

        let unit_snapshot: Vec<UnitSnap> = self
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

        let building_snapshot: Vec<BuildingSnap> = self
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

        let ctx = UnitStepCtx {
            aggro_r,
            protected: &protected,
            grid: &grid,
            b_grid: &b_grid,
            unit_snapshot: &unit_snapshot,
            building_snapshot: &building_snapshot,
        };

        for (i, u) in self.units.iter_mut().enumerate() {
            if u.hp == 0 {
                continue;
            }
            if u.attack_cooldown_left > 0.0 {
                u.attack_cooldown_left = (u.attack_cooldown_left - dt).max(0.0);
            }

            let owner_hq = hq_levels.get(&u.owner_id).copied().unwrap_or(1);
            let Some(grav_speed_mult) =
                Self::apply_unit_ring_radiation(u, owner_hq, dt, &mut rad_damage_events)
            else {
                continue;
            };

            let (unit_base_speed, unit_base_dmg, unit_cd, attack_range, is_ranged, my_r) =
                match u.kind {
                    1 => (
                        archer_cfg.as_ref().map(|c| c.speed).unwrap_or(158.0),
                        archer_cfg.as_ref().map(|c| c.damage).unwrap_or(15.0),
                        archer_cfg
                            .as_ref()
                            .map(|c| c.attack_cooldown_sec)
                            .unwrap_or(0.90),
                        archer_range,
                        true,
                        archer_r,
                    ),
                    2 => (
                        titan_cfg.as_ref().map(|c| c.speed).unwrap_or(128.0),
                        titan_cfg.as_ref().map(|c| c.damage).unwrap_or(40.0),
                        titan_cfg
                            .as_ref()
                            .map(|c| c.attack_cooldown_sec)
                            .unwrap_or(0.95),
                        0.0,
                        false,
                        titan_r,
                    ),
                    _ => (
                        raider_cfg.as_ref().map(|c| c.speed).unwrap_or(235.0),
                        raider_cfg.as_ref().map(|c| c.damage).unwrap_or(13.0),
                        raider_cfg
                            .as_ref()
                            .map(|c| c.attack_cooldown_sec)
                            .unwrap_or(0.55),
                        0.0,
                        false,
                        raider_r,
                    ),
                };

            let speed = unit_base_speed * grav_speed_mult;
            let damage = unit_base_dmg.round() as u16;

            let (owner_is_bot, raw_order) = if u.owner_id != 0 {
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
            let squad_order = raw_order.map(|o| SquadOrder {
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
            let hold_course = strict_manual || (squad_order.is_some() && dist_to_order > 85.0);

            let unit_hit_r = if is_ranged {
                attack_range
            } else {
                my_r * 2.0 + 4.5
            };

            let target = Self::find_unit_target(
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
                &mut neighbors,
                &mut b_neighbors,
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
                    let dist = (order.target_x - u.x).hypot(order.target_y - u.y);
                    if dist > 8.0 && dist > 0.001 {
                        let slow = (dist / 24.0).clamp(0.25, 1.0);
                        let (dir_x, dir_y) =
                            Self::polar_move_dir(u.x, u.y, order.target_x, order.target_y);
                        vx = dir_x * speed * slow;
                        vy = dir_y * speed * slow;
                    }
                }
            } else if target.unit.is_none() && target.building.is_none() && u.owner_id != 0 {
                if let Some(order) = squad_order {
                    let dist = (order.target_x - u.x).hypot(order.target_y - u.y);
                    if dist > 8.0 && dist > 0.001 {
                        let slow = (dist / 24.0).clamp(0.25, 1.0);
                        let (dir_x, dir_y) =
                            Self::polar_move_dir(u.x, u.y, order.target_x, order.target_y);
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
                    let tang_weight = if (dist - orbit_r).abs() > 75.0 {
                        0.25
                    } else {
                        1.0
                    };
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

            Self::resolve_unit_collisions(
                i,
                u,
                &ctx,
                my_r,
                max_sep_query,
                max_b_coll_r,
                strict_manual,
                &mut neighbors,
                &mut b_neighbors,
            );

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

        self.apply_unit_damage_events(
            &hq_levels,
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
        let u_r = u.x.hypot(u.y);
        let unit_ring = Self::ring_for_radius(u_r);
        if u.owner_id != 0 && unit_ring < owner_hq {
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
        neighbors: &mut Vec<(usize, f32, f32)>,
        b_neighbors: &mut Vec<(usize, f32, f32)>,
    ) -> UnitTarget {
        let mut best_edge_dist = ctx.aggro_r;
        let mut target_unit: Option<(usize, f32, f32)> = None;
        let mut target_building: Option<(usize, f32, f32, f32, u8)> = None;

        if at_manual_target {
            if let Some(ro) = raw_order {
                // Attack only enemy buildings in the ordered target area (no auto-attack on enemy units)
                let mut best_b_d = f32::MAX;
                ctx.b_grid
                    .query_radius(ro.target_x, ro.target_y, 130.0, b_neighbors);
                for &(bi, bx, by) in b_neighbors.iter() {
                    let Some((b_owner, _, _, br, bhp, bkind)) = ctx.building_snapshot[bi] else {
                        continue;
                    };
                    if b_owner == u.owner_id || bhp == 0 || ctx.protected.contains(&b_owner) {
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
            let search_r = if hold_course { unit_hit_r } else { ctx.aggro_r };
            let search_r2 = search_r * search_r;

            ctx.grid.query_radius(u.x, u.y, search_r + 16.0, neighbors);
            for &(other_i, ox, oy) in neighbors.iter() {
                if other_i == i {
                    continue;
                }
                let (other_owner, _, _, other_hp, _, other_r) = ctx.unit_snapshot[other_i];
                if other_owner == u.owner_id
                    || other_hp == 0
                    || (other_owner != 0 && ctx.protected.contains(&other_owner))
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
                    (if is_ranged { attack_range } else { my_r + 6.0 }) + 32.0
                } else {
                    ctx.aggro_r
                };
                let b_search_r2 = b_search_r * b_search_r;
                ctx.b_grid.query_radius(u.x, u.y, b_search_r, b_neighbors);
                for &(bi, bx, by) in b_neighbors.iter() {
                    let Some((b_owner, _, _, br, bhp, bkind)) = ctx.building_snapshot[bi] else {
                        continue;
                    };
                    if b_owner == u.owner_id || bhp == 0 || ctx.protected.contains(&b_owner) {
                        continue;
                    }
                    if strict_manual {
                        let d_edge = (bx - u.x).hypot(by - u.y);
                        let hit_d = if is_ranged {
                            attack_range + br
                        } else {
                            my_r + br + 6.0
                        };
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

        if let Some((ti, tx, ty)) = target.unit {
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
                        if !target.hold_course && dist > 0.001 {
                            let recoil = if u.kind == 2 { 110.0 } else { 195.0 };
                            u.knockback_vx = -(dx / dist) * recoil;
                            u.knockback_vy = -(dy / dist) * recoil;
                        }
                    }
                    u.attack_cooldown_left = unit_cd;
                }
            } else if !target.hold_course && dist > 0.001 {
                vx = (dx / dist) * speed;
                vy = (dy / dist) * speed;
            }
        } else if let Some((bi, bx, by, br, bkind)) = target.building {
            let dx = bx - u.x;
            let dy = by - u.y;
            let dist = dx.hypot(dy);
            let hit_dist = if is_ranged {
                attack_range + br
            } else {
                my_r + br + 6.0
            };
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
                        if !target.hold_course && dist > 0.001 {
                            let bounce = if u.kind == 2 { 175.0 } else { 290.0 };
                            u.knockback_vx = -(dx / dist) * bounce;
                            u.knockback_vy = -(dy / dist) * bounce;
                        }
                    }
                    u.attack_cooldown_left = unit_cd;
                }
            } else if !target.hold_course && dist > 0.001 {
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
        neighbors: &mut Vec<(usize, f32, f32)>,
        b_neighbors: &mut Vec<(usize, f32, f32)>,
    ) {
        // Unit <-> Unit collision separation + elastic knockback on collision
        ctx.grid.query_radius(u.x, u.y, max_sep_query, neighbors);
        for &(other_i, ox, oy) in neighbors.iter() {
            if other_i == i {
                continue;
            }
            let (other_owner, _, _, other_hp, other_kind, other_r) = ctx.unit_snapshot[other_i];
            if other_hp == 0 {
                continue;
            }
            let min_pair = my_r + other_r + 2.5;
            let dx = u.x - ox;
            let dy = u.y - oy;
            let dist = dx.hypot(dy);
            if dist < min_pair {
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
            }
        }

        // Unit <-> Enemy Building collision pushout + bounce (allies pass freely through own buildings!)
        ctx.b_grid.query_radius(u.x, u.y, max_b_coll_r, b_neighbors);
        for &(bi, bx, by) in b_neighbors.iter() {
            let Some((b_owner, _, _, br, bhp, _)) = ctx.building_snapshot[bi] else {
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
        }
    }

    fn apply_unit_damage_events(
        &mut self,
        hq_levels: &HashMap<PlayerId, u8>,
        unit_damage_events: Vec<(usize, u16, PlayerId, u8)>,
        building_damage_events: Vec<(usize, u16, PlayerId, u8)>,
        rad_damage_events: Vec<(u8, u8, u16)>,
    ) {
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
