mod attractors;
mod hives;

use super::World;
use crate::types::CombatHeatSpot;

impl World {
    pub(super) fn step_pve(&mut self, dt: f32) {
        self.step_hives(dt);
        self.step_attractors(dt);

        let now = self.elapsed_sec;
        self.combat_heats.retain(|h| now < h.until_sec);
    }

    pub(super) fn record_combat_heat(&mut self, x: f32, y: f32, weight: f32) {
        let now = self.elapsed_sec;
        for h in &mut self.combat_heats {
            let dx = h.x - x;
            let dy = h.y - y;
            if dx * dx + dy * dy <= 320.0 * 320.0 {
                h.x = h.x * 0.7 + x * 0.3;
                h.y = h.y * 0.7 + y * 0.3;
                h.power = (h.power + weight).min(12.0);
                h.until_sec = h.until_sec.max(now + 7.5);
                return;
            }
        }
        if self.combat_heats.len() < 16 {
            self.combat_heats.push(CombatHeatSpot {
                x,
                y,
                power: weight,
                show_at_sec: now + 1.2,
                until_sec: now + 7.5,
            });
        }
    }
}
