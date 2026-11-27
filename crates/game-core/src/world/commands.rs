use serde_json::json;

use crate::protocol::ClientMessage;
use crate::telemetry::SRC_UNKNOWN;
use crate::types::{ArmyMode, Building, EntityId, PlayerId, SquadOrder};

use super::hex::{beacon_ext_radius, hq_zone_radius, RING1_RADIUS};
use super::World;

impl World {
    pub fn apply_command(&mut self, player_id: PlayerId, cmd: ClientMessage) -> Option<u8> {
        if !self.players.contains_key(&player_id) {
            return Some(1);
        }

        match cmd {
            ClientMessage::Build {
                building_kind,
                rel_x,
                rel_y,
            } => self.cmd_build(player_id, building_kind, rel_x, rel_y),
            ClientMessage::UpgradeBuilding { building_id } => {
                self.cmd_upgrade_building(player_id, building_id)
            }
            ClientMessage::SellBuilding { building_id } => {
                self.cmd_sell_building(player_id, building_id)
            }
            ClientMessage::ArmyCommand {
                squad,
                mode,
                target_x,
                target_y,
            } => self.cmd_army_command(player_id, squad, mode, target_x, target_y),
            ClientMessage::ConfigureBuilding { building_id, meta } => {
                self.cmd_configure_building(player_id, building_id, meta)
            }
            ClientMessage::TeleportZone { target_x, target_y } => {
                self.cmd_teleport_zone(player_id, target_x, target_y)
            }
            _ => None,
        }
    }

    fn cmd_build(
        &mut self,
        player_id: PlayerId,
        building_kind: u8,
        rel_x: f32,
        rel_y: f32,
    ) -> Option<u8> {
        if !rel_x.is_finite() || !rel_y.is_finite() {
            return Some(3);
        }
        let Some(cfg) = self.config.building_by_kind(building_kind).cloned() else {
            return Some(6);
        };
        if cfg.kind_id == 0 {
            return Some(6);
        }
        let hq_lvl = self.hq_level_of(player_id);
        if building_kind == 5 && hq_lvl < 3 {
            return Some(14);
        }
        let Some(lvl0) = cfg.levels.first() else {
            return Some(6);
        };

        if let Some(max_count) = cfg.max_per_player {
            let count = self
                .buildings
                .iter()
                .filter(|b| b.owner_id == player_id && b.kind == building_kind)
                .count() as u32;
            if count >= max_count {
                return Some(5);
            }
        }

        let existing_beacons = if building_kind == 5 {
            self.buildings
                .iter()
                .filter(|b| b.owner_id == player_id && b.kind == 5)
                .count() as i32
        } else {
            0
        };
        let trophy_cost = if building_kind == 5 {
            (90.0 * 1.65_f32.powi(existing_beacons)).round()
        } else {
            lvl0.trophy_cost
        };

        let Some((final_x, final_y)) =
            self.find_valid_placement(player_id, building_kind, rel_x, rel_y, 75.0)
        else {
            return Some(3);
        };

        let cost = lvl0.cost;
        let scaled_hp = lvl0.hp;

        let is_bot;
        {
            let Some(player) = self.players.get_mut(&player_id) else {
                return Some(1);
            };
            let now_sec = player.survival_sec;
            player
                .build_combat_locks
                .retain(|lk| lk.expires_at_sec > now_sec);
            if player
                .build_combat_locks
                .iter()
                .any(|lk| (final_x - lk.rel_x).hypot(final_y - lk.rel_y) < lk.radius)
            {
                return Some(16);
            }
            if player.trophies < trophy_cost {
                return Some(15);
            }
            if player.energy < cost {
                return Some(2);
            }
            player.trophies -= trophy_cost;
            player.energy -= cost;
            is_bot = player.is_bot;
        }

        let id = self.alloc_entity_id();
        self.buildings.push(Building {
            id,
            owner_id: player_id,
            kind: building_kind,
            level: 1,
            meta: 0,
            rel_x: final_x,
            rel_y: final_y,
            hp: scaled_hp,
            max_hp: scaled_hp,
            angle: 0,
            cooldown_left: 0.0,
            invested_cost: cost,
            invested_trophies: trophy_cost,
            last_hit_by: None,
            last_hit_src: SRC_UNKNOWN,
        });
        if let Some(tlog) = self.telemetry.as_mut() {
            let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
            let ring = self
                .zones
                .get(&player_id)
                .map(|z| Self::ring_for_radius(z.x.hypot(z.y)))
                .unwrap_or(1);
            tlog.log_action(
                self.tick,
                sec,
                player_id,
                is_bot,
                "build",
                ring,
                json!({"bkind": building_kind, "cost": cost}),
            );
        }
        self.invalidate_base_grid(player_id);
        self.recompute_unit_cap(player_id);
        None
    }

