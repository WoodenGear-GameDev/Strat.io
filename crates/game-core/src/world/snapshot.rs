use std::collections::{HashMap, HashSet};

use crate::protocol::{
    write_snapshot_header, LeaderboardEntry, PlayerMetaEntry, ServerMessage, SnapshotAttractor,
    SnapshotBuilding, SnapshotHeat, SnapshotProjectile, SnapshotTeleport, SnapshotUnit,
    SnapshotZone, SNAPSHOT_ATTRACTOR_BYTES, SNAPSHOT_BUILDING_BYTES, SNAPSHOT_HEADER_BYTES,
    SNAPSHOT_HEAT_BYTES, SNAPSHOT_PROJECTILE_BYTES, SNAPSHOT_TELEPORT_BYTES, SNAPSHOT_UNIT_BYTES,
    SNAPSHOT_ZONE_BYTES,
};
use crate::types::PlayerId;

use super::hex::beacon_ext_radius;
use super::World;

const SERVER_FOG_PAD: f32 = 36.0;

pub struct SnapshotSharedMeta {
    pub unit_counts: HashMap<PlayerId, u16>,
    pub top3_leaders: HashSet<PlayerId>,
    pub leader_id: Option<PlayerId>,
}

#[inline]
fn point_in_vision(x: f32, y: f32, pad: f32, circles: &[(f32, f32, f32)]) -> bool {
    for &(cx, cy, cr) in circles {
        let r = cr + pad;
        let dx = x - cx;
        let dy = y - cy;
        if dx * dx + dy * dy <= r * r {
            return true;
        }
    }
    false
}

impl World {
    #[inline]
    fn supply_cost_table(&self) -> [u16; 4] {
        [
            self.unit_supply_cost(0),
            self.unit_supply_cost(1),
            self.unit_supply_cost(2),
            self.unit_supply_cost(3),
        ]
    }

    #[inline]
    fn fast_supply_cost(&self, table: &[u16; 4], kind: u8) -> u16 {
        table
            .get(kind as usize)
            .copied()
            .unwrap_or_else(|| self.unit_supply_cost(kind))
    }

    /// Computes once-per-tick shared metadata (per-player supply counts + Top-3 leader IDs)
    /// used when encoding personalized server-side Fog-of-War snapshots.
    pub fn compute_snapshot_shared_meta(&self) -> SnapshotSharedMeta {
        let supply_costs = self.supply_cost_table();
        let mut unit_counts: HashMap<PlayerId, u16> = HashMap::with_capacity(self.players.len());
        for u in &self.units {
            if u.hp > 0 && u.owner_id != 0 {
                let cost = self.fast_supply_cost(&supply_costs, u.kind);
                let entry = unit_counts.entry(u.owner_id).or_insert(0);
                *entry = entry.saturating_add(cost);
            }
        }
        let mut ranked: Vec<(PlayerId, u32)> = self
            .players
            .values()
            .filter(|p| !p.is_hive && p.score >= 400)
            .map(|p| (p.id, p.score))
            .collect();
        ranked.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        let leader_id = ranked.first().map(|&(id, _)| id);
        let top3_leaders = ranked.into_iter().take(3).map(|(id, _)| id).collect();
        SnapshotSharedMeta {
            unit_counts,
            top3_leaders,
            leader_id,
        }
    }

    fn collect_vision_circles(&self, for_player: PlayerId) -> Vec<(f32, f32, f32)> {
        let mut circles = Vec::with_capacity(24);
        if let Some(z) = self.zones.get(&for_player) {
            circles.push((z.x, z.y, z.radius + 360.0 + SERVER_FOG_PAD));
            for b in &self.buildings {
                if b.owner_id == for_player && b.kind == 5 && b.hp > 0 {
                    circles.push((
                        z.x + b.rel_x,
                        z.y + b.rel_y,
                        beacon_ext_radius(b.level) + 220.0 + SERVER_FOG_PAD,
                    ));
                }
            }
        }
        for u in &self.units {
            if u.owner_id == for_player && u.hp > 0 {
                circles.push((u.x, u.y, 290.0 + SERVER_FOG_PAD));
            }
        }
        circles
    }

    fn zone_flags(&self, pid: PlayerId, leader_id: Option<PlayerId>) -> u8 {
        let p = self.players.get(&pid);
        let protected = p.is_some_and(|pl| pl.spawn_protection_left > 0.0);
        let regen_on = p.is_some_and(|pl| pl.regen_enabled);
        let is_hive = p.is_some_and(|pl| pl.is_hive);
        let is_leader = leader_id == Some(pid);
        let mut flags = 0u8;
        if protected {
            flags |= 1;
        }
        if is_leader {
            flags |= 2;
        }
        if regen_on {
            flags |= 8;
        }
        if is_hive {
            flags |= 16;
        }
        flags
    }

