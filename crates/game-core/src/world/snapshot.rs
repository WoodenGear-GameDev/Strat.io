use std::collections::HashMap;

use crate::protocol::{
    write_snapshot_header, LeaderboardEntry, PlayerMetaEntry, ServerMessage, SnapshotBuilding,
    SnapshotProjectile, SnapshotUnit, SnapshotZone, SNAPSHOT_BUILDING_BYTES, SNAPSHOT_HEADER_BYTES,
    SNAPSHOT_PROJECTILE_BYTES, SNAPSHOT_UNIT_BYTES, SNAPSHOT_ZONE_BYTES,
};
use crate::types::PlayerId;

use super::World;

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

    #[inline]
    fn snapshot_leader_id(&self) -> Option<PlayerId> {
        self.players
            .values()
            .filter(|p| p.score >= 400)
            .max_by_key(|p| p.score)
            .map(|p| p.id)
    }

    #[inline]
    fn shared_snapshot_tail_capacity(&self) -> usize {
        2 + self.zones.len() * SNAPSHOT_ZONE_BYTES
            + 2
            + self.buildings.len() * SNAPSHOT_BUILDING_BYTES
            + 2
            + self.units.len() * SNAPSHOT_UNIT_BYTES
            + 2
            + self.projectiles.len() * SNAPSHOT_PROJECTILE_BYTES
    }

    fn write_shared_snapshot_tail(
        &self,
        out: &mut Vec<u8>,
        mut unit_counts: Option<&mut HashMap<PlayerId, u16>>,
    ) {
        let leader_id = self.snapshot_leader_id();
        let supply_costs = self.supply_cost_table();

        out.extend_from_slice(&(self.zones.len() as u16).to_le_bytes());
        for z in self.zones.values() {
            let p = self.players.get(&z.player_id);
            let protected = p.map(|pl| pl.spawn_protection_left > 0.0).unwrap_or(false);
            let is_leader = leader_id == Some(z.player_id);
            let mut flags = 0u8;
            if protected {
                flags |= 1;
            }
            if is_leader {
                flags |= 2;
            }
            out.extend_from_slice(&z.player_id.to_le_bytes());
            out.extend_from_slice(&z.x.to_le_bytes());
            out.extend_from_slice(&z.y.to_le_bytes());
            out.extend_from_slice(&z.radius.to_le_bytes());
            out.push(z.team_id);
            out.extend_from_slice(&z.skin_id.to_le_bytes());
            out.push(flags);
        }

        out.extend_from_slice(&(self.buildings.len() as u16).to_le_bytes());
        for b in &self.buildings {
            out.extend_from_slice(&b.id.to_le_bytes());
            out.extend_from_slice(&b.owner_id.to_le_bytes());
            out.push(b.kind);
            out.push(b.level);
            out.push(0); // mutation
            out.push(b.meta);
            out.extend_from_slice(&b.rel_x.to_le_bytes());
            out.extend_from_slice(&b.rel_y.to_le_bytes());
            out.extend_from_slice(&b.hp.to_le_bytes());
            out.extend_from_slice(&b.max_hp.to_le_bytes());
            out.push(b.angle);
        }

        out.extend_from_slice(&(self.units.len() as u16).to_le_bytes());
        for u in &self.units {
            if let Some(counts) = &mut unit_counts {
                if u.hp > 0 && u.kind != 4 {
                    let cost = self.fast_supply_cost(&supply_costs, u.kind);
                    let entry = counts.entry(u.owner_id).or_insert(0);
                    *entry = entry.saturating_add(cost);
                }
            }
            out.extend_from_slice(&u.id.to_le_bytes());
            out.extend_from_slice(&u.owner_id.to_le_bytes());
            out.push(u.kind);
            out.push(u.squad);
            out.extend_from_slice(&u.x.to_le_bytes());
            out.extend_from_slice(&u.y.to_le_bytes());
            out.extend_from_slice(&u.hp.to_le_bytes());
            out.extend_from_slice(&u.max_hp.to_le_bytes());
        }

        out.extend_from_slice(&(self.projectiles.len() as u16).to_le_bytes());
        for p in &self.projectiles {
            out.extend_from_slice(&p.id.to_le_bytes());
            out.extend_from_slice(&p.owner_id.to_le_bytes());
            out.push(p.kind);
            out.extend_from_slice(&p.x.to_le_bytes());
            out.extend_from_slice(&p.y.to_le_bytes());
        }
    }

    /// Encode the shared snapshot body (zones, buildings, units, projectiles) once per tick.
    pub fn build_shared_snapshot_tail(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.shared_snapshot_tail_capacity());
        self.write_shared_snapshot_tail(&mut out, None);
        out
    }

    /// Encode the shared snapshot body once per tick while also computing per-player unit supply
    /// counts in the same single pass over `self.units`.
    pub fn build_shared_snapshot_tail_with_counts(&self) -> (Vec<u8>, HashMap<PlayerId, u16>) {
        let mut out = Vec::with_capacity(self.shared_snapshot_tail_capacity());
        let mut unit_counts = HashMap::with_capacity(self.players.len());
        self.write_shared_snapshot_tail(&mut out, Some(&mut unit_counts));
        (out, unit_counts)
    }

    /// Encode a complete Snapshot packet for `for_player` by prepending their 26-byte header
    /// to a pre-encoded shared tail from `build_shared_snapshot_tail`.
    pub fn encode_snapshot_with_tail(&self, for_player: PlayerId, tail: &[u8]) -> Option<Vec<u8>> {
        if !self.players.contains_key(&for_player) {
            return None;
        }
        let supply_costs = self.supply_cost_table();
        let unit_count = self
            .units
            .iter()
            .filter(|u| u.owner_id == for_player && u.hp > 0 && u.kind != 4)
            .fold(0u16, |acc, u| {
                acc.saturating_add(self.fast_supply_cost(&supply_costs, u.kind))
            });
        self.encode_snapshot_with_tail_and_count(for_player, unit_count, tail)
    }

    /// Encode a complete Snapshot packet for `for_player` using a precomputed `unit_count`
    /// and a pre-encoded shared tail from `build_shared_snapshot_tail_with_counts`.
    pub fn encode_snapshot_with_tail_and_count(
        &self,
        for_player: PlayerId,
        unit_count: u16,
        tail: &[u8],
    ) -> Option<Vec<u8>> {
        let player = self.players.get(&for_player)?;
        let r_eq = self.equilibrium_radius(for_player);
        let mut out = Vec::with_capacity(SNAPSHOT_HEADER_BYTES + tail.len());
        write_snapshot_header(
            &mut out,
            self.tick,
            player.score,
            r_eq,
            player.energy,
            player.trophies,
            unit_count,
            player.unit_cap,
            player.teleport_cooldown_left.ceil().clamp(0.0, 255.0) as u8,
        );
        out.extend_from_slice(tail);
        Some(out)
    }

    pub fn build_snapshot(&self, for_player: PlayerId) -> Option<ServerMessage> {
        let player = self.players.get(&for_player)?;
        let supply_costs = self.supply_cost_table();
        let unit_count = self
            .units
            .iter()
            .filter(|u| u.owner_id == for_player && u.hp > 0 && u.kind != 4)
            .fold(0u16, |acc, u| {
                acc.saturating_add(self.fast_supply_cost(&supply_costs, u.kind))
            });
        let r_eq = self.equilibrium_radius(for_player);
        let leader_id = self.snapshot_leader_id();

        let mut zones = Vec::with_capacity(self.zones.len());
        for z in self.zones.values() {
            let p = self.players.get(&z.player_id);
            let protected = p.map(|pl| pl.spawn_protection_left > 0.0).unwrap_or(false);
            let is_leader = leader_id == Some(z.player_id);
            let mut flags = 0u8;
            if protected {
                flags |= 1;
            }
            if is_leader {
                flags |= 2;
            }
            zones.push(SnapshotZone {
                player_id: z.player_id,
                x: z.x,
                y: z.y,
                radius: z.radius,
                team_id: z.team_id,
                skin_id: z.skin_id,
                flags,
            });
        }

        let mut buildings = Vec::with_capacity(self.buildings.len());
        for b in &self.buildings {
            buildings.push(SnapshotBuilding {
                id: b.id,
                owner_id: b.owner_id,
                kind: b.kind,
                level: b.level,
                mutation: 0,
                meta: b.meta,
                rel_x: b.rel_x,
                rel_y: b.rel_y,
                hp: b.hp,
                max_hp: b.max_hp,
                angle: b.angle,
            });
        }

        let mut units = Vec::with_capacity(self.units.len());
        for u in &self.units {
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

        let mut projectiles = Vec::with_capacity(self.projectiles.len());
        for p in &self.projectiles {
            projectiles.push(SnapshotProjectile {
                id: p.id,
                owner_id: p.owner_id,
                kind: p.kind,
                x: p.x,
                y: p.y,
            });
        }

        Some(ServerMessage::Snapshot {
            tick: self.tick,
            score: player.score,
            r_eq,
            energy: player.energy,
            trophies: player.trophies,
            unit_count,
            unit_cap: player.unit_cap,
            teleport_cd_sec: player.teleport_cooldown_left.ceil().clamp(0.0, 255.0) as u8,
            zones,
            buildings,
            units,
            projectiles,
        })
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

#[cfg(test)]
mod tests {
    use crate::config::BalanceConfig;
    use crate::world::World;

    #[test]
    fn shared_tail_snapshot_matches_full_encode() {
        let raw = include_str!("../../../../config/balance.json");
        let cfg = BalanceConfig::from_json(raw).expect("balance.json");
        let mut world = World::new(cfg, 123);
        let pid = world.add_player("Tester".to_string(), None, 0, 1);
        for _ in 0..120 {
            world.step(0.05);
        }
        let full = world.build_snapshot(pid).expect("snap").encode();
        let (tail, counts) = world.build_shared_snapshot_tail_with_counts();
        let cnt = counts.get(&pid).copied().unwrap_or(0);
        let fast = world
            .encode_snapshot_with_tail_and_count(pid, cnt, &tail)
            .expect("fast snap");
        assert_eq!(full, fast);
    }

    #[test]
    fn low_tps_variable_dt_preserves_realtime_progression() {
        let raw = include_str!("../../../../config/balance.json");
        let mut cfg_20 = BalanceConfig::from_json(raw).expect("balance.json");
        cfg_20.world.bot_count = 0;
        let cfg_5 = cfg_20.clone();

        let mut w20 = World::new(cfg_20, 77);
        let p20 = w20.add_player("P20".to_string(), None, 0, 1);
        w20.zones.get_mut(&p20).unwrap().x = 1500.0;
        w20.zones.get_mut(&p20).unwrap().y = 0.0;

        let mut w5 = World::new(cfg_5, 77);
        let p5 = w5.add_player("P5".to_string(), None, 0, 1);
        w5.zones.get_mut(&p5).unwrap().x = 1500.0;
        w5.zones.get_mut(&p5).unwrap().y = 0.0;

        // Simulate 20 real seconds: 400 ticks @ 20 TPS vs 100 ticks @ 5 TPS
        for _ in 0..400 {
            w20.step(0.05);
        }
        for _ in 0..100 {
            w5.step(0.20);
        }

        assert!((w20.elapsed_sec - 20.0).abs() < 0.01);
        assert!((w5.elapsed_sec - 20.0).abs() < 0.01);
        let pl20 = &w20.players[&p20];
        let pl5 = &w5.players[&p5];
        assert!((pl20.survival_sec - pl5.survival_sec).abs() < 0.01);
        assert_eq!(pl20.score, pl5.score, "Ring 2 score/sec must match at 5 TPS vs 20 TPS");
        assert!((pl20.energy - pl5.energy).abs() < 0.5, "Passive income must match at 5 TPS vs 20 TPS");
    }
}
