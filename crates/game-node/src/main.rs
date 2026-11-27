use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use game_core::config::BalanceConfig;
use game_core::protocol::{ClientMessage, ServerMessage};
use game_core::types::{GameOverEvent, PlayerId};
use game_core::world::World;
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{info, warn};

type ConnId = u64;

#[derive(Deserialize)]
struct JwtClaims {
    sub: i64,
    username: String,
    skin_id: u16,
}

#[derive(Serialize)]
struct MatchResultPayload {
    user_id: i64,
    mode: &'static str,
    score: u32,
    kills: u32,
    buildings_destroyed: u32,
    survival_sec: u32,
}

#[derive(Clone)]
struct ReporterConfig {
    meta_server_url: String,
    internal_secret: String,
    http: reqwest::Client,
}

enum RoomEvent {
    Join {
        conn_id: ConnId,
        user_id: Option<i64>,
        name: String,
        skin_id: u16,
        team_id: u8,
        tx: mpsc::UnboundedSender<Vec<u8>>,
    },
    Command {
        conn_id: ConnId,
        msg: ClientMessage,
    },
    Disconnect {
        conn_id: ConnId,
    },
}

#[derive(Clone)]
struct AppState {
    room_tx: mpsc::Sender<RoomEvent>,
    next_conn_id: std::sync::Arc<AtomicU64>,
    jwt_secret: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "game_node=info".into()),
        )
        .init();

    let balance_path =
        std::env::var("BALANCE_PATH").unwrap_or_else(|_| "config/balance.json".to_string());
    let balance_raw = std::fs::read_to_string(&balance_path)
        .unwrap_or_else(|e| panic!("Failed to read {balance_path}: {e}"));
    let config = BalanceConfig::from_json(&balance_raw)
        .unwrap_or_else(|e| panic!("Failed to parse {balance_path}: {e}"));

    let addr_str = std::env::var("GAME_NODE_ADDR").unwrap_or_else(|_| "127.0.0.1:8081".to_string());
    let addr: SocketAddr = addr_str.parse().expect("Invalid GAME_NODE_ADDR");

    let jwt_secret =
        std::env::var("JWT_SECRET").unwrap_or_else(|_| "stratio-dev-jwt-secret".to_string());
    let meta_server_url =
        std::env::var("META_SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let internal_secret = std::env::var("INTERNAL_SECRET")
        .unwrap_or_else(|_| "stratio-dev-internal-secret".to_string());

    let reporter = ReporterConfig {
        meta_server_url,
        internal_secret,
        http: reqwest::Client::new(),
    };

    let (room_tx, room_rx) = mpsc::channel::<RoomEvent>(1024);
    tokio::spawn(run_room(config, room_rx, reporter));

    let state = AppState {
        room_tx,
        next_conn_id: std::sync::Arc::new(AtomicU64::new(1)),
        jwt_secret,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ws", get(ws_handler))
        .with_state(state);

    info!("game-node listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    let conn_id = state.next_conn_id.fetch_add(1, Ordering::Relaxed);
    ws.on_upgrade(move |socket| handle_socket(socket, conn_id, state.room_tx, state.jwt_secret))
}

async fn handle_socket(
    socket: WebSocket,
    conn_id: ConnId,
    room_tx: mpsc::Sender<RoomEvent>,
    jwt_secret: String,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let writer = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            if ws_tx.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Binary(data) => match ClientMessage::decode(&data) {
                Ok(ClientMessage::Join {
                    auth_or_name,
                    skin_id,
                    team_id,
                }) => {
                    let (user_id, name, resolved_skin) = if let Some(token) =
                        auth_or_name.strip_prefix("jwt:")
                    {
                        match decode::<JwtClaims>(
                            token,
                            &DecodingKey::from_secret(jwt_secret.as_bytes()),
                            &Validation::default(),
                        ) {
                            Ok(data) => (Some(data.claims.sub), data.claims.username, data.claims.skin_id),
                            Err(e) => {
                                warn!("conn {conn_id}: invalid JWT: {e:?}");
                                (None, format!("Guest-{conn_id}"), skin_id)
                            }
                        }
                    } else if auth_or_name.trim().is_empty() {
                        (None, format!("Guest-{conn_id}"), skin_id)
                    } else {
                        (None, auth_or_name.chars().take(24).collect(), skin_id)
                    };

                    let _ = room_tx
                        .send(RoomEvent::Join {
                            conn_id,
                            user_id,
                            name,
                            skin_id: resolved_skin,
                            team_id,
                            tx: out_tx.clone(),
                        })
                        .await;
                }
                Ok(cmd) => {
                    let _ = room_tx.send(RoomEvent::Command { conn_id, msg: cmd }).await;
                }
                Err(e) => {
                    warn!("conn {conn_id}: invalid packet: {e:?}");
                }
            },
            Message::Close(_) => break,
            _ => {}
        }
    }

    let _ = room_tx.send(RoomEvent::Disconnect { conn_id }).await;
    writer.abort();
}

