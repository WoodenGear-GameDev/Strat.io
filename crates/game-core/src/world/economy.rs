use std::collections::HashMap;
use std::f32::consts::TAU;

use rand::Rng;

use crate::telemetry::SRC_UNKNOWN;
use crate::types::{EntityId, PlayerId, Unit};

use super::World;

impl World {
    pub(super) fn recompute_unit_cap(&mut self, player_id: PlayerId) {
        let mut cap: u16 = 0;
        for b in &self.buildings {
            if b.owner_id == player_id {
                if let Some(cfg) = self.config.building_by_kind(b.kind) {
                    if let Some(lvl) = cfg.levels.get((b.level as usize).saturating_sub(1)) {
                        cap = cap.saturating_add(lvl.unit_cap_bonus);
                    }
                }
            }
        }
        if let Some(p) = self.players.get_mut(&player_id) {
            p.unit_cap = cap;
        }
    }

    #[inline]
    pub(super) fn hq_level_of(&self, player_id: PlayerId) -> u8 {
        self.players
            .get(&player_id)
            .map(|p| p.hq_level)
            .unwrap_or(1)
    }

    #[allow(dead_code)]
    pub(super) fn hq_level_map(&self) -> HashMap<PlayerId, u8> {
        let mut map = HashMap::new();
        for b in &self.buildings {
            if b.kind == 0 {
                map.insert(b.owner_id, b.level);
            }
        }
        map
    }

    pub(super) fn energy_cap_for_hq(&self, hq_lvl: u8) -> f32 {
        let cfg_cap = self
            .config
            .building_by_kind(0)
            .and_then(|c| c.levels.get(usize::from(hq_lvl.saturating_sub(1))))
            .map(|l| l.energy_cap)
            .unwrap_or(0.0);
        if cfg_cap > 0.0 {
            cfg_cap
        } else {
            match hq_lvl {
                0 | 1 => 400.0,
                2 => 950.0,
                _ => 2000.0,
            }
        }
    }

    pub(super) fn trophy_cap_for_hq(&self, hq_lvl: u8) -> f32 {
        let cfg_cap = self
            .config
            .building_by_kind(0)
            .and_then(|c| c.levels.get(usize::from(hq_lvl.saturating_sub(1))))
            .map(|l| l.trophy_cap)
            .unwrap_or(0.0);
        if cfg_cap > 0.0 {
            cfg_cap
        } else {
            match hq_lvl {
                0 | 1 => 200.0,
                2 => 420.0,
                _ => 600.0,
            }
        }
    }

    pub(super) fn unit_supply_cost(&self, ukind: u8) -> u16 {
        self.config
            .unit_by_kind(ukind)
            .map(|c| c.supply_cost.max(1))
            .unwrap_or_else(|| if ukind == 2 { 3 } else { 1 })
    }

    pub(super) fn unit_energy_cost(&self, ukind: u8) -> f32 {
        self.config
            .unit_by_kind(ukind)
            .map(|c| c.cost.max(0.0))
            .unwrap_or_else(|| match ukind {
                1 => 24.0,
                2 => 65.0,
                _ => 14.0,
            })
    }

    pub(super) fn tier_dmg_mult(attacker_hq: u8, victim_hq: u8, is_hq_building: bool) -> f32 {
        if attacker_hq > victim_hq {
            let diff = attacker_hq - victim_hq;
            if is_hq_building {
                match diff {
                    1 => 0.25,
                    _ => 0.15,
                }
            } else {
                match diff {
                    1 => 0.50,
                    _ => 0.30,
                }
            }
        } else if attacker_hq < victim_hq {
            let diff = victim_hq - attacker_hq;
            match diff {
                1 => 1.35,
                _ => 1.75,
            }
        } else {
            1.0
        }
    }

    pub(super) fn tier_reward_mult(killer_hq: u8, victim_hq: u8) -> (f32, f32) {
        if killer_hq > victim_hq {
            let diff = killer_hq - victim_hq;
            match diff {
                1 => (0.50, 0.50),
                _ => (0.20, 0.25),
            }
        } else if killer_hq < victim_hq {
            let diff = victim_hq - killer_hq;
            match diff {
                1 => (1.45, 1.35),
                _ => (1.90, 1.65),
            }
        } else {
            (1.0, 1.0)
        }
    }

    pub(super) fn grant_energy(&mut self, pid: PlayerId, amount: f32, cap_mult: f32, hq_lvl: u8) {
        if amount <= 0.0 {
            return;
        }
        let max_e = self.energy_cap_for_hq(hq_lvl) * cap_mult;
        if let Some(p) = self.players.get_mut(&pid) {
            p.total_energy_earned += amount;
            if p.energy < max_e {
                p.energy = (p.energy + amount).min(max_e);
            }
        }
    }

    pub(super) fn grant_trophies(&mut self, pid: PlayerId, amount: f32, hq_lvl: u8) {
        if amount <= 0.0 {
            return;
        }
        let max_t = self.trophy_cap_for_hq(hq_lvl);
        if let Some(p) = self.players.get_mut(&pid) {
            p.trophies = (p.trophies + amount).min(max_t);
        }
    }

