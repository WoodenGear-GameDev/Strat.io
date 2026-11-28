use crate::types::PlayerId;

#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Join {
        auth_or_name: String,
        skin_id: u16,
        team_id: u8,
    },
    Build {
        building_kind: u8,
        rel_x: f32,
        rel_y: f32,
    },
    UpgradeBuilding {
        building_id: u32,
    },
    SellBuilding {
        building_id: u32,
    },
    ArmyCommand {
        squad: u8,
        mode: u8,
        target_x: f32,
        target_y: f32,
    },
    TeleportZone {
        target_x: f32,
        target_y: f32,
    },
    Ping {
        client_time_ms: u32,
    },
    ConfigureBuilding {
        building_id: u32,
        meta: u8,
    },
    ToggleBaseRegen,
}

#[derive(Debug, Clone)]
pub struct SnapshotZone {
    pub player_id: PlayerId,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub team_id: u8,
    pub skin_id: u16,
    pub flags: u8,
}

#[derive(Debug, Clone)]
pub struct SnapshotBuilding {
    pub id: u32,
    pub owner_id: PlayerId,
    pub kind: u8,
    pub level: u8,
    pub meta: u8,
    pub rel_x: f32,
    pub rel_y: f32,
    pub hp: u16,
    pub max_hp: u16,
    pub angle: u8,
}

#[derive(Debug, Clone)]
pub struct SnapshotUnit {
    pub id: u32,
    pub owner_id: PlayerId,
    pub kind: u8,
    pub squad: u8,
    pub x: f32,
    pub y: f32,
    pub hp: u16,
    pub max_hp: u16,
}

