use std::collections::{HashMap, HashSet};

use crate::telemetry::DST_BUILDING_OFFSET;
use crate::types::PlayerId;

use super::super::World;

impl World {
    pub(in super::super) fn step_projectiles(&mut self, dt: f32) {
        if self.projectiles.is_empty() {
            return;
        }

        let ring3_r = self.config.world.ring3_radius;
        let ring2_r = self.config.world.ring2_radius;
        let mut tlog = self.telemetry.take();
        let owner_rings: HashMap<PlayerId, u8> = if tlog.is_some() {
            self.zones
                .iter()
                .map(|(&pid, z)| {
                    (
                        pid,
                        Self::ring_for_radius_with(
                            (z.x * z.x + z.y * z.y).sqrt(),
                            ring3_r,
                            ring2_r,
                        ),
                    )
                })
                .collect()
        } else {
            HashMap::new()
        };

        let mut damaged_owners: HashSet<PlayerId> = HashSet::new();

        {
            let unit_grid = &self.unit_grid;
            let unit_snap = &mut self.unit_world_snap;
            let building_snap = &mut self.building_world_snap;
            let units = &mut self.units;
            let buildings = &mut self.buildings;
            let projectiles = &mut self.projectiles;
            let players = &self.players;

            projectiles.retain_mut(|p| {
                p.ttl -= dt;
                if p.ttl > 0.0 {
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                    return true;
                }

                let mut impact_x = p.target_x;
                let mut impact_y = p.target_y;
                let mut direct_hit_unit_idx: Option<usize> = None;

                if p.target_id != 0 {
                    if !p.target_is_building {
                        let resolved = if units
                            .get(p.target_idx)
                            .is_some_and(|u| u.id == p.target_id)
                        {
                            Some(p.target_idx)
                        } else {
                            units.iter().position(|u| u.id == p.target_id)
                        };
                        if let Some(ui) = resolved {
                            let u = &mut units[ui];
                            impact_x = u.x;
                            impact_y = u.y;
                            if u.owner_id != p.owner_id && u.hp > 0 {
                                let protected = u.owner_id != 0
                                    && players
                                        .get(&u.owner_id)
                                        .is_some_and(|pl| pl.spawn_protection_left > 0.0);
                                if !protected {
                                    let dmg_amt = p.damage;
                                    let dealt = u.hp.min(dmg_amt);
                                    u.hp = u.hp.saturating_sub(dmg_amt);
                                    unit_snap[ui].4 = u.hp;
                                    u.last_hit_by = Some(p.owner_id);
                                    u.last_hit_src = p.src_tag;
                                    if u.owner_id != 0 {
                                        damaged_owners.insert(u.owner_id);
                                    }
                                    if let Some(tl) = tlog.as_mut() {
                                        let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                                        let dr = Self::ring_for_radius_with(
                                            (impact_x * impact_x + impact_y * impact_y).sqrt(),
                                            ring3_r,
                                            ring2_r,
                                        );
                                        tl.log_damage(p.src_tag, u.kind, sr, dr, dealt);
                                    }
                                    direct_hit_unit_idx = Some(ui);
                                }
                            }
                        }
                    } else {
                        let resolved = if buildings
                            .get(p.target_idx)
                            .is_some_and(|b| b.id == p.target_id)
                        {
                            Some(p.target_idx)
                        } else {
                            buildings.iter().position(|b| b.id == p.target_id)
                        };
                        if let Some(bi) = resolved {
                            if let Some((_, _, bx, by, _, _, _)) = building_snap[bi] {
                                impact_x = bx;
                                impact_y = by;
                            }
                            let b = &mut buildings[bi];
                            if b.owner_id != p.owner_id && b.hp > 0 {
                                let protected = players
                                    .get(&b.owner_id)
                                    .is_some_and(|pl| pl.spawn_protection_left > 0.0);
                                if !protected {
                                    let dmg_amt = p.damage;
                                    let dealt = b.hp.min(dmg_amt);
                                    b.hp = b.hp.saturating_sub(dmg_amt);
                                    if let Some(ref mut bs) = building_snap[bi] {
                                        bs.5 = b.hp;
                                    }
                                    b.last_hit_by = Some(p.owner_id);
                                    b.last_hit_src = p.src_tag;
                                    damaged_owners.insert(b.owner_id);
                                    if let Some(tl) = tlog.as_mut() {
                                        let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                                        let dr = Self::ring_for_radius_with(
                                            (impact_x * impact_x + impact_y * impact_y).sqrt(),
                                            ring3_r,
                                            ring2_r,
                                        );
                                        tl.log_damage(
                                            p.src_tag,
                                            DST_BUILDING_OFFSET + b.kind,
                                            sr,
                                            dr,
                                            dealt,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                if p.splash_radius > 0.0 {
                    let splash_dmg = (p.damage / 2).max(1);
                    unit_grid.for_each_in_radius(
                        impact_x,
                        impact_y,
                        p.splash_radius,
                        |ui, ux, uy, _d2| {
                            if Some(ui) == direct_hit_unit_idx {
                                return;
                            }
                            let u = &mut units[ui];
                            if u.owner_id == p.owner_id || u.hp == 0 {
                                return;
                            }
                            if u.owner_id != 0
                                && players
                                    .get(&u.owner_id)
                                    .is_some_and(|pl| pl.spawn_protection_left > 0.0)
                            {
                                return;
                            }
                            let dealt = u.hp.min(splash_dmg);
                            u.hp = u.hp.saturating_sub(splash_dmg);
                            unit_snap[ui].4 = u.hp;
                            u.last_hit_by = Some(p.owner_id);
                            u.last_hit_src = p.src_tag;
                            if u.owner_id != 0 {
                                damaged_owners.insert(u.owner_id);
                            }
                            if let Some(tl) = tlog.as_mut() {
                                let sr = owner_rings.get(&p.owner_id).copied().unwrap_or(0);
                                let dr = Self::ring_for_radius_with(
                                    (ux * ux + uy * uy).sqrt(),
                                    ring3_r,
                                    ring2_r,
                                );
                                tl.log_damage(p.src_tag, u.kind, sr, dr, dealt);
                            }
                        },
                    );
                }

                false
            });
        }

        for pid in damaged_owners {
            if let Some(pl) = self.players.get_mut(&pid) {
                pl.last_damaged_sec = pl.survival_sec;
            }
        }
        self.telemetry = tlog;
    }
}
