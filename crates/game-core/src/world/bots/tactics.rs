use std::collections::HashMap;
use std::f32::consts::TAU;

use serde_json::json;

use crate::protocol::ClientMessage;
use crate::types::{BotKnownTarget, PlayerId, SquadOrder, Zone};

use super::super::World;
use super::intel::{bot_has_vision_fast, BotPlayerIntel};
use super::roster::ArchetypeProfile;

impl World {
    /// Priority defense: recalls nearby combat squad and cancels auto-reinforce if base is under threat.
    pub(super) fn bot_handle_defense(
        &mut self,
        pid: PlayerId,
        bot_intel: &BotPlayerIntel,
        own_z: &Zone,
        reinforce_sq: Option<u8>,
        live_squads: &[(u8, usize, f32, f32)],
        squad_orders: &HashMap<u8, SquadOrder>,
    ) -> bool {
        let under_attack = bot_intel.enemy_threat_power >= 2.5
            && bot_intel.defending_unit_power < bot_intel.enemy_threat_power * 0.9;
        if !under_attack {
            return false;
        }
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
        let _ = self.apply_command(
            pid,
            ClientMessage::ArmyCommand {
                squad: 0,
                mode: 0,
                target_x: 0.0,
                target_y: 0.0,
            },
        );
        if bot_intel.enemy_threat_power >= 3.5 {
            let mut closest_sq: Option<(u8, f32)> = None;
            for &(sq, _, cx, cy) in live_squads {
                let dx = cx - own_z.x;
                let dy = cy - own_z.y;
                let d2 = dx * dx + dy * dy;
                if d2 <= 750.0 * 750.0 && closest_sq.is_none_or(|(_, bd2)| d2 < bd2) {
                    closest_sq = Some((sq, d2));
                }
            }
            if let Some((sq, _)) = closest_sq {
                let home_r = own_z.radius + 80.0;
                let home_r2 = home_r * home_r;
                let already_heading_home = squad_orders
                    .get(&sq)
                    .map(|o| {
                        let dx = o.target_x - own_z.x;
                        let dy = o.target_y - own_z.y;
                        dx * dx + dy * dy <= home_r2
                    })
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
        true
    }

    /// Picks best attack target by archetype utility and returns `(garrison_ready, best_target)`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bot_select_target(
        &self,
        arch: u8,
        profile: &ArchetypeProfile,
        own_z: &Zone,
        unit_cap: u16,
        garrison_count: usize,
        bot_intel: &BotPlayerIntel,
        known: &HashMap<PlayerId, BotKnownTarget>,
        live_squads: &[(u8, usize, f32, f32)],
        squad_orders: &HashMap<u8, SquadOrder>,
        leader_id: Option<PlayerId>,
    ) -> (bool, Option<(f32, f32)>) {
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
        let base_army_power = if garrison_ready {
            bot_intel
                .defending_unit_power
                .max(bot_intel.own_combat_power * 0.65)
        } else {
            bot_intel.own_combat_power
        };
        let active_combat_power = if arch == 4 {
            base_army_power + bot_intel.turret_power * 0.75
        } else {
            base_army_power
        };

        let mut best_target: Option<(f32, f32)> = None;
        let mut best_utility = f32::NEG_INFINITY;

        for (&target_id, kt) in known {
            let Some(tp) = self.players.get(&target_id) else {
                continue;
            };
            let Some(cur_z) = self.zones.get(&target_id) else {
                continue;
            };
            let currently_visible = bot_has_vision_fast(own_z, bot_intel, cur_z.x, cur_z.y);
            if currently_visible && tp.spawn_protection_left > 0.0 {
                continue;
            }

            let hq_base_def = (f32::from(kt.hq_hp) / 700.0).max(0.75);
            let effective_attack = active_combat_power;
            let effective_defense = kt.turret_power + kt.defending_unit_power * 1.1 + hq_base_def;
            let win_ratio = effective_attack / effective_defense;
            let current_primary_order = live_squads
                .first()
                .and_then(|&(sq, _, _, _)| squad_orders.get(&sq).copied());
            let is_current_target = current_primary_order
                .map(|o| {
                    let dx = kt.x - o.target_x;
                    let dy = kt.y - o.target_y;
                    dx * dx + dy * dy <= 240.0 * 240.0
                })
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
            let dx = kt.x - own_z.x;
            let dy = kt.y - own_z.y;
            let dist = (dx * dx + dy * dy).sqrt();
            let utility = (crown_mult
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

        (garrison_ready, best_target)
    }

    /// Dispatches multi-squad attack orders, early scouting, and auto-reinforce.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bot_dispatch_orders(
        &mut self,
        pid: PlayerId,
        tier: u8,
        arch: u8,
        profile: &ArchetypeProfile,
        own_z: &Zone,
        own_r: f32,
        own_r_eq: f32,
        garrison_count: usize,
        garrison_ready: bool,
        reinforce_sq: Option<u8>,
        live_squads: &[(u8, usize, f32, f32)],
        squad_orders: &HashMap<u8, SquadOrder>,
        best_target: Option<(f32, f32)>,
        scout_ang: &mut f32,
    ) {
        if let Some((tx, ty)) = best_target {
            let tdx = tx - own_z.x;
            let tdy = ty - own_z.y;
            let target_dist2 = tdx * tdx + tdy * tdy;
            for &(sq, _, _, _) in live_squads {
                let already_on_target = squad_orders
                    .get(&sq)
                    .map(|o| {
                        let dx = o.target_x - tx;
                        let dy = o.target_y - ty;
                        dx * dx + dy * dy <= 120.0 * 120.0
                    })
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
            if garrison_ready {
                let from_ring = self.ring_for_radius(own_r);
                let to_ring = self.ring_for_radius((tx * tx + ty * ty).sqrt());
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
            let main_sq = self
                .players
                .get(&pid)
                .and_then(|p| p.squads.keys().copied().min());
            if profile.uses_reinforce && target_dist2 > 550.0 * 550.0 {
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
                for &(sq, _, cx, cy) in live_squads {
                    let need_next_wp = squad_orders
                        .get(&sq)
                        .map(|o| {
                            let dx = cx - o.target_x;
                            let dy = cy - o.target_y;
                            dx * dx + dy * dy <= 170.0 * 170.0
                        })
                        .unwrap_or(true);
                    if need_next_wp {
                        *scout_ang = (*scout_ang + 1.15).rem_euclid(TAU);
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
                *scout_ang = (*scout_ang + 1.15).rem_euclid(TAU);
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
    }
}
