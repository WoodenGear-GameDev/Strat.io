use game_core::config::BalanceConfig;
use game_core::protocol::ClientMessage;
use game_core::types::{ArmyMode, PlayerId};
use game_core::world::{StepTimings, World};
use std::env;
use std::fs;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Strategy {
    TurtleAfk, // Sits in base, never rallies out
    RaiderMob, // Raiders + aggressive crystal/base raids
    SiegePoi,  // Turrets + Archers/Titans + crystal control
    Industry,  // Economic scaling + HQ3 beacons + mass archers
}

impl Strategy {
    fn tag(self) -> &'static str {
        match self {
            Strategy::TurtleAfk => "Черепаха-AFK",
            Strategy::RaiderMob => "Рейдер-Моб",
            Strategy::SiegePoi => "Осадник",
            Strategy::Industry => "Индустр-Макро",
        }
    }
}

struct SimAgent {
    id: PlayerId,
    life_num: u32,
    strategy: Strategy,
    next_action_sec: f32,
}

fn parse_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
}

fn spawn_agent(world: &mut World, strategy: Strategy, life_num: u32, skin_id: u16) -> PlayerId {
    world.add_player(format!("{}-{}", strategy.tag(), life_num), None, 0, skin_id)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let minutes: f32 = parse_arg(&args, "--minutes")
        .and_then(|s| s.parse().ok())
        .unwrap_or(15.0);
    let seed: u64 = parse_arg(&args, "--seed")
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);
    let out_path = parse_arg(&args, "--out").unwrap_or_else(|| "logs/sim.jsonl".to_string());
    let balance_path =
        parse_arg(&args, "--balance").unwrap_or_else(|| "config/balance.json".to_string());
    let profile = args.iter().any(|a| a == "--profile");
    let no_telemetry = args.iter().any(|a| a == "--no-telemetry");
    let bots_override: Option<usize> = parse_arg(&args, "--bots").and_then(|s| s.parse().ok());

    let raw = fs::read_to_string(&balance_path)
        .unwrap_or_else(|e| panic!("Failed to read {balance_path}: {e}"));
    let mut config = BalanceConfig::from_json(&raw)
        .unwrap_or_else(|e| panic!("Failed to parse {balance_path}: {e}"));
    if let Some(bc) = bots_override {
        config.world.bot_count = bc;
    }

    let tick_rate = config.world.tick_rate.max(1);
    let dt = 1.0 / f32::from(tick_rate);
    let total_ticks = ((minutes * 60.0) * f32::from(tick_rate)).round() as u32;

    let mut world = World::new(config, seed);
    if !no_telemetry {
        world
            .enable_telemetry(&out_path)
            .unwrap_or_else(|e| panic!("Failed to enable telemetry at {out_path}: {e}"));
    }

    let strategies = [
        Strategy::TurtleAfk,
        Strategy::RaiderMob,
        Strategy::SiegePoi,
        Strategy::Industry,
    ];
    let mut agents: Vec<SimAgent> = strategies
        .iter()
        .enumerate()
        .map(|(i, &st)| {
            let pid = spawn_agent(&mut world, st, 1, (i as u16) + 1);
            SimAgent {
                id: pid,
                life_num: 1,
                strategy: st,
                next_action_sec: 25.0 + (i as f32) * 3.0,
            }
        })
        .collect();

    let rhombus_spots: [(f32, f32); 16] = [
        (29.4, 17.0),
        (-29.4, -17.0),
        (0.0, 34.0),
        (0.0, -34.0),
        (29.4, -17.0),
        (-29.4, 17.0),
        (58.9, 17.0),
        (-58.9, -17.0),
        (29.4, 68.0),
        (-29.4, -68.0),
        (58.9, -17.0),
        (-58.9, 17.0),
        (88.3, 17.0),
        (-88.3, -17.0),
        (0.0, 85.0),
        (0.0, -85.0),
    ];
    let hex_spots: [(f32, f32); 16] = [
        (58.9, 0.0),
        (-58.9, 0.0),
        (29.4, 51.0),
        (-29.4, -51.0),
        (29.4, -51.0),
        (-29.4, 51.0),
        (117.8, 0.0),
        (-117.8, 0.0),
        (88.3, 51.0),
        (-88.3, -51.0),
        (88.3, -51.0),
        (-88.3, 51.0),
        (0.0, 102.0),
        (0.0, -102.0),
        (147.2, 51.0),
        (-147.2, -51.0),
    ];

    let started = Instant::now();
    let mut timings = StepTimings::default();
    let mut last_snap_bytes = 0usize;
    for t in 0..total_ticks {
        let sec = (t as f32) * dt;
        let game_overs = if profile {
            let go = world.step_profiled(dt, &mut timings);
            if let Some(first) = agents.first() {
                let ts = Instant::now();
                let meta = world.compute_snapshot_shared_meta();
                if let Some(bytes) = world.encode_snapshot_for_player(first.id, &meta) {
                    last_snap_bytes = bytes.len();
                }
                timings.snapshot_ns += ts.elapsed().as_nanos() as u64;
            }
            go
        } else {
            world.step(dt)
        };

        if !game_overs.is_empty() {
            for ev in &game_overs {
                if let Some(idx) = agents.iter().position(|a| a.id == ev.player_id) {
                    let st = agents[idx].strategy;
                    agents[idx].life_num += 1;
                    let life = agents[idx].life_num;
                    agents[idx].id = spawn_agent(&mut world, st, life, (idx as u16) + 1);
                    agents[idx].next_action_sec = sec + 25.0;
                }
            }
        }

        for agent in &mut agents {
            if sec < agent.next_action_sec {
                continue;
            }
            agent.next_action_sec = sec + 2.5;

            let Some(p) = world.players.get(&agent.id) else {
                continue;
            };
            let energy = p.energy;
            let trophies = p.trophies;
            let unit_cap = p.unit_cap;
            let cur_mode = p.army_mode;
            let prot_left = p.spawn_protection_left;
            let st = agent.strategy;

            let mut gens = 0u32;
            let mut turrets = 0u32;
            let mut barracks = 0u32;
            let mut beacons = 0u32;
            let mut hq_lvl = 1u8;
            let mut hq_id = 0u32;
            let mut upg_target: Option<u32> = None;
            let mut cfg_barrack: Option<(u32, u8, u8)> = None;

            for b in &world.buildings {
                if b.owner_id != agent.id {
                    continue;
                }
                match b.kind {
                    0 => {
                        hq_lvl = b.level;
                        hq_id = b.id;
                    }
                    1 => gens += 1,
                    2 => turrets += 1,
                    3 => {
                        barracks += 1;
                        cfg_barrack = Some((b.id, b.level, b.meta));
                    }
                    5 => beacons += 1,
                    _ => {}
                }
                if b.kind != 0 && b.level < hq_lvl && upg_target.is_none() {
                    upg_target = Some(b.id);
                }
            }

            if let Some((bid, blvl, cur_meta)) = cfg_barrack {
                let desired_utype = match st {
                    Strategy::RaiderMob => 0,
                    Strategy::SiegePoi if blvl >= 3 => 2,
                    Strategy::SiegePoi if blvl >= 2 => 1,
                    Strategy::Industry if blvl >= 2 => 1,
                    _ => 0,
                };
                if ((cur_meta >> 2) & 0x07) != desired_utype {
                    let _ = world.apply_command(
                        agent.id,
                        ClientMessage::ConfigureBuilding {
                            building_id: bid,
                            meta: desired_utype << 2,
                        },
                    );
                }
            }

            // Build & upgrade according to strategy
            let target_gens = match st {
                Strategy::Industry => 3,
                Strategy::TurtleAfk => 2,
                _ => 2,
            };
            let want_hq_upg = (hq_lvl == 1 && trophies >= 120.0)
                || (hq_lvl == 2 && trophies >= 320.0);

            if gens < target_gens && energy >= 50.0 {
                for &(rx, ry) in &rhombus_spots {
                    if world
                        .apply_command(
                            agent.id,
                            ClientMessage::Build {
                                building_kind: 1,
                                rel_x: rx,
                                rel_y: ry,
                            },
                        )
                        .is_none()
                    {
                        break;
                    }
                }
            } else if want_hq_upg && hq_id != 0 {
                let _ = world.apply_command(
                    agent.id,
                    ClientMessage::UpgradeBuilding { building_id: hq_id },
                );
            } else if turrets < 1 && energy >= 70.0 {
                for &(rx, ry) in &hex_spots {
                    if world
                        .apply_command(
                            agent.id,
                            ClientMessage::Build {
                                building_kind: 2,
                                rel_x: rx,
                                rel_y: ry,
                            },
                        )
                        .is_none()
                    {
                        break;
                    }
                }
            } else if barracks < 1 && st != Strategy::TurtleAfk && energy >= 90.0 {
                for &(rx, ry) in &hex_spots {
                    if world
                        .apply_command(
                            agent.id,
                            ClientMessage::Build {
                                building_kind: 3,
                                rel_x: rx,
                                rel_y: ry,
                            },
                        )
                        .is_none()
                    {
                        break;
                    }
                }
            } else if hq_lvl >= 3
                && beacons < (if st == Strategy::Industry { 3 } else { 1 })
                && energy >= 130.0
            {
                for &(rx, ry) in rhombus_spots.iter().rev() {
                    if world
                        .apply_command(
                            agent.id,
                            ClientMessage::Build {
                                building_kind: 5,
                                rel_x: rx,
                                rel_y: ry,
                            },
                        )
                        .is_none()
                    {
                        break;
                    }
                }
            } else if let Some(bid) = upg_target {
                let _ = world.apply_command(
                    agent.id,
                    ClientMessage::UpgradeBuilding { building_id: bid },
                );
            } else if energy >= 120.0 {
                let next_kind = match st {
                    Strategy::TurtleAfk => {
                        if turrets < 3 {
                            2
                        } else if gens < 4 {
                            1
                        } else {
                            0
                        }
                    }
                    Strategy::RaiderMob => {
                        if barracks < 3 && barracks <= turrets + 1 {
                            3
                        } else if gens < 4 {
                            1
                        } else if turrets < 2 {
                            2
                        } else {
                            0
                        }
                    }
                    Strategy::SiegePoi => {
                        if turrets < 3 && turrets <= barracks {
                            2
                        } else if barracks < 3 {
                            3
                        } else if gens < 4 {
                            1
                        } else {
                            0
                        }
                    }
                    Strategy::Industry => {
                        if hq_lvl >= 3 && beacons < 4 {
                            5
                        } else if gens < 5 {
                            1
                        } else if barracks < 3 {
                            3
                        } else if turrets < 2 {
                            2
                        } else {
                            0
                        }
                    }
                };
                if next_kind == 0 {
                    // Already at building cap for this strategy
                } else if next_kind == 5 || next_kind == 1 {
                    for &(rx, ry) in rhombus_spots.iter().rev() {
                        if world
                            .apply_command(
                                agent.id,
                                ClientMessage::Build {
                                    building_kind: next_kind,
                                    rel_x: rx,
                                    rel_y: ry,
                                },
                            )
                            .is_none()
                        {
                            break;
                        }
                    }
                } else {
                    for &(rx, ry) in &hex_spots {
                        if world
                            .apply_command(
                                agent.id,
                                ClientMessage::Build {
                                    building_kind: next_kind,
                                    rel_x: rx,
                                    rel_y: ry,
                                },
                            )
                            .is_none()
                        {
                            break;
                        }
                    }
                }
            }

            // Active army commands for non-AFK strategies
            if st == Strategy::TurtleAfk {
                continue;
            }
            if prot_left > 1.0 && hq_lvl < 2 {
                continue;
            }
            let own_units = world
                .units
                .iter()
                .filter(|u| u.owner_id == agent.id)
                .count();
            let threshold = ((unit_cap as usize * 2) / 3).max(3);
            if let Some(own_z) = world.zones.get(&agent.id) {
                let zx = own_z.x;
                let zy = own_z.y;
                if own_units >= threshold {
                    let find_enemy = || -> Option<(f32, f32)> {
                        let mut target = None;
                        let mut best_d = f32::MAX;
                        for oz in world.zones.values() {
                            if oz.player_id == agent.id {
                                continue;
                            }
                            let oprot = world
                                .players
                                .get(&oz.player_id)
                                .map(|op| op.spawn_protection_left > 0.0)
                                .unwrap_or(true);
                            if oprot {
                                continue;
                            }
                            let oz_r = oz.x.hypot(oz.y);
                            if hq_lvl >= 2 && oz_r > world.config.world.ring2_radius {
                                continue;
                            }
                            let d = (oz.x - zx).hypot(oz.y - zy);
                            if d < best_d {
                                best_d = d;
                                target = Some((oz.x, oz.y));
                            }
                        }
                        target
                    };
                    let (tx, ty) = match st {
                        Strategy::TurtleAfk => (zx, zy),
                        _ => find_enemy().unwrap_or((zx, zy)),
                    };
                    if (tx - zx).hypot(ty - zy) > 50.0 {
                        let _ = world.apply_command(
                            agent.id,
                            ClientMessage::ArmyCommand {
                                squad: 0,
                                mode: 1,
                                target_x: tx,
                                target_y: ty,
                            },
                        );
                    }
                } else if cur_mode == ArmyMode::Rally && own_units <= 2 {
                    let _ = world.apply_command(
                        agent.id,
                        ClientMessage::ArmyCommand {
                            squad: 0,
                            mode: 0,
                            target_x: 0.0,
                            target_y: 0.0,
                        },
                    );
                }
            }
        }
    }

    let elapsed = started.elapsed().as_secs_f32();
    println!(
        "Simulated {:.1}m ({} ticks, seed={}) in {:.3}s -> {}",
        minutes, total_ticks, seed, elapsed, out_path
    );
    if profile && timings.ticks > 0 {
        let n = timings.ticks as f64;
        let to_us = |ns: u64| (ns as f64) / n / 1000.0;
        let total_step_us = to_us(
            timings.bots_ns
                + timings.push_repulsion_ns
                + timings.eco_spawn_ns
                + timings.grid_ns
                + timings.turrets_ns
                + timings.projectiles_ns
                + timings.units_ns
                + timings.clustering_ns
                + timings.cleanup_ns
                + timings.telemetry_ns,
        );
        let snap_us = to_us(timings.snapshot_ns);
        println!(
            "PROFILE (end state: players={}, buildings={}, units={}, projs={}, snap={}B):",
            world.players.len(),
            world.buildings.len(),
            world.units.len(),
            world.projectiles.len(),
            last_snap_bytes
        );
        println!(
            "  avg step={:.1}us (max={:.1}us) | bots={:.1}us push={:.1}us eco={:.1}us grid={:.1}us turrets={:.1}us projs={:.1}us units={:.1}us cluster={:.1}us cleanup={:.1}us tlog={:.1}us | snap={:.1}us",
            total_step_us,
            (timings.max_tick_ns as f64) / 1000.0,
            to_us(timings.bots_ns),
            to_us(timings.push_repulsion_ns),
            to_us(timings.eco_spawn_ns),
            to_us(timings.grid_ns),
            to_us(timings.turrets_ns),
            to_us(timings.projectiles_ns),
            to_us(timings.units_ns),
            to_us(timings.clustering_ns),
            to_us(timings.cleanup_ns),
            to_us(timings.telemetry_ns),
            snap_us,
        );
        let mp = timings.max_tick_phases_ns;
        println!(
            "  max tick #{} ({:.1}us): bots={:.1}us push={:.1}us eco={:.1}us grid={:.1}us turrets={:.1}us projs={:.1}us units={:.1}us cluster={:.1}us cleanup={:.1}us",
            timings.max_tick_num,
            (timings.max_tick_ns as f64) / 1000.0,
            (mp[0] as f64) / 1000.0,
            (mp[1] as f64) / 1000.0,
            (mp[2] as f64) / 1000.0,
            (mp[3] as f64) / 1000.0,
            (mp[4] as f64) / 1000.0,
            (mp[5] as f64) / 1000.0,
            (mp[6] as f64) / 1000.0,
            (mp[7] as f64) / 1000.0,
            (mp[8] as f64) / 1000.0,
        );
    }
}
