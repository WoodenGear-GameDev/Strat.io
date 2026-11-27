use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

use serde_json::json;

use crate::telemetry::{
    tag_name, DST_BUILDING_OFFSET, SRC_RAD, SRC_TURRET_1, SRC_TURRET_2, SRC_TURRET_3,
};
use crate::types::{BuildCombatLock, EntityId, GameOverEvent, PlayerId, Projectile};

use super::World;

impl World {
    pub(super) fn step_turrets(&mut self, dt: f32) {
        let gen_cfg = self.config.building_by_kind(1);
        let gen_aura_r = gen_cfg
            .map(|c| c.effects.aura_radius)
            .filter(|&v| v > 0.0)
            .unwrap_or(140.0);
        let gen_fr_mult = gen_cfg
            .map(|c| c.effects.fire_rate_mult)
            .filter(|&v| v > 1.0)
            .unwrap_or(1.18);

        let mut new_projectiles: Vec<Projectile> = Vec::new();

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
                // (target_id, target_idx, is_building, tx, ty)
                let mut target_info: Option<(EntityId, usize, bool, f32, f32)> = None;

                unit_grid.for_each_in_radius(bx, by, base_range, |ui, ux, uy, d2| {
                    let u = &units[ui];
                    if u.owner_id == owner_id || u.hp == 0 {
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
                    let mut fire_rate = lvl.fire_rate;
                    let mut has_gen_aura = false;
                    building_grid.for_each_in_radius(bx, by, gen_aura_r, |bi, _, _, _| {
                        if !has_gen_aura {
                            if let Some((_, bo, _, _, _, bhp, bk)) = building_snap[bi] {
                                if bo == owner_id && bk == 1 && bhp > 0 {
                                    has_gen_aura = true;
                                }
                            }
                        }
                    });
                    if has_gen_aura {
                        fire_rate *= gen_fr_mult;
                    }

                    let base_dmg = lvl.damage;
                    let proj_speed = lvl.projectile_speed.max(120.0);
                    let splash_r = match b.level {
                        1 => 44.0_f32,
                        2 => 48.0_f32,
                        _ => 54.0_f32,
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
                        damage: base_dmg.round() as u16,
                        ttl: flight_sec,
                        splash_radius: splash_r,
                    });
                }
            }
        }

