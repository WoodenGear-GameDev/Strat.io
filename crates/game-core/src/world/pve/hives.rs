use std::collections::HashMap;
use std::f32::consts::TAU;

use rand::Rng;

use crate::protocol::ClientMessage;
use crate::telemetry::SRC_UNKNOWN;
use crate::types::{ArmyMode, Building, PlayerId, PlayerState, Zone};

use super::super::hex::{hex_to_rel, hq_zone_radius};
use super::super::World;

impl World {
    pub(super) fn step_hives(&mut self, dt: f32) {
        let target_count = self.config.pve.hive_count;
        if target_count == 0 {
            return;
        }

        let live_hives: Vec<PlayerId> = self
            .players
            .values()
            .filter(|p| p.is_hive)
            .map(|p| p.id)
            .collect();

        if live_hives.len() < target_count {
            self.hive_respawn_accum -= dt;
            if self.hive_respawn_accum <= 0.0 {
                self.spawn_hive();
                self.hive_respawn_accum = if live_hives.len() + 1 < target_count {
                    0.5
                } else {
                    self.config.pve.hive_respawn_sec.max(5.0)
                };
            }
        } else {
            self.hive_respawn_accum = self.config.pve.hive_respawn_sec.max(5.0);
        }

        if live_hives.is_empty() {
            return;
        }

        let regrow_cd = self.config.pve.hive_regrow_interval_sec.max(1.0);
        let mut ready_hives = Vec::new();
        for pid in live_hives {
            if let Some(p) = self.players.get_mut(&pid) {
                p.bot_think_cooldown -= dt;
                if p.bot_think_cooldown <= 0.0 {
                    p.bot_think_cooldown = regrow_cd * self.rng.gen_range(0.85..1.15);
                    ready_hives.push(pid);
                }
            }
        }

        for pid in ready_hives {
            self.hive_regrow_step(pid);
            self.hive_aggro_step(pid);
        }
    }

    fn spawn_hive(&mut self) -> PlayerId {
        let player_id = self.next_player_id;
        self.next_player_id = self.next_player_id.wrapping_add(1).max(1);

        let ring2_r = self.config.world.ring2_radius;
        let map_r = self.config.world.map_radius;
        let min_r = ring2_r + 260.0;
        let max_r = (map_r - 280.0).max(min_r + 60.0);
        let hq_lvl = self.config.pve.hive_hq_level.clamp(1, 3);
        let zone_r = hq_zone_radius(self.config.world.zone_base_radius, hq_lvl);

        let (mut spawn_x, mut spawn_y) = (min_r, 0.0_f32);
        let mut best_clearance = f32::NEG_INFINITY;
        for _ in 0..24 {
            let angle = self.rng.gen_range(0.0..TAU);
            let dist = self.rng.gen_range(min_r..=max_r);
            let cx = angle.cos() * dist;
            let cy = angle.sin() * dist;
            let mut min_clear = f32::MAX;
            for z in self.zones.values() {
                let clear = (cx - z.x).hypot(cy - z.y) - (zone_r + z.radius);
                if clear < min_clear {
                    min_clear = clear;
                }
            }
            if min_clear > best_clearance {
                best_clearance = min_clear;
                spawn_x = cx;
                spawn_y = cy;
                if best_clearance >= 260.0 {
                    break;
                }
            }
        }

        let hq_cfg = self.config.buildings.get("hq");
        let hq_kind = hq_cfg.map(|c| c.kind_id).unwrap_or(0);
        let hq_lvl_cfg = hq_cfg.and_then(|c| c.levels.get(usize::from(hq_lvl - 1)));
        let hq_hp = hq_lvl_cfg.map(|l| l.hp).unwrap_or(3200);

        let hive_idx = self.players.values().filter(|p| p.is_hive).count() + 1;
        let player = PlayerState {
            id: player_id,
            user_id: None,
            name: format!("Улей-{hive_idx}"),
            skin_id: 0,
            energy: 260.0,
            trophies: 0.0,
            total_energy_earned: 0.0,
            unit_cap: 0,
            hq_level: hq_lvl,
            army_mode: ArmyMode::Defend,
            rally_x: spawn_x,
            rally_y: spawn_y,
            spawn_protection_left: 0.0,
            score: 0,
            kills: 0,
            buildings_destroyed: 0,
            survival_sec: 0.0,
            is_bot: false,
            is_hive: true,
            bot_tier: 0,
            bot_think_cooldown: self.rng.gen_range(1.2..2.5),
            pending_teleport: None,
            spawn_counter: 0,
            last_damaged_sec: -100.0,
            bot_archetype: 0,
            regen_accum: 0.0,
            regen_enabled: false,
            bot_known_intel: HashMap::new(),
            bot_scout_angle: 0.0,
            last_teleport_sec: -100.0,
            squads: HashMap::new(),
            next_squad_id: 1,
            base_rally: None,
            reinforce_squad: None,
        };

        let zone = Zone {
            player_id,
            x: spawn_x,
            y: spawn_y,
            radius: zone_r,
            team_id: 0,
            skin_id: 0,
        };

        let hq_id = self.alloc_entity_id();
        let hq = Building {
            id: hq_id,
            owner_id: player_id,
            kind: hq_kind,
            level: hq_lvl,
            meta: 0,
            rel_x: 0.0,
            rel_y: 0.0,
            hp: hq_hp,
            max_hp: hq_hp,
            angle: 0,
            cooldown_left: 0.0,
            invested_cost: 0.0,
            invested_trophies: 0.0,
            last_hit_by: None,
            last_hit_src: SRC_UNKNOWN,
        };

        self.players.insert(player_id, player);
        self.zones.insert(player_id, zone);
        self.buildings.push(hq);

        // Seed large hive structure: Nodes -> Barracks (drones only, 0 turrets) -> Generators
        let target_nodes = self.config.pve.hive_target_nodes;
        let target_barracks = self.config.pve.hive_target_barracks;
        let target_gens = self.config.pve.hive_target_generators;

        for _ in 0..target_nodes {
            self.hive_place_module(player_id, 5, true);
        }
        for _ in 0..target_barracks {
            self.hive_place_module(player_id, 3, true);
        }
        for _ in 0..target_gens {
            self.hive_place_module(player_id, 1, true);
        }
        self.recompute_unit_cap(player_id);
        player_id
    }

