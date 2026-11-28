use serde_json::json;

use crate::types::{ArmyMode, PendingTeleport, PlayerId, SquadOrder};

use super::super::hex::beacon_ext_radius;
use super::super::World;

impl World {
    pub(super) fn cmd_toggle_base_regen(&mut self, player_id: PlayerId) -> Option<u8> {
        if let Some(player) = self.players.get_mut(&player_id) {
            player.regen_enabled = !player.regen_enabled;
            if !player.regen_enabled {
                player.regen_accum = 0.0;
            }
            None
        } else {
            Some(1)
        }
    }

    pub(super) fn cmd_army_command(
        &mut self,
        player_id: PlayerId,
        squad: u8,
        mode: u8,
        target_x: f32,
        target_y: f32,
    ) -> Option<u8> {
        if !target_x.is_finite() || !target_y.is_finite() {
            return None;
        }
        if mode == 2 {
            if let Some(player) = self.players.get_mut(&player_id) {
                if squad == 0 || player.reinforce_squad == Some(squad) {
                    player.reinforce_squad = None;
                } else if player.squads.contains_key(&squad) {
                    player.reinforce_squad = Some(squad);
                }
            }
            return None;
        }
        if let Some(m) = ArmyMode::from_u8(mode) {
            let (outside_zone, in_base_vision, zone_ring) = self
                .zones
                .get(&player_id)
                .map(|z| {
                    let d = (target_x - z.x).hypot(target_y - z.y);
                    (
                        d > z.radius,
                        d <= z.radius + 360.0,
                        self.ring_for_radius(z.x.hypot(z.y)),
                    )
                })
                .unwrap_or((false, false, 1));
            let mut changed = false;
            let mut is_bot = false;
            let mut assigned_squad: Option<u8> = None;

            if m == ArmyMode::Defend {
                if let Some(player) = self.players.get_mut(&player_id) {
                    is_bot = player.is_bot;
                    if squad == 0 {
                        if player.base_rally.is_some() || player.army_mode != ArmyMode::Defend {
                            changed = true;
                        }
                        player.base_rally = None;
                        if player.squads.is_empty() {
                            player.army_mode = ArmyMode::Defend;
                        }
                    } else if player.squads.remove(&squad).is_some() {
                        if player.reinforce_squad == Some(squad) {
                            player.reinforce_squad = None;
                        }
                        changed = true;
                    }
                }
                if squad > 0 {
                    for u in &mut self.units {
                        if u.owner_id == player_id && u.squad == squad {
                            u.squad = 0;
                        }
                    }
                }
            } else if squad == 0 {
                let has_base_units = self
                    .units
                    .iter()
                    .any(|u| u.owner_id == player_id && u.hp > 0 && u.squad == 0);
                if has_base_units {
                    if in_base_vision {
                        if let Some(player) = self.players.get_mut(&player_id) {
                            is_bot = player.is_bot;
                            changed = true;
                            player.army_mode = ArmyMode::Rally;
                            player.rally_x = target_x;
                            player.rally_y = target_y;
                            player.base_rally = if outside_zone {
                                Some(SquadOrder { target_x, target_y })
                            } else {
                                None
                            };
                        }
                    } else {
                        if let Some(player) = self.players.get_mut(&player_id) {
                            is_bot = player.is_bot;
                            changed = player.army_mode != ArmyMode::Rally;
                            player.army_mode = ArmyMode::Rally;
                            player.rally_x = target_x;
                            player.rally_y = target_y;
                            player.base_rally = None;
                            if outside_zone {
                                player.spawn_protection_left = 0.0;
                            }
                            let new_id = (1..=5_u8)
                                .find(|id| !player.squads.contains_key(id))
                                .unwrap_or_else(|| *player.squads.keys().min().unwrap_or(&1));
                            player.next_squad_id = new_id;
                            player
                                .squads
                                .insert(new_id, SquadOrder { target_x, target_y });
                            assigned_squad = Some(new_id);
                        }
                        if let Some(sid) = assigned_squad {
                            for u in &mut self.units {
                                if u.owner_id == player_id && u.hp > 0 && u.squad == 0 {
                                    u.squad = sid;
                                }
                            }
                        }
                    }
                }
            } else {
                let has_squad_units = self
                    .units
                    .iter()
                    .any(|u| u.owner_id == player_id && u.hp > 0 && u.squad == squad);
                if has_squad_units {
                    if let Some(player) = self.players.get_mut(&player_id) {
                        is_bot = player.is_bot;
                        changed = true;
                        player.army_mode = ArmyMode::Rally;
                        player.rally_x = target_x;
                        player.rally_y = target_y;
                        if outside_zone {
                            player.spawn_protection_left = 0.0;
                        }
                        player
                            .squads
                            .insert(squad, SquadOrder { target_x, target_y });
                    }
                }
            }
            if changed {
                let to_ring = self.ring_for_radius(target_x.hypot(target_y));
                if let Some(tlog) = self.telemetry.as_mut() {
                    let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                    let cmd = if m == ArmyMode::Rally {
                        "rally"
                    } else {
                        "defend"
                    };
                    tlog.log_action(
                        self.tick,
                        sec,
                        player_id,
                        is_bot,
                        cmd,
                        zone_ring,
                        json!({"to_ring": to_ring}),
                    );
                }
            }
        }
        None
    }

