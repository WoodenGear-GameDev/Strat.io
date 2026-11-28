use std::collections::{HashMap, HashSet, VecDeque};

use crate::types::{Building, EntityId, PlayerId};

use super::hex::{
    building_triangles, collect_rhombuses, core_hex_radius, hex_to_rel, insert_hex_disk,
    is_large_building, rel_to_hex_round, tri_neighbors, TriCoord,
};
use super::{BaseGridCache, World};

impl World {

    pub(super) fn collect_player_open_large_hexes(&self, player_id: PlayerId) -> HashSet<(i32, i32)> {
        let mut open = HashSet::new();
        let hq_lvl = self.hq_level_of(player_id);
        insert_hex_disk(0, 0, core_hex_radius(hq_lvl), &mut open);

        let is_hive = self.players.get(&player_id).is_some_and(|p| p.is_hive);
        if hq_lvl >= 3 || is_hive {
            let node_r = self.node_hex_radius();
            for b in &self.buildings {
                if b.owner_id == player_id && b.kind == 5 && b.hp > 0 {
                    let (cq, cr) = rel_to_hex_round(b.rel_x, b.rel_y);
                    insert_hex_disk(cq, cr, node_r, &mut open);
                }
            }
        }
        open
    }

    pub(super) fn collect_player_occupied_tris(&self, player_id: PlayerId) -> HashSet<TriCoord> {
        let mut occupied = HashSet::new();
        for b in &self.buildings {
            if b.owner_id != player_id || b.hp == 0 {
                continue;
            }
            let (_, _, tris) = building_triangles(b.kind, b.rel_x, b.rel_y);
            for t in tris {
                occupied.insert(t);
            }
        }
        occupied
    }

    pub(super) fn ensure_base_grid(&mut self, player_id: PlayerId) -> &BaseGridCache {
        if !self.base_grid_cache.contains_key(&player_id) {
            let open_hexes = self.collect_player_open_large_hexes(player_id);
            let rhombuses = collect_rhombuses(&open_hexes);
            let occupied_tris = self.collect_player_occupied_tris(player_id);
            let free_hexes: Vec<(i32, i32)> = open_hexes
                .iter()
                .copied()
                .filter(|&(q, r)| {
                    !(0..6_u8).any(|s| occupied_tris.contains(&(q, r, s)))
                        && (0..6_u8).any(|s| {
                            tri_neighbors((q, r, s))
                                .iter()
                                .any(|n| occupied_tris.contains(n))
                        })
                })
                .collect();
            let free_rhombuses: Vec<([TriCoord; 2], (f32, f32))> = rhombuses
                .iter()
                .copied()
                .filter(|&(tris, _)| {
                    !occupied_tris.contains(&tris[0])
                        && !occupied_tris.contains(&tris[1])
                        && tris.iter().any(|&t| {
                            tri_neighbors(t)
                                .iter()
                                .any(|n| occupied_tris.contains(n))
                        })
                })
                .collect();
            self.base_grid_cache.insert(
                player_id,
                BaseGridCache {
                    occupied_tris,
                    free_hexes,
                    free_rhombuses,
                },
            );
        }
        self.base_grid_cache
            .get(&player_id)
            .expect("base_grid_cache just populated")
    }

    pub(super) fn find_valid_placement(
        &mut self,
        player_id: PlayerId,
        building_kind: u8,
        rel_x: f32,
        rel_y: f32,
        max_dist: f32,
    ) -> Option<(f32, f32)> {
        if building_kind == 0 {
            return None;
        }
        let cache = self.ensure_base_grid(player_id);

        let mut best: Option<(f32, f32)> = None;
        let mut best_d2 = max_dist * max_dist;

        if is_large_building(building_kind) {
            for &(q, r) in &cache.free_hexes {
                let (cx, cy) = hex_to_rel(q, r);
                let dx = cx - rel_x;
                let dy = cy - rel_y;
                let d2 = dx * dx + dy * dy;
                if d2 <= best_d2 {
                    best_d2 = d2;
                    best = Some((cx, cy));
                }
            }
        } else {
            for &(_, (mx, my)) in &cache.free_rhombuses {
                let dx = mx - rel_x;
                let dy = my - rel_y;
                let d2 = dx * dx + dy * dy;
                if d2 <= best_d2 {
                    best_d2 = d2;
                    best = Some((mx, my));
                }
            }
        }
        best
    }

