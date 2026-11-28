mod economy;
mod intel;
mod roster;
mod tactics;
mod teleport;

use std::collections::{HashMap, HashSet};

use rand::Rng;

use crate::types::PlayerId;

use super::World;
use roster::archetype_profile;

impl World {
    pub(super) fn step_bots(&mut self, dt: f32) {
        self.bot_maintain_roster();

        let bot_ids: Vec<(PlayerId, u8, u8)> = self
            .players
            .values()
            .filter(|p| p.is_bot)
            .map(|p| (p.id, p.bot_tier, p.bot_archetype))
            .collect();

        let mut ready_bots: Vec<(PlayerId, u8, u8)> = Vec::new();
        for (pid, tier, arch) in bot_ids {
            let think_cd = match tier {
                1 => 1.1,
                3 => 0.45,
                _ => 0.72,
            };
            if let Some(p) = self.players.get_mut(&pid) {
                p.bot_think_cooldown -= dt;
                if p.bot_think_cooldown <= 0.0 {
                    p.bot_think_cooldown = think_cd * self.rng.gen_range(0.92..1.08);
                    ready_bots.push((pid, tier, arch));
                }
            }
        }
        if ready_bots.is_empty() {
            return;
        }

        let mut sorted_scores: Vec<(PlayerId, u32)> = self
            .players
            .values()
            .filter(|p| p.score >= 400)
            .map(|p| (p.id, p.score))
            .collect();
        sorted_scores.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        let top3_leaders: HashSet<PlayerId> =
            sorted_scores.iter().take(3).map(|&(id, _)| id).collect();
        let leader_id = sorted_scores.first().map(|&(id, _)| id);

        let mut intel_map = self.gather_bot_intel();

        for (pid, tier, arch) in ready_bots {
            let Some(mut bot_intel) = intel_map.remove(&pid) else {
                continue;
            };
            let profile = archetype_profile(arch, tier);

            // 1. Configure barracks mix & run economy/upgrades
            if !bot_intel.barracks_info.is_empty() {
                self.bot_configure_barracks(pid, arch, &bot_intel.barracks_info);
            }
            self.bot_step_economy(pid, tier, arch, &mut bot_intel);

            let Some(own_z) = self.zones.get(&pid).cloned() else {
                intel_map.insert(pid, bot_intel);
                continue;
            };
            let (unit_cap, bot_now_sec, reinforce_sq, squad_orders) = self
                .players
                .get(&pid)
                .map(|p| {
                    (
                        p.unit_cap,
                        p.survival_sec,
                        p.reinforce_squad,
                        p.squads.clone(),
                    )
                })
                .unwrap_or((4, 0.0, None, HashMap::new()));

            // 2. Priority Defense
            if self.bot_handle_defense(
                pid,
                &bot_intel,
                &own_z,
                reinforce_sq,
                &bot_intel.live_squads,
                &squad_orders,
            ) {
                intel_map.insert(pid, bot_intel);
                continue;
            }

            // 3. Update honest Fog-of-War intel
            let own_r = (own_z.x * own_z.x + own_z.y * own_z.y).sqrt();
            let own_r_eq = self.equilibrium_radius(pid);
            let mut known = self
                .players
                .get_mut(&pid)
                .map(|p| std::mem::take(&mut p.bot_known_intel))
                .unwrap_or_default();
            let mut scout_ang = self
                .players
                .get(&pid)
                .map(|p| p.bot_scout_angle)
                .unwrap_or(0.0);

            self.bot_update_fog_intel(
                pid,
                &own_z,
                &bot_intel,
                bot_now_sec,
                &top3_leaders,
                &intel_map,
                &mut known,
            );

            // 4. Pick best target by archetype utility
            let (garrison_ready, best_target) = self.bot_select_target(
                arch,
                &profile,
                &own_z,
                unit_cap,
                bot_intel.garrison_count,
                &bot_intel,
                &known,
                &bot_intel.live_squads,
                &squad_orders,
                leader_id,
            );

            // 5. Honest vision-based tactical teleport
            let _ = self.bot_try_tactical_teleport(
                pid,
                arch,
                &own_z,
                own_r,
                &bot_intel,
                best_target,
                profile.uses_tactical_tp && bot_intel.enemy_threat_power < 1.0,
            );

            // 6. Multi-squad command dispatch, early scouting, and auto-reinforce
            self.bot_dispatch_orders(
                pid,
                tier,
                arch,
                &profile,
                &own_z,
                own_r,
                own_r_eq,
                bot_intel.garrison_count,
                garrison_ready,
                reinforce_sq,
                &bot_intel.live_squads,
                &squad_orders,
                best_target,
                &mut scout_ang,
            );

            if let Some(p) = self.players.get_mut(&pid) {
                p.bot_known_intel = known;
                p.bot_scout_angle = scout_ang;
            }

            intel_map.insert(pid, bot_intel);
        }
    }
}
