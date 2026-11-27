CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    email TEXT UNIQUE,
    password_hash TEXT,
    telegram_id INTEGER UNIQUE,
    selected_skin_id INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS match_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id),
    mode TEXT NOT NULL DEFAULT 'ffa',
    score INTEGER NOT NULL,
    kills INTEGER NOT NULL,
    buildings_destroyed INTEGER NOT NULL,
    survival_sec INTEGER NOT NULL,
    finished_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS user_stats (
    user_id INTEGER PRIMARY KEY REFERENCES users(id),
    games_played INTEGER NOT NULL DEFAULT 0,
    best_score INTEGER NOT NULL DEFAULT 0,
    total_score INTEGER NOT NULL DEFAULT 0,
    total_kills INTEGER NOT NULL DEFAULT 0,
    total_buildings_destroyed INTEGER NOT NULL DEFAULT 0,
    total_survival_sec INTEGER NOT NULL DEFAULT 0
);
