mod bots;
mod combat;
mod commands;
mod economy;
mod hex;
mod placement;
mod snapshot;
mod units;

pub use hex::*;

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use serde_json::json;

use crate::config::BalanceConfig;
use crate::spatial::SpatialGrid;
use crate::telemetry::{TelemetryLogger, SRC_UNKNOWN};
use crate::types::{
    ArmyMode, Building, EntityId, GameOverEvent, PlayerId, PlayerState, Projectile, Unit, Zone,
};

#[derive(Debug, Clone)]
pub(super) struct BaseGridCache {
    pub occupied_tris: HashSet<TriCoord>,
    pub free_hexes: Vec<(i32, i32)>,
    pub free_rhombuses: Vec<([TriCoord; 2], (f32, f32))>,
}

pub(super) type UnitSnap = (EntityId, PlayerId, f32, f32, u16, u8, f32);
pub(super) type BuildingSnap = Option<(EntityId, PlayerId, f32, f32, f32, u16, u8)>;

pub struct World {
    pub config: BalanceConfig,
    pub tick: u32,
    pub elapsed_sec: f32,
    pub(super) ring_score_accum: f32,
    pub(super) cluster_accum: f32,
    pub(super) last_tlog_snap_sec: f32,
    pub(super) seed: u64,
    pub(super) next_entity_id: EntityId,
    pub(super) next_player_id: PlayerId,
    pub(super) rng: SmallRng,
    pub players: HashMap<PlayerId, PlayerState>,
    pub zones: HashMap<PlayerId, Zone>,
    pub buildings: Vec<Building>,
    pub units: Vec<Unit>,
    pub projectiles: Vec<Projectile>,
    pub telemetry: Option<TelemetryLogger>,
    pub(super) unit_grid: SpatialGrid,
    pub(super) building_grid: SpatialGrid,
    pub(super) building_world_snap: Vec<BuildingSnap>,
    pub(super) unit_world_snap: Vec<UnitSnap>,
    pub(super) base_grid_cache: HashMap<PlayerId, BaseGridCache>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StepTimings {
    pub ticks: u64,
    pub bots_ns: u64,
    pub push_repulsion_ns: u64,
    pub eco_spawn_ns: u64,
    pub grid_ns: u64,
    pub turrets_ns: u64,
    pub projectiles_ns: u64,
    pub units_ns: u64,
    pub clustering_ns: u64,
    pub cleanup_ns: u64,
    pub telemetry_ns: u64,
    pub snapshot_ns: u64,
    pub max_tick_ns: u64,
    pub max_tick_num: u32,
    pub max_tick_phases_ns: [u64; 9],
}

impl World {
    pub fn new(mut config: BalanceConfig, seed: u64) -> Self {
        config.rebuild_caches();
        let map_r = config.world.map_radius;
        Self {
            config,
            tick: 0,
            elapsed_sec: 0.0,
            ring_score_accum: 0.0,
            cluster_accum: 0.0,
            last_tlog_snap_sec: 0.0,
            seed,
            next_entity_id: 1,
            next_player_id: 1,
            rng: SmallRng::seed_from_u64(seed),
            players: HashMap::new(),
            zones: HashMap::new(),
            buildings: Vec::new(),
            units: Vec::new(),
            projectiles: Vec::new(),
            telemetry: None,
            unit_grid: SpatialGrid::new_for_world(map_r, 80.0),
            building_grid: SpatialGrid::new_for_world(map_r, 96.0),
            building_world_snap: Vec::with_capacity(2048),
            unit_world_snap: Vec::with_capacity(2048),
            base_grid_cache: HashMap::new(),
        }
    }

    #[inline]
    pub(super) fn invalidate_base_grid(&mut self, player_id: PlayerId) {
        self.base_grid_cache.remove(&player_id);
    }

    pub(super) fn rebuild_spatial_grids(&mut self) {
        self.unit_grid.clear();
        self.unit_world_snap.clear();
        let r0 = self.config.unit_fast_stats(0).radius;
        let r1 = self.config.unit_fast_stats(1).radius;
        let r2 = self.config.unit_fast_stats(2).radius;
        for (i, u) in self.units.iter().enumerate() {
            let r = match u.kind {
                1 => r1,
                2 => r2,
                _ => r0,
            };
            self.unit_world_snap
                .push((u.id, u.owner_id, u.x, u.y, u.hp, u.kind, r));
            if u.hp > 0 {
                self.unit_grid.insert(i, u.x, u.y);
            }
        }

        self.building_grid.clear();
        self.building_world_snap.clear();
        for (bi, b) in self.buildings.iter().enumerate() {
            if b.hp == 0 {
                self.building_world_snap.push(None);
                continue;
            }
            let Some(z) = self.zones.get(&b.owner_id) else {
                self.building_world_snap.push(None);
                continue;
            };
            let br = self
                .config
                .building_by_kind(b.kind)
                .map(|c| c.radius)
                .unwrap_or(22.0);
            let bx = z.x + b.rel_x;
            let by = z.y + b.rel_y;
            self.building_world_snap
                .push(Some((b.id, b.owner_id, bx, by, br, b.hp, b.kind)));
            self.building_grid.insert(bi, bx, by);
        }
    }

    pub fn enable_telemetry(&mut self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let mut logger = TelemetryLogger::new(path.as_ref(), self.config.world.tick_rate)?;
        logger.log_meta(
            self.seed,
            self.config.world.tick_rate,
            self.config.world.spawn_protection_sec,
        );
        self.telemetry = Some(logger);
        Ok(())
    }

