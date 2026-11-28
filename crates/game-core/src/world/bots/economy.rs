use rand::Rng;

use crate::protocol::ClientMessage;
use crate::types::{EntityId, PlayerId};

use super::super::hex::{hex_to_rel, is_large_building};
use super::super::World;
use super::intel::BotPlayerIntel;
use super::roster::archetype_profile;

impl World {
    /// Configures barrack unit types by archetype for a balanced Raider/Archer/Titan mix.
    pub(super) fn bot_configure_barracks(
        &mut self,
        pid: PlayerId,
        arch: u8,
        barracks_info: &[(EntityId, u8, u8)],
    ) {
        for (idx, &(bid, blvl, bmeta)) in barracks_info.iter().enumerate() {
            let slot = idx % 4;
            let desired_utype = match arch {
                1 => match blvl {
                    3.. => [2_u8, 1, 2, 0][slot],
                    2 => [1_u8, 1, 0, 1][slot],
                    _ => 0,
                },
                2 => match blvl {
                    3.. => [0_u8, 1, 0, 2][slot],
                    2 => [0_u8, 1, 0, 0][slot],
                    _ => 0,
                },
                _ => match blvl {
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
        intel: &BotPlayerIntel,
    ) -> Option<(f32, f32)> {
        let _ = self.ensure_base_grid(pid);
        let Some(cache) = self.base_grid_cache.get(&pid) else {
            return None;
        };
        let rng = &mut self.rng;

        if is_large_building(build_kind) {
            let mut best: Option<((f32, f32), f32)> = None;
            for &(q, r) in &cache.free_hexes {
                let (cx, cy) = hex_to_rel(q, r);
                let dist_hq2 = cx * cx + cy * cy;
                let jitter = rng.gen_range(0.0..6.0);
                let score = if build_kind == 2 {
                    let dist_hq = dist_hq2.sqrt();
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
                    ring_pref + min_t_dist2.sqrt() * 0.3 + jitter
                } else if build_kind == 5 {
                    let dist_hq = dist_hq2.sqrt();
                    let min_b_dist2 = intel
                        .beacon_positions
                        .iter()
                        .map(|&(bx, by)| {
                            let dx = cx - bx;
                            let dy = cy - by;
                            dx * dx + dy * dy
                        })
                        .fold(f32::MAX, f32::min);
                    let spacing_bonus = if min_b_dist2 >= 110.0 * 110.0 {
                        350.0
                    } else {
                        min_b_dist2.sqrt() * 2.0
                    };
                    dist_hq * 1.4 + spacing_bonus + jitter
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
            let mut best: Option<((f32, f32), f32)> = None;
            for &(tris, (mx, my)) in &cache.free_rhombuses {
                let dist_hq = (mx * mx + my * my).sqrt();
                let jitter = rng.gen_range(0.0..5.0);
                let mut shared_hex_bonus = 0.0_f32;
                for (hq_q, hq_r, _) in tris {
                    if (0..6_u8).any(|s| cache.occupied_tris.contains(&(hq_q, hq_r, s))) {
                        shared_hex_bonus += 65.0;
                    }
                }
                let score = shared_hex_bonus - dist_hq * 0.15 + jitter;
                if best.as_ref().is_none_or(|&(_, bs)| score > bs) {
                    best = Some(((mx, my), score));
                }
            }
            best.map(|(pos, _)| pos)
        }
    }

    /// Executes up to `max_actions` prioritized upgrades and builds for a single bot think tick.
    pub(super) fn bot_step_economy(
        &mut self,
        pid: PlayerId,
        tier: u8,
        arch: u8,
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

            // 1. Always upgrade HQ first once affordable
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
            let opener_kind = if intel.gens < 5 && free_rhombuses > 0 {
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
                    if let Some((bx, by)) = self.bot_pick_build_slot(pid, bk, intel) {
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

            // 3. Evaluate best affordable upgrade candidate
            let mut best_upgrade: Option<(EntityId, f32)> = None;
            let max_allowed_lvl = intel.hq_lvl.saturating_add(1).min(max_b_lvl);
            for &(bid, kind, lvl) in &intel.non_hq_buildings {
                if lvl >= max_allowed_lvl {
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

            // 4. Full-base expansion using soft archetype ratios/priorities
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
            let target_gens = 5 + usize::from(intel.hq_lvl) * 5 + profile.extra_gens * 2;
            let turret_ratio = if profile.extra_turrets { 2 } else { 3 };
            let need_eco_for_barracks =
                intel.gens < (intel.barracks + intel.turrets) && free_rhombuses > 0;
            let beacon_trophy_cost = self.node_trophy_cost(intel.beacons as i32);

            let build_choice: Option<(u8, f32)> = if intel.turrets < min_turrets && free_hexes > 0
            {
                Some((2, if threatened { 145.0 } else { 122.0 }))
            } else if need_eco_for_barracks {
                Some((1, 118.0))
            } else if intel.hq_lvl >= 3
                && free_hexes > 0
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
                    if intel.gens < usize::from(intel.hq_lvl) * 5 {
                        104.0
                    } else {
                        78.0
                    },
                ))
            } else if free_hexes > 0 {
                let want_turret = if arch == 4 {
                    intel.barracks >= 2
                } else {
                    intel.turrets * turret_ratio < intel.barracks
                };
                Some((if want_turret { 2 } else { 3 }, 94.0))
            } else if free_rhombuses > 0 {
                Some((1, 72.0))
            } else {
                None
            };

            let affordable_build = build_choice.and_then(|(bk, sc)| {
                let c = self.building_build_cost(bk);
                let tc = if bk == 5 { beacon_trophy_cost } else { 0.0 };
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
                if let Some((bx, by)) = self.bot_pick_build_slot(pid, bk, intel) {
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
}
