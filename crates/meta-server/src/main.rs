use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{FromRow, SqlitePool};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tower_http::cors::CorsLayer;
use tracing::info;

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    game_ws_url: String,
    balance_json: serde_json::Value,
    jwt_secret: String,
    internal_secret: String,
}

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: i64,
    username: String,
    skin_id: u16,
    exp: usize,
}

#[derive(Serialize)]
struct ServerNodeInfo {
    id: &'static str,
    name: &'static str,
    region: &'static str,
    ws_url: String,
}

#[derive(Deserialize)]
struct RegisterReq {
    email: String,
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginReq {
    email: String,
    password: String,
}

#[derive(Serialize, FromRow)]
struct PublicUser {
    id: i64,
    username: String,
    selected_skin_id: i64,
    created_at: i64,
}

#[derive(FromRow)]
struct UserAuthRow {
    id: i64,
    username: String,
    password_hash: Option<String>,
    selected_skin_id: i64,
    created_at: i64,
}

#[derive(Serialize, FromRow)]
struct UserStatsRow {
    games_played: i64,
    best_score: i64,
    total_score: i64,
    total_kills: i64,
    total_buildings_destroyed: i64,
    total_survival_sec: i64,
}

#[derive(Serialize, FromRow)]
struct MatchResultRow {
    id: i64,
    mode: String,
    score: i64,
    kills: i64,
    buildings_destroyed: i64,
    survival_sec: i64,
    finished_at: i64,
}

#[derive(Deserialize)]
struct LeaderboardQuery {
    sort: Option<String>,
    limit: Option<i64>,
}

#[derive(Serialize, FromRow)]
struct LeaderboardRow {
    username: String,
    selected_skin_id: i64,
    games_played: i64,
    best_score: i64,
    total_score: i64,
    total_kills: i64,
}

#[derive(Deserialize)]
struct InternalMatchResultReq {
    user_id: i64,
    mode: String,
    score: i64,
    kills: i64,
    buildings_destroyed: i64,
    survival_sec: i64,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

fn issue_token(secret: &str, user_id: i64, username: &str, skin_id: u16) -> Result<String, StatusCode> {
    let exp = (now_unix() + 7 * 24 * 3600) as usize;
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        skin_id,
        exp,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn verify_bearer(headers: &HeaderMap, secret: &str) -> Result<Claims, (StatusCode, Json<serde_json::Value>)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth
        .strip_prefix("Bearer ")
        .ok_or_else(|| err_json(StatusCode::UNAUTHORIZED, "Требуется авторизация"))?;
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| err_json(StatusCode::UNAUTHORIZED, "Недействительный токен"))?;
    Ok(data.claims)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "meta_server=info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://stratio.db?mode=rwc".to_string());
    let addr_str =
        std::env::var("META_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let game_ws_url =
        std::env::var("GAME_WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:8081/ws".to_string());
    let balance_path =
        std::env::var("BALANCE_PATH").unwrap_or_else(|_| "config/balance.json".to_string());
    let jwt_secret =
        std::env::var("JWT_SECRET").unwrap_or_else(|_| "stratio-dev-jwt-secret".to_string());
    let internal_secret = std::env::var("INTERNAL_SECRET")
        .unwrap_or_else(|_| "stratio-dev-internal-secret".to_string());

    let balance_raw = std::fs::read_to_string(&balance_path)
        .unwrap_or_else(|e| panic!("Failed to read {balance_path}: {e}"));
    let balance_json: serde_json::Value = serde_json::from_str(&balance_raw)
        .unwrap_or_else(|e| panic!("Failed to parse {balance_path}: {e}"));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to SQLite");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run SQLite migrations");