    pub(super) fn building_build_cost(&self, kind: u8) -> f32 {
        self.config
            .building_by_kind(kind)
            .and_then(|c| c.levels.first())
            .map(|l| l.cost)
            .unwrap_or(9999.0)
    }

    pub(super) fn building_upgrade_cost(&self, kind: u8, cur_level: u8) -> Option<(f32, f32)> {
        let cfg = self.config.building_by_kind(kind)?;
        let next = cfg.levels.get(usize::from(cur_level))?;
        Some((next.cost, next.trophy_cost))
    }

    pub(super) fn step_ring_scoring(&mut self, dt: f32) {
        self.ring_score_accum += dt;
        if self.ring_score_accum < 1.0 {
            return;
        }
        let whole_secs = self.ring_score_accum.floor() as u32;
        self.ring_score_accum -= whole_secs as f32;

        let zones = &self.zones;
        let players = &mut self.players;
        for z in zones.values() {
            let ring = Self::ring_for_radius((z.x * z.x + z.y * z.y).sqrt());
            let pts = match ring {
                2 => 1,
                3 => 4,
                _ => 0,
            };
            if pts > 0 {
                if let Some(p) = players.get_mut(&z.player_id) {
                    p.score += pts * whole_secs;
                }
            }
        }
    }

    pub(super) fn step_ooc_regen(&mut self, dt: f32) {
        const REPAIR_ENERGY_PER_HP: f32 = 0.09;
        let mut regen_rates: Option<HashMap<PlayerId, f32>> = None;
        for p in self.players.values_mut() {
            if (p.survival_sec - p.last_damaged_sec) >= 6.0 {
                p.regen_accum += dt;
                if p.regen_accum >= 0.5 {
                    p.regen_accum -= 0.5;
                    regen_rates
                        .get_or_insert_with(HashMap::new)
                        .insert(p.id, 0.015);
                }
            } else {
                p.regen_accum = 0.0;
            }
        }
        let Some(regen_rates) = regen_rates else {
            return;
        };
        for b in &mut self.buildings {
            if b.hp > 0 && b.hp < b.max_hp {
                if let Some(&rate) = regen_rates.get(&b.owner_id) {
                    let Some(p) = self.players.get_mut(&b.owner_id) else {
                        continue;
                    };
                    let eff_rate = if b.kind == 0 { rate * 0.35 } else { rate };
                    let missing = b.max_hp.saturating_sub(b.hp);
                    let want_heal = ((f32::from(b.max_hp) * eff_rate * 0.5).round().max(1.0) as u16)
                        .min(missing);
                    let affordable_hp = (p.energy / REPAIR_ENERGY_PER_HP).floor().max(0.0) as u16;
                    let actual_heal = want_heal.min(affordable_hp);
                    if actual_heal > 0 {
                        p.energy = (p.energy - f32::from(actual_heal) * REPAIR_ENERGY_PER_HP).max(0.0);
                        b.hp = b.hp.saturating_add(actual_heal).min(b.max_hp);
                    }
                }
            }
        }
    }