struct ClientSession {
    player_id: PlayerId,
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

fn spawn_report_match(reporter: &ReporterConfig, ev: &GameOverEvent) {
    let Some(user_id) = ev.user_id else {
        return;
    };
    if reporter.meta_server_url.trim().is_empty() {
        return;
    }

    let url = format!(
        "{}/internal/match-result",
        reporter.meta_server_url.trim_end_matches('/')
    );
    let secret = reporter.internal_secret.clone();
    let http = reporter.http.clone();
    let payload = MatchResultPayload {
        user_id,
        mode: "ffa",
        score: ev.score,
        kills: ev.kills,
        buildings_destroyed: ev.buildings_destroyed,
        survival_sec: ev.survival_sec,
    };

    tokio::spawn(async move {
        if let Err(e) = http
            .post(url)
            .header("x-internal-secret", secret)
            .json(&payload)
            .send()
            .await
        {
            warn!("Failed to report match result for user {user_id}: {e}");
        }
    });
}

async fn run_room(
    config: BalanceConfig,
    mut rx: mpsc::Receiver<RoomEvent>,
    reporter: ReporterConfig,
) {
    let tick_rate = config.world.tick_rate.max(1);
    let nominal_dt = 1.0 / f32::from(tick_rate);
    let tick_ms = (1000 / u64::from(tick_rate)).max(1);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42);

