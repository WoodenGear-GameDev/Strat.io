# База данных и HTTP API (`meta-server`)

СУБД: **SQLite** (через `sqlx` с миграциями). Игровой узел (`game-node`) к базе напрямую не подключается.

---

## 1. Схема базы данных (SQLite)

### Таблица `users`
| Поле | Тип | Ограничения | Описание |
| :--- | :--- | :--- | :--- |
| `id` | `INTEGER` | `PRIMARY KEY AUTOINCREMENT` | Внутренний ID игрока |
| `username` | `TEXT` | `UNIQUE NOT NULL` | Отображаемый ник |
| `email` | `TEXT` | `UNIQUE NULL` | Email (`NULL` допустим для входа через Telegram в Этапе 3) |
| `password_hash` | `TEXT` | `NULL` | Хэш пароля (Argon2) |
| `telegram_id` | `INTEGER` | `UNIQUE NULL` | Задел под Telegram Mini App (Этап 3) |
| `selected_skin_id` | `INTEGER` | `NOT NULL DEFAULT 0` | Задел под скины (Этап 3) |
| `created_at` | `INTEGER` | `NOT NULL` | Unix timestamp (секунды) |

### Таблица `match_results`
| Поле | Тип | Ограничения | Описание |
| :--- | :--- | :--- | :--- |
| `id` | `INTEGER` | `PRIMARY KEY AUTOINCREMENT` | ID записи забега |
| `user_id` | `INTEGER` | `NOT NULL, REFERENCES users(id)` | Игрок |
| `mode` | `TEXT` | `NOT NULL DEFAULT 'ffa'` | Режим (`ffa`, позже `team`) |
| `score` | `INTEGER` | `NOT NULL` | Итоговые очки |
| `kills` | `INTEGER` | `NOT NULL` | Уничтожено юнитов |
| `buildings_destroyed` | `INTEGER` | `NOT NULL` | Разрушено построек |
| `survival_sec` | `INTEGER` | `NOT NULL` | Время жизни в матче |
| `finished_at` | `INTEGER` | `NOT NULL` | Unix timestamp окончания |

### Таблица `user_stats`
Кэш агрегированной статистики (обновляется в одной транзакции со вставкой в `match_results` для мгновенной работы лидерборда и профиля):
- `user_id` (`INTEGER PRIMARY KEY REFERENCES users(id)`)
- `games_played` (`INTEGER NOT NULL DEFAULT 0`)
- `best_score` (`INTEGER NOT NULL DEFAULT 0`)
- `total_score` (`INTEGER NOT NULL DEFAULT 0`)
- `total_kills` (`INTEGER NOT NULL DEFAULT 0`)
- `total_buildings_destroyed` (`INTEGER NOT NULL DEFAULT 0`)
- `total_survival_sec` (`INTEGER NOT NULL DEFAULT 0`)

*(В Этапе 3 отдельными миграциями добавляются таблицы `user_skins` и `user_achievements`.)*

---

## 2. Механизм JWT и связь `meta-server` ↔ `game-node`

1. После `register` или `login` сервер `meta-server` выдаёт **JWT**, подписанный общим `JWT_SECRET`.
   - Payload токена: `{ user_id: number, username: string, skin_id: number, exp: number }`.
2. При подключении к `game-node` клиент передаёт JWT в пакете `0x01 Join` (или обычный ник, если играет гостем без аккаунта).
3. `game-node` проверяет подпись JWT локально в памяти.
4. Когда авторизованный игрок проигрывает (`GameOver`), `game-node` отправляет асинхронный HTTP-запрос на `meta-server` с заголовком `X-Internal-Secret`.

---

## 3. HTTP API эндпоинты (`meta-server`)

### Авторизация
- `POST /api/auth/register` — `{ email, username, password }` → `{ token, user }` (хэширование Argon2, создание строки в `users` и `user_stats`).
- `POST /api/auth/login` — `{ email, password }` → `{ token, user }`.
- `GET /api/auth/me` *(Header: `Authorization: Bearer <token>`)* → проверка токена и возврат текущего профиля.
- *Гостевой вход не обращается к `meta-server`.*

### Инфраструктура и конфиг
- `GET /api/servers` → список игровых узлов `[{ id, name, region, ws_url }]` (в MVP возвращает основной `game-node`).
- `GET /api/config` → отдача актуального `config/balance.json` (опционально, если не вшит в сборку клиента).

### Профили и лидерборд
- `GET /api/users/:username` → публичный профиль + `user_stats` + последние 10 записей из `match_results`.
- `GET /api/leaderboard?sort=best_score&limit=50` → глобальный топ игроков (сортировка по `best_score`, `total_kills` или `total_score`).

### Внутренний API (`game-node` → `meta-server`)
- `POST /internal/match-result` *(Header: `X-Internal-Secret`)* — тело `{ user_id, mode, score, kills, buildings_destroyed, survival_sec }`. Сохраняет матч в `match_results` и инкрементирует `user_stats`.