    fn hive_place_module(&mut self, pid: PlayerId, kind: u8, free_seed: bool) -> bool {
        let cost = self.building_build_cost(kind);
        if !free_seed {
            let energy = self.players.get(&pid).map(|p| p.energy).unwrap_or(0.0);
            if energy < cost {
                return false;
            }
        }
        let _ = self.ensure_base_grid(pid);
        let pos = if kind == 1 {
            let len = self
                .base_grid_cache
                .get(&pid)
                .map(|c| c.free_rhombuses.len())
                .unwrap_or(0);
            if len == 0 {
                None
            } else {
                let idx = self.rng.gen_range(0..len);
                self.base_grid_cache
                    .get(&pid)
                    .map(|c| c.free_rhombuses[idx].1)
            }
        } else {
            let len = self
                .base_grid_cache
                .get(&pid)
                .map(|c| c.free_hexes.len())
                .unwrap_or(0);
            if len == 0 {
                None
            } else if kind == 5 {
                self.base_grid_cache.get(&pid).and_then(|c| {
                    c.free_hexes
                        .iter()
                        .copied()
                        .max_by(|a, b| {
                            let (ax, ay) = hex_to_rel(a.0, a.1);
                            let (bx, by) = hex_to_rel(b.0, b.1);
                            (ax * ax + ay * ay)
                                .partial_cmp(&(bx * bx + by * by))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(q, r)| hex_to_rel(q, r))
                })
            } else {
                let idx = self.rng.gen_range(0..len);
                self.base_grid_cache.get(&pid).map(|c| {
                    let (q, r) = c.free_hexes[idx];
                    hex_to_rel(q, r)
                })
            }
        };
        let Some((rel_x, rel_y)) = pos else {
            return false;
        };
        let hp = self
            .config
            .building_by_kind(kind)
            .and_then(|c| c.levels.first())
            .map(|l| l.hp)
            .unwrap_or(200);
        if !free_seed {
            if let Some(p) = self.players.get_mut(&pid) {
                p.energy = (p.energy - cost).max(0.0);
            }
        }
        let id = self.alloc_entity_id();
        self.buildings.push(Building {
            id,
            owner_id: pid,
            kind,
            level: 1,
            meta: 0, // Barracks spawn basic Raiders (kind 0)
            rel_x,
            rel_y,
            hp,
            max_hp: hp,
            angle: 0,
            cooldown_left: 0.0,
            invested_cost: cost,
            invested_trophies: 0.0,
            last_hit_by: None,
            last_hit_src: SRC_UNKNOWN,
        });
        self.invalidate_base_grid(pid);
        self.recompute_unit_cap(pid);
        true
    }

    fn hive_regrow_step(&mut self, pid: PlayerId) {
        let mut nodes = 0_usize;
        let mut barracks = 0_usize;
        let mut gens = 0_usize;
        for b in &self.buildings {
            if b.owner_id != pid || b.hp == 0 {
                continue;
            }
            match b.kind {
                1 => gens += 1,
                3 => barracks += 1,
                5 => nodes += 1,
                _ => {}
            }
        }
        if nodes < self.config.pve.hive_target_nodes {
            let _ = self.hive_place_module(pid, 5, false);
        } else if barracks < self.config.pve.hive_target_barracks {
            let _ = self.hive_place_module(pid, 3, false);
        } else if gens < self.config.pve.hive_target_generators {
            let _ = self.hive_place_module(pid, 1, false);
        }
    }

    fn hive_aggro_step(&mut self, pid: PlayerId) {
        let Some(hz) = self.zones.get(&pid).cloned() else {
            return;
        };
        let aggro_r = self.config.pve.hive_aggro_range;
        let aggro_r2 = aggro_r * aggro_r;
        let ring2_r = self.config.world.ring2_radius;
        let gen_trigger = self.config.pve.hive_gen_trigger_count;
        let wave_min = self.config.pve.hive_wave_size.max(3);

        let mut garrison = 0_usize;
        let mut active_squads = 0_usize;
        let mut seen_sq = [false; 16];
        for u in &self.units {
            if u.owner_id == pid && u.hp > 0 {
                if u.squad == 0 {
                    garrison += 1;
                } else {
                    let s = usize::from(u.squad);
                    if s < seen_sq.len() && !seen_sq[s] {
                        seen_sq[s] = true;
                        active_squads += 1;
                    }
                }
            }
        }
        if garrison < 3 {
            return;
        }

        // Priority 1: Instant aggro on any Tier 2+ player drone intruding in Ring 1 near the hive
        let mut high_tier_target: Option<(f32, f32, f32)> = None;
        for u in &self.units {
            if u.owner_id == 0 || u.owner_id == pid || u.hp == 0 {
                continue;
            }
            let Some(op) = self.players.get(&u.owner_id) else {
                continue;
            };
            if op.is_hive || op.spawn_protection_left > 0.0 || op.hq_level < 2 {
                continue;
            }
            let ur2 = u.x * u.x + u.y * u.y;
            if ur2 < ring2_r * ring2_r {
                continue;
            }
            let dx = u.x - hz.x;
            let dy = u.y - hz.y;
            let d2 = dx * dx + dy * dy;
            if d2 <= aggro_r2 && high_tier_target.is_none_or(|(_, _, bd2)| d2 < bd2) {
                high_tier_target = Some((u.x, u.y, d2));
            }
        }
        if let Some((tx, ty, _)) = high_tier_target {
            if active_squads < 2 {
                let _ = self.apply_command(
                    pid,
                    ClientMessage::ArmyCommand {
                        squad: 0,
                        mode: 1,
                        target_x: tx,
                        target_y: ty,
                    },
                );
            }
            return;
        }

        // Priority 2: Send wave against nearby Ring 1 bases with excess generators
        if garrison < wave_min || active_squads >= 2 {
            return;
        }
        let mut gen_counts: HashMap<PlayerId, usize> = HashMap::new();
        for b in &self.buildings {
            if b.kind == 1 && b.hp > 0 && b.owner_id != pid && b.owner_id != 0 {
                *gen_counts.entry(b.owner_id).or_insert(0) += 1;
            }
        }
        let mut greedy_target: Option<(f32, f32, f32)> = None;
        for (opid, count) in gen_counts {
            if count < gen_trigger {
                continue;
            }
            let Some(op) = self.players.get(&opid) else {
                continue;
            };
            if op.is_hive || op.spawn_protection_left > 0.0 {
                continue;
            }
            let Some(oz) = self.zones.get(&opid) else {
                continue;
            };
            if oz.x * oz.x + oz.y * oz.y < ring2_r * ring2_r {
                continue;
            }
            let dx = oz.x - hz.x;
            let dy = oz.y - hz.y;
            let d2 = dx * dx + dy * dy;
            if d2 <= aggro_r2 && greedy_target.is_none_or(|(_, _, bd2)| d2 < bd2) {
                greedy_target = Some((oz.x, oz.y, d2));
            }
        }
        if let Some((tx, ty, _)) = greedy_target {
            let _ = self.apply_command(
                pid,
                ClientMessage::ArmyCommand {
                    squad: 0,
                    mode: 1,
                    target_x: tx,
                    target_y: ty,
                },
            );
        }
    }
}