    fn cmd_upgrade_building(&mut self, player_id: PlayerId, building_id: EntityId) -> Option<u8> {
        let Some(idx) = self
            .buildings
            .iter()
            .position(|b| b.id == building_id && b.owner_id == player_id)
        else {
            return Some(7);
        };

        let kind = self.buildings[idx].kind;
        let cur_level = self.buildings[idx].level as usize;
        if kind != 0 {
            let hq_lvl = usize::from(self.hq_level_of(player_id));
            if cur_level >= hq_lvl {
                return Some(9);
            }
        }

        let Some(cfg) = self.config.building_by_kind(kind).cloned() else {
            return Some(6);
        };
        let Some(next_lvl) = cfg.levels.get(cur_level) else {
            return Some(6);
        };

        let cost = next_lvl.cost;
        let trophy_cost = next_lvl.trophy_cost;
        let scaled_hp = next_lvl.hp;

        let is_bot;
        {
            let Some(player) = self.players.get_mut(&player_id) else {
                return Some(1);
            };
            if player.trophies < trophy_cost {
                return Some(15);
            }
            if player.energy < cost {
                return Some(2);
            }
            player.trophies -= trophy_cost;
            player.energy -= cost;
            is_bot = player.is_bot;
        }

        let b = &mut self.buildings[idx];
        let hp_gain = scaled_hp.saturating_sub(b.max_hp);
        b.level += 1;
        b.max_hp = scaled_hp;
        b.hp = b.hp.saturating_add(hp_gain).min(b.max_hp);
        b.invested_cost += cost;
        b.invested_trophies += trophy_cost;
        let new_level = b.level;

        if kind == 0 {
            let base_r = self.config.world.zone_base_radius;
            let new_zone_r = hq_zone_radius(base_r, new_level);
            let target_r = self.equilibrium_radius(player_id);
            let (old_x, old_y, cur_ang) = self
                .zones
                .get(&player_id)
                .map(|z| (z.x, z.y, z.y.atan2(z.x)))
                .unwrap_or((target_r, 0.0, 0.0));
            let (dest_x, dest_y) =
                self.find_safe_orbit_spot(player_id, target_r, new_zone_r, cur_ang);
            if let Some(z) = self.zones.get_mut(&player_id) {
                z.radius = new_zone_r;
                z.x = dest_x;
                z.y = dest_y;
            }
            let shift_x = dest_x - old_x;
            let shift_y = dest_y - old_y;
            if let Some(pl) = self.players.get_mut(&player_id) {
                pl.hq_level = new_level;
                pl.spawn_protection_left = pl.spawn_protection_left.max(3.0);
                if pl.army_mode == ArmyMode::Defend {
                    pl.rally_x = dest_x;
                    pl.rally_y = dest_y;
                }
            }
            let wave_r = new_zone_r + 160.0;
            for u in &mut self.units {
                if u.owner_id == player_id && u.kind != 4 && u.squad == 0 {
                    u.x += shift_x;
                    u.y += shift_y;
                } else if u.owner_id != player_id && u.hp > 0 {
                    let dx = u.x - dest_x;
                    let dy = u.y - dest_y;
                    let d = dx.hypot(dy);
                    if d < wave_r {
                        let (nx, ny) = if d > 0.001 {
                            (dx / d, dy / d)
                        } else {
                            (1.0, 0.0)
                        };
                        u.knockback_vx += nx * 380.0;
                        u.knockback_vy += ny * 380.0;
                    }
                }
            }
        }

        if let Some(tlog) = self.telemetry.as_mut() {
            let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
            let ring = self
                .zones
                .get(&player_id)
                .map(|z| Self::ring_for_radius(z.x.hypot(z.y)))
                .unwrap_or(1);
            tlog.log_action(
                self.tick,
                sec,
                player_id,
                is_bot,
                "upgrade",
                ring,
                json!({"bkind": kind, "level": new_level, "cost": cost}),
            );
        }

        if kind == 0 || kind == 5 {
            self.invalidate_base_grid(player_id);
        }
        self.recompute_unit_cap(player_id);
        None
    }

    fn cmd_sell_building(&mut self, player_id: PlayerId, building_id: EntityId) -> Option<u8> {
        let Some(idx) = self
            .buildings
            .iter()
            .position(|b| b.id == building_id && b.owner_id == player_id)
        else {
            return Some(7);
        };
        if self.buildings[idx].kind == 0 {
            return Some(8);
        }
        let sold_id = self.buildings[idx].id;
        let sold_kind = self.buildings[idx].kind;
        let energy_refund =
            self.buildings[idx].invested_cost * self.config.world.sell_refund_ratio;
        let trophy_refund = (self.buildings[idx].invested_trophies * 0.5).round();
        self.buildings.swap_remove(idx);
        if sold_kind == 1 {
            self.units
                .retain(|u| !(u.kind == 4 && u.home_id == sold_id));
        }
        let hq_lvl = self.hq_level_of(player_id);
        self.grant_energy(player_id, energy_refund, 1.25, hq_lvl);
        self.grant_trophies(player_id, trophy_refund, hq_lvl);
        if sold_kind == 5 {
            self.prune_orphaned_buildings(player_id);
        } else {
            self.recompute_unit_cap(player_id);
        }
        self.invalidate_base_grid(player_id);
        None
    }

