use crate::types::PlayerId;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

pub const SRC_TURRET_1: u8 = 20;
pub const SRC_TURRET_2: u8 = 21;
pub const SRC_TURRET_3: u8 = 22;
pub const SRC_RAD: u8 = 30;
pub const SRC_UNKNOWN: u8 = 255;

pub const DST_BUILDING_OFFSET: u8 = 100;

pub fn tag_name(tag: u8) -> &'static str {
    match tag {
        0 => "raider",
        1 => "archer",
        2 => "titan",
        4 => "drone",
        10 => "crystal",
        20 => "turret_l1",
        21 => "turret_l2",
        22 => "turret_l3",
        30 => "cradle_rad",
        100 => "b_hq",
        101 => "b_gen",
        102 => "b_turret",
        103 => "b_barracks",
        105 => "b_beacon",
        _ => "unknown",
    }
}

pub struct TelemetryLogger {
    writer: BufWriter<File>,
    pub snapshot_interval_ticks: u32,
    combat_agg: HashMap<(u8, u8, u8, u8), (u64, u32)>,
    income_agg: HashMap<PlayerId, [f64; 5]>,
    pub last_rings: HashMap<PlayerId, u8>,
}

impl TelemetryLogger {
    pub fn new(path: &Path, tick_rate: u8) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        // Auto-rotate existing live.jsonl so previous human sessions are never overwritten on restart
        if path.file_name().and_then(|n| n.to_str()) == Some("live.jsonl") {
            if let Ok(meta) = fs::metadata(path) {
                if meta.len() > 0 {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let archived = path.with_file_name(format!("live_{ts}.jsonl"));
                    let _ = fs::rename(path, archived);
                }
            }
        }
        let file = File::create(path)?;
        let snapshot_interval_ticks = u32::from(tick_rate.max(1)) * 5;
        Ok(Self {
            writer: BufWriter::new(file),
            snapshot_interval_ticks,
            combat_agg: HashMap::new(),
            income_agg: HashMap::new(),
            last_rings: HashMap::new(),
        })
    }

    fn write_line(&mut self, value: &Value) {
        if let Ok(line) = serde_json::to_string(value) {
            let _ = self.writer.write_all(line.as_bytes());
            let _ = self.writer.write_all(b"\n");
        }
    }

    pub fn log_meta(&mut self, seed: u64, tick_rate: u8, spawn_protection_sec: f32) {
        self.write_line(&json!({
            "type": "meta",
            "seed": seed,
            "tick_rate": tick_rate,
            "spawn_protection_sec": spawn_protection_sec,
        }));
        let _ = self.writer.flush();
    }

    pub fn log_damage(&mut self, src_tag: u8, dst_tag: u8, src_ring: u8, dst_ring: u8, dmg: u16) {
        if dmg == 0 {
            return;
        }
        let entry = self
            .combat_agg
            .entry((src_tag, dst_tag, src_ring, dst_ring))
            .or_insert((0, 0));
        entry.0 += u64::from(dmg);
    }

    pub fn log_kill(&mut self, src_tag: u8, dst_tag: u8, src_ring: u8, dst_ring: u8) {
        let entry = self
            .combat_agg
            .entry((src_tag, dst_tag, src_ring, dst_ring))
            .or_insert((0, 0));
        entry.1 += 1;
    }

    pub fn log_milestone(&mut self, tick: u32, sec: f32, pid: PlayerId, kind: &str, data: Value) {
        self.write_line(&json!({
            "type": "milestone",
            "tick": tick,
            "sec": (sec * 10.0).round() / 10.0,
            "pid": pid,
            "kind": kind,
            "data": data,
        }));
    }

    pub fn log_death(&mut self, tick: u32, sec: f32, data: Value) {
        self.write_line(&json!({
            "type": "death",
            "tick": tick,
            "sec": (sec * 10.0).round() / 10.0,
            "data": data,
        }));
        let _ = self.writer.flush();
    }

    pub fn log_income(&mut self, pid: PlayerId, src_idx: usize, amount: f32) {
        if amount <= 0.0 || src_idx >= 5 {
            return;
        }
        let entry = self.income_agg.entry(pid).or_insert([0.0; 5]);
        entry[src_idx] += f64::from(amount);
    }

    pub fn log_action(
        &mut self,
        tick: u32,
        sec: f32,
        pid: PlayerId,
        is_bot: bool,
        cmd: &str,
        ring: u8,
        data: Value,
    ) {
        self.write_line(&json!({
            "type": "action",
            "tick": tick,
            "sec": (sec * 10.0).round() / 10.0,
            "pid": pid,
            "is_bot": is_bot,
            "cmd": cmd,
            "ring": ring,
            "data": data,
        }));
    }

    pub fn flush_combat_window(&mut self, tick: u32, sec: f32) {
        if !self.combat_agg.is_empty() {
            let entries: Vec<Value> = self
                .combat_agg
                .drain()
                .map(|((src, dst, sr, dr), (dmg, kills))| {
                    json!({
                        "src": tag_name(src),
                        "dst": tag_name(dst),
                        "sr": sr,
                        "dr": dr,
                        "dmg": dmg,
                        "kills": kills,
                    })
                })
                .collect();
            self.write_line(&json!({
                "type": "combat_window",
                "tick": tick,
                "sec": (sec * 10.0).round() / 10.0,
                "entries": entries,
            }));
        }
        if !self.income_agg.is_empty() {
            let entries: Vec<Value> = self
                .income_agg
                .drain()
                .map(|(pid, vals)| {
                    json!({
                        "pid": pid,
                        "hq": (vals[0] * 10.0).round() / 10.0,
                        "mine": (vals[1] * 10.0).round() / 10.0,
                        "poi": (vals[2] * 10.0).round() / 10.0,
                        "pve": (vals[3] * 10.0).round() / 10.0,
                        "pvp": (vals[4] * 10.0).round() / 10.0,
                    })
                })
                .collect();
            self.write_line(&json!({
                "type": "income_window",
                "tick": tick,
                "sec": (sec * 10.0).round() / 10.0,
                "entries": entries,
            }));
        }
    }

    pub fn log_snapshot(&mut self, tick: u32, sec: f32, players: Vec<Value>) {
        self.write_line(&json!({
            "type": "snapshot",
            "tick": tick,
            "sec": (sec * 10.0).round() / 10.0,
            "players": players,
        }));
        let _ = self.writer.flush();
    }
}
