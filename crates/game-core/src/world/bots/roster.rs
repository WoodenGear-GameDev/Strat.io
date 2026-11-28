use rand::Rng;

use super::super::World;

pub(super) struct ArchetypeProfile {
    pub wave_min: usize,
    pub wave_cap_pct: usize,
    pub scout_min: usize,
    pub min_win_ratio: f32,
    pub crown_mult: f32,
    pub finisher_mult: f32,
    pub exposed_mult: f32,
    pub gen_loot_weight: f32,
    pub dist_bias: f32,
    pub uses_reinforce: bool,
    pub uses_tactical_tp: bool,
    pub extra_gens: usize,
    pub extra_turrets: bool,
    pub desired_beacons: usize,
}

pub(super) fn archetype_profile(arch: u8, tier: u8) -> ArchetypeProfile {
    match arch {
        // 0: Старатель — осторожный экономист
        0 => ArchetypeProfile {
            wave_min: 7,
            wave_cap_pct: 85,
            scout_min: 3,
            min_win_ratio: 1.05,
            crown_mult: if tier == 3 { 1.5 } else { 1.2 },
            finisher_mult: 2.0,
            exposed_mult: 1.55,
            gen_loot_weight: 0.30,
            dist_bias: 140.0,
            uses_reinforce: false,
            uses_tactical_tp: false,
            extra_gens: 1,
            extra_turrets: false,
            desired_beacons: 3,
        },
        // 1: Дредноут — тяжёлый танк, охотник на лидера
        1 => ArchetypeProfile {
            wave_min: 8,
            wave_cap_pct: 75,
            scout_min: 3,
            min_win_ratio: 0.78,
            crown_mult: 2.8,
            finisher_mult: 2.1,
            exposed_mult: 1.55,
            gen_loot_weight: 0.28,
            dist_bias: 240.0,
            uses_reinforce: true,
            uses_tactical_tp: true,
            extra_gens: 0,
            extra_turrets: true,
            desired_beacons: 3,
        },
        // 2: Агрессор / Охотник / Рейдер — быстрые частые волны, высокий риск
        2 => ArchetypeProfile {
            wave_min: 4,
            wave_cap_pct: 45,
            scout_min: 2,
            min_win_ratio: 0.62,
            crown_mult: if tier == 3 { 1.65 } else { 1.25 },
            finisher_mult: 2.6,
            exposed_mult: 1.65,
            gen_loot_weight: 0.28,
            dist_bias: 260.0,
            uses_reinforce: true,
            uses_tactical_tp: true,
            extra_gens: 0,
            extra_turrets: false,
            desired_beacons: 3,
        },
        // 3: Мародёр — охотник за оголёнными тылами и генераторами
        3 => ArchetypeProfile {
            wave_min: 5,
            wave_cap_pct: 55,
            scout_min: 2,
            min_win_ratio: 0.74,
            crown_mult: if tier == 3 { 1.55 } else { 1.25 },
            finisher_mult: 2.6,
            exposed_mult: 2.6,
            gen_loot_weight: 0.42,
            dist_bias: 240.0,
            uses_reinforce: true,
            uses_tactical_tp: true,
            extra_gens: 0,
            extra_turrets: false,
            desired_beacons: 3,
        },
        // 4: Архитектор — чисто турельный осадник
        _ => ArchetypeProfile {
            wave_min: 3,
            wave_cap_pct: 50,
            scout_min: 2,
            min_win_ratio: 0.78,
            crown_mult: if tier == 3 { 1.65 } else { 1.25 },
            finisher_mult: 2.5,
            exposed_mult: 1.85,
            gen_loot_weight: 0.28,
            dist_bias: 160.0,
            uses_reinforce: false,
            uses_tactical_tp: true,
            extra_gens: 2,
            extra_turrets: true,
            desired_beacons: 4,
        },
    }
}

const BOT_ROSTER: [(u8, u8, &str); 15] = [
    (1, 2, "Малек-1"),
    (1, 3, "Малек-2"),
    (1, 0, "Старатель-1"),
    (1, 2, "Малек-3"),
    (2, 2, "Рейдер-1"),
    (2, 3, "Рейдер-2"),
    (2, 1, "Дредноут-1"),
    (2, 0, "Старатель-2"),
    (2, 3, "Рейдер-3"),
    (3, 4, "Архитектор-1"),
    (3, 2, "Охотник-1"),
    (3, 1, "Дредноут-2"),
    (3, 3, "Мародёр-1"),
    (3, 4, "Архитектор-2"),
    (3, 2, "Охотник-2"),
];

impl World {
    /// Maintains `config.world.bot_count` bots + Ring 1/Ring 2 ecosystem quotas.
    pub(super) fn bot_maintain_roster(&mut self) {
        let target_bot_count = self.config.world.bot_count;
        let cur_bot_count = self.players.values().filter(|p| p.is_bot).count();
        if cur_bot_count < target_bot_count {
            for &(tier, arch, name) in BOT_ROSTER.iter().take(target_bot_count) {
                if self.players.values().filter(|p| p.is_bot).count() >= target_bot_count {
                    break;
                }
                if !self.players.values().any(|p| p.is_bot && p.name == name) {
                    self.add_player_internal(name.to_string(), None, 0, tier, arch);
                }
            }
        }
        let max_with_overflow = target_bot_count + 4;
        if self.players.values().filter(|p| p.is_bot).count() < max_with_overflow {
            let mut r1_bots = 0_usize;
            let mut r2_bots = 0_usize;
            for p in self.players.values().filter(|p| p.is_bot) {
                if let Some(z) = self.zones.get(&p.id) {
                    match self.ring_for_radius((z.x * z.x + z.y * z.y).sqrt()) {
                        1 => r1_bots += 1,
                        2 => r2_bots += 1,
                        _ => {}
                    }
                }
            }
            if r1_bots < 3 {
                let arch = [2_u8, 3, 0][self.rng.gen_range(0..3)];
                let num = self.next_player_id;
                self.add_player_internal(format!("Новичок-{num}"), None, 0, 1, arch);
            } else if r2_bots < 3 {
                let arch = [2_u8, 3, 1][self.rng.gen_range(0..3)];
                let num = self.next_player_id;
                self.add_player_internal(format!("Корсар-{num}"), None, 0, 2, arch);
            }
        }
    }
}