    pub(super) fn alloc_entity_id(&mut self) -> EntityId {
        let id = self.next_entity_id;
        self.next_entity_id = self.next_entity_id.wrapping_add(1).max(1);
        id
    }

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

        let (min_r, max_r) = match bot_tier {
            1 => (520.0_f32, 720.0_f32),
            2 => (1440.0_f32, 1720.0_f32),
            3 => (2420.0_f32, 2760.0_f32),
            _ => (520.0_f32, 720.0_f32),
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
        let base_cap = hq_lvl_cfg.map(|l| l.unit_cap_bonus).unwrap_or(6);

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
            bot_tier,
            bot_think_cooldown: if bot_tier > 0 {
                self.rng.gen_range(0.05..1.05)
            } else {
                1.0
            },
            teleport_cooldown_left: 0.0,
            spawn_counter: 0,
            last_damaged_sec: -100.0,
            bot_archetype,
            regen_accum: 0.0,
            build_combat_locks: Vec::new(),
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

    pub fn step(&mut self, dt: f32) -> Vec<GameOverEvent> {
        self.tick = self.tick.wrapping_add(1);
        self.elapsed_sec += dt;
        for player in self.players.values_mut() {
            player.survival_sec += dt;
            if player.spawn_protection_left > 0.0 {
                player.spawn_protection_left = (player.spawn_protection_left - dt).max(0.0);
            }
            if player.teleport_cooldown_left > 0.0 {
                player.teleport_cooldown_left = (player.teleport_cooldown_left - dt).max(0.0);
            }
        }

        self.step_bots(dt);
        self.step_core_push(dt);
        self.step_zone_repulsion(dt);
        self.step_ring_scoring(dt);
        self.step_ooc_regen(dt);
        self.step_income_and_spawning(dt);
        self.rebuild_spatial_grids();
        self.step_turrets(dt);
        self.step_projectiles(dt);
        self.step_units(dt);
        self.cluster_accum += dt;
        if self.cluster_accum >= 0.5 {
            self.cluster_accum %= 0.5;
            self.step_squad_clustering();
        }
        let game_overs = self.step_cleanup();
        self.record_telemetry_tick();
        game_overs
    }

    pub fn step_profiled(&mut self, dt: f32, timings: &mut StepTimings) -> Vec<GameOverEvent> {
        let t_start = Instant::now();
        self.tick = self.tick.wrapping_add(1);
        self.elapsed_sec += dt;
        for player in self.players.values_mut() {
            player.survival_sec += dt;
            if player.spawn_protection_left > 0.0 {
                player.spawn_protection_left = (player.spawn_protection_left - dt).max(0.0);
            }
            if player.teleport_cooldown_left > 0.0 {
                player.teleport_cooldown_left = (player.teleport_cooldown_left - dt).max(0.0);
            }
        }

        let t0 = Instant::now();
        self.step_bots(dt);
        let t1 = Instant::now();
        self.step_core_push(dt);
        self.step_zone_repulsion(dt);
        let t2 = Instant::now();
        self.step_ring_scoring(dt);
        self.step_ooc_regen(dt);
        self.step_income_and_spawning(dt);
        let t3 = Instant::now();
        self.rebuild_spatial_grids();
        let t3b = Instant::now();
        self.step_turrets(dt);
        let t4 = Instant::now();
        self.step_projectiles(dt);
        let t5 = Instant::now();
        self.step_units(dt);
        let t6 = Instant::now();
        self.cluster_accum += dt;
        if self.cluster_accum >= 0.5 {
            self.cluster_accum %= 0.5;
            self.step_squad_clustering();
        }
        let t7 = Instant::now();
        let game_overs = self.step_cleanup();
        let t8 = Instant::now();
        self.record_telemetry_tick();
        let t9 = Instant::now();

        let b_ns = (t1 - t0).as_nanos() as u64;
        let p_ns = (t2 - t1).as_nanos() as u64;
        let e_ns = (t3 - t2).as_nanos() as u64;
        let g_ns = (t3b - t3).as_nanos() as u64;
        let tu_ns = (t4 - t3b).as_nanos() as u64;
        let pr_ns = (t5 - t4).as_nanos() as u64;
        let u_ns = (t6 - t5).as_nanos() as u64;
        let cl_ns = (t7 - t6).as_nanos() as u64;
        let cu_ns = (t8 - t7).as_nanos() as u64;
        let tl_ns = (t9 - t8).as_nanos() as u64;

        timings.ticks += 1;
        timings.bots_ns += b_ns;
        timings.push_repulsion_ns += p_ns;
        timings.eco_spawn_ns += e_ns;
        timings.grid_ns += g_ns;
        timings.turrets_ns += tu_ns;
        timings.projectiles_ns += pr_ns;
        timings.units_ns += u_ns;
        timings.clustering_ns += cl_ns;
        timings.cleanup_ns += cu_ns;
        timings.telemetry_ns += tl_ns;
        let total = (t9 - t_start).as_nanos() as u64;
        if total > timings.max_tick_ns {
            timings.max_tick_ns = total;
            timings.max_tick_num = self.tick;
            timings.max_tick_phases_ns =
                [b_ns, p_ns, e_ns, g_ns, tu_ns, pr_ns, u_ns, cl_ns, cu_ns];
        }
        game_overs
    }

    fn record_telemetry_tick(&mut self) {
        if self.telemetry.is_none() {
            return;
        }
        let sec = self.elapsed_sec;

        let mut ring_updates: Vec<(PlayerId, u8, f32)> = Vec::new();
        for (&pid, z) in &self.zones {
            let r = (z.x * z.x + z.y * z.y).sqrt();
            let ring = Self::ring_for_radius(r);
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
                        "ring": Self::ring_for_radius(r),
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
