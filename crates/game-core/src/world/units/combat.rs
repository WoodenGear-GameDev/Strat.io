use std::collections::{HashMap, HashSet};

use crate::telemetry::DST_BUILDING_OFFSET;
use crate::types::{PlayerId, Projectile, Unit};

use super::super::World;
use super::{UnitStepCtx, UnitTarget};

impl World {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_unit_combat(
        u: &mut Unit,
        target: &UnitTarget,
        speed: f32,
        damage: u16,
        unit_cd: f32,
        attack_range: f32,
        is_ranged: bool,
        my_r: f32,
        unit_hit_r: f32,
        new_projectiles: &mut Vec<Projectile>,
        unit_damage_events: &mut Vec<(usize, u16, PlayerId, u8)>,
        building_damage_events: &mut Vec<(usize, u16, PlayerId, u8)>,
    ) -> (f32, f32) {
        let mut vx = 0.0_f32;
        let mut vy = 0.0_f32;

        if let Some((tid, ti, tx, ty)) = target.unit {
            let dx = tx - u.x;
            let dy = ty - u.y;
            let d2 = dx * dx + dy * dy;
            let hit_dist = unit_hit_r;
            if d2 <= hit_dist * hit_dist {
                if u.attack_cooldown_left <= 0.0 {
                    if is_ranged && d2 > 0.000001 {
                        let dist = d2.sqrt();
                        let p_speed = 540.0;
                        let flight_sec = (dist / p_speed).clamp(0.02, 1.0);
                        new_projectiles.push(Projectile {
                            id: 0,
                            owner_id: u.owner_id,
                            kind: 2,
                            src_tag: u.kind,
                            x: u.x,
                            y: u.y,
                            vx: dx / flight_sec,
                            vy: dy / flight_sec,
                            target_x: tx,
                            target_y: ty,
                            target_id: tid,
                            target_idx: ti,
                            target_is_building: false,
                            damage,
                            ttl: flight_sec,
                            splash_radius: 0.0,
                        });
                    } else {
                        unit_damage_events.push((ti, damage, u.owner_id, u.kind));
                        if !target.hold_course && d2 > 0.000001 {
                            let dist = d2.sqrt();
                            let recoil = if u.kind == 2 { 110.0 } else { 195.0 };
                            u.knockback_vx = -(dx / dist) * recoil;
                            u.knockback_vy = -(dy / dist) * recoil;
                        }
                    }
                    u.attack_cooldown_left = unit_cd;
                }
            } else if !target.hold_course && d2 > 0.000001 {
                let dist = d2.sqrt();
                vx = (dx / dist) * speed;
                vy = (dy / dist) * speed;
            }
        } else if let Some((bid, bi, bx, by, br, bkind)) = target.building {
            let dx = bx - u.x;
            let dy = by - u.y;
            let d2 = dx * dx + dy * dy;
            let hit_dist = if is_ranged {
                attack_range + br
            } else {
                my_r + br + 6.0
            };
            if d2 <= hit_dist * hit_dist {
                if u.attack_cooldown_left <= 0.0 {
                    let b_dmg = if u.kind == 0 && (bkind == 1 || bkind == 5) {
                        (f32::from(damage) * 1.3).round() as u16
                    } else if u.kind == 0 && bkind == 0 {
                        (f32::from(damage) * 0.85).round().max(1.0) as u16
                    } else {
                        damage
                    };
                    if is_ranged && d2 > 0.000001 {
                        let dist = d2.sqrt();
                        let p_speed = 540.0;
                        let flight_sec = (dist / p_speed).clamp(0.02, 1.0);
                        new_projectiles.push(Projectile {
                            id: 0,
                            owner_id: u.owner_id,
                            kind: 2,
                            src_tag: u.kind,
                            x: u.x,
                            y: u.y,
                            vx: dx / flight_sec,
                            vy: dy / flight_sec,
                            target_x: bx,
                            target_y: by,
                            target_id: bid,
                            target_idx: bi,
                            target_is_building: true,
                            damage: b_dmg,
                            ttl: flight_sec,
                            splash_radius: 0.0,
                        });
                    } else {
                        building_damage_events.push((bi, b_dmg, u.owner_id, u.kind));
                        if !target.hold_course && d2 > 0.000001 {
                            let dist = d2.sqrt();
                            let bounce = if u.kind == 2 { 175.0 } else { 290.0 };
                            u.knockback_vx = -(dx / dist) * bounce;
                            u.knockback_vy = -(dy / dist) * bounce;
                        }
                    }
                    u.attack_cooldown_left = unit_cd;
                }
            } else if !target.hold_course && d2 > 0.000001 {
                let dist = d2.sqrt();
                vx = (dx / dist) * speed;
                vy = (dy / dist) * speed;
            }
        }

        (vx, vy)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_unit_collisions(
        i: usize,
        u: &mut Unit,
        ctx: &UnitStepCtx<'_>,
        my_r: f32,
        max_sep_query: f32,
        max_b_coll_r: f32,
        strict_manual: bool,
    ) {
        let ux0 = u.x;
        let uy0 = u.y;
        let mut sep_hits = 0_u8;
        ctx.grid
            .for_each_in_radius_until(ux0, uy0, max_sep_query, |other_i, ox, oy, _d2| {
                if other_i == i {
                    return true;
                }
                let (_, other_owner, _, _, other_hp, other_kind, other_r) =
                    ctx.unit_snapshot[other_i];
                if other_hp == 0 {
                    return true;
                }
                let min_pair = my_r + other_r + 2.5;
                let dx = u.x - ox;
                let dy = u.y - oy;
                let d2 = dx * dx + dy * dy;
                if d2 < min_pair * min_pair {
                    let dist = d2.sqrt();
                    let (nx, ny) = if dist > 0.001 {
                        (dx / dist, dy / dist)
                    } else {
                        (1.0, 0.0)
                    };
                    let overlap = min_pair - dist;
                    u.x += nx * overlap * 0.52;
                    u.y += ny * overlap * 0.52;

                    if other_owner != u.owner_id {
                        let mass_factor = if other_kind == 2 && u.kind != 2 {
                            1.45
                        } else if u.kind == 2 && other_kind != 2 {
                            0.45
                        } else {
                            1.0
                        };
                        let imp = 190.0 * mass_factor;
                        u.knockback_vx += nx * imp * 0.45;
                        u.knockback_vy += ny * imp * 0.45;
                    }
                    sep_hits += 1;
                    if sep_hits >= 6 {
                        return false;
                    }
                }
                true
            });

        let ux1 = u.x;
        let uy1 = u.y;
        ctx.b_grid
            .for_each_in_radius(ux1, uy1, max_b_coll_r, |bi, bx, by, _d2| {
                let Some((_, b_owner, _, _, br, bhp, _)) = ctx.building_snapshot[bi] else {
                    return;
                };
                if bhp == 0 || b_owner == u.owner_id {
                    return;
                }
                let min_b_dist = br + my_r + 2.0;
                let dx = u.x - bx;
                let dy = u.y - by;
                let d2 = dx * dx + dy * dy;
                if d2 < min_b_dist * min_b_dist {
                    let dist = d2.sqrt();
                    let (nx, ny) = if dist > 0.001 {
                        (dx / dist, dy / dist)
                    } else {
                        (1.0, 0.0)
                    };
                    u.x = bx + nx * min_b_dist;
                    u.y = by + ny * min_b_dist;

                    if !strict_manual {
                        let bounce = if u.kind == 2 { 155.0 } else { 240.0 };
                        let radial_kb = u.knockback_vx * nx + u.knockback_vy * ny;
                        if radial_kb < bounce {
                            let add = bounce - radial_kb;
                            u.knockback_vx += nx * add;
                            u.knockback_vy += ny * add;
                        }
                    }
                }
            });
    }

    pub(super) fn apply_unit_damage_events(
        &mut self,
        unit_damage_events: Vec<(usize, u16, PlayerId, u8)>,
        building_damage_events: Vec<(usize, u16, PlayerId, u8)>,
    ) {
        if unit_damage_events.is_empty() && building_damage_events.is_empty() {
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
            let units = &mut self.units;
            let buildings = &mut self.buildings;

            for (ti, dmg, attacker, src_kind) in unit_damage_events {
                if let Some(target) = units.get_mut(ti) {
                    let dealt = target.hp.min(dmg);
                    target.hp = target.hp.saturating_sub(dmg);
                    target.last_hit_by = Some(attacker);
                    target.last_hit_src = src_kind;
                    if target.owner_id != 0 {
                        damaged_owners.insert(target.owner_id);
                    }
                    if let Some(tl) = tlog.as_mut() {
                        let sr = owner_rings.get(&attacker).copied().unwrap_or(0);
                        let dr = Self::ring_for_radius_with(
                            (target.x * target.x + target.y * target.y).sqrt(),
                            ring3_r,
                            ring2_r,
                        );
                        tl.log_damage(src_kind, target.kind, sr, dr, dealt);
                    }
                }
            }
            for (bi, dmg, attacker, src_kind) in building_damage_events {
                if let Some(target) = buildings.get_mut(bi) {
                    let dealt = target.hp.min(dmg);
                    target.hp = target.hp.saturating_sub(dmg);
                    target.last_hit_by = Some(attacker);
                    target.last_hit_src = src_kind;
                    damaged_owners.insert(target.owner_id);
                    if let Some(tl) = tlog.as_mut() {
                        let sr = owner_rings.get(&attacker).copied().unwrap_or(0);
                        let dr = owner_rings.get(&target.owner_id).copied().unwrap_or(1);
                        tl.log_damage(src_kind, DST_BUILDING_OFFSET + target.kind, sr, dr, dealt);
                    }
                }
            }
        }
        for pid in damaged_owners {
            if let Some(pl) = self.players.get_mut(&pid) {
                pl.last_damaged_sec = pl.survival_sec;
            }
        }
        self.telemetry = tlog;
    }
}