    let mut world = World::new(config, seed);
    if let Ok(log_path) = std::env::var("STRATIO_LOG_JSONL") {
        let trimmed = log_path.trim();
        if !trimmed.is_empty() {
            match world.enable_telemetry(trimmed) {
                Ok(()) => info!("Telemetry logging enabled -> {trimmed}"),
                Err(e) => warn!("Failed to enable telemetry at {trimmed}: {e}"),
            }
        }
    }
    let mut sessions: HashMap<ConnId, ClientSession> = HashMap::new();
    let mut interval = tokio::time::interval(Duration::from_millis(tick_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_tick_at = Instant::now()
        .checked_sub(Duration::from_millis(tick_ms))
        .unwrap_or_else(Instant::now);
    let mut last_lb_sent = Instant::now();
    let mut last_slow_tick_log: Option<Instant> = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let tick_start = Instant::now();
                let raw_dt = tick_start.duration_since(last_tick_at).as_secs_f32();
                last_tick_at = tick_start;
                // Preserve 1:1 real-time simulation speed even if TPS drops (down to 4 TPS / 0.25s max step).
                let step_dt = raw_dt.clamp(nominal_dt * 0.5, 0.25);

                while let Ok(ev) = rx.try_recv() {
                    handle_room_event(&mut world, &mut sessions, ev);
                }

                let prev_players = world.players.len();
                let step_start = Instant::now();
                let game_overs = world.step(step_dt);
                let step_ms = step_start.elapsed().as_secs_f64() * 1000.0;
                if !game_overs.is_empty() {
                    for ev in game_overs {
                        spawn_report_match(&reporter, &ev);
                        if let Some((&conn_id, _)) =
                            sessions.iter().find(|(_, s)| s.player_id == ev.player_id)
                        {
                            if let Some(session) = sessions.remove(&conn_id) {
                                let msg = ServerMessage::GameOver {
                                    score: ev.score,
                                    survival_sec: ev.survival_sec,
                                    kills: ev.kills,
                                    buildings_destroyed: ev.buildings_destroyed,
                                    reason: ev.reason,
                                }
                                .encode();
                                let _ = session.tx.send(msg);
                            }
                        }
                    }
                }
                if world.players.len() != prev_players {
                    let meta = world.build_player_meta().encode();
                    for s in sessions.values() {
                        let _ = s.tx.send(meta.clone());
                    }
                }

                let snap_start = Instant::now();
                if !sessions.is_empty() {
                    let (shared_tail, unit_counts) = world.build_shared_snapshot_tail_with_counts();
                    for session in sessions.values() {
                        let unit_count = unit_counts.get(&session.player_id).copied().unwrap_or(0);
                        if let Some(encoded) = world.encode_snapshot_with_tail_and_count(
                            session.player_id,
                            unit_count,
                            &shared_tail,
                        ) {
                            let _ = session.tx.send(encoded);
                        }
                    }
                }
                let snap_ms = snap_start.elapsed().as_secs_f64() * 1000.0;

                if !sessions.is_empty() && tick_start.duration_since(last_lb_sent) >= Duration::from_secs(1) {
                    last_lb_sent = tick_start;
                    let lb = world.build_leaderboard().encode();
                    for session in sessions.values() {
                        let _ = session.tx.send(lb.clone());
                    }
                }

                let total_ms = tick_start.elapsed().as_secs_f64() * 1000.0;
                if total_ms > 40.0 {
                    let now = Instant::now();
                    let should_log = last_slow_tick_log
                        .map(|prev| now.duration_since(prev) >= Duration::from_secs(2))
                        .unwrap_or(true);
                    if should_log {
                        last_slow_tick_log = Some(now);
                        warn!(
                            "Slow tick: tick={} total_ms={:.2} step_ms={:.2} snap_ms={:.2} players={} buildings={} units={} projs={}",
                            world.tick,
                            total_ms,
                            step_ms,
                            snap_ms,
                            world.players.len(),
                            world.buildings.len(),
                            world.units.len(),
                            world.projectiles.len(),
                        );
                    }
                }
            }
            Some(ev) = rx.recv() => {
                handle_room_event(&mut world, &mut sessions, ev);
            }
        }
    }
}

fn handle_room_event(
    world: &mut World,
    sessions: &mut HashMap<ConnId, ClientSession>,
    ev: RoomEvent,
) {
    match ev {
        RoomEvent::Join {
            conn_id,
            user_id,
            name,
            skin_id,
            team_id,
            tx,
        } => {
            if sessions.contains_key(&conn_id) {
                return;
            }
            let player_id = world.add_player(name, user_id, team_id, skin_id);
            let welcome = ServerMessage::Welcome {
                player_id,
                map_radius: world.config.world.map_radius,
                tick_rate: world.config.world.tick_rate,
            }
            .encode();
            let _ = tx.send(welcome);

            sessions.insert(conn_id, ClientSession { player_id, tx });

            let meta = world.build_player_meta().encode();
            for s in sessions.values() {
                let _ = s.tx.send(meta.clone());
            }
        }
        RoomEvent::Command { conn_id, msg } => {
            let Some(session) = sessions.get(&conn_id) else {
                return;
            };
            if let ClientMessage::Ping { client_time_ms } = msg {
                let pong = ServerMessage::Pong {
                    client_time_ms,
                    server_tick: world.tick,
                }
                .encode();
                let _ = session.tx.send(pong);
                return;
            }
            if let Some(err_code) = world.apply_command(session.player_id, msg) {
                let err = ServerMessage::ActionError { code: err_code }.encode();
                let _ = session.tx.send(err);
            }
        }
        RoomEvent::Disconnect { conn_id } => {
            if let Some(session) = sessions.remove(&conn_id) {
                world.remove_player(session.player_id);
                let meta = world.build_player_meta().encode();
                for s in sessions.values() {
                    let _ = s.tx.send(meta.clone());
                }
            }
        }
    }
}