        if !new_projectiles.is_empty() {
            for p in &mut new_projectiles {
                p.id = self.alloc_entity_id();
            }
            self.projectiles.extend(new_projectiles);
        }
    }

    pub(super) fn step_projectiles(&mut self, dt: f32) {
        if self.projectiles.is_empty() {
            return;
        }

        let mut tlog = self.telemetry.take();
        let owner_rings: HashMap<PlayerId, u8> = if tlog.is_some() {
            self.zones
                .iter()
                .map(|(&pid, z)| (pid, Self::ring_for_radius((z.x * z.x + z.y * z.y).sqrt())))
                .collect()
        } else {
            HashMap::new()
        };

        let mut damaged_owners: HashSet<PlayerId> = HashSet::new();

        {
            let unit_grid = &self.unit_grid;
            let unit_snap = &mut self.unit_world_snap;
            let building_snap = &mut self.building_world_snap;
            let units = &mut self.units;
            let buildings = &mut self.buildings;
            let projectiles = &mut self.projectiles;
            let players = &self.players;

            projectiles.retain_mut(|p| {
                p.ttl -= dt;
                if p.ttl > 0.0 {
                    // Pure visual flight step — zero spatial queries while airborne!
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    return true;
                }

                // Delayed-hit impact: guaranteed hit on target_id regardless of dt size (no tunneling).
                let att_hq = players.get(&p.owner_id).map(|pl| pl.hq_level).unwrap_or(1);
                let mut impact_x = p.target_x;
                let mut impact_y = p.target_y;
                let mut direct_hit_unit_idx: Option<usize> = None;

                if p.target_id != 0 {
                    if !p.target_is_building {
                        let resolved = if units
                            .get(p.target_idx)
                            .is_some_and(|u| u.id == p.target_id)
                        {
                            Some(p.target_idx)
                        } else {
                            units.iter().position(|u| u.id == p.target_id)
                        };
                        if let Some(ui) = resolved {
                            let u = &mut units[ui];
                            impact_x = u.x;
                            impact_y = u.y;
                            if u.owner_id != p.owner_id && u.hp > 0 {
                                let protected = u.owner_id != 0
                                    && players
                                        .get(&u.owner_id)
                                        .is_some_and(|pl| pl.spawn_protection_left > 0.0);
                                if !protected {
                                    let dmg_amt = if u.owner_id != 0 {
                                        let def_hq = players
                                            .get(&u.owner_id)
                                            .map(|pl| pl.hq_level)
                                            .unwrap_or(1);
                                        (f32::from(p.damage)
                                            * Self::tier_dmg_mult(att_hq, def_hq, false))
                                        .round()
                                        .max(1.0) as u16
                                    } else {
                                        p.damage
                                    };
                                    let dealt = u.hp.min(dmg_amt);
                                    u.hp = u.hp.saturating_sub(dmg_amt);
                                    unit_snap[ui].4 = u.hp;
                                    u.last_hit_by = Some(p.owner_id);
                                    u.last_hit_src = p.src_tag;
                                    if u.owner_id != 0 {
                                        damaged_owners.insert(u.owner_id);
                                    }
                                    if let Some(tl) = tlog.as_mut() {
                                        let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                                        let dr = Self::ring_for_radius(
                                            (impact_x * impact_x + impact_y * impact_y).sqrt(),
                                        );
                                        tl.log_damage(p.src_tag, u.kind, sr, dr, dealt);
                                    }
                                    direct_hit_unit_idx = Some(ui);
                                }
                            }
                        }
                    } else {
                        let resolved = if buildings
                            .get(p.target_idx)
                            .is_some_and(|b| b.id == p.target_id)
                        {
                            Some(p.target_idx)
                        } else {
                            buildings.iter().position(|b| b.id == p.target_id)
                        };
                        if let Some(bi) = resolved {
                            if let Some((_, _, bx, by, _, _, _)) = building_snap[bi] {
                                impact_x = bx;
                                impact_y = by;
                            }
                            let b = &mut buildings[bi];
                            if b.owner_id != p.owner_id && b.hp > 0 {
                                let (protected, def_hq) = players
                                    .get(&b.owner_id)
                                    .map(|pl| (pl.spawn_protection_left > 0.0, pl.hq_level))
                                    .unwrap_or((false, 1));
                                if !protected {
                                    let dmg_amt = (f32::from(p.damage)
                                        * Self::tier_dmg_mult(att_hq, def_hq, b.kind == 0))
                                    .round()
                                    .max(1.0) as u16;
                                    let dealt = b.hp.min(dmg_amt);
                                    b.hp = b.hp.saturating_sub(dmg_amt);
                                    if let Some(ref mut bs) = building_snap[bi] {
                                        bs.5 = b.hp;
                                    }
                                    b.last_hit_by = Some(p.owner_id);
                                    b.last_hit_src = p.src_tag;
                                    damaged_owners.insert(b.owner_id);
                                    if let Some(tl) = tlog.as_mut() {
                                        let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                                        let dr = Self::ring_for_radius(
                                            (impact_x * impact_x + impact_y * impact_y).sqrt(),
                                        );
                                        tl.log_damage(
                                            p.src_tag,
                                            DST_BUILDING_OFFSET + b.kind,
                                            sr,
                                            dr,
                                            dealt,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                if p.splash_radius > 0.0 {
                    let splash_base = (p.damage / 2).max(1);
                    unit_grid.for_each_in_radius(
                        impact_x,
                        impact_y,
                        p.splash_radius,
                        |ui, ux, uy, _d2| {
                            if Some(ui) == direct_hit_unit_idx {
                                return;
                            }
                            let u = &mut units[ui];
                            if u.owner_id == p.owner_id || u.hp == 0 {
                                return;
                            }
                            let splash_dmg = if u.owner_id != 0 {
                                let def_hq = if let Some(pl) = players.get(&u.owner_id) {
                                    if pl.spawn_protection_left > 0.0 {
                                        return;
                                    }
                                    pl.hq_level
                                } else {
                                    1
                                };
                                (f32::from(splash_base)
                                    * Self::tier_dmg_mult(att_hq, def_hq, false))
                                .round()
                                .max(1.0) as u16
                            } else {
                                splash_base
                            };
                            let dealt = u.hp.min(splash_dmg);
                            u.hp = u.hp.saturating_sub(splash_dmg);
                            unit_snap[ui].4 = u.hp;
                            u.last_hit_by = Some(p.owner_id);
                            u.last_hit_src = p.src_tag;
                            if u.owner_id != 0 {
                                damaged_owners.insert(u.owner_id);
                            }
                            if let Some(tl) = tlog.as_mut() {
                                let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                                let dr = Self::ring_for_radius((ux * ux + uy * uy).sqrt());
                                tl.log_damage(p.src_tag, u.kind, sr, dr, dealt);
                            }
                        },
                    );
                }

                false
            });
        }

        for pid in damaged_owners {
            if let Some(pl) = self.players.get_mut(&pid) {
                pl.last_damaged_sec = pl.survival_sec;
            }
        }
        self.telemetry = tlog;
    }

    pub(super) fn step_cleanup(&mut self) -> Vec<GameOverEvent> {
        let any_dead_unit = self.units.iter().any(|u| u.hp == 0);
        let any_dead_building = self.buildings.iter().any(|b| b.hp == 0);
        if !any_dead_unit && !any_dead_building {
            return Vec::new();
        }

        let mut tlog = self.telemetry.take();
        let owner_rings: HashMap<PlayerId, u8> = if tlog.is_some() {
            self.zones
                .iter()
                .map(|(&pid, z)| (pid, Self::ring_for_radius((z.x * z.x + z.y * z.y).sqrt())))
                .collect()
        } else {
            HashMap::new()
        };

        if any_dead_unit {
            let mut unit_killers: Vec<(PlayerId, PlayerId, u8, u8, u8)> = Vec::new();
            self.units.retain(|u| {
                if u.hp == 0 {
                    let dr = Self::ring_for_radius((u.x * u.x + u.y * u.y).sqrt());
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

            for (k, victim_id, ukind, killer_src, death_ring) in unit_killers {
                let turret_kill = (20..=22).contains(&killer_src);
                let unit_cost = self.unit_energy_cost(ukind);
                let killer_hq = self.hq_level_of(k);
                let victim_hq = if victim_id == 0 {
                    1
                } else {
                    self.hq_level_of(victim_id)
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
        }

        if !any_dead_building {
            self.telemetry = tlog;
            return Vec::new();
        }

        let mut building_destroyers: Vec<(PlayerId, PlayerId, u8, bool, f32, f32)> = Vec::new();
        let mut hq_destroyed_owners: Vec<(PlayerId, Option<PlayerId>, u8)> = Vec::new();
        let mut affected_cap_owners: HashSet<PlayerId> = HashSet::new();
        let mut beacon_destroyed_owners: HashSet<PlayerId> = HashSet::new();
        let mut new_combat_locks: Vec<(PlayerId, BuildCombatLock)> = Vec::new();

        {
            let config = &self.config;
            let zones = &self.zones;
            let players = &self.players;
            let buildings = &mut self.buildings;

            buildings.retain(|b| {
                if b.hp == 0 {
                    let ring = zones
                        .get(&b.owner_id)
                        .map(|z| Self::ring_for_radius((z.x * z.x + z.y * z.y).sqrt()))
                        .unwrap_or(2);
                    if let Some(tl) = tlog.as_mut() {
                        let sr = b
                            .last_hit_by
                            .and_then(|k| owner_rings.get(&k).copied())
                            .unwrap_or(0);
                        tl.log_kill(b.last_hit_src, DST_BUILDING_OFFSET + b.kind, sr, ring);
                    }
                    if let Some(k) = b.last_hit_by {
                        let victim_t = if b.kind == 0 {
                            players.get(&b.owner_id).map(|p| p.trophies).unwrap_or(0.0)
                        } else {
                            0.0
                        };
                        building_destroyers.push((
                            k,
                            b.owner_id,
                            ring,
                            b.kind == 0,
                            b.invested_cost,
                            victim_t,
                        ));
                    }
                    if b.kind == 0 {
                        hq_destroyed_owners.push((b.owner_id, b.last_hit_by, b.last_hit_src));
                    } else {
                        let br = config
                            .building_by_kind(b.kind)
                            .map(|c| c.radius)
                            .unwrap_or(22.0);
                        let now_sec = players
                            .get(&b.owner_id)
                            .map(|p| p.survival_sec)
                            .unwrap_or(0.0);
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
        }

        for (owner_id, lock) in new_combat_locks {
            if let Some(p) = self.players.get_mut(&owner_id) {
                let now_sec = p.survival_sec;
                p.build_combat_locks.retain(|lk| lk.expires_at_sec > now_sec);
                p.build_combat_locks.push(lock);
            }
        }

        for (k, victim_id, ring, is_hq, invested_cost, victim_t) in building_destroyers {
            let killer_hq = self.hq_level_of(k);
            let victim_hq = self.hq_level_of(victim_id);
            let (t_tier_mult, e_tier_mult) = Self::tier_reward_mult(killer_hq, victim_hq);
            let ring_bonus = if ring >= 3 { 1.2 } else { 1.0 };

            let base_e = if is_hq {
                (invested_cost * 0.60).max(140.0)
            } else {
                (invested_cost * 0.60).max(30.0)
            };
            let base_t = if is_hq {
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
            self.invalidate_base_grid(owner_id);
            self.prune_orphaned_buildings(owner_id);
        }
        for owner_id in affected_cap_owners {
            self.recompute_unit_cap(owner_id);
            self.invalidate_base_grid(owner_id);
        }

        let mut game_overs = Vec::new();
        for (owner_id, killer_id, fatal_src) in hq_destroyed_owners {
            if let Some(tl) = tlog.as_mut() {
                if let Some(p) = self.players.get(&owner_id) {
                    let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                    let v_ring = self
                        .zones
                        .get(&owner_id)
                        .map(|z| Self::ring_for_radius((z.x * z.x + z.y * z.y).sqrt()))
                        .unwrap_or(1);
                    let v_req = self.equilibrium_radius(owner_id);
                    let k_info = killer_id.and_then(|kid| {
                        self.players.get(&kid).map(|kp| {
                            let k_ring = self
                                .zones
                                .get(&kid)
                                .map(|z| Self::ring_for_radius((z.x * z.x + z.y * z.y).sqrt()))
                                .unwrap_or(0);
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
