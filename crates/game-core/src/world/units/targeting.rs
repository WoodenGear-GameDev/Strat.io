use crate::types::{EntityId, SquadOrder, Unit};

use super::super::World;
use super::{UnitStepCtx, UnitTarget};

impl World {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn find_unit_target_cached(
        cur_tick: u32,
        i: usize,
        u: &mut Unit,
        ctx: &UnitStepCtx<'_>,
        raw_order: Option<SquadOrder>,
        squad_order: Option<SquadOrder>,
        strict_manual: bool,
        at_manual_target: bool,
        hold_course: bool,
        is_ranged: bool,
        attack_range: f32,
        my_r: f32,
        unit_hit_r: f32,
    ) -> UnitTarget {
        let should_rescan = ((cur_tick.wrapping_add(u.id)) & 3) == 0;

        // 1. O(1) validation of cached target (avoids 75%+ of spatial queries)
        if u.target_id != 0 {
            if !u.target_is_building && !at_manual_target {
                if let Some(&(other_id, other_owner, ox, oy, other_hp, _, _other_r)) =
                    ctx.unit_snapshot.get(u.target_idx)
                {
                    if other_id == u.target_id
                        && other_hp > 0
                        && other_owner != u.owner_id
                        && (other_owner == 0 || !ctx.is_protected(other_owner))
                    {
                        let search_r = if hold_course { unit_hit_r } else { ctx.aggro_r };
                        let dx = ox - u.x;
                        let dy = oy - u.y;
                        let in_range = dx * dx + dy * dy <= search_r * search_r;
                        let in_leash = hold_course
                            || squad_order.map_or(true, |o| {
                                let odx = ox - o.target_x;
                                let ody = oy - o.target_y;
                                odx * odx + ody * ody <= 210.0 * 210.0
                            });
                        if in_range && in_leash {
                            if !should_rescan {
                                return UnitTarget {
                                    unit: Some((other_id, u.target_idx, ox, oy)),
                                    building: None,
                                    hold_course,
                                };
                            }
                        } else {
                            u.target_id = 0;
                        }
                    } else {
                        u.target_id = 0;
                    }
                } else {
                    u.target_id = 0;
                }
            } else if u.target_is_building {
                if let Some(Some((bid, b_owner, bx, by, br, bhp, bkind))) =
                    ctx.building_snapshot.get(u.target_idx).copied()
                {
                    if bid == u.target_id
                        && bhp > 0
                        && b_owner != u.owner_id
                        && !ctx.is_protected(b_owner)
                    {
                        let valid = if at_manual_target {
                            raw_order.is_some_and(|ro| {
                                let dx = bx - ro.target_x;
                                let dy = by - ro.target_y;
                                let lim = 110.0 + br;
                                dx * dx + dy * dy <= lim * lim
                            })
                        } else if strict_manual {
                            let hit_d = if is_ranged {
                                attack_range + br
                            } else {
                                my_r + br + 6.0
                            };
                            let dx = bx - u.x;
                            let dy = by - u.y;
                            dx * dx + dy * dy <= hit_d * hit_d
                        } else if !hold_course {
                            let dx = bx - u.x;
                            let dy = by - u.y;
                            let lim = ctx.aggro_r + br;
                            let in_r = dx * dx + dy * dy <= lim * lim;
                            let in_leash = squad_order.map_or(true, |o| {
                                let odx = bx - o.target_x;
                                let ody = by - o.target_y;
                                let l2 = 210.0 + br;
                                odx * odx + ody * ody <= l2 * l2
                            });
                            in_r && in_leash
                        } else {
                            false
                        };
                        if valid {
                            if !should_rescan {
                                return UnitTarget {
                                    unit: None,
                                    building: Some((bid, u.target_idx, bx, by, br, bkind)),
                                    hold_course: if at_manual_target {
                                        false
                                    } else {
                                        hold_course
                                    },
                                };
                            }
                        } else {
                            u.target_id = 0;
                        }
                    } else {
                        u.target_id = 0;
                    }
                } else {
                    u.target_id = 0;
                }
            } else {
                u.target_id = 0;
            }
        } else if !should_rescan && !at_manual_target {
            return UnitTarget {
                unit: None,
                building: None,
                hold_course,
            };
        }

        let target = Self::find_unit_target(
            i,
            u,
            ctx,
            raw_order,
            squad_order,
            strict_manual,
            at_manual_target,
            hold_course,
            is_ranged,
            attack_range,
            my_r,
            unit_hit_r,
        );

        if let Some((uid, idx, _, _)) = target.unit {
            u.target_id = uid;
            u.target_idx = idx;
            u.target_is_building = false;
        } else if let Some((bid, idx, _, _, _, _)) = target.building {
            u.target_id = bid;
            u.target_idx = idx;
            u.target_is_building = true;
        } else {
            u.target_id = 0;
        }
        target
    }

