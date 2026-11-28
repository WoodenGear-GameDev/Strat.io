use std::collections::HashMap;
use std::f32::consts::TAU;

use rand::Rng;
use serde_json::json;

use crate::telemetry::SRC_UNKNOWN;
use crate::types::{ArmyMode, Building, PlayerId, PlayerState, Zone};

use super::hex::hq_zone_radius;
use super::World;

impl World {
    pub fn add_player(
        &mut self,
        name: String,
        user_id: Option<i64>,
        _team_id: u8,
        skin_id: u16,
    ) -> PlayerId {
        self.add_player_internal(name, user_id, skin_id, 0, 0)
    }

    pub(super) fn add_player_internal(
        &mut self,
        name: String,
        user_id: Option<i64>,
        skin_id: u16,
        bot_tier: u8,
        bot_archetype: u8,
    ) -> PlayerId {
        let player_id = self.next_player_id;
        self.next_player_id = self.next_player_id.wrapping_add(1).max(1);

        let ring3_r = self.config.world.ring3_radius;
        let ring2_r = self.config.world.ring2_radius;
        let map_r = self.config.world.map_radius;
        let (min_r, max_r) = match bot_tier {
            2 => (ring3_r + 220.0, (ring2_r - 220.0).max(ring3_r + 260.0)),
            3 => (160.0_f32, (ring3_r - 200.0).max(240.0)),
            _ => (ring2_r + 220.0, (map_r - 260.0).max(ring2_r + 260.0)),
        };
        let new_r = hq_zone_radius(self.config.world.zone_base_radius, 1);
        let (mut spawn_x, mut spawn_y) = (min_r, 0.0_f32);
        let mut best_clearance = f32::NEG_INFINITY;
        for _ in 0..16 {
            let angle = self.rng.gen_range(0.0..TAU);
            let dist = self.rng.gen_range(min_r..=max_r);
            let cx = angle.cos() * dist;
            let cy = angle.sin() * dist;
            let mut min_clear = f32::MAX;
            for z in self.zones.values() {
                let clear = (cx - z.x).hypot(cy - z.y) - (new_r + z.radius);
                if clear < min_clear {
                    min_clear = clear;
                }
            }
            if min_clear > best_clearance {
                best_clearance = min_clear;
                spawn_x = cx;
                spawn_y = cy;
                if best_clearance >= 170.0 {
                    break;
                }
            }
        }

        let start_hq_lvl: u8 = match bot_tier {
            2 => 2,
            3 => 3,
            _ => 1,
        };
        let hq_cfg = self.config.buildings.get("hq");
        let hq_kind = hq_cfg.map(|c| c.kind_id).unwrap_or(0);
        let hq_lvl_cfg = hq_cfg.and_then(|c| c.levels.get(usize::from(start_hq_lvl - 1)));
        let hq_hp = hq_lvl_cfg.map(|l| l.hp).unwrap_or(1800);
        let base_cap = hq_lvl_cfg.map(|l| l.unit_cap_bonus).unwrap_or(0);

        let seed_earned = match bot_tier {
            2 => 600.0,
            3 => 1800.0,
            _ => 0.0,
        };
        let seed_trophies = match bot_tier {
            2 => 70.0,
            3 => 160.0,
            _ => 0.0,
        };

        let player = PlayerState {
            id: player_id,
            user_id,
            name,
            skin_id,
            energy: self.config.world.starting_energy,
            trophies: seed_trophies,
            total_energy_earned: seed_earned,
            unit_cap: base_cap,
            hq_level: start_hq_lvl,
            army_mode: ArmyMode::Defend,
            rally_x: spawn_x,
            rally_y: spawn_y,
            spawn_protection_left: self.config.world.spawn_protection_sec,
            score: 0,
            kills: 0,
            buildings_destroyed: 0,
            survival_sec: 0.0,
            is_bot: bot_tier > 0,
            is_hive: false,
            bot_tier,
            bot_think_cooldown: if bot_tier > 0 {
                self.rng.gen_range(0.05..1.05)
            } else {
                1.0
            },
            pending_teleport: None,
            spawn_counter: 0,
            last_damaged_sec: -100.0,
            bot_archetype,
            regen_accum: 0.0,
            regen_enabled: false,
            bot_known_intel: HashMap::new(),
            bot_scout_angle: self.rng.gen_range(0.0..TAU),
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
            radius: hq_zone_radius(self.config.world.zone_base_radius, start_hq_lvl),
            team_id: 0,
            skin_id,
        };

        let hq_id = self.alloc_entity_id();
        let hq = Building {
            id: hq_id,
            owner_id: player_id,
            kind: hq_kind,
            level: start_hq_lvl,
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

        player_id
    }

    pub fn remove_player(&mut self, player_id: PlayerId) {
        self.players.remove(&player_id);
        self.zones.remove(&player_id);
        self.base_grid_cache.remove(&player_id);
        self.buildings.retain(|b| b.owner_id != player_id);
        self.units.retain(|u| u.owner_id != player_id);
        self.projectiles.retain(|p| p.owner_id != player_id);
    }

    pub(super) fn record_telemetry_tick(&mut self) {
        if self.telemetry.is_none() {
            return;
        }
        let sec = self.elapsed_sec;

        let mut ring_updates: Vec<(PlayerId, u8, f32)> = Vec::new();
        for (&pid, z) in &self.zones {
            let r = (z.x * z.x + z.y * z.y).sqrt();
            let ring = self.ring_for_radius(r);
            ring_updates.push((pid, ring, self.equilibrium_radius(pid)));
        }

        let Some(mut tlog) = self.telemetry.take() else {
            return;
        };
        for (pid, ring, r_eq) in ring_updates {
            let prev = tlog.last_rings.insert(pid, ring);
            if let Some(from_ring) = prev {
                if from_ring != ring {
                    tlog.log_milestone(
                        self.tick,
                        sec,
                        pid,
                        "ring_change",
                        json!({"from_ring": from_ring, "to_ring": ring, "r_eq": r_eq.round()}),
                    );
                }
            }
        }

        if sec - self.last_tlog_snap_sec >= 5.0 {
            self.last_tlog_snap_sec = sec;
            tlog.flush_combat_window(self.tick, sec);

            let mut b_counts: HashMap<PlayerId, ([u16; 5], u8, f32)> = HashMap::new();
            for b in &self.buildings {
                let entry = b_counts.entry(b.owner_id).or_insert(([0; 5], 1, 0.0));
                entry.2 += b.invested_cost;
                match b.kind {
                    0 => {
                        entry.0[0] += 1;
                        entry.1 = b.level;
                    }
                    1 => entry.0[1] += 1,
                    2 => entry.0[2] += 1,
                    3 => entry.0[3] += 1,
                    5 => entry.0[4] += 1,
                    _ => {}
                }
            }
            let mut u_counts: HashMap<PlayerId, [u16; 5]> = HashMap::new();
            for u in &self.units {
                if u.owner_id == 0 || u.hp == 0 {
                    continue;
                }
                let entry = u_counts.entry(u.owner_id).or_insert([0; 5]);
                match u.kind {
                    0 => entry[0] += 1,
                    1 => entry[1] += 1,
                    2 => entry[2] += 1,
                    4 => entry[4] += 1,
                    _ => {}
                }
            }

            let mut pids: Vec<PlayerId> = self.players.keys().copied().collect();
            pids.sort_unstable();
            let players_json: Vec<serde_json::Value> = pids
                .into_iter()
                .filter_map(|pid| {
                    let p = self.players.get(&pid)?;
                    let z = self.zones.get(&pid)?;
                    let r = z.x.hypot(z.y);
                    let r_eq = self.equilibrium_radius(pid);
                    let (bc, hq_lvl, invested) =
                        b_counts.get(&pid).copied().unwrap_or(([0; 5], 1, 0.0));
                    let uc = u_counts.get(&pid).copied().unwrap_or([0; 5]);
                    Some(json!({
                        "id": pid,
                        "name": p.name,
                        "is_bot": p.is_bot,
                        "tier": p.bot_tier,
                        "arch": p.bot_archetype,
                        "spec": 0,
                        "hq_lvl": hq_lvl,
                        "level": hq_lvl,
                        "score": p.score,
                        "kills": p.kills,
                        "bd": p.buildings_destroyed,
                        "energy": p.energy.round(),
                        "trophies": p.trophies.round(),
                        "r": r.round(),
                        "r_eq": r_eq.round(),
                        "ring": self.ring_for_radius(r),
                        "zone_r": z.radius.round(),
                        "b": bc,
                        "u": uc,
                        "cap": p.unit_cap,
                        "inv": invested.round(),
                    }))
                })
                .collect();
            tlog.log_snapshot(self.tick, sec, players_json);
        }
        self.telemetry = Some(tlog);
    }
}