    /// Iterative fixed-point BFS over built buildings via shared triangle edges to HQ `(0,0)`
    /// combined with support-field check (HQ disk or any living Node's hex disk).
    /// Returns all detached buildings that were removed.
    pub(super) fn prune_orphaned_buildings(&mut self, player_id: PlayerId) -> Vec<Building> {
        if !self.zones.contains_key(&player_id) {
            return Vec::new();
        }
        let hq_lvl = self.hq_level_of(player_id);
        let core_r = core_hex_radius(hq_lvl);
        let node_r = self.node_hex_radius();

        struct NodeInfo {
            id: EntityId,
            kind: u8,
            hex: (i32, i32),
            tris: Vec<TriCoord>,
        }

        let mut nodes: Vec<NodeInfo> = Vec::new();
        let mut hq_local_idx: Option<usize> = None;
        for b in &self.buildings {
            if b.owner_id != player_id || b.hp == 0 {
                continue;
            }
            let (q, r, tris) = building_triangles(b.kind, b.rel_x, b.rel_y);
            let local_idx = nodes.len();
            if b.kind == 0 {
                hq_local_idx = Some(local_idx);
            }
            nodes.push(NodeInfo {
                id: b.id,
                kind: b.kind,
                hex: (q, r),
                tris,
            });
        }

        let Some(hq_idx) = hq_local_idx else {
            return Vec::new();
        };

        let mut tri_to_node: HashMap<TriCoord, usize> = HashMap::with_capacity(nodes.len() * 4);
        for (i, n) in nodes.iter().enumerate() {
            for &t in &n.tris {
                tri_to_node.insert(t, i);
            }
        }

        let mut alive = vec![true; nodes.len()];
        let mut supported_hexes: HashSet<(i32, i32)> = HashSet::new();
        let mut queue: VecDeque<usize> = VecDeque::new();

        loop {
            supported_hexes.clear();
            insert_hex_disk(0, 0, core_r, &mut supported_hexes);
            let is_hive = self.players.get(&player_id).is_some_and(|p| p.is_hive);
            if hq_lvl >= 3 || is_hive {
                for (i, n) in nodes.iter().enumerate() {
                    if alive[i] && n.kind == 5 {
                        insert_hex_disk(n.hex.0, n.hex.1, node_r, &mut supported_hexes);
                    }
                }
            }

            let mut visited = vec![false; nodes.len()];
            queue.clear();
            visited[hq_idx] = true;
            queue.push_back(hq_idx);

            while let Some(u) = queue.pop_front() {
                for &t in &nodes[u].tris {
                    for nt in tri_neighbors(t) {
                        if let Some(&v) = tri_to_node.get(&nt) {
                            if alive[v] && !visited[v] && supported_hexes.contains(&nodes[v].hex) {
                                visited[v] = true;
                                queue.push_back(v);
                            }
                        }
                    }
                }
            }

            let mut changed = false;
            for i in 0..nodes.len() {
                if alive[i] && !visited[i] {
                    alive[i] = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let dead_ids: HashSet<EntityId> = nodes
            .iter()
            .enumerate()
            .filter(|&(i, n)| !alive[i] && n.kind != 0)
            .map(|(_, n)| n.id)
            .collect();

        let mut detached = Vec::new();
        if !dead_ids.is_empty() {
            let mut i = 0;
            while i < self.buildings.len() {
                if self.buildings[i].owner_id == player_id
                    && dead_ids.contains(&self.buildings[i].id)
                {
                    detached.push(self.buildings.swap_remove(i));
                } else {
                    i += 1;
                }
            }
        }

        self.recompute_unit_cap(player_id);
        self.invalidate_base_grid(player_id);
        detached
    }
}
