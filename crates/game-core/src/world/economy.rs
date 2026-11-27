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

    pub(super) fn hq_level_of(&self, player_id: PlayerId) -> u8 {
        self.buildings
            .iter()
            .find(|b| b.owner_id == player_id && b.kind == 0)
            .map(|b| b.level)
            .unwrap_or(1)
    }

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

    pub(super) fn step_ring_scoring(&mut self, _dt: f32) {
        let tick_rate = u32::from(self.config.world.tick_rate.max(1));
        if self.tick % tick_rate != 0 {
            return;
        }
        let zone_rings: Vec<(PlayerId, u8)> = self
            .zones
            .values()
            .map(|z| (z.player_id, Self::ring_for_radius(z.x.hypot(z.y))))
            .collect();
        for (pid, ring) in zone_rings {
            let pts = match ring {
                2 => 1,
                3 => 4,
                _ => 0,
            };
            if pts > 0 {
                if let Some(p) = self.players.get_mut(&pid) {
                    p.score += pts;
                }
            }
        }
    }

    pub(super) fn step_ooc_regen(&mut self, dt: f32) {
        let mut regen_rates: HashMap<PlayerId, f32> = HashMap::new();
        for p in self.players.values_mut() {
            if (p.survival_sec - p.last_damaged_sec) >= 6.0 {
                p.regen_accum += dt;
                if p.regen_accum >= 0.5 {
                    p.regen_accum -= 0.5;
                    regen_rates.insert(p.id, 0.015);
                }
            } else {
                p.regen_accum = 0.0;
            }
        }
        if regen_rates.is_empty() {
            return;
        }
        for b in &mut self.buildings {
            if b.hp > 0 && b.hp < b.max_hp {
                if let Some(&rate) = regen_rates.get(&b.owner_id) {
                    let eff_rate = if b.kind == 0 { rate * 0.35 } else { rate };
                    let heal = (f32::from(b.max_hp) * eff_rate * 0.5).round().max(1.0) as u16;
                    b.hp = b.hp.saturating_add(heal).min(b.max_hp);
                }
            }
        }
    }

    pub(super) fn step_income_and_spawning(&mut self, dt: f32) {
        let hq_levels = self.hq_level_map();
        let mut player_supply: HashMap<PlayerId, u16> = HashMap::new();
        let mut barrack_supply: HashMap<EntityId, u16> = HashMap::new();
        for u in &self.units {
            if u.owner_id != 0 && u.hp > 0 && u.kind != 4 {
                let sup = self.unit_supply_cost(u.kind);
                *player_supply.entry(u.owner_id).or_insert(0) += sup;
                *barrack_supply.entry(u.home_id).or_insert(0) += sup;
            }
        }

        // 1. Passive energy generation (clamped to HQ energy_cap)
        let mut passive_income: Vec<(PlayerId, u8, f32)> = Vec::new();
        for b in &self.buildings {
            if b.kind != 0 && b.kind != 1 {
                continue;
            }
            if let Some(cfg) = self.config.building_by_kind(b.kind) {
                if let Some(lvl) = cfg.levels.get((b.level as usize).saturating_sub(1)) {
                    if lvl.income_per_sec > 0.0 {
                        passive_income.push((
                            b.owner_id,
                            if b.kind == 0 { 0 } else { 1 },
                            lvl.income_per_sec * dt,
                        ));
                    }
                }
            }
        }

        for (pid, src_slot, amount) in passive_income {
            let hq_lvl = hq_levels.get(&pid).copied().unwrap_or(1);
            self.grant_energy(pid, amount, 1.0, hq_lvl);
            if let Some(tl) = self.telemetry.as_mut() {
                tl.log_income(pid, usize::from(src_slot), amount);
            }
        }

        // 2. Paid Barracks spawning (per-barrack local supply cap + global supply cap + unit energy cost)
        let raider_cfg = self.config.unit_by_kind(0).cloned();
        let archer_cfg = self.config.unit_by_kind(1).cloned();
        let titan_cfg = self.config.unit_by_kind(2).cloned();
        let mut player_energy: HashMap<PlayerId, f32> = self
            .players
            .iter()
            .map(|(&pid, p)| (pid, p.energy))
            .collect();
        let player_caps: HashMap<PlayerId, u16> = self
            .players
            .iter()
            .map(|(&pid, p)| (pid, p.unit_cap))
            .collect();

        let mut spawned_units: Vec<Unit> = Vec::new();
        for b in &mut self.buildings {
            if b.kind != 3 || (b.meta & 0x01) != 0 {
                continue;
            }
            let Some(cfg) = self.config.building_by_kind(3) else {
                continue;
            };
            let Some(lvl) = cfg.levels.get((b.level as usize).saturating_sub(1)) else {
                continue;
            };
            if lvl.spawn_interval_sec <= 0.0 {
                continue;
            }

            b.cooldown_left -= dt;
            if b.cooldown_left > 0.0 {
                continue;
            }

            let requested_type = (b.meta >> 2) & 0x07;
            let (ukind, raw_hp, u_cost, u_sup, cd_mult) = match requested_type {
                1 if b.level >= 2 => (
                    1_u8,
                    archer_cfg.as_ref().map(|c| c.hp).unwrap_or(44.0),
                    archer_cfg.as_ref().map(|c| c.cost).unwrap_or(24.0),
                    archer_cfg.as_ref().map(|c| c.supply_cost.max(1)).unwrap_or(1),
                    1.25_f32,
                ),
                2 if b.level >= 3 => (
                    2_u8,
                    titan_cfg.as_ref().map(|c| c.hp).unwrap_or(540.0),
                    titan_cfg.as_ref().map(|c| c.cost).unwrap_or(65.0),
                    titan_cfg.as_ref().map(|c| c.supply_cost.max(1)).unwrap_or(3),
                    2.1_f32,
                ),
                _ => (
                    0_u8,
                    raider_cfg.as_ref().map(|c| c.hp).unwrap_or(78.0),
                    raider_cfg.as_ref().map(|c| c.cost).unwrap_or(14.0),
                    raider_cfg.as_ref().map(|c| c.supply_cost.max(1)).unwrap_or(1),
                    1.0_f32,
                ),
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
            let global_cap = player_caps.get(&b.owner_id).copied().unwrap_or(0);
            let cur_global = player_supply.get(&b.owner_id).copied().unwrap_or(0);
            let cur_local = barrack_supply.get(&b.id).copied().unwrap_or(0);
            let cur_energy = player_energy.get(&b.owner_id).copied().unwrap_or(0.0);

            if cur_local + u_sup <= local_cap
                && cur_global + u_sup <= global_cap
                && cur_energy >= u_cost
            {
                if let Some(zone) = self.zones.get(&b.owner_id) {
                    let angle = self.rng.gen_range(0.0..TAU);
                    let offset = cfg.radius + 14.0;
                    let ux = zone.x + b.rel_x + angle.cos() * offset;
                    let uy = zone.y + b.rel_y + angle.sin() * offset;
                    let max_hp = raw_hp.round() as u16;
                    let spawn_squad = self
                        .players
                        .get(&b.owner_id)
                        .and_then(|p| p.reinforce_squad.filter(|sid| p.squads.contains_key(sid)))
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
                    });
                    *player_supply.entry(b.owner_id).or_insert(0) += u_sup;
                    *barrack_supply.entry(b.id).or_insert(0) += u_sup;
                    player_energy.insert(b.owner_id, cur_energy - u_cost);
                    b.cooldown_left = lvl.spawn_interval_sec * cd_mult;
                    continue;
                }
            }
            b.cooldown_left = 0.0;
        }

        for (pid, e_left) in player_energy {
            if let Some(p) = self.players.get_mut(&pid) {
                p.energy = e_left;
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
