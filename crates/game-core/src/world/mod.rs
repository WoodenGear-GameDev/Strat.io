mod bots;
mod combat;
mod commands;
mod economy;
mod hex;
mod placement;
mod pve;
mod snapshot;
mod spawn;
mod units;

pub use hex::*;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::SeedableRng;

use crate::config::BalanceConfig;
use crate::spatial::SpatialGrid;
use crate::telemetry::TelemetryLogger;
use crate::types::{
    Building, CombatHeatSpot, EntityId, GameOverEvent, PlayerId, PlayerState, Projectile, Unit,
    Zone,
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
    pub(super) hive_respawn_accum: f32,
    pub(super) attractor_spawn_accum: f32,
    pub(super) combat_heats: Vec<CombatHeatSpot>,
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
        let attr_init = config.pve.attractor_initial_delay_sec.max(2.0);
        Self {
            config,
            tick: 0,
            elapsed_sec: 0.0,
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
            hive_respawn_accum: 0.5,
            attractor_spawn_accum: attr_init,
            combat_heats: Vec::with_capacity(16),
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
            if b.owner_id == 0 {
                let br = self.config.pve.attractor_radius.max(20.0);
                self.building_world_snap
                    .push(Some((b.id, 0, b.rel_x, b.rel_y, br, b.hp, b.kind)));
                self.building_grid.insert(bi, b.rel_x, b.rel_y);
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

    pub fn step(&mut self, dt: f32) -> Vec<GameOverEvent> {
        self.tick = self.tick.wrapping_add(1);
        self.elapsed_sec += dt;
        for player in self.players.values_mut() {
            player.survival_sec += dt;
            if player.spawn_protection_left > 0.0 {
                player.spawn_protection_left = (player.spawn_protection_left - dt).max(0.0);
            }
        }

        self.step_bots(dt);
        self.step_pve(dt);
        self.step_mass_gravity(dt);
        self.step_zone_repulsion(dt);
        self.step_base_regen(dt);
        self.step_income_and_spawning(dt);
        self.rebuild_spatial_grids();
        self.step_turrets(dt);
        self.step_projectiles(dt);
        self.step_units(dt);
        self.step_teleport(dt);
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
        }

        let t0 = Instant::now();
        self.step_bots(dt);
        self.step_pve(dt);
        let t1 = Instant::now();
        self.step_mass_gravity(dt);
        self.step_zone_repulsion(dt);
        let t2 = Instant::now();
        self.step_base_regen(dt);
        self.step_income_and_spawning(dt);
        let t3 = Instant::now();
        self.rebuild_spatial_grids();
        let t3b = Instant::now();
        self.step_turrets(dt);
        let t4 = Instant::now();
        self.step_projectiles(dt);
        let t5 = Instant::now();
        self.step_units(dt);
        self.step_teleport(dt);
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
}
