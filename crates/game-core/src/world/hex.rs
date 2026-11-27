use std::collections::HashSet;
use std::f32::consts::TAU;

use crate::types::PlayerId;

use super::World;

pub const RING1_RADIUS: f32 = 1100.0;
pub const RING2_RADIUS: f32 = 2150.0;
pub const HEX_SIZE: f32 = 34.0;
pub type TriCoord = (i32, i32, u8);

pub const HEX_EDGE_DIRS: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

const SQRT3: f32 = 1.7320508;
const HEX_HALF_W: f32 = HEX_SIZE * (SQRT3 * 0.5);
const HEX_HALF_H: f32 = HEX_SIZE * 0.5;
const HEX_CORNERS: [(f32, f32); 6] = [
    (HEX_HALF_W, -HEX_HALF_H),
    (HEX_HALF_W, HEX_HALF_H),
    (0.0, HEX_SIZE),
    (-HEX_HALF_W, HEX_HALF_H),
    (-HEX_HALF_W, -HEX_HALF_H),
    (0.0, -HEX_SIZE),
];

pub fn is_large_building(kind: u8) -> bool {
    kind == 0 || kind == 2 || kind == 3
}

#[inline]
pub fn hex_to_rel(q: i32, r: i32) -> (f32, f32) {
    let qf = q as f32;
    let rf = r as f32;
    let x = HEX_SIZE * (SQRT3 * qf + (SQRT3 * 0.5) * rf);
    let y = HEX_SIZE * 1.5 * rf;
    (x, y)
}

#[inline]
pub fn rel_to_hex_round(x: f32, y: f32) -> (i32, i32) {
    let qf = ((SQRT3 / 3.0) * x - (1.0 / 3.0) * y) / HEX_SIZE;
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

#[inline]
pub fn hex_vertex(q: i32, r: i32, i: u8) -> (f32, f32) {
    let (cx, cy) = hex_to_rel(q, r);
    let (ox, oy) = HEX_CORNERS[(i % 6) as usize];
    (cx + ox, cy + oy)
}

pub fn collect_rhombuses(open_hexes: &HashSet<(i32, i32)>) -> Vec<([TriCoord; 2], (f32, f32))> {
    let mut out = Vec::with_capacity(open_hexes.len() * 8);
    for &(q, r) in open_hexes {
        let (cx, cy) = hex_to_rel(q, r);
        for s in 0..6_u8 {
            let s_next = (s + 1) % 6;
            let (ox1, oy1) = HEX_CORNERS[s_next as usize];
            let vx1 = cx + ox1;
            let vy1 = cy + oy1;
            out.push((
                [(q, r, s), (q, r, s_next)],
                (cx + ox1 * 0.5, cy + oy1 * 0.5),
            ));

            let (dq, dr) = HEX_EDGE_DIRS[s as usize];
            let nq = q + dq;
            let nr = r + dr;
            if open_hexes.contains(&(nq, nr)) && (q, r) < (nq, nr) {
                let (ox0, oy0) = HEX_CORNERS[s as usize];
                let vx0 = cx + ox0;
                let vy0 = cy + oy0;
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
        let dist2 = dx * dx + dy * dy;
        if dist2 <= 1e-6 {
            return (0.0, 0.0);
        }
        let dist = dist2.sqrt();
        let ur2 = ux * ux + uy * uy;
        let tr2 = tx * tx + ty * ty;
        if dist2 < 180.0 * 180.0 || ur2 < 90.0 * 90.0 || tr2 < 90.0 * 90.0 {
            return (dx / dist, dy / dist);
        }
        let ur = ur2.sqrt();
        let tr = tr2.sqrt();
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
        let len2 = vx * vx + vy * vy;
        if len2 > 1e-6 {
            let len = len2.sqrt();
            (vx / len, vy / len)
        } else {
            (dx / dist, dy / dist)
        }
    }

    pub(super) fn has_vision_at(&self, player_id: PlayerId, tx: f32, ty: f32) -> bool {
        if let Some(z) = self.zones.get(&player_id) {
            let dx = tx - z.x;
            let dy = ty - z.y;
            let r = z.radius + 360.0;
            if dx * dx + dy * dy <= r * r {
                return true;
            }
            for b in &self.buildings {
                if b.owner_id == player_id && b.kind == 5 {
                    let vr = beacon_ext_radius(b.level) + 220.0;
                    let bdx = tx - (z.x + b.rel_x);
                    let bdy = ty - (z.y + b.rel_y);
                    if bdx * bdx + bdy * bdy <= vr * vr {
                        return true;
                    }
                }
            }
        }
        const UNIT_VR2: f32 = 290.0 * 290.0;
        for u in &self.units {
            if u.owner_id == player_id && u.hp > 0 && u.kind != 4 {
                let dx = tx - u.x;
                let dy = ty - u.y;
                if dx * dx + dy * dy <= UNIT_VR2 {
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