    let state = AppState {
        pool,
        game_ws_url,
        balance_json,
        jwt_secret,
        internal_secret,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/servers", get(list_servers))
        .route("/api/config", get(get_config))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/me", get(auth_me))
        .route("/api/users/{username}", get(get_user_profile))
        .route("/api/leaderboard", get(get_leaderboard))
        .route("/internal/match-result", post(record_match_result))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr: SocketAddr = addr_str.parse().expect("Invalid META_SERVER_ADDR");
    info!("meta-server listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn list_servers(State(state): State<AppState>) -> Json<Vec<ServerNodeInfo>> {
    Json(vec![ServerNodeInfo {
        id: "main",
        name: "Primary Arena",
        region: "local",
        ws_url: state.game_ws_url,
    }])
}

async fn get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(state.balance_json)
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> impl IntoResponse {
    let email = req.email.trim().to_lowercase();
    let username = req.username.trim().to_string();
    if !email.contains('@') || email.len() < 5 {
        return err_json(StatusCode::BAD_REQUEST, "Некорректный email").into_response();
    }
    if username.len() < 2 || username.len() > 24 {
        return err_json(StatusCode::BAD_REQUEST, "Ник должен быть от 2 до 24 символов").into_response();
    }
    if req.password.len() < 6 {
        return err_json(StatusCode::BAD_REQUEST, "Пароль минимум 6 символов").into_response();
    }

    let salt = SaltString::generate(&mut OsRng);
    let hash = match Argon2::default().hash_password(req.password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Ошибка хэширования").into_response(),
    };

    let created_at = now_unix();
    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Ошибка БД").into_response(),
    };

    let res = sqlx::query(
        "INSERT INTO users (username, email, password_hash, selected_skin_id, created_at) VALUES (?, ?, ?, 0, ?)",
    )
    .bind(&username)
    .bind(&email)
    .bind(&hash)
    .bind(created_at)
    .execute(&mut *tx)
    .await;

    let user_id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(e) => {
            let msg = if e.to_string().contains("UNIQUE") {
                "Email или ник уже заняты"
            } else {
                "Ошибка создания пользователя"
            };
            return err_json(StatusCode::CONFLICT, msg).into_response();
        }
    };

    if sqlx::query("INSERT INTO user_stats (user_id) VALUES (?)")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Ошибка инициализации статистики").into_response();
    }

    if tx.commit().await.is_err() {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Ошибка коммита БД").into_response();
    }

    let Ok(token) = issue_token(&state.jwt_secret, user_id, &username, 0) else {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Ошибка генерации токена").into_response();
    };

    let user = PublicUser {
        id: user_id,
        username,
        selected_skin_id: 0,
        created_at,
    };
    (StatusCode::OK, Json(serde_json::json!({ "token": token, "user": user }))).into_response()
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginReq>,
) -> impl IntoResponse {
    let email = req.email.trim().to_lowercase();
    let row = sqlx::query_as::<_, UserAuthRow>(
        "SELECT id, username, password_hash, selected_skin_id, created_at FROM users WHERE email = ?",
    )
    .bind(&email)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some(user)) = row else {
        return err_json(StatusCode::UNAUTHORIZED, "Неверный email или пароль").into_response();
    };
    let Some(hash_str) = user.password_hash.as_deref() else {
        return err_json(StatusCode::UNAUTHORIZED, "Вход по паролю недоступен").into_response();
    };
    let Ok(parsed_hash) = PasswordHash::new(hash_str) else {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Повреждён хэш пароля").into_response();
    };
    if Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        return err_json(StatusCode::UNAUTHORIZED, "Неверный email или пароль").into_response();
    }

    let Ok(token) = issue_token(
        &state.jwt_secret,
        user.id,
        &user.username,
        user.selected_skin_id as u16,
    ) else {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Ошибка токена").into_response();
    };

    let public_user = PublicUser {
        id: user.id,
        username: user.username,
        selected_skin_id: user.selected_skin_id,
        created_at: user.created_at,
    };
    (StatusCode::OK, Json(serde_json::json!({ "token": token, "user": public_user }))).into_response()
}