#[derive(Debug, Clone)]
pub struct SnapshotProjectile {
    pub id: u32,
    pub owner_id: PlayerId,
    pub kind: u8,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct PlayerMetaEntry {
    pub player_id: PlayerId,
    pub name: String,
    pub team_id: u8,
    pub skin_id: u16,
}

#[derive(Debug, Clone)]
pub struct LeaderboardEntry {
    pub player_id: PlayerId,
    pub score: u32,
}

#[derive(Debug, Clone)]
pub enum ServerMessage {
    Welcome {
        player_id: PlayerId,
        map_radius: f32,
        tick_rate: u8,
    },
    Snapshot {
        tick: u32,
        score: u32,
        r_eq: f32,
        energy: f32,
        trophies: f32,
        unit_count: u16,
        unit_cap: u16,
        teleport_cd_sec: u8,
        zones: Vec<SnapshotZone>,
        buildings: Vec<SnapshotBuilding>,
        units: Vec<SnapshotUnit>,
        projectiles: Vec<SnapshotProjectile>,
    },
    PlayerMeta {
        players: Vec<PlayerMetaEntry>,
    },
    MatchLeaderboard {
        entries: Vec<LeaderboardEntry>,
    },
    GameOver {
        score: u32,
        survival_sec: u32,
        kills: u32,
        buildings_destroyed: u32,
        reason: u8,
    },
    ActionError {
        code: u8,
    },
    Pong {
        client_time_ms: u32,
        server_tick: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    EmptyPacket,
    UnknownOpcode(u8),
    UnexpectedEof,
    InvalidUtf8,
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], ProtocolError> {
        if self.pos + len > self.buf.len() {
            return Err(ProtocolError::UnexpectedEof);
        }
        let slice = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, ProtocolError> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, ProtocolError> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_f32(&mut self) -> Result<f32, ProtocolError> {
        let b = self.read_bytes(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_string(&mut self) -> Result<String, ProtocolError> {
        let len = self.read_u16()? as usize;
        let bytes = self.read_bytes(len)?;
        std::str::from_utf8(bytes)
            .map(|s| s.to_owned())
            .map_err(|_| ProtocolError::InvalidUtf8)
    }
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&bytes[..len as usize]);
}

impl ClientMessage {
    pub fn decode(data: &[u8]) -> Result<Self, ProtocolError> {
        if data.is_empty() {
            return Err(ProtocolError::EmptyPacket);
        }
        let mut r = Reader::new(data);
        let opcode = r.read_u8()?;
        match opcode {
            0x01 => Ok(Self::Join {
                auth_or_name: r.read_string()?,
                skin_id: r.read_u16()?,
                team_id: r.read_u8()?,
            }),
            0x02 => Ok(Self::Build {
                building_kind: r.read_u8()?,
                rel_x: r.read_f32()?,
                rel_y: r.read_f32()?,
            }),
            0x03 => Ok(Self::UpgradeBuilding {
                building_id: r.read_u32()?,
            }),
            0x04 => Ok(Self::SellBuilding {
                building_id: r.read_u32()?,
            }),
            0x05 => Ok(Self::ArmyCommand {
                squad: r.read_u8()?,
                mode: r.read_u8()?,
                target_x: r.read_f32()?,
                target_y: r.read_f32()?,
            }),
            0x07 => Ok(Self::TeleportZone {
                target_x: r.read_f32()?,
                target_y: r.read_f32()?,
            }),
            0x08 => Ok(Self::Ping {
                client_time_ms: r.read_u32()?,
            }),
            0x0A => Ok(Self::ConfigureBuilding {
                building_id: r.read_u32()?,
                meta: r.read_u8()?,
            }),
            0x0B => Ok(Self::ToggleBaseRegen),
            other => Err(ProtocolError::UnknownOpcode(other)),
        }
    }
}

pub const SNAPSHOT_HEADER_BYTES: usize = 26;
pub const SNAPSHOT_ZONE_BYTES: usize = 20;
pub const SNAPSHOT_BUILDING_BYTES: usize = 24;
pub const SNAPSHOT_UNIT_BYTES: usize = 22;
pub const SNAPSHOT_PROJECTILE_BYTES: usize = 17;

#[allow(clippy::too_many_arguments)]
pub fn write_snapshot_header(
    out: &mut Vec<u8>,
    tick: u32,
    score: u32,
    r_eq: f32,
    energy: f32,
    trophies: f32,
    unit_count: u16,
    unit_cap: u16,
    teleport_cd_sec: u8,
) {
    out.push(0x82);
    out.extend_from_slice(&tick.to_le_bytes());
    out.extend_from_slice(&score.to_le_bytes());
    out.extend_from_slice(&r_eq.to_le_bytes());
    out.extend_from_slice(&energy.to_le_bytes());
    out.extend_from_slice(&trophies.to_le_bytes());
    out.extend_from_slice(&unit_count.to_le_bytes());
    out.extend_from_slice(&unit_cap.to_le_bytes());
    out.push(teleport_cd_sec);
}

impl ServerMessage {
    pub fn encode(&self) -> Vec<u8> {
        let cap = match self {
            Self::Welcome { .. } => 10,
            Self::Snapshot {
                zones,
                buildings,
                units,
                projectiles,
                ..
            } => {
                SNAPSHOT_HEADER_BYTES
                    + 2
                    + zones.len() * SNAPSHOT_ZONE_BYTES
                    + 2
                    + buildings.len() * SNAPSHOT_BUILDING_BYTES
                    + 2
                    + units.len() * SNAPSHOT_UNIT_BYTES
                    + 2
                    + projectiles.len() * SNAPSHOT_PROJECTILE_BYTES
            }
            Self::PlayerMeta { players } => {
                3 + players
                    .iter()
                    .map(|p| 9 + p.name.len().min(u16::MAX as usize))
                    .sum::<usize>()
            }
            Self::MatchLeaderboard { entries } => 2 + entries.len().min(u8::MAX as usize) * 8,
            Self::GameOver { .. } => 18,
            Self::ActionError { .. } => 2,
            Self::Pong { .. } => 9,
        };
        let mut out = Vec::with_capacity(cap);
        match self {
            Self::Welcome {
                player_id,
                map_radius,
                tick_rate,
            } => {
                out.push(0x81);
                out.extend_from_slice(&player_id.to_le_bytes());
                out.extend_from_slice(&map_radius.to_le_bytes());
                out.push(*tick_rate);
            }
            Self::Snapshot {
                tick,
                score,
                r_eq,
                energy,
                trophies,
                unit_count,
                unit_cap,
                teleport_cd_sec,
                zones,
                buildings,
                units,
                projectiles,
            } => {
                write_snapshot_header(
                    &mut out,
                    *tick,
                    *score,
                    *r_eq,
                    *energy,
                    *trophies,
                    *unit_count,
                    *unit_cap,
                    *teleport_cd_sec,
                );

                out.extend_from_slice(&(zones.len() as u16).to_le_bytes());
                for z in zones {
                    out.extend_from_slice(&z.player_id.to_le_bytes());
                    out.extend_from_slice(&z.x.to_le_bytes());
                    out.extend_from_slice(&z.y.to_le_bytes());
                    out.extend_from_slice(&z.radius.to_le_bytes());
                    out.push(z.team_id);
                    out.extend_from_slice(&z.skin_id.to_le_bytes());
                    out.push(z.flags);
                }

                out.extend_from_slice(&(buildings.len() as u16).to_le_bytes());
                for b in buildings {
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
                for u in units {
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
                for p in projectiles {
                    out.extend_from_slice(&p.id.to_le_bytes());
                    out.extend_from_slice(&p.owner_id.to_le_bytes());
                    out.push(p.kind);
                    out.extend_from_slice(&p.x.to_le_bytes());
                    out.extend_from_slice(&p.y.to_le_bytes());
                }
            }
            Self::PlayerMeta { players } => {
                out.push(0x83);
                out.extend_from_slice(&(players.len() as u16).to_le_bytes());
                for p in players {
                    out.extend_from_slice(&p.player_id.to_le_bytes());
                    write_string(&mut out, &p.name);
                    out.push(p.team_id);
                    out.extend_from_slice(&p.skin_id.to_le_bytes());
                }
            }
            Self::MatchLeaderboard { entries } => {
                out.push(0x84);
                let count = entries.len().min(u8::MAX as usize) as u8;
                out.push(count);
                for e in entries.iter().take(count as usize) {
                    out.extend_from_slice(&e.player_id.to_le_bytes());
                    out.extend_from_slice(&e.score.to_le_bytes());
                }
            }
            Self::GameOver {
                score,
                survival_sec,
                kills,
                buildings_destroyed,
                reason,
            } => {
                out.push(0x85);
                out.extend_from_slice(&score.to_le_bytes());
                out.extend_from_slice(&survival_sec.to_le_bytes());
                out.extend_from_slice(&kills.to_le_bytes());
                out.extend_from_slice(&buildings_destroyed.to_le_bytes());
                out.push(*reason);
            }
            Self::ActionError { code } => {
                out.push(0x86);
                out.push(*code);
            }
            Self::Pong {
                client_time_ms,
                server_tick,
            } => {
                out.push(0x87);
                out.extend_from_slice(&client_time_ms.to_le_bytes());
                out.extend_from_slice(&server_tick.to_le_bytes());
            }
        }
        out
    }
}
