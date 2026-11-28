use std::f32::consts::TAU;

use rand::Rng;

use crate::telemetry::SRC_UNKNOWN;
use crate::types::Building;

use super::super::World;

pub const ATTRACTOR_KIND: u8 = 6;

impl World {
    pub(super) fn step_attractors(&mut self, dt: f32) {
        let max_active = self.config.pve.attractor_max_active;
        if max_active == 0 {
            return;
        }
        let active_count = self
            .buildings
            .iter()
            .filter(|b| b.owner_id == 0 && b.kind == ATTRACTOR_KIND && b.hp > 0)
            .count();
        if active_count >= max_active {
            return;
        }

        self.attractor_spawn_accum -= dt;
        if self.attractor_spawn_accum > 0.0 {
            return;
        }

        self.spawn_attractor();
        self.attractor_spawn_accum = self.config.pve.attractor_spawn_interval_sec.max(10.0);
    }

    fn spawn_attractor(&mut self) {
        let ring3_r = self.config.world.ring3_radius;
        let map_r = self.config.world.map_radius;
        let min_r = ring3_r + 180.0;
        let max_r = (map_r - 260.0).max(min_r + 60.0);
        let attr_r = self.config.pve.attractor_radius.max(20.0);

        let (mut sx, mut sy) = (min_r, 0.0_f32);
        let mut best_clearance = f32::NEG_INFINITY;
        for _ in 0..24 {
            let angle = self.rng.gen_range(0.0..TAU);
            let dist = self.rng.gen_range(min_r..=max_r);
            let cx = angle.cos() * dist;
            let cy = angle.sin() * dist;
            let mut min_clear = f32::MAX;
            for z in self.zones.values() {
                let clear = (cx - z.x).hypot(cy - z.y) - (attr_r + z.radius);
                if clear < min_clear {
                    min_clear = clear;
                }
            }
            for b in &self.buildings {
                if b.owner_id == 0 && b.kind == ATTRACTOR_KIND && b.hp > 0 {
                    let clear = (cx - b.rel_x).hypot(cy - b.rel_y) - attr_r * 4.0;
                    if clear < min_clear {
                        min_clear = clear;
                    }
                }
            }
            if min_clear > best_clearance {
                best_clearance = min_clear;
                sx = cx;
                sy = cy;
                if best_clearance >= 300.0 {
                    break;
                }
            }
        }

        let hp = self.config.pve.attractor_hp.max(200);
        let id = self.alloc_entity_id();
        self.buildings.push(Building {
            id,
            owner_id: 0,
            kind: ATTRACTOR_KIND,
            level: 1,
            meta: 0,
            rel_x: sx,
            rel_y: sy,
            hp,
            max_hp: hp,
            angle: 0,
            cooldown_left: 0.0,
            invested_cost: 0.0,
            invested_trophies: 0.0,
            last_hit_by: None,
            last_hit_src: SRC_UNKNOWN,
        });
    }
}