    pub(super) fn cmd_teleport_zone(
        &mut self,
        player_id: PlayerId,
        target_x: f32,
        target_y: f32,
    ) -> Option<u8> {
        if !target_x.is_finite() || !target_y.is_finite() {
            return Some(12);
        }
        let already_charging = self
            .players
            .get(&player_id)
            .map(|p| p.pending_teleport.is_some())
            .unwrap_or(false);
        if already_charging {
            return Some(11);
        }
        let Some(zone) = self.zones.get(&player_id).cloned() else {
            return Some(1);
        };
        let target_r = target_x.hypot(target_y);
        let hq_lvl = self.hq_level_of(player_id);
        let r_eq = self.equilibrium_radius(player_id);
        let max_r = self.max_radius_for_hq(hq_lvl, zone.radius);
        if target_r > max_r {
            return Some(12);
        }
        if !self.has_vision_at(player_id, target_x, target_y) {
            return Some(18);
        }

        let zone_clearance = self.config.world.teleport_zone_clearance;
        for other in self.zones.values() {
            if other.player_id == player_id {
                continue;
            }
            if (target_x - other.x).hypot(target_y - other.y)
                < zone.radius + other.radius + zone_clearance
            {
                return Some(13);
            }
        }
        let node_clearance = self.config.world.teleport_node_clearance;
        for b in &self.buildings {
            if b.owner_id != player_id && b.kind == 5 {
                if let Some(other) = self.zones.get(&b.owner_id) {
                    let ext_r = beacon_ext_radius(b.level);
                    if (target_x - (other.x + b.rel_x)).hypot(target_y - (other.y + b.rel_y))
                        < zone.radius + ext_r + node_clearance
                    {
                        return Some(13);
                    }
                }
            }
        }

        let dist = (target_x - zone.x).hypot(target_y - zone.y);
        let ring_pen = self.config.world.teleport_ring_penalty;
        let cost = (self.config.world.teleport_base_cost
            + dist * self.config.world.teleport_dist_factor
            + (target_r - r_eq).abs() * ring_pen)
            .round();
        let charge_sec = self.config.world.teleport_charge_sec.max(0.05);

        let Some(player) = self.players.get_mut(&player_id) else {
            return Some(1);
        };
        if player.energy < cost {
            return Some(2);
        }
        player.energy -= cost;
        player.spawn_protection_left = 0.0;
        player.last_teleport_sec = player.survival_sec;
        player.pending_teleport = Some(PendingTeleport {
            target_x,
            target_y,
            remaining_sec: charge_sec,
            total_sec: charge_sec,
        });
        let is_bot = player.is_bot;
        let ring = self.ring_for_radius(target_r);
        if let Some(tlog) = self.telemetry.as_mut() {
            let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
            tlog.log_action(
                self.tick,
                sec,
                player_id,
                is_bot,
                "tp",
                ring,
                json!({"cost": cost}),
            );
        }
        None
    }

    pub(in super::super) fn step_teleport(&mut self, dt: f32) {
        let pull_speed = self.config.world.teleport_patrol_pull_speed;
        let map_r = self.config.world.map_radius;
        let mut charging: Vec<(PlayerId, f32, f32)> = Vec::new();
        let mut completed: Vec<(PlayerId, f32, f32)> = Vec::new();

        for (&pid, player) in &mut self.players {
            let Some(tp) = player.pending_teleport.as_mut() else {
                continue;
            };
            tp.remaining_sec -= dt;
            if tp.remaining_sec <= 0.0 {
                let (tx, ty) = (tp.target_x, tp.target_y);
                player.pending_teleport = None;
                player.base_rally = None;
                completed.push((pid, tx, ty));
            } else if let Some(z) = self.zones.get(&pid) {
                charging.push((pid, z.x, z.y));
            }
        }

        if !charging.is_empty() {
            let max_step = pull_speed * dt;
            for u in &mut self.units {
                if u.hp == 0 || u.squad != 0 {
                    continue;
                }
                if let Some(&(_, zx, zy)) = charging.iter().find(|&&(pid, _, _)| pid == u.owner_id) {
                    let dx = zx - u.x;
                    let dy = zy - u.y;
                    let dist = dx.hypot(dy);
                    if dist > 1.0 {
                        let step = dist.min(max_step);
                        u.x += (dx / dist) * step;
                        u.y += (dy / dist) * step;
                    }
                }
            }
        }

        for (pid, tx, ty) in completed {
            let Some((dx, dy, from_r)) = self.zones.get_mut(&pid).map(|z| {
                let from_r = z.x.hypot(z.y);
                let dx = tx - z.x;
                let dy = ty - z.y;
                z.x = tx;
                z.y = ty;
                (dx, dy, from_r)
            }) else {
                continue;
            };

            let clamp_r = (map_r - 16.0).max(0.0);
            let clamp_r2 = clamp_r * clamp_r;
            for u in &mut self.units {
                if u.owner_id == pid && u.hp > 0 && u.squad == 0 {
                    u.x += dx;
                    u.y += dy;
                    let d2 = u.x * u.x + u.y * u.y;
                    if d2 > clamp_r2 {
                        let d = d2.sqrt();
                        if d > 0.001 {
                            u.x = u.x / d * clamp_r;
                            u.y = u.y / d * clamp_r;
                        }
                    }
                }
            }

            if let Some(tlog) = self.telemetry.as_mut() {
                let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
                let target_r = tx.hypot(ty);
                tlog.log_milestone(
                    self.tick,
                    sec,
                    pid,
                    "teleport",
                    json!({"from_r": from_r.round(), "to_r": target_r.round()}),
                );
            }
        }
    }
}
