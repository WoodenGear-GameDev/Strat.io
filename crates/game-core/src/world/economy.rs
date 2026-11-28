use std::collections::HashMap;
use std::f32::consts::TAU;

use rand::Rng;

use crate::telemetry::SRC_UNKNOWN;
use crate::types::{PlayerId, Unit};

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

    pub(super) fn energy_cap_for_player(&self, player_id: PlayerId) -> f32 {
        let hq_lvl = self.hq_level_of(player_id);
        let mut cap = self.energy_cap_for_hq(hq_lvl);
        if let Some(gen_cfg) = self.config.building_by_kind(1) {
            for b in &self.buildings {
                if b.owner_id == player_id && b.kind == 1 && b.hp > 0 {
                    if let Some(lvl) = gen_cfg.levels.get(usize::from(b.level.saturating_sub(1))) {
                        cap += lvl.energy_cap;
                    }
                }
            }
        }
        cap
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

    pub(super) fn grant_energy(&mut self, pid: PlayerId, amount: f32, cap_mult: f32) {
        if amount <= 0.0 {
            return;
        }
        let max_e = self.energy_cap_for_player(pid) * cap_mult;
        if let Some(p) = self.players.get_mut(&pid) {
            p.total_energy_earned += amount;
            if p.energy < max_e {
                p.energy = (p.energy + amount).min(max_e);
            }
        }
    }

    pub(super) fn grant_trophies(&mut self, pid: PlayerId, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        if let Some(p) = self.players.get_mut(&pid) {
            p.trophies += amount;
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

    pub(super) fn step_base_regen(&mut self, dt: f32) {
        let regen_pct = self.config.world.base_regen_hp_per_sec_pct;
        let hq_mult = self.config.world.base_regen_hq_mult;
        let energy_per_hp = self.config.world.base_regen_energy_per_hp.max(0.001);

        let mut active_players: Option<Vec<PlayerId>> = None;
        for p in self.players.values_mut() {
            if p.regen_enabled {
                p.regen_accum += dt;
                if p.regen_accum >= 0.5 {
                    p.regen_accum -= 0.5;
                    active_players.get_or_insert_with(Vec::new).push(p.id);
                }
            } else {
                p.regen_accum = 0.0;
            }
        }
        let Some(active_players) = active_players else {
            return;
        };
        for b in &mut self.buildings {
            if b.hp > 0 && b.hp < b.max_hp && active_players.contains(&b.owner_id) {
                let Some(p) = self.players.get_mut(&b.owner_id) else {
                    continue;
                };
                let eff_rate = if b.kind == 0 {
                    regen_pct * hq_mult
                } else {
                    regen_pct
                };
                let missing = b.max_hp.saturating_sub(b.hp);
                let want_heal = ((f32::from(b.max_hp) * eff_rate * 0.5).round().max(1.0) as u16)
                    .min(missing);
                let affordable_hp = (p.energy / energy_per_hp).floor().max(0.0) as u16;
                let actual_heal = want_heal.min(affordable_hp);
                if actual_heal > 0 {
                    p.energy = (p.energy - f32::from(actual_heal) * energy_per_hp).max(0.0);
                    b.hp = b.hp.saturating_add(actual_heal).min(b.max_hp);
                }
            }
        }
    }

    pub(super) fn step_income_and_spawning(&mut self, dt: f32) {
        // 1. Passive energy generation (clamped to HQ + generator tank cap)
        {
            let config = &self.config;
            let hq_cfg = config.building_by_kind(0);
            let gen_cfg = config.building_by_kind(1);
            let mut gen_cap_bonus: HashMap<PlayerId, f32> = HashMap::new();
            if let Some(gcfg) = gen_cfg {
                for b in &self.buildings {
                    if b.kind == 1 && b.hp > 0 {
                        if let Some(lvl) = gcfg.levels.get(usize::from(b.level.saturating_sub(1))) {
                            *gen_cap_bonus.entry(b.owner_id).or_insert(0.0) += lvl.energy_cap;
                        }
                    }
                }
            }
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
                    let base_cap = if cfg_cap > 0.0 {
                        cfg_cap
                    } else {
                        match hq_lvl {
                            0 | 1 => 400.0,
                            2 => 950.0,
                            _ => 2000.0,
                        }
                    };
                    let max_e = base_cap + gen_cap_bonus.get(&b.owner_id).copied().unwrap_or(0.0);
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

            let mut player_supply_opt: Option<HashMap<PlayerId, u16>> = None;

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

                let player_supply = player_supply_opt.get_or_insert_with(|| {
                    let mut ps: HashMap<PlayerId, u16> = HashMap::new();
                    for u in units.iter() {
                        if u.owner_id != 0 && u.hp > 0 {
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
                        }
                    }
                    ps
                });

                let requested_type = (b.meta >> 2) & 0x07;
                let (ukind, raw_hp, u_cost, u_sup, cd_mult) = match requested_type {
                    1 if b.level >= 2 => (1_u8, archer.hp, archer.cost, archer.supply_cost, 1.25_f32),
                    2 if b.level >= 3 => (2_u8, titan.hp, titan.cost, titan.supply_cost, 2.1_f32),
                    _ => (0_u8, raider.hp, raider.cost, raider.supply_cost, 1.0_f32),
                };

                let cur_global = player_supply.get(&b.owner_id).copied().unwrap_or(0);

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
                                regen_accum: 0.0,
                                home_id: b.id,
                                knockback_vx: 0.0,
                                knockback_vy: 0.0,
                                target_id: 0,
                                target_idx: 0,
                                target_is_building: false,
                            });
                            *player_supply.entry(b.owner_id).or_insert(0) += u_sup;
                            p.energy -= u_cost;
                            b.cooldown_left = lvl.spawn_interval_sec * cd_mult;
                            continue;
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
