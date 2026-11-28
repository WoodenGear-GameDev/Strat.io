use crate::protocol::ClientMessage;
use crate::types::{PlayerId, Zone};

use super::super::hex::beacon_ext_radius;
use super::super::World;
use super::intel::{bot_has_vision_fast, BotPlayerIntel};

impl World {
    /// Attempts a legal vision-checked teleport to a candidate position `(tx, ty)`.
    fn bot_try_teleport_to_point(
        &mut self,
        pid: PlayerId,
        zone: &Zone,
        intel: &BotPlayerIntel,
        tx: f32,
        ty: f32,
        max_ring_r: f32,
    ) -> bool {
        let Some(p) = self.players.get(&pid) else {
            return false;
        };
        if p.pending_teleport.is_some() {
            return false;
        }
        let energy = p.energy;
        let tr = (tx * tx + ty * ty).sqrt();
        if tr > max_ring_r {
            return false;
        }
        if !bot_has_vision_fast(zone, intel, tx, ty) {
            return false;
        }
        let zone_clearance = self.config.world.teleport_zone_clearance + 15.0;
        let clear_zones = self.zones.values().all(|other| {
            if other.player_id == pid {
                return true;
            }
            let dx = tx - other.x;
            let dy = ty - other.y;
            let min_d = zone.radius + other.radius + zone_clearance;
            dx * dx + dy * dy >= min_d * min_d
        });
        if !clear_zones {
            return false;
        }
        let node_clearance = self.config.world.teleport_node_clearance + 5.0;
        let clear_beacons = self.buildings.iter().all(|b| {
            if b.owner_id == pid || b.kind != 5 {
                return true;
            }
            let Some(other) = self.zones.get(&b.owner_id) else {
                return true;
            };
            let ext_r = beacon_ext_radius(b.level);
            let dx = tx - (other.x + b.rel_x);
            let dy = ty - (other.y + b.rel_y);
            let min_d = zone.radius + ext_r + node_clearance;
            dx * dx + dy * dy >= min_d * min_d
        });
        if !clear_beacons {
            return false;
        }
        let r_eq = self.equilibrium_radius(pid);
        let dx = tx - zone.x;
        let dy = ty - zone.y;
        let dist2 = dx * dx + dy * dy;
        if dist2 < 240.0 * 240.0 {
            return false;
        }
        let dist = dist2.sqrt();
        let ring_pen = self.config.world.teleport_ring_penalty;
        let est_cost = self.config.world.teleport_base_cost
            + dist * self.config.world.teleport_dist_factor
            + (tr - r_eq).abs() * ring_pen;
        if energy < est_cost + 35.0 {
            return false;
        }
        self.apply_command(
            pid,
            ClientMessage::TeleportZone {
                target_x: tx,
                target_y: ty,
            },
        )
        .is_none()
    }

    /// Tries to teleport the bot using its own drones' vision: either to a higher ring or closer to a distant target.
    pub(super) fn bot_try_tactical_teleport(
        &mut self,
        pid: PlayerId,
        arch: u8,
        own_z: &Zone,
        own_r: f32,
        intel: &BotPlayerIntel,
        desired_target: Option<(f32, f32)>,
        allow_tactical: bool,
    ) -> bool {
        let Some(p) = self.players.get(&pid) else {
            return false;
        };
        let min_tp_e = if arch == 4 { 550.0 } else { 220.0 };
        if p.pending_teleport.is_some() || p.energy < min_tp_e {
            return false;
        }
        let max_ring_r = self.max_radius_for_hq(intel.hq_lvl, own_z.radius);

        if allow_tactical {
            if let Some((tx, ty)) = desired_target {
                let dtx = tx - own_z.x;
                let dty = ty - own_z.y;
                let cur_d2 = dtx * dtx + dty * dty;
                let min_jump_trigger = if arch == 4 && intel.hq_lvl >= 3 && intel.turrets >= 6 {
                    580.0
                } else {
                    850.0
                };
                if cur_d2 > min_jump_trigger * min_jump_trigger {
                    let (min_e_dist, max_e_dist) = if arch == 4 {
                        (340.0_f32, 570.0_f32)
                    } else {
                        (280.0_f32, 560.0_f32)
                    };
                    for &(dx, dy) in intel.squad_unit_positions.iter().step_by(2) {
                        let ex = dx - tx;
                        let ey = dy - ty;
                        let e2 = ex * ex + ey * ey;
                        if (min_e_dist * min_e_dist..=max_e_dist * max_e_dist).contains(&e2)
                            && self.bot_try_teleport_to_point(pid, own_z, intel, dx, dy, max_ring_r)
                        {
                            return true;
                        }
                    }
                }
            }
        }

        if intel.hq_lvl >= 2 && own_r > max_ring_r {
            let max_ring_r2 = max_ring_r * max_ring_r;
            for &(dx, dy) in &intel.squad_unit_positions {
                if dx * dx + dy * dy <= max_ring_r2
                    && self.bot_try_teleport_to_point(pid, own_z, intel, dx, dy, max_ring_r)
                {
                    return true;
                }
            }
            let ang = own_z.y.atan2(own_z.x);
            let edge_r = (own_r - 280.0).min(max_ring_r).max(120.0);
            if self.bot_try_teleport_to_point(
                pid,
                own_z,
                intel,
                ang.cos() * edge_r,
                ang.sin() * edge_r,
                max_ring_r,
            ) {
                return true;
            }
        }
        false
    }
}
