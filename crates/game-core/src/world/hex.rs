use std::collections::HashSet;
use std::f32::consts::TAU;

use crate::types::PlayerId;

use super::World;

pub const RING1_RADIUS: f32 = 1100.0;
pub const RING2_RADIUS: f32 = 2150.0;
pub const HEX_SIZE: f32 = 34.0;
pub type TriCoord = (i32, i32, u8);

pub const HEX_EDGE_DIRS: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

pub fn is_large_building(kind: u8) -> bool {
    kind == 0 || kind == 2 || kind == 3
}

pub fn hex_to_rel(q: i32, r: i32) -> (f32, f32) {
    let sqrt3 = 3.0_f32.sqrt();
    let qf = q as f32;
    let rf = r as f32;
    let x = HEX_SIZE * (sqrt3 * qf + (sqrt3 / 2.0) * rf);
    let y = HEX_SIZE * 1.5 * rf;
    (x, y)
}

pub fn rel_to_hex_round(x: f32, y: f32) -> (i32, i32) {
    let sqrt3 = 3.0_f32.sqrt();
    let qf = ((sqrt3 / 3.0) * x - (1.0 / 3.0) * y) / HEX_SIZE;
    let rf = ((2.0 / 3.0) * y) / HEX_SIZE;
    let sf = -qf - rf;

    let mut rq = qf.round();
    let mut rr = rf.round();
    let rs = sf.round();

    let q_diff = (rq - qf).abs();
    let r_diff = (rr - rf).abs();
    let s_diff = (rs - sf).abs();

    if q_diff > r_diff && q_diff > s_diff {
        rq = -rr - rs;
    } else if r_diff > s_diff {
        rr = -rq - rs;
    }

    (rq as i32, rr as i32)
}

pub fn hex_dist(q1: i32, r1: i32, q2: i32, r2: i32) -> i32 {
    ((q1 - q2).abs() + (q1 + r1 - q2 - r2).abs() + (r1 - r2).abs()) / 2
}

pub fn hex_vertex(q: i32, r: i32, i: u8) -> (f32, f32) {
    let (cx, cy) = hex_to_rel(q, r);
    let ang = f32::from(i) * (TAU / 6.0) - TAU / 12.0;
    (cx + ang.cos() * HEX_SIZE, cy + ang.sin() * HEX_SIZE)
}

pub fn collect_rhombuses(open_hexes: &HashSet<(i32, i32)>) -> Vec<([TriCoord; 2], (f32, f32))> {
    let mut out = Vec::with_capacity(open_hexes.len() * 8);
    for &(q, r) in open_hexes {
        let (cx, cy) = hex_to_rel(q, r);
        for s in 0..6_u8 {
            let s_next = (s + 1) % 6;
            let (vx1, vy1) = hex_vertex(q, r, s_next);
            out.push((
                [(q, r, s), (q, r, s_next)],
                ((cx + vx1) * 0.5, (cy + vy1) * 0.5),
            ));

            let (dq, dr) = HEX_EDGE_DIRS[s as usize];
            let nq = q + dq;
            let nr = r + dr;
            if open_hexes.contains(&(nq, nr)) && (q, r) < (nq, nr) {
                let (vx0, vy0) = hex_vertex(q, r, s);
                let s_opp = (s + 3) % 6;
                out.push((
                    [(q, r, s), (nq, nr, s_opp)],
                    ((vx0 + vx1) * 0.5, (vy0 + vy1) * 0.5),
                ));
            }
        }
    }
    out
}

pub fn core_hex_radius(level: u8) -> i32 {
    match level {
        0 | 1 => 1,
        2 => 2,
        _ => 3,
    }
}

pub(super) fn insert_hex_disk(
    center_q: i32,
    center_r: i32,
    hex_r: i32,
    out: &mut HashSet<(i32, i32)>,
) {
    for dq in -hex_r..=hex_r {
        let min_dr = (-hex_r).max(-dq - hex_r);
        let max_dr = hex_r.min(-dq + hex_r);
        for dr in min_dr..=max_dr {
            out.insert((center_q + dq, center_r + dr));
        }
    }
}

pub fn hq_zone_radius(_base: f32, level: u8) -> f32 {
    match level {
        0 | 1 => 95.0,
        2 => 145.0,
        _ => 200.0,
    }
}

pub fn beacon_ext_radius(level: u8) -> f32 {
    match level {
        0 | 1 => 68.0,
        2 => 85.0,
        _ => 105.0,
    }
}

impl World {
    pub(super) fn ring_for_radius(r: f32) -> u8 {
        if r < RING1_RADIUS {
            1
        } else if r < RING2_RADIUS {
            2
        } else {
            3
        }
    }

    /// Computes movement direction from `(ux, uy)` to `(tx, ty)` in polar coordinates `(r, theta)`
    /// so long routes curve along arena rings instead of cutting through inner radiation rings.
    pub(super) fn polar_move_dir(ux: f32, uy: f32, tx: f32, ty: f32) -> (f32, f32) {
        let dx = tx - ux;
        let dy = ty - uy;
        let dist = dx.hypot(dy);
        if dist <= 0.001 {
            return (0.0, 0.0);
        }
        let ur = ux.hypot(uy);
        let tr = tx.hypot(ty);
        if dist < 180.0 || ur < 90.0 || tr < 90.0 {
            return (dx / dist, dy / dist);
        }
        let ut = uy.atan2(ux);
        let tt = ty.atan2(tx);
        let mut dtheta = tt - ut;
        while dtheta > std::f32::consts::PI {
            dtheta -= TAU;
        }
        while dtheta < -std::f32::consts::PI {
            dtheta += TAU;
        }
        // Prevent flip-flopping between +pi and -pi when target is nearly opposite across the arena
        if dtheta.abs() > 2.85 {
            dtheta = dtheta.abs();
        }
        if dtheta.abs() < 0.14 {
            return (dx / dist, dy / dist);
        }
        let rx = ux / ur;
        let ry = uy / ur;
        let tx_hat = -ry;
        let ty_hat = rx;
        let dr = tr - ur;
        let arc = ((ur + tr) * 0.5) * dtheta;
        let vx = rx * dr + tx_hat * arc;
        let vy = ry * dr + ty_hat * arc;
        let len = vx.hypot(vy);
        if len > 0.001 {
            (vx / len, vy / len)
        } else {
            (dx / dist, dy / dist)
        }
    }

    pub(super) fn has_vision_at(&self, player_id: PlayerId, tx: f32, ty: f32) -> bool {
        if let Some(z) = self.zones.get(&player_id) {
            if (tx - z.x).hypot(ty - z.y) <= z.radius + 360.0 {
                return true;
            }
            for b in &self.buildings {
                if b.owner_id == player_id && b.kind == 5 {
                    let vr = beacon_ext_radius(b.level) + 220.0;
                    if (tx - (z.x + b.rel_x)).hypot(ty - (z.y + b.rel_y)) <= vr {
                        return true;
                    }
                }
            }
        }
        for u in &self.units {
            if u.owner_id == player_id && u.hp > 0 && u.kind != 4 {
                if (tx - u.x).hypot(ty - u.y) <= 290.0 {
                    return true;
                }
            }
        }
        false
    }

    /// Discrete HQ-locked orbital ring radius: Ring 1 (620), Ring 2 (1580), Ring 3 (2600).
    pub fn equilibrium_radius(&self, player_id: PlayerId) -> f32 {
        let hq_lvl = self.hq_level_of(player_id);
        match hq_lvl {
            0 | 1 => 620.0,
            2 => 1580.0,
            _ => 2600.0,
        }
    }
}
