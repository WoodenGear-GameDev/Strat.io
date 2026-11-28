use std::collections::HashMap;

use serde_json::json;

use crate::telemetry::{tag_name, DST_BUILDING_OFFSET};
use crate::types::{GameOverEvent, PlayerId};

use super::super::World;

impl World {
    pub(in super::super) fn step_cleanup(&mut self) -> Vec<GameOverEvent> {
        let any_dead_unit = self.units.iter().any(|u| u.hp == 0);
        let any_dead_building = self.buildings.iter().any(|b| b.hp == 0);
        if !any_dead_unit && !any_dead_building {
            return Vec::new();
        }

        let ring3_r = self.config.world.ring3_radius;
        let ring2_r = self.config.world.ring2_radius;
        let mut tlog = self.telemetry.take();
        let owner_rings: HashMap<PlayerId, u8> = if tlog.is_some() {
            self.zones
                .iter()
                .map(|(&pid, z)| {
                    (
                        pid,
                        Self::ring_for_radius_with(
                            (z.x * z.x + z.y * z.y).sqrt(),
                            ring3_r,
                            ring2_r,
                        ),
                    )
                })
                .collect()
        } else {
            HashMap::new()
        };

        if any_dead_unit {
            let mut unit_killers: Vec<(PlayerId, u8, u8, u8)> = Vec::new();
            self.units.retain(|u| {
                if u.hp == 0 {
                    let dr = Self::ring_for_radius_with(
                        (u.x * u.x + u.y * u.y).sqrt(),
                        ring3_r,
                        ring2_r,
                    );
                    if let Some(tl) = tlog.as_mut() {
                        let sr = u
                            .last_hit_by
                            .and_then(|k| owner_rings.get(&k).copied())
                            .unwrap_or(0);
                        tl.log_kill(u.last_hit_src, u.kind, sr, dr);
                    }
                    if let Some(k) = u.last_hit_by {
                        unit_killers.push((k, u.kind, u.last_hit_src, dr));
                    }
                    false
                } else {
                    true
                }
            });

            for (k, ukind, killer_src, death_ring) in unit_killers {
                let turret_kill = (20..=22).contains(&killer_src);
                let unit_cost = self.unit_energy_cost(ukind);
                let ring_bonus = if death_ring >= 3 { 1.2 } else { 1.0 };

                let base_e = unit_cost * 0.35;
                let base_t = unit_cost * if turret_kill { 0.12 } else { 0.32 };
                let base_s = if turret_kill { 8.0 } else { unit_cost * 0.9 };

                let gained_t = (base_t * ring_bonus).round();
                let gained_e = (base_e * ring_bonus).round();
                if let Some(p) = self.players.get_mut(&k) {
                    p.score += (base_s * ring_bonus).round().max(1.0) as u32;
                    p.kills += 1;
                }
                self.grant_trophies(k, gained_t);
                self.grant_energy(k, gained_e, 1.25);
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

        let mut building_destroyers: Vec<(PlayerId, u8, bool, f32, f32)> = Vec::new();
        let mut hq_destroyed_owners: Vec<(PlayerId, Option<PlayerId>, u8)> = Vec::new();
        let mut affected_owners: HashMap<PlayerId, (Option<PlayerId>, u8)> = HashMap::new();

        {
            let zones = &self.zones;
            let players = &self.players;
            let buildings = &mut self.buildings;

            buildings.retain(|b| {
                if b.hp == 0 {
                    let ring = zones
                        .get(&b.owner_id)
                        .map(|z| {
                            Self::ring_for_radius_with(
                                (z.x * z.x + z.y * z.y).sqrt(),
                                ring3_r,
                                ring2_r,
                            )
                        })
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
                            ring,
                            b.kind == 0,
                            b.invested_cost,
                            victim_t,
                        ));
                    }
                    if b.kind == 0 {
                        hq_destroyed_owners.push((b.owner_id, b.last_hit_by, b.last_hit_src));
                    } else {
                        affected_owners
                            .entry(b.owner_id)
                            .and_modify(|e| {
                                if e.0.is_none() && b.last_hit_by.is_some() {
                                    e.0 = b.last_hit_by;
                                }
                            })
                            .or_insert((b.last_hit_by, ring));
                    }
                    false
                } else {
                    true
                }
            });
        }

        // Honest fixed-point BFS detachment on any destroyed non-HQ module;
        // attacker receives 100% reward for every detached module.
        for (owner_id, (killer_opt, ring)) in affected_owners {
            let detached = self.prune_orphaned_buildings(owner_id);
            if let Some(k) = killer_opt {
                for db in detached {
                    building_destroyers.push((k, ring, false, db.invested_cost, 0.0));
                }
            }
        }

        for (k, ring, is_hq, invested_cost, victim_t) in building_destroyers {
            let ring_bonus = if ring >= 3 { 1.2 } else { 1.0 };

            let base_e = if is_hq {
                (invested_cost * 0.60).max(140.0)
            } else {
                (invested_cost * 0.60).max(30.0)
            };
            let base_t = if is_hq {
                80.0 + victim_t
            } else {
                24.0 + invested_cost * 0.08
            };
            let base_s = if is_hq { 250.0 } else { 75.0 };

            let gained_t = (base_t * ring_bonus).round();
            let gained_e = (base_e * ring_bonus).round();
            if let Some(p) = self.players.get_mut(&k) {
                p.score += (base_s * ring_bonus).round().max(1.0) as u32;
                p.buildings_destroyed += 1;
            }
            self.grant_trophies(k, gained_t);
            self.grant_energy(k, gained_e, 1.25);
            if let Some(tl) = tlog.as_mut() {
                tl.log_income(k, 4, gained_t);
            }
        }

        let mut game_overs = Vec::new();
        for (owner_id, killer_id, fatal_src) in hq_destroyed_owners {
            if let Some(tl) = tlog.as_mut() {
                if let Some(p) = self.players.get(&owner_id) {
                    let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                    let v_ring = self
                        .zones
                        .get(&owner_id)
                        .map(|z| {
                            Self::ring_for_radius_with(
                                (z.x * z.x + z.y * z.y).sqrt(),
                                ring3_r,
                                ring2_r,
                            )
                        })
                        .unwrap_or(1);
                    let v_req = self.equilibrium_radius(owner_id);
                    let k_info = killer_id.and_then(|kid| {
                        self.players.get(&kid).map(|kp| {
                            let k_ring = self
                                .zones
                                .get(&kid)
                                .map(|z| {
                                    Self::ring_for_radius_with(
                                        (z.x * z.x + z.y * z.y).sqrt(),
                                        ring3_r,
                                        ring2_r,
                                    )
                                })
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
