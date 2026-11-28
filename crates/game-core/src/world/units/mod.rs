mod combat;
mod movement;
mod squads;
mod targeting;

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::spatial::SpatialGrid;
use crate::types::{EntityId, PlayerId, PlayerState};

use super::{BuildingSnap, UnitSnap};

#[inline]
pub(super) fn formation_offset(slot: u16) -> (f32, f32) {
    static TABLE: OnceLock<[(f32, f32); 64]> = OnceLock::new();
    let tbl = TABLE.get_or_init(|| {
        std::array::from_fn(|s| {
            if s == 0 {
                (0.0, 0.0)
            } else {
                let angle = (s as f32) * 2.399963_f32;
                let r = 28.0 * (s as f32).sqrt();
                (angle.cos() * r, angle.sin() * r)
            }
        })
    });
    let idx = usize::from(slot);
    if idx < tbl.len() {
        tbl[idx]
    } else {
        let angle = (slot as f32) * 2.399963_f32;
        let r = 28.0 * (slot as f32).sqrt();
        (angle.cos() * r, angle.sin() * r)
    }
}

pub(super) struct UnitStepCtx<'a> {
    pub aggro_r: f32,
    pub players: &'a HashMap<PlayerId, PlayerState>,
    pub grid: &'a SpatialGrid,
    pub b_grid: &'a SpatialGrid,
    pub unit_snapshot: &'a [UnitSnap],
    pub building_snapshot: &'a [BuildingSnap],
}

impl UnitStepCtx<'_> {
    #[inline]
    pub fn is_protected(&self, pid: PlayerId) -> bool {
        self.players
            .get(&pid)
            .is_some_and(|p| p.spawn_protection_left > 0.0)
    }
}

pub(super) struct UnitTarget {
    pub unit: Option<(EntityId, usize, f32, f32)>,
    pub building: Option<(EntityId, usize, f32, f32, f32, u8)>,
    pub hold_course: bool,
}
