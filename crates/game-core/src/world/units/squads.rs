use std::collections::HashMap;

use crate::types::PlayerId;

use super::super::World;

impl World {
    /// Dynamically splits lagging unit clusters (e.g., slow Titans) into separate squads and merges nearby ones.
    pub(in super::super) fn step_squad_clustering(&mut self) {
        let mut per_player: HashMap<PlayerId, HashMap<u8, Vec<usize>>> = HashMap::new();
        for (i, u) in self.units.iter().enumerate() {
            if u.owner_id != 0 && u.hp > 0 && u.squad > 0 {
                per_player
                    .entry(u.owner_id)
                    .or_default()
                    .entry(u.squad)
                    .or_default()
                    .push(i);
            }
        }

        let player_ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for pid in player_ids {
            let Some(mut squad_map) = per_player.remove(&pid) else {
                if let Some(pl) = self.players.get_mut(&pid) {
                    if let Some(rsid) = pl.reinforce_squad {
                        pl.squads.retain(|&sid, _| sid == rsid);
                    } else {
                        pl.squads.clear();
                    }
                }
                continue;
            };
            let Some(mut orders) = self.players.get(&pid).map(|p| p.squads.clone()) else {
                continue;
            };
            let mut next_id = self
                .players
                .get(&pid)
                .map(|p| p.next_squad_id)
                .unwrap_or(1);
            let mut reinforce_sid = self.players.get(&pid).and_then(|p| p.reinforce_squad);

            orders.retain(|sid, _| squad_map.contains_key(sid) || reinforce_sid == Some(*sid));
            if let Some(rsid) = reinforce_sid {
                if !orders.contains_key(&rsid) {
                    reinforce_sid = None;
                }
            }

            // 1. Split spatially disconnected clusters within each squad (>250 px gap)
            let sids: Vec<u8> = squad_map.keys().copied().collect();
            for sid in sids {
                if reinforce_sid == Some(sid) {
                    continue;
                }
                let Some(order) = orders.get(&sid).copied() else {
                    continue;
                };
                let indices = match squad_map.get(&sid) {
                    Some(v) if v.len() >= 2 => v.clone(),
                    _ => continue,
                };
                let n = indices.len();
                let mut visited = vec![false; n];
                let mut components: Vec<Vec<usize>> = Vec::new();
                for start in 0..n {
                    if visited[start] {
                        continue;
                    }
                    visited[start] = true;
                    let mut comp = vec![indices[start]];
                    let mut queue = vec![start];
                    let mut qi = 0;
                    while qi < queue.len() {
                        let cur = queue[qi];
                        qi += 1;
                        let ui = indices[cur];
                        let (ux, uy) = (self.units[ui].x, self.units[ui].y);
                        for nxt in 0..n {
                            if visited[nxt] {
                                continue;
                            }
                            let vi = indices[nxt];
                            let dx = self.units[vi].x - ux;
                            let dy = self.units[vi].y - uy;
                            if dx * dx + dy * dy <= 250.0 * 250.0 {
                                visited[nxt] = true;
                                queue.push(nxt);
                                comp.push(vi);
                            }
                        }
                    }
                    components.push(comp);
                }

                if components.len() <= 1 {
                    continue;
                }

                components.sort_by(|a, b| {
                    let da = a
                        .iter()
                        .map(|&idx| {
                            let dx = self.units[idx].x - order.target_x;
                            let dy = self.units[idx].y - order.target_y;
                            dx * dx + dy * dy
                        })
                        .fold(f32::MAX, f32::min);
                    let db = b
                        .iter()
                        .map(|&idx| {
                            let dx = self.units[idx].x - order.target_x;
                            let dy = self.units[idx].y - order.target_y;
                            dx * dx + dy * dy
                        })
                        .fold(f32::MAX, f32::min);
                    da.total_cmp(&db)
                });

                let mut comp_iter = components.into_iter();
                let mut kept_indices = comp_iter.next().unwrap_or_default();
                let mut split_full = false;
                for comp in comp_iter {
                    if split_full {
                        kept_indices.extend(comp);
                        continue;
                    }
                    let Some(cand) = (1..=5_u8).find(|id| !orders.contains_key(id)) else {
                        split_full = true;
                        kept_indices.extend(comp);
                        continue;
                    };
                    next_id = cand;
                    orders.insert(cand, order);
                    for &ui in &comp {
                        self.units[ui].squad = cand;
                    }
                    squad_map.entry(cand).or_default().extend(comp);
                }
                squad_map.insert(sid, kept_indices);
            }

            // 2. Auto-merge two active squads if they converge closely (<165 px) toward similar targets
            let mut active_sids: Vec<u8> = orders.keys().copied().collect();
            active_sids.sort_unstable();
            let mut centroids: HashMap<u8, (f32, f32, usize)> = HashMap::new();
            for &sid in &active_sids {
                if let Some(indices) = squad_map.get(&sid) {
                    if !indices.is_empty() {
                        let mut sx = 0.0_f32;
                        let mut sy = 0.0_f32;
                        for &ui in indices {
                            let u = &self.units[ui];
                            sx += u.x;
                            sy += u.y;
                        }
                        let cnt = indices.len();
                        centroids.insert(sid, (sx / cnt as f32, sy / cnt as f32, cnt));
                    }
                }
            }
            for i in 0..active_sids.len() {
                let a = active_sids[i];
                let Some(&(ax, ay, acnt)) = centroids.get(&a) else {
                    continue;
                };
                let Some(&oa) = orders.get(&a) else {
                    continue;
                };
                for j in (i + 1)..active_sids.len() {
                    let b = active_sids[j];
                    let Some(&(bx, by, bcnt)) = centroids.get(&b) else {
                        continue;
                    };
                    let Some(&ob) = orders.get(&b) else {
                        continue;
                    };
                    let cdx = ax - bx;
                    let cdy = ay - by;
                    let tdx = oa.target_x - ob.target_x;
                    let tdy = oa.target_y - ob.target_y;
                    if cdx * cdx + cdy * cdy <= 165.0 * 165.0
                        && tdx * tdx + tdy * tdy <= 260.0 * 260.0
                    {
                        let (keep, drop) = if acnt >= bcnt { (a, b) } else { (b, a) };
                        if let Some(dropped) = squad_map.remove(&drop) {
                            for &ui in &dropped {
                                self.units[ui].squad = keep;
                            }
                            squad_map.entry(keep).or_default().extend(dropped);
                        }
                        orders.remove(&drop);
                        centroids.remove(&drop);
                        if reinforce_sid == Some(drop) {
                            reinforce_sid = Some(keep);
                        }
                        break;
                    }
                }
            }

            // 3. Re-attach any combat squad whose order and all units are back inside base vision
            if let Some(z) = self.zones.get(&pid) {
                let zx = z.x;
                let zy = z.y;
                let z_rad = z.radius;
                let vision_r = z_rad + 360.0;
                let vision_r2 = vision_r * vision_r;
                let z_rad2 = z_rad * z_rad;
                let mut return_sids: Vec<u8> = Vec::new();
                for (&sid, &ord) in &orders {
                    let odx = ord.target_x - zx;
                    let ody = ord.target_y - zy;
                    if odx * odx + ody * ody <= vision_r2 {
                        let all_inside = squad_map.get(&sid).map_or(true, |indices| {
                            indices.iter().all(|&ui| {
                                let u = &self.units[ui];
                                let udx = u.x - zx;
                                let udy = u.y - zy;
                                udx * udx + udy * udy <= vision_r2
                            })
                        });
                        if all_inside {
                            return_sids.push(sid);
                        }
                    }
                }
                for sid in return_sids {
                    if let Some(ord) = orders.remove(&sid) {
                        if let Some(indices) = squad_map.remove(&sid) {
                            for ui in indices {
                                self.units[ui].squad = 0;
                            }
                        }
                        if let Some(pl) = self.players.get_mut(&pid) {
                            let odx = ord.target_x - zx;
                            let ody = ord.target_y - zy;
                            pl.base_rally = if odx * odx + ody * ody > z_rad2 {
                                Some(ord)
                            } else {
                                None
                            };
                        }
                    }
                }
            }

            if let Some(rsid) = reinforce_sid {
                if !orders.contains_key(&rsid) {
                    reinforce_sid = None;
                }
            }

            if let Some(pl) = self.players.get_mut(&pid) {
                pl.squads = orders;
                pl.next_squad_id = next_id;
                pl.reinforce_squad = reinforce_sid;
            }
        }
    }
}