    /// Server-side Fog-of-War snapshot encoder for a single player (Anti-MapHack).
    pub fn encode_snapshot_for_player(
        &self,
        for_player: PlayerId,
        meta: &SnapshotSharedMeta,
    ) -> Option<Vec<u8>> {
        let player = self.players.get(&for_player)?;
        let unit_count = meta.unit_counts.get(&for_player).copied().unwrap_or(0);
        let r_eq = self.equilibrium_radius(for_player);
        let vision = self.collect_vision_circles(for_player);

        let mut zones: Vec<SnapshotZone> = Vec::with_capacity(8);
        for z in self.zones.values() {
            let is_own = z.player_id == for_player;
            let is_top = meta.top3_leaders.contains(&z.player_id);
            if !is_own && !is_top && !point_in_vision(z.x, z.y, z.radius, &vision) {
                continue;
            }
            zones.push(SnapshotZone {
                player_id: z.player_id,
                x: z.x,
                y: z.y,
                radius: z.radius,
                team_id: z.team_id,
                skin_id: z.skin_id,
                flags: self.zone_flags(z.player_id, meta.leader_id),
            });
        }

        let mut buildings: Vec<SnapshotBuilding> = Vec::with_capacity(48);
        let mut attractors: Vec<SnapshotAttractor> = Vec::with_capacity(4);
        let attr_r = self.config.pve.attractor_radius.max(20.0);
        for b in &self.buildings {
            if b.hp == 0 {
                continue;
            }
            if b.owner_id == 0 && b.kind == 6 {
                attractors.push(SnapshotAttractor {
                    id: b.id,
                    x: b.rel_x,
                    y: b.rel_y,
                    hp: b.hp,
                    max_hp: b.max_hp,
                    radius: attr_r,
                });
                continue;
            }
            let is_own = b.owner_id == for_player;
            if !is_own {
                let Some(z) = self.zones.get(&b.owner_id) else {
                    continue;
                };
                let br = self
                    .config
                    .building_by_kind(b.kind)
                    .map(|c| c.radius)
                    .unwrap_or(24.0);
                if !point_in_vision(z.x + b.rel_x, z.y + b.rel_y, br, &vision) {
                    continue;
                }
            }
            buildings.push(SnapshotBuilding {
                id: b.id,
                owner_id: b.owner_id,
                kind: b.kind,
                level: b.level,
                meta: b.meta,
                rel_x: b.rel_x,
                rel_y: b.rel_y,
                hp: b.hp,
                max_hp: b.max_hp,
                angle: b.angle,
            });
        }

        let mut units: Vec<SnapshotUnit> = Vec::with_capacity(64);
        for u in &self.units {
            if u.hp == 0 {
                continue;
            }
            if u.owner_id != for_player && !point_in_vision(u.x, u.y, 12.0, &vision) {
                continue;
            }
            units.push(SnapshotUnit {
                id: u.id,
                owner_id: u.owner_id,
                kind: u.kind,
                squad: u.squad,
                x: u.x,
                y: u.y,
                hp: u.hp,
                max_hp: u.max_hp,
            });
        }

        let mut projectiles: Vec<SnapshotProjectile> = Vec::with_capacity(24);
        for p in &self.projectiles {
            if p.owner_id != for_player && !point_in_vision(p.x, p.y, 12.0, &vision) {
                continue;
            }
            projectiles.push(SnapshotProjectile {
                id: p.id,
                owner_id: p.owner_id,
                kind: p.kind,
                x: p.x,
                y: p.y,
            });
        }

        let mut teleports: Vec<SnapshotTeleport> = Vec::new();
        for p in self.players.values() {
            let Some(tp) = p.pending_teleport else {
                continue;
            };
            let is_own = p.id == for_player;
            if !is_own {
                let src_vis = self
                    .zones
                    .get(&p.id)
                    .is_some_and(|z| point_in_vision(z.x, z.y, z.radius, &vision));
                let dst_vis = point_in_vision(tp.target_x, tp.target_y, 80.0, &vision);
                if !src_vis && !dst_vis {
                    continue;
                }
            }
            let ratio = (1.0 - tp.remaining_sec / tp.total_sec.max(0.01)).clamp(0.0, 1.0);
            teleports.push(SnapshotTeleport {
                owner_id: p.id,
                target_x: tp.target_x,
                target_y: tp.target_y,
                progress_u8: (ratio * 255.0).round() as u8,
            });
        }

        let now = self.elapsed_sec;
        let mut heats: Vec<SnapshotHeat> = Vec::new();
        for h in &self.combat_heats {
            if now >= h.show_at_sec && now < h.until_sec && h.power >= 2.2 {
                let fade = ((h.until_sec - now) / 2.5).clamp(0.0, 1.0);
                let norm = ((h.power / 12.0).clamp(0.2, 1.0) * fade * 255.0).round() as u8;
                if norm > 0 {
                    heats.push(SnapshotHeat {
                        x: h.x,
                        y: h.y,
                        intensity_u8: norm,
                    });
                }
            }
        }

        let cap = SNAPSHOT_HEADER_BYTES
            + 2
            + zones.len() * SNAPSHOT_ZONE_BYTES
            + 2
            + buildings.len() * SNAPSHOT_BUILDING_BYTES
            + 2
            + units.len() * SNAPSHOT_UNIT_BYTES
            + 2
            + projectiles.len() * SNAPSHOT_PROJECTILE_BYTES
            + 2
            + teleports.len() * SNAPSHOT_TELEPORT_BYTES
            + 2
            + attractors.len() * SNAPSHOT_ATTRACTOR_BYTES
            + 2
            + heats.len() * SNAPSHOT_HEAT_BYTES;
        let mut out = Vec::with_capacity(cap);
        write_snapshot_header(
            &mut out,
            self.tick,
            player.score,
            r_eq,
            player.energy,
            player.trophies,
            unit_count,
            player.unit_cap,
            0,
        );

        out.extend_from_slice(&(zones.len() as u16).to_le_bytes());
        for z in &zones {
            out.extend_from_slice(&z.player_id.to_le_bytes());
            out.extend_from_slice(&z.x.to_le_bytes());
            out.extend_from_slice(&z.y.to_le_bytes());
            out.extend_from_slice(&z.radius.to_le_bytes());
            out.push(z.team_id);
            out.extend_from_slice(&z.skin_id.to_le_bytes());
            out.push(z.flags);
        }

        out.extend_from_slice(&(buildings.len() as u16).to_le_bytes());
        for b in &buildings {
            out.extend_from_slice(&b.id.to_le_bytes());
            out.extend_from_slice(&b.owner_id.to_le_bytes());
            out.push(b.kind);
            out.push(b.level);
            out.push(b.meta);
            out.extend_from_slice(&b.rel_x.to_le_bytes());
            out.extend_from_slice(&b.rel_y.to_le_bytes());
            out.extend_from_slice(&b.hp.to_le_bytes());
            out.extend_from_slice(&b.max_hp.to_le_bytes());
            out.push(b.angle);
        }

        out.extend_from_slice(&(units.len() as u16).to_le_bytes());
        for u in &units {
            out.extend_from_slice(&u.id.to_le_bytes());
            out.extend_from_slice(&u.owner_id.to_le_bytes());
            out.push(u.kind);
            out.push(u.squad);
            out.extend_from_slice(&u.x.to_le_bytes());
            out.extend_from_slice(&u.y.to_le_bytes());
            out.extend_from_slice(&u.hp.to_le_bytes());
            out.extend_from_slice(&u.max_hp.to_le_bytes());
        }

        out.extend_from_slice(&(projectiles.len() as u16).to_le_bytes());
        for p in &projectiles {
            out.extend_from_slice(&p.id.to_le_bytes());
            out.extend_from_slice(&p.owner_id.to_le_bytes());
            out.push(p.kind);
            out.extend_from_slice(&p.x.to_le_bytes());
            out.extend_from_slice(&p.y.to_le_bytes());
        }

        out.extend_from_slice(&(teleports.len() as u16).to_le_bytes());
        for t in &teleports {
            out.extend_from_slice(&t.owner_id.to_le_bytes());
            out.extend_from_slice(&t.target_x.to_le_bytes());
            out.extend_from_slice(&t.target_y.to_le_bytes());
            out.push(t.progress_u8);
        }

        out.extend_from_slice(&(attractors.len() as u16).to_le_bytes());
        for a in &attractors {
            out.extend_from_slice(&a.id.to_le_bytes());
            out.extend_from_slice(&a.x.to_le_bytes());
            out.extend_from_slice(&a.y.to_le_bytes());
            out.extend_from_slice(&a.hp.to_le_bytes());
            out.extend_from_slice(&a.max_hp.to_le_bytes());
            out.extend_from_slice(&a.radius.to_le_bytes());
        }

        out.extend_from_slice(&(heats.len() as u16).to_le_bytes());
        for h in &heats {
            out.extend_from_slice(&h.x.to_le_bytes());
            out.extend_from_slice(&h.y.to_le_bytes());
            out.push(h.intensity_u8);
        }

        Some(out)
    }

    pub fn build_player_meta(&self) -> ServerMessage {
        let players = self
            .players
            .values()
            .map(|p| PlayerMetaEntry {
                player_id: p.id,
                name: p.name.clone(),
                team_id: 0,
                skin_id: p.skin_id,
            })
            .collect();
        ServerMessage::PlayerMeta { players }
    }

    pub fn build_leaderboard(&self) -> ServerMessage {
        let mut entries: Vec<LeaderboardEntry> = self
            .players
            .values()
            .filter(|p| !p.is_hive)
            .map(|p| LeaderboardEntry {
                player_id: p.id,
                score: p.score,
            })
            .collect();
        entries.sort_by(|a, b| b.score.cmp(&a.score));
        entries.truncate(10);
        ServerMessage::MatchLeaderboard { entries }
    }
}