async fn auth_me(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let claims = match verify_bearer(&headers, &state.jwt_secret) {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };

    let user = sqlx::query_as::<_, PublicUser>(
        "SELECT id, username, selected_skin_id, created_at FROM users WHERE id = ?",
    )
    .bind(claims.sub)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some(user)) = user else {
        return err_json(StatusCode::NOT_FOUND, "Пользователь не найден").into_response();
    };

    let stats = sqlx::query_as::<_, UserStatsRow>(
        "SELECT games_played, best_score, total_score, total_kills, total_buildings_destroyed, total_survival_sec FROM user_stats WHERE user_id = ?",
    )
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    (StatusCode::OK, Json(serde_json::json!({ "user": user, "stats": stats }))).into_response()
}

async fn get_user_profile(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> impl IntoResponse {
    let user = sqlx::query_as::<_, PublicUser>(
        "SELECT id, username, selected_skin_id, created_at FROM users WHERE username = ?",
    )
    .bind(&username)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some(user)) = user else {
        return err_json(StatusCode::NOT_FOUND, "Игрок не найден").into_response();
    };

    let stats = sqlx::query_as::<_, UserStatsRow>(
        "SELECT games_played, best_score, total_score, total_kills, total_buildings_destroyed, total_survival_sec FROM user_stats WHERE user_id = ?",
    )
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();

    let recent_matches = sqlx::query_as::<_, MatchResultRow>(
        "SELECT id, mode, score, kills, buildings_destroyed, survival_sec, finished_at FROM match_results WHERE user_id = ? ORDER BY finished_at DESC, id DESC LIMIT 10",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "user": user,
            "stats": stats,
            "recent_matches": recent_matches
        })),
    )
        .into_response()
}

async fn get_leaderboard(
    State(state): State<AppState>,
    Query(q): Query<LeaderboardQuery>,
) -> impl IntoResponse {
    let order_col = match q.sort.as_deref() {
        Some("total_kills") => "s.total_kills",
        Some("total_score") => "s.total_score",
        _ => "s.best_score",
    };
    let limit = q.limit.unwrap_or(50).clamp(1, 100);

    let sql = format!(
        "SELECT u.username, u.selected_skin_id, s.games_played, s.best_score, s.total_score, s.total_kills \
         FROM user_stats s JOIN users u ON u.id = s.user_id \
         ORDER BY {order_col} DESC, s.best_score DESC LIMIT ?"
    );

    let rows = sqlx::query_as::<_, LeaderboardRow>(&sql)
        .bind(limit)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    (StatusCode::OK, Json(rows)).into_response()
}

async fn record_match_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<InternalMatchResultReq>,
) -> impl IntoResponse {
    let secret = headers
        .get("x-internal-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if secret != state.internal_secret {
        return err_json(StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    let finished_at = now_unix();
    let Ok(mut tx) = state.pool.begin().await else {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "DB error").into_response();
    };

    if sqlx::query(
        "INSERT INTO match_results (user_id, mode, score, kills, buildings_destroyed, survival_sec, finished_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(req.user_id)
    .bind(&req.mode)
    .bind(req.score)
    .bind(req.kills)
    .bind(req.buildings_destroyed)
    .bind(req.survival_sec)
    .bind(finished_at)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Insert failed").into_response();
    }

    if sqlx::query(
        "UPDATE user_stats SET \
         games_played = games_played + 1, \
         best_score = MAX(best_score, ?), \
         total_score = total_score + ?, \
         total_kills = total_kills + ?, \
         total_buildings_destroyed = total_buildings_destroyed + ?, \
         total_survival_sec = total_survival_sec + ? \
         WHERE user_id = ?",
    )
    .bind(req.score)
    .bind(req.score)
    .bind(req.kills)
    .bind(req.buildings_destroyed)
    .bind(req.survival_sec)
    .bind(req.user_id)
    .execute(&mut *tx)
    .await
    .is_err()
    {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Stats update failed").into_response();
    }

    if tx.commit().await.is_err() {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, "Commit failed").into_response();
    }

    StatusCode::NO_CONTENT.into_response()
}
