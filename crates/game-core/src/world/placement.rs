use std::collections::HashSet;
use std::f32::consts::TAU;

use crate::types::{EntityId, PlayerId};

use super::hex::{
    beacon_ext_radius, collect_rhombuses, core_hex_radius, hex_to_rel, insert_hex_disk,
    is_large_building, rel_to_hex_round, TriCoord, HEX_EDGE_DIRS, HEX_SIZE,
};
use super::World;

impl World {
    pub(super) fn find_safe_orbit_spot(
        &self,
        player_id: PlayerId,
        target_r: f32,
        zone_r: f32,
        cur_angle: f32,
    ) -> (f32, f32) {
        let mut best_x = cur_angle.cos() * target_r;
        let mut best_y = cur_angle.sin() * target_r;
        let mut best_clear = f32::NEG_INFINITY;
        let steps = 24_i32;
        for step in 0..steps {
            let signed = if step == 0 {
                0
            } else if step % 2 == 1 {
                (step + 1) / 2
            } else {
                -(step / 2)
            };
            let ang = cur_angle + (signed as f32) * (TAU / steps as f32);
            let cx = ang.cos() * target_r;
            let cy = ang.sin() * target_r;
            let mut min_clear = f32::MAX;
            for other in self.zones.values() {
                if other.player_id == player_id {
                    continue;
                }
                let clear = (cx - other.x).hypot(cy - other.y) - (zone_r + other.radius);
                if clear < min_clear {
                    min_clear = clear;
                }
            }
            for b in &self.buildings {
                if b.owner_id != player_id && b.kind == 5 {
                    if let Some(other) = self.zones.get(&b.owner_id) {
                        let ext_r = beacon_ext_radius(b.level);
                        let clear =
                            (cx - (other.x + b.rel_x)).hypot(cy - (other.y + b.rel_y)) - (zone_r + ext_r);
                        if clear < min_clear {
                            min_clear = clear;
                        }
                    }
                }
            }
            if min_clear > best_clear {
                best_clear = min_clear;
                best_x = cx;
                best_y = cy;
                if best_clear >= 220.0 {
                    break;
                }
            }
        }
        (best_x, best_y)
    }

    pub(super) fn collect_player_open_large_hexes(&self, player_id: PlayerId) -> HashSet<(i32, i32)> {
        let mut open = HashSet::new();
        let hq_lvl = self.hq_level_of(player_id);
        insert_hex_disk(0, 0, core_hex_radius(hq_lvl), &mut open);

        if hq_lvl >= 3 {
            for b in &self.buildings {
                if b.owner_id == player_id && b.kind == 5 {
                    let rad = beacon_ext_radius(b.level);
                    let hex_span = ((rad / (HEX_SIZE * 1.5)).ceil() as i32) + 1;
                    let (cq, cr) = rel_to_hex_round(b.rel_x, b.rel_y);
                    for dq in -hex_span..=hex_span {
                        for dr in -hex_span..=hex_span {
                            let lq = cq + dq;
                            let lr = cr + dr;
                            let (lx, ly) = hex_to_rel(lq, lr);
                            if (lx - b.rel_x).hypot(ly - b.rel_y) <= rad {
                                open.insert((lq, lr));
                            }
                        }
                    }
                }
            }
        }
        open
    }

    pub(super) fn collect_player_occupied_tris(
        &self,
        player_id: PlayerId,
        rhombuses: &[([TriCoord; 2], (f32, f32))],
    ) -> HashSet<TriCoord> {
        let mut occupied = HashSet::new();
        for b in &self.buildings {
            if b.owner_id != player_id {
                continue;
            }
            if is_large_building(b.kind) {
                let (aq, ar) = rel_to_hex_round(b.rel_x, b.rel_y);
                for s in 0..6_u8 {
                    occupied.insert((aq, ar, s));
                }
            } else if let Some((tris, _)) = rhombuses.iter().min_by(|(_, (ax, ay)), (_, (bx, by))| {
                let da = (ax - b.rel_x).hypot(ay - b.rel_y);
                let db = (bx - b.rel_x).hypot(by - b.rel_y);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            }) {
                occupied.insert(tris[0]);
                occupied.insert(tris[1]);
            }
        }
        occupied
    }

    pub(super) fn find_valid_placement(
        &self,
        player_id: PlayerId,
        building_kind: u8,
        rel_x: f32,
        rel_y: f32,
        max_dist: f32,
    ) -> Option<(f32, f32)> {
        if building_kind == 0 {
            return None;
        }
        let open_hexes = self.collect_player_open_large_hexes(player_id);
        let rhombuses = collect_rhombuses(&open_hexes);
        let occupied = self.collect_player_occupied_tris(player_id, &rhombuses);

        let mut best: Option<(f32, f32)> = None;
        let mut best_d = max_dist;

        if is_large_building(building_kind) {
            for &(q, r) in &open_hexes {
                if (0..6_u8).any(|s| occupied.contains(&(q, r, s))) {
                    continue;
                }
                let (cx, cy) = hex_to_rel(q, r);
                let d = (cx - rel_x).hypot(cy - rel_y);
                if d <= best_d {
                    best_d = d;
                    best = Some((cx, cy));
                }
            }
        } else {
            for &(tris, (mx, my)) in &rhombuses {
                if occupied.contains(&tris[0]) || occupied.contains(&tris[1]) {
                    continue;
                }
                let d = (mx - rel_x).hypot(my - rel_y);
                if d <= best_d {
                    best_d = d;
                    best = Some((mx, my));
                }
            }
        }
        best
    }

    pub(super) fn prune_orphaned_buildings(&mut self, player_id: PlayerId) {
        if !self.zones.contains_key(&player_id) {
            return;
        }
        let open_hexes = self.collect_player_open_large_hexes(player_id);
        let mut reachable: HashSet<(i32, i32)> = HashSet::new();
        let mut queue: Vec<(i32, i32)> = Vec::new();
        if open_hexes.contains(&(0, 0)) {
            reachable.insert((0, 0));
            queue.push((0, 0));
        }

        let mut head = 0;
        while head < queue.len() {
            let (cq, cr) = queue[head];
            head += 1;
            for (dq, dr) in HEX_EDGE_DIRS {
                let next = (cq + dq, cr + dr);
                if open_hexes.contains(&next) && reachable.insert(next) {
                    queue.push(next);
                }
            }
        }

        let valid_rhombuses = collect_rhombuses(&reachable);
        let mut removed_station_ids: HashSet<EntityId> = HashSet::new();
        self.buildings.retain(|b| {
            if b.owner_id != player_id || b.kind == 0 {
                return true;
            }
            let keep = if is_large_building(b.kind) {
                let (aq, ar) = rel_to_hex_round(b.rel_x, b.rel_y);
                reachable.contains(&(aq, ar))
            } else {
                valid_rhombuses
                    .iter()
                    .any(|&(_, (mx, my))| (mx - b.rel_x).hypot(my - b.rel_y) <= 6.0)
            };
            if !keep && b.kind == 1 {
                removed_station_ids.insert(b.id);
            }
            keep
        });

        if !removed_station_ids.is_empty() {
            self.units
                .retain(|u| !(u.kind == 4 && removed_station_ids.contains(&u.home_id)));
        }

        self.recompute_unit_cap(player_id);
    }
}
