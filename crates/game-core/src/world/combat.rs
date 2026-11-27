use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

use serde_json::json;

use crate::spatial::SpatialGrid;
use crate::telemetry::{
    tag_name, DST_BUILDING_OFFSET, SRC_RAD, SRC_TURRET_1, SRC_TURRET_2, SRC_TURRET_3,
};
use crate::types::{BuildCombatLock, GameOverEvent, PlayerId, Projectile};

use super::World;

impl World {
    pub(super) fn step_turrets(&mut self, dt: f32) {
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

    pub(super) fn step_projectiles(&mut self, dt: f32) {
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
                        let dmg_amt =
                            (f32::from(p.damage) * Self::tier_dmg_mult(att_hq, def_hq, b.kind == 0))
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

    pub(super) fn step_cleanup(&mut self) -> Vec<GameOverEvent> {
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
                            expires_at_sec: now_sec + 20.0,
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
}
