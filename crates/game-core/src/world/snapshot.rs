use crate::protocol::{
    LeaderboardEntry, PlayerMetaEntry, ServerMessage, SnapshotBuilding, SnapshotProjectile,
    SnapshotUnit, SnapshotZone,
};
use crate::types::PlayerId;

use super::World;

impl World {
    pub fn build_snapshot(&self, for_player: PlayerId) -> Option<ServerMessage> {
        let player = self.players.get(&for_player)?;
        let unit_count = self
            .units
            .iter()
            .filter(|u| u.owner_id == for_player && u.hp > 0 && u.kind != 4)
            .map(|u| self.unit_supply_cost(u.kind))
            .sum::<u16>();
        let r_eq = self.equilibrium_radius(for_player);
        let leader_id = self
            .players
            .values()
            .filter(|p| p.score >= 400)
            .max_by_key(|p| p.score)
            .map(|p| p.id);

        let zones = self
            .zones
            .values()
            .map(|z| {
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
                SnapshotZone {
                    player_id: z.player_id,
                    x: z.x,
                    y: z.y,
                    radius: z.radius,
                    team_id: z.team_id,
                    skin_id: z.skin_id,
                    flags,
                }
            })
            .collect();

        let buildings = self
            .buildings
            .iter()
            .map(|b| SnapshotBuilding {
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
            })
            .collect();

        let units = self
            .units
            .iter()
            .map(|u| SnapshotUnit {
                id: u.id,
                owner_id: u.owner_id,
                kind: u.kind,
                squad: u.squad,
                x: u.x,
                y: u.y,
                hp: u.hp,
                max_hp: u.max_hp,
            })
            .collect();

        let projectiles = self
            .projectiles
            .iter()
            .map(|p| SnapshotProjectile {
                id: p.id,
                owner_id: p.owner_id,
                kind: p.kind,
                x: p.x,
                y: p.y,
            })
            .collect();

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