    fn cmd_army_command(
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
                        Self::ring_for_radius(z.x.hypot(z.y)),
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
                        changed = true;
                    }
                }
                if squad > 0 {
                    for u in &mut self.units {
                        if u.owner_id == player_id && u.kind != 4 && u.squad == squad {
                            u.squad = 0;
                        }
                    }
                }
            } else if squad == 0 {
                let has_base_units = self
                    .units
                    .iter()
                    .any(|u| u.owner_id == player_id && u.kind != 4 && u.hp > 0 && u.squad == 0);
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
                                if u.owner_id == player_id
                                    && u.kind != 4
                                    && u.hp > 0
                                    && u.squad == 0
                                {
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
                    .any(|u| u.owner_id == player_id && u.kind != 4 && u.hp > 0 && u.squad == squad);
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
                        json!({"to_ring": Self::ring_for_radius(target_x.hypot(target_y))}),
                    );
                }
            }
        }
        None
    }

    fn cmd_configure_building(
        &mut self,
        player_id: PlayerId,
        building_id: EntityId,
        meta: u8,
    ) -> Option<u8> {
        let Some(b) = self
            .buildings
            .iter_mut()
            .find(|b| b.id == building_id && b.owner_id == player_id)
        else {
            return Some(7);
        };
        if b.kind == 3 {
            let unit_type = (meta >> 2) & 0x07;
            let allowed = match unit_type {
                0 => true,         // Raider (Lv1)
                1 => b.level >= 2, // Archer (Lv2)
                2 => b.level >= 3, // Titan (Lv3)
                _ => false,
            };
            if !allowed {
                return Some(9);
            }
            b.meta = meta & 0x1F;
        }
        None
    }

    fn cmd_teleport_zone(&mut self, player_id: PlayerId, target_x: f32, target_y: f32) -> Option<u8> {
        if !target_x.is_finite() || !target_y.is_finite() {
            return Some(12);
        }
        let cd_left = self
            .players
            .get(&player_id)
            .map(|p| p.teleport_cooldown_left)
            .unwrap_or(0.0);
        if cd_left > 0.0 {
            return Some(11);
        }
        let Some(zone) = self.zones.get(&player_id).cloned() else {
            return Some(1);
        };
        let is_bot_caller = self
            .players
            .get(&player_id)
            .map(|p| p.is_bot)
            .unwrap_or(false);
        let target_r = target_x.hypot(target_y);
        let r_eq = self.equilibrium_radius(player_id);
        let min_r = if r_eq >= RING1_RADIUS {
            RING1_RADIUS + zone.radius + 200.0
        } else {
            self.config.world.core_radius + zone.radius
        };
        let max_r = self.config.world.map_radius - zone.radius;
        if target_r < min_r || target_r > max_r {
            return Some(12);
        }
        if !is_bot_caller && !self.has_vision_at(player_id, target_x, target_y) {
            return Some(18);
        }

        for other in self.zones.values() {
            if other.player_id == player_id {
                continue;
            }
            if (target_x - other.x).hypot(target_y - other.y) < zone.radius + other.radius + 150.0
            {
                return Some(13);
            }
        }
        for b in &self.buildings {
            if b.owner_id != player_id && b.kind == 5 {
                if let Some(other) = self.zones.get(&b.owner_id) {
                    let ext_r = beacon_ext_radius(b.level);
                    if (target_x - (other.x + b.rel_x)).hypot(target_y - (other.y + b.rel_y))
                        < zone.radius + ext_r + 120.0
                    {
                        return Some(13);
                    }
                }
            }
        }

        let dist = (target_x - zone.x).hypot(target_y - zone.y);
        let ring_pen = if target_r < r_eq {
            self.config.world.teleport_ring_penalty * 3.0
        } else {
            self.config.world.teleport_ring_penalty
        };
        let cost = (self.config.world.teleport_base_cost
            + dist * self.config.world.teleport_dist_factor
            + (target_r - r_eq).abs() * ring_pen)
            .round();
        let cd_sec = self.config.world.teleport_cooldown_sec;

        let Some(player) = self.players.get_mut(&player_id) else {
            return Some(1);
        };
        if player.energy < cost {
            return Some(2);
        }
        let from_r = zone.x.hypot(zone.y);
        player.energy -= cost;
        player.teleport_cooldown_left = cd_sec;
        player.spawn_protection_left = 0.0;
        player.last_teleport_sec = player.survival_sec;
        let is_bot = player.is_bot;
        if let Some(z) = self.zones.get_mut(&player_id) {
            z.x = target_x;
            z.y = target_y;
        }
        if let Some(tlog) = self.telemetry.as_mut() {
            let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
            let ring = Self::ring_for_radius(target_r);
            tlog.log_action(
                self.tick,
                sec,
                player_id,
                is_bot,
                "tp",
                ring,
                json!({"cost": cost}),
            );
            tlog.log_milestone(
                self.tick,
                sec,
                player_id,
                "teleport",
                json!({"from_r": from_r.round(), "to_r": target_r.round(), "cost": cost}),
            );
        }
        None
    }
}
