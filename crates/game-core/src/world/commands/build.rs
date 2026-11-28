use serde_json::json;

use crate::telemetry::SRC_UNKNOWN;
use crate::types::{Building, EntityId, PlayerId};

use super::super::hex::hq_zone_radius;
use super::super::World;

impl World {
    pub(super) fn cmd_build(
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
            self.node_trophy_cost(existing_beacons)
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
        let ring = self
            .zones
            .get(&player_id)
            .map(|z| self.ring_for_radius(z.x.hypot(z.y)))
            .unwrap_or(1);
        if let Some(tlog) = self.telemetry.as_mut() {
            let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
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

    pub(super) fn cmd_upgrade_building(
        &mut self,
        player_id: PlayerId,
        building_id: EntityId,
    ) -> Option<u8> {
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
            let max_allowed = (hq_lvl + 1).min(3);
            if cur_level >= max_allowed {
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
            if let Some(z) = self.zones.get_mut(&player_id) {
                z.radius = new_zone_r;
            }
            if let Some(pl) = self.players.get_mut(&player_id) {
                pl.hq_level = new_level;
                pl.spawn_protection_left = pl.spawn_protection_left.max(3.0);
            }
        }

        let ring = self
            .zones
            .get(&player_id)
            .map(|z| self.ring_for_radius(z.x.hypot(z.y)))
            .unwrap_or(1);
        if let Some(tlog) = self.telemetry.as_mut() {
            let sec = self.tick as f32 / f32::from(self.config.world.tick_rate.max(1));
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

    pub(super) fn cmd_sell_building(
        &mut self,
        player_id: PlayerId,
        building_id: EntityId,
    ) -> Option<u8> {
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
        let energy_refund =
            self.buildings[idx].invested_cost * self.config.world.sell_refund_ratio;
        let trophy_refund = (self.buildings[idx].invested_trophies * 0.5).round();
        self.buildings.swap_remove(idx);
        self.grant_energy(player_id, energy_refund, 1.25);
        self.grant_trophies(player_id, trophy_refund);
        let _ = self.prune_orphaned_buildings(player_id);
        None
    }

    pub(super) fn cmd_configure_building(
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
}
