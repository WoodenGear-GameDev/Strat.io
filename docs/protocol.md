# Бинарный протокол и конфиг баланса

## 1. Общие правила протокола

- **Тикрейт симуляции:** `20 TPS` (`50 мс` на тик).
- **Формат:** ручная бинарная сериализация, **little-endian**.
- **Заголовок пакета:** первый байт — `opcode: u8`.
- **Строки:** `len: u16` (или `u8` для коротких ников/токенов) + UTF-8 байты без нуль-терминатора.
- **Задел под Interest Management (Этап 3):** клиент хранит словарь сущностей по `id`. В каждом `Snapshot` обновляются пришедшие сущности, а отсутствующие в снэпшоте — удаляются из локального состояния. В Этапе 3 сервер сможет просто фильтровать снэпшот по области видимости камеры без переписывания клиента.

---

## 2. Пакеты Клиент → Сервер (`C2S`)

| Opcode | Имя | Поля | Примечание |
| :--- | :--- | :--- | :--- |
| `0x01` | `Join` | `auth_or_name: string`, `skin_id: u16`, `team_id: u8` | Если передан JWT — сервер валидирует токен; иначе гостевой ник. `skin_id`/`team_id` — задел под Этап 3. |
| `0x02` | `Build` | `building_kind: u8`, `rel_x: f32`, `rel_y: f32` | Координаты относительно центра своей зоны владения. |
| `0x03` | `UpgradeBuilding` | `building_id: u32` | Улучшение постройки на следующий уровень. |
| `0x04` | `SellBuilding` | `building_id: u32` | Снос постройки с возвратом части Энергии. |
| `0x05` | `ArmyCommand` | `mode: u8`, `target_x: f32`, `target_y: f32` | `0 = Defend` (защита базы), `1 = Rally` (точка сбора/атаки). |
| `0x06` | `UpgradeStat` | `stat_id: u8` | Этап 2: прокачка одной из 6 характеристик за очки уровня. |
| `0x07` | `TeleportZone` | `target_x: f32`, `target_y: f32` | Этап 3A: разовый телепорт зоны с динамической ценой и кулдауном. |
| `0x08` | `Ping` | `client_time_ms: u32` | Замер RTT/пинга. |

---

## 3. Пакеты Сервер → Клиент (`S2C`)

| Opcode | Имя | Поля | Примечание |
| :--- | :--- | :--- | :--- |
| `0x81` | `Welcome` | `player_id: u32`, `map_radius: f32`, `tick_rate: u8` | Ответ на успешный `Join`. |
| `0x82` | `Snapshot` | `tick: u32`, свои статы (`energy: f32`, `xp: u32`, `level: u16`, `stat_points: u8`, `stat_levels: [u8; 6]`, `unit_count: u16`, `unit_cap: u16`), массивы зон, построек, юнитов, снарядов | Рассылается 20 раз в секунду. |
| `0x83` | `PlayerMeta` | `count: u16`, `[(player_id: u32, name: string, team_id: u8, skin_id: u16)]` | Отправляется при входе/выходе игроков, чтобы не гонять строки-ники каждый тик. |
| `0x84` | `MatchLeaderboard` | `count: u8`, `[(player_id: u32, score: u32)]` | Топ-10 текущего матча, раз в 1–2 секунды. |
| `0x85` | `GameOver` | `score: u32`, `survival_sec: u32`, `kills: u32`, `buildings_destroyed: u32`, `reason: u8` | Итоговая статистика забега при уничтожении Штаба. |
| `0x86` | `ActionError` | `code: u8` | Причина отказа действия (мало Энергии, занято место, вне зоны, лимит). |
| `0x87` | `Pong` | `client_time_ms: u32`, `server_tick: u32` | Ответ на `Ping`. |

### Детализация массивов в `0x82 Snapshot`
- **Зоны (`zones`):** `player_id: u32`, `x: f32`, `y: f32`, `radius: f32`, `team_id: u8`, `skin_id: u16`, `flags: u8` *(бит 0 = защита новичка)*.
- **Постройки (`buildings`):** `id: u32`, `owner_id: u32`, `kind: u8`, `level: u8`, `rel_x: f32`, `rel_y: f32`, `hp: u16`, `max_hp: u16`, `angle: u8` *(угол поворота турели `0..255`)*.
- **Юниты (`units`):** `id: u32`, `owner_id: u32`, `kind: u8`, `x: f32`, `y: f32`, `hp: u16`, `max_hp: u16`.
- **Снаряды (`projectiles`):** `id: u32`, `owner_id: u32`, `x: f32`, `y: f32`.

---

## 4. Структура `config/balance.json`

Конфиг баланса хранится в одном JSON-файле и используется как сервером, так и клиентом (для отображения цен, радиусов и описаний в UI):

- `world`: `map_radius`, `tick_rate`, `spawn_protection_sec`, `zone_base_radius`, `zone_repulsion_strength`, `sell_refund_ratio`.
- `progression`: `xp_per_level[]`, `stats` (коэффициенты прироста для 6 характеристик).
- `buildings`: словарь по типам (`hq`, `generator`, `turret`, `barracks`, `wall` и будущие уникальные блоки):
  - `kind_id`, `name`, `radius`, `max_per_player` (опционально)
  - `levels[]`: `cost`, `hp`, `income_per_sec`, `fire_rate`, `damage`, `range`, `projectile_speed`, `spawn_interval_sec`, `unit_cap_bonus`
  - `effects` / `aura` *(пусто в MVP, задел под Этап 3)*: `aura_radius`, `production_mult`, `fire_rate_mult`, `zone_radius_bonus`, `zone_speed_bonus`, `shield_hp`.
- `units`: словарь по типам (`melee`, и др.): `kind_id`, `radius`, `hp`, `damage`, `attack_cooldown_sec`, `speed`, `aggro_range`.