    #[allow(clippy::too_many_arguments)]
    fn find_unit_target(
        i: usize,
        u: &Unit,
        ctx: &UnitStepCtx<'_>,
        raw_order: Option<SquadOrder>,
        squad_order: Option<SquadOrder>,
        strict_manual: bool,
        at_manual_target: bool,
        mut hold_course: bool,
        is_ranged: bool,
        attack_range: f32,
        my_r: f32,
        unit_hit_r: f32,
    ) -> UnitTarget {
        let mut best_edge_dist = ctx.aggro_r;
        let mut target_unit: Option<(EntityId, usize, f32, f32)> = None;
        let mut target_building: Option<(EntityId, usize, f32, f32, f32, u8)> = None;

        if at_manual_target {
            if let Some(ro) = raw_order {
                let mut best_b_d = f32::MAX;
                ctx.b_grid
                    .for_each_in_radius(ro.target_x, ro.target_y, 130.0, |bi, bx, by, d2| {
                        let Some((bid, b_owner, _, _, br, bhp, bkind)) = ctx.building_snapshot[bi]
                        else {
                            return;
                        };
                        if b_owner == u.owner_id || bhp == 0 || ctx.is_protected(b_owner) {
                            return;
                        }
                        let d_raw_edge = (d2.sqrt() - br).max(0.0);
                        if d_raw_edge <= 110.0 && d_raw_edge < best_b_d {
                            best_b_d = d_raw_edge;
                            target_building = Some((bid, bi, bx, by, br, bkind));
                        }
                    });
                if target_building.is_some() {
                    hold_course = false;
                }
            }
        } else {
            let titan_building_prio =
                u.kind == 2 && u.owner_id != 0 && !strict_manual && !hold_course;

            // 1. Titan siege role: prioritize enemy buildings (Turrets -> HQ -> others)
            if titan_building_prio {
                let mut best_prio: u8 = u8::MAX;
                let mut best_b_edge = ctx.aggro_r;
                ctx.b_grid
                    .for_each_in_radius(u.x, u.y, ctx.aggro_r, |bi, bx, by, d2| {
                        let Some((bid, b_owner, _, _, br, bhp, bkind)) = ctx.building_snapshot[bi]
                        else {
                            return;
                        };
                        if b_owner == u.owner_id || bhp == 0 {
                            return;
                        }
                        if let Some(o) = squad_order {
                            let odx = bx - o.target_x;
                            let ody = by - o.target_y;
                            let lim = 220.0 + br;
                            if odx * odx + ody * ody > lim * lim {
                                return;
                            }
                        }
                        if ctx.is_protected(b_owner) {
                            return;
                        }
                        let prio: u8 = match bkind {
                            2 => 0,
                            0 => 1,
                            _ => 2,
                        };
                        let edge_d = (d2.sqrt() - br).max(0.0);
                        if edge_d <= ctx.aggro_r
                            && (prio < best_prio || (prio == best_prio && edge_d < best_b_edge))
                        {
                            best_prio = prio;
                            best_b_edge = edge_d;
                            target_building = Some((bid, bi, bx, by, br, bkind));
                        }
                    });
            }

            // 2. Unit search by role
            let titan_in_b_hit = target_building.is_some_and(|(_, _, bx, by, br, _)| {
                let dx = bx - u.x;
                let dy = by - u.y;
                let hit_d = my_r + br + 8.0;
                dx * dx + dy * dy <= hit_d * hit_d
            });
            let unit_search_r = if hold_course {
                unit_hit_r
            } else if titan_building_prio && target_building.is_some() {
                if titan_in_b_hit {
                    0.0
                } else {
                    45.0
                }
            } else if u.kind == 0 {
                ctx.aggro_r.min(85.0)
            } else {
                ctx.aggro_r
            };

            if unit_search_r > 0.0 {
                let mut best_u_edge = unit_search_r;
                ctx.grid
                    .for_each_in_radius(u.x, u.y, unit_search_r, |other_i, ox, oy, d2| {
                        if other_i == i {
                            return;
                        }
                        let (other_id, other_owner, _, _, other_hp, _, other_r) =
                            ctx.unit_snapshot[other_i];
                        if other_owner == u.owner_id || other_hp == 0 {
                            return;
                        }
                        let max_d = best_u_edge + other_r + 0.01;
                        if d2 > max_d * max_d {
                            return;
                        }
                        if !hold_course {
                            if let Some(o) = squad_order {
                                let odx = ox - o.target_x;
                                let ody = oy - o.target_y;
                                if odx * odx + ody * ody > 210.0 * 210.0 {
                                    return;
                                }
                            }
                        }
                        if other_owner != 0 && ctx.is_protected(other_owner) {
                            return;
                        }
                        let edge_d = (d2.sqrt() - other_r).max(0.0);
                        if edge_d <= best_u_edge {
                            best_u_edge = edge_d;
                            target_unit = Some((other_id, other_i, ox, oy));
                        }
                    });
                if target_unit.is_some() {
                    best_edge_dist = best_u_edge;
                }
            }

            if !titan_building_prio
                && u.owner_id != 0
                && target_unit.is_none()
                && (strict_manual || !hold_course)
            {
                let b_search_r = if strict_manual {
                    (if is_ranged { attack_range } else { my_r + 6.0 }) + 32.0
                } else {
                    ctx.aggro_r
                };
                ctx.b_grid
                    .for_each_in_radius(u.x, u.y, b_search_r, |bi, bx, by, d2| {
                        let Some((bid, b_owner, _, _, br, bhp, bkind)) = ctx.building_snapshot[bi]
                        else {
                            return;
                        };
                        if b_owner == u.owner_id || bhp == 0 {
                            return;
                        }
                        let max_d = best_edge_dist + br + 0.01;
                        if d2 > max_d * max_d {
                            return;
                        }
                        if strict_manual {
                            let hit_d = if is_ranged {
                                attack_range + br
                            } else {
                                my_r + br + 6.0
                            };
                            if d2 > hit_d * hit_d {
                                return;
                            }
                        } else if let Some(o) = squad_order {
                            let odx = bx - o.target_x;
                            let ody = by - o.target_y;
                            let lim = 210.0 + br;
                            if odx * odx + ody * ody > lim * lim {
                                return;
                            }
                        }
                        if ctx.is_protected(b_owner) {
                            return;
                        }
                        let edge_d = (d2.sqrt() - br).max(0.0);
                        if edge_d <= best_edge_dist {
                            best_edge_dist = edge_d;
                            target_building = Some((bid, bi, bx, by, br, bkind));
                        }
                    });
            }
        }

        UnitTarget {
            unit: target_unit,
            building: target_building,
            hold_course,
        }
    }
}