    pub(super) fn step_income_and_spawning(&mut self, dt: f32) {
        // 1. Passive energy generation (clamped to HQ energy_cap)
        {
            let config = &self.config;
            let hq_cfg = config.building_by_kind(0);
            let gen_cfg = config.building_by_kind(1);
            let players = &mut self.players;
            let telemetry = &mut self.telemetry;
            for b in &self.buildings {
                let (cfg_opt, src_slot) = match b.kind {
                    0 => (hq_cfg, 0_usize),
                    1 => (gen_cfg, 1_usize),
                    _ => continue,
                };
                let Some(cfg) = cfg_opt else {
                    continue;
                };
                let Some(lvl) = cfg.levels.get((b.level as usize).saturating_sub(1)) else {
                    continue;
                };
                if lvl.income_per_sec <= 0.0 {
                    continue;
                }
                let amount = lvl.income_per_sec * dt;
                if let Some(p) = players.get_mut(&b.owner_id) {
                    let hq_lvl = p.hq_level;
                    let cfg_cap = hq_cfg
                        .and_then(|c| c.levels.get(usize::from(hq_lvl.saturating_sub(1))))
                        .map(|l| l.energy_cap)
                        .unwrap_or(0.0);
                    let max_e = if cfg_cap > 0.0 {
                        cfg_cap
                    } else {
                        match hq_lvl {
                            0 | 1 => 400.0,
                            2 => 950.0,
                            _ => 2000.0,
                        }
                    };
                    p.total_energy_earned += amount;
                    if p.energy < max_e {
                        p.energy = (p.energy + amount).min(max_e);
                    }
                }
                if let Some(tl) = telemetry.as_mut() {
                    tl.log_income(b.owner_id, src_slot, amount);
                }
            }
        }

        // 2. Paid Barracks spawning (per-barrack local supply cap + global supply cap + unit energy cost)
        let raider = self.config.unit_fast_stats(0);
        let archer = self.config.unit_fast_stats(1);
        let titan = self.config.unit_fast_stats(2);

        let mut spawned_units: Vec<Unit> = Vec::new();
        {
            let config = &self.config;
            let Some(barrack_cfg) = config.building_by_kind(3) else {
                return;
            };
            let buildings = &mut self.buildings;
            let players = &mut self.players;
            let zones = &self.zones;
            let units = &self.units;
            let rng = &mut self.rng;

            let mut supply_maps: Option<(HashMap<PlayerId, u16>, HashMap<EntityId, u16>)> = None;

            for b in buildings.iter_mut() {
                if b.kind != 3 || (b.meta & 0x01) != 0 {
                    continue;
                }
                let Some(lvl) = barrack_cfg.levels.get((b.level as usize).saturating_sub(1)) else {
                    continue;
                };
                if lvl.spawn_interval_sec <= 0.0 {
                    continue;
                }

                b.cooldown_left -= dt;
                if b.cooldown_left > 0.0 {
                    continue;
                }

                let (player_supply, barrack_supply) = supply_maps.get_or_insert_with(|| {
                    let mut ps: HashMap<PlayerId, u16> = HashMap::new();
                    let mut bs: HashMap<EntityId, u16> = HashMap::new();
                    for u in units.iter() {
                        if u.owner_id != 0 && u.hp > 0 && u.kind != 4 {
                            let sup = match u.kind {
                                0 => raider.supply_cost,
                                1 => archer.supply_cost,
                                2 => titan.supply_cost,
                                k => config
                                    .unit_by_kind(k)
                                    .map(|c| c.supply_cost.max(1))
                                    .unwrap_or(1),
                            };
                            *ps.entry(u.owner_id).or_insert(0) += sup;
                            *bs.entry(u.home_id).or_insert(0) += sup;
                        }
                    }
                    (ps, bs)
                });

                let requested_type = (b.meta >> 2) & 0x07;
                let (ukind, raw_hp, u_cost, u_sup, cd_mult) = match requested_type {
                    1 if b.level >= 2 => (1_u8, archer.hp, archer.cost, archer.supply_cost, 1.25_f32),
                    2 if b.level >= 3 => (2_u8, titan.hp, titan.cost, titan.supply_cost, 2.1_f32),
                    _ => (0_u8, raider.hp, raider.cost, raider.supply_cost, 1.0_f32),
                };

                let local_cap = if lvl.local_supply_cap > 0 {
                    lvl.local_supply_cap
                } else {
                    match b.level {
                        1 => 3,
                        2 => 4,
                        _ => 6,
                    }
                };
                let cur_global = player_supply.get(&b.owner_id).copied().unwrap_or(0);
                let cur_local = barrack_supply.get(&b.id).copied().unwrap_or(0);

                if cur_local + u_sup <= local_cap {
                    if let Some(p) = players.get_mut(&b.owner_id) {
                        if cur_global + u_sup <= p.unit_cap && p.energy >= u_cost {
                            if let Some(zone) = zones.get(&b.owner_id) {
                                let angle = rng.gen_range(0.0..TAU);
                                let offset = barrack_cfg.radius + 14.0;
                                let ux = zone.x + b.rel_x + angle.cos() * offset;
                                let uy = zone.y + b.rel_y + angle.sin() * offset;
                                let max_hp = raw_hp.round() as u16;
                                let spawn_squad = p
                                    .reinforce_squad
                                    .filter(|sid| p.squads.contains_key(sid))
                                    .unwrap_or(0);
                                spawned_units.push(Unit {
                                    id: 0,
                                    owner_id: b.owner_id,
                                    kind: ukind,
                                    squad: spawn_squad,
                                    x: ux,
                                    y: uy,
                                    vx: 0.0,
                                    vy: 0.0,
                                    hp: max_hp,
                                    max_hp,
                                    attack_cooldown_left: 0.0,
                                    last_hit_by: None,
                                    last_hit_src: SRC_UNKNOWN,
                                    rad_accum: 0.0,
                                    in_lower_ring_sec: 0.0,
                                    regen_accum: 0.0,
                                    home_id: b.id,
                                    knockback_vx: 0.0,
                                    knockback_vy: 0.0,
                                    target_id: 0,
                                    target_idx: 0,
                                    target_is_building: false,
                                });
                                *player_supply.entry(b.owner_id).or_insert(0) += u_sup;
                                *barrack_supply.entry(b.id).or_insert(0) += u_sup;
                                p.energy -= u_cost;
                                b.cooldown_left = lvl.spawn_interval_sec * cd_mult;
                                continue;
                            }
                        }
                    }
                }
                b.cooldown_left = 0.0;
            }
        }

        if !spawned_units.is_empty() {
            for u in &mut spawned_units {
                u.id = self.alloc_entity_id();
            }
            self.units.extend(spawned_units);
        }
    }
}
