#!/usr/bin/env python3
"""Stratio player-flow, economy, AI, and balance telemetry analyzer (.jsonl)."""

import json
import sys
from collections import defaultdict

ROLE_NAMES = {0: "Игрок/Сим", 1: "Бот Тир-1 (Малёк)", 2: "Бот Тир-2", 3: "Бот Тир-3"}
ARCH_NAMES = {
    0: "Старатель",
    1: "Дредноут",
    2: "Рейдер",
    3: "Сбаланс",
    4: "Архитектор",
}
PHASES = [
    ("0-1м", 0.0, 60.0),
    ("1-3м", 60.0, 180.0),
    ("3-6м", 180.0, 360.0),
    ("6-10м", 360.0, 600.0),
    ("10-15м", 600.0, 9999.0),
]


def phase_of(sec):
    for name, lo, hi in PHASES:
        if lo <= sec < hi:
            return name
    return PHASES[-1][0]


def strategy_of(name):
    if "-" in name and not any(x in name for x in ("Старатель", "Рейдер-1", "Рейдер-2", "Дредноут", "Архитектор", "Охотник")):
        parts = name.rsplit("-", 1)
        return parts[0]
    return name


def fmt_table(headers, rows):
    widths = [len(str(h)) for h in headers]
    for r in rows:
        for i, c in enumerate(r):
            widths[i] = max(widths[i], len(str(c)))
    sep = " | "
    out = [
        sep.join(str(h).ljust(widths[i]) for i, h in enumerate(headers)),
        "-+-".join("-" * w for w in widths),
    ]
    for r in rows:
        out.append(sep.join(str(c).ljust(widths[i]) for i, c in enumerate(r)))
    return "\n".join(out)


def load_runs(paths):
    runs = []
    for f_idx, p in enumerate(paths):
        events = []
        with open(p, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    events.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
        runs.append((f_idx, p, events))
    return runs


def analyze(paths):
    runs = load_runs(paths)
    if not runs:
        print("Нет данных.")
        return

    max_sec = max((e.get("sec", 0.0) for _, _, evs in runs for e in evs), default=0.0)
    print(f"=== АНАЛИЗ ФЛОУ И БАЛАНСА STRATIO ({len(runs)} прогон(а/ов), ~{max_sec/60:.1f} мин) ===")

    # Track per-(file, pid) profiles
    players_meta = {}  # (f, pid) -> dict
    player_snaps = defaultdict(list)  # (f, pid) -> [(sec, p)]
    player_actions = defaultdict(list)  # (f, pid) -> [(sec, cmd)]
    player_incomes = defaultdict(lambda: [0.0] * 5)  # (f, pid) -> [hq, mine, poi, pve, pvp]
    phase_income = defaultdict(lambda: [0.0] * 5)  # phase -> [hq, mine, poi, pve, pvp]
    phase_actions = defaultdict(int)  # (phase, cmd)
    phase_player_secs = defaultdict(float)  # phase -> sum of active player seconds

    checkpoints = [60, 180, 300, 600, 900]
    ring_by_cp = defaultdict(lambda: defaultdict(int))
    seen_cp = set()

    pair_agg = defaultdict(lambda: [0, 0])
    rad_agg = defaultdict(lambda: [0, 0])
    rally_matrix = defaultdict(int)
    deaths = []
    mutations = defaultdict(int)

    for f_idx, _, events in runs:
        for e in events:
            et = e.get("type")
            sec = e.get("sec", 0.0)
            ph = phase_of(sec)

            if et == "snapshot":
                cp_hit = next((cp for cp in checkpoints if abs(sec - cp) <= 2.6), None)
                for p in e.get("players", []):
                    key = (f_idx, p["id"])
                    players_meta[key] = {
                        "name": p["name"],
                        "is_bot": p["is_bot"],
                        "tier": p["tier"],
                        "arch": p["arch"],
                    }
                    player_snaps[key].append((sec, p))
                    if not p["is_bot"]:
                        phase_player_secs[ph] += 5.0
                    if cp_hit and (cp_hit, key) not in seen_cp:
                        seen_cp.add((cp_hit, key))
                        ring_by_cp[cp_hit][p["ring"]] += 1

            elif et == "action":
                key = (f_idx, e["pid"])
                cmd = e["cmd"]
                player_actions[key].append((sec, cmd))
                if not e.get("is_bot", True):
                    phase_actions[(ph, cmd)] += 1

            elif et == "income_window":
                for row in e.get("entries", []):
                    key = (f_idx, row["pid"])
                    vals = [row["hq"], row["mine"], row["poi"], row["pve"], row["pvp"]]
                    for i in range(5):
                        player_incomes[key][i] += vals[i]
                    if not players_meta.get(key, {}).get("is_bot", True):
                        for i in range(5):
                            phase_income[ph][i] += vals[i]

            elif et == "combat_window":
                for row in e.get("entries", []):
                    k = (row["src"], row["dst"])
                    pair_agg[k][0] += row["dmg"]
                    pair_agg[k][1] += row["kills"]
                    if row["src"] == "cradle_rad":
                        rad_agg[row["dst"]][0] += row["dmg"]
                        rad_agg[row["dst"]][1] += row["kills"]

            elif et == "milestone":
                if e.get("kind") == "bot_rally":
                    d = e["data"]
                    rally_matrix[(d["from_ring"], d["to_ring"])] += 1
                elif e.get("kind") == "mutation":
                    d = e["data"]
                    mutations[(d["bkind"], d["mutation"])] += 1

            elif et == "death":
                deaths.append((f_idx, sec, e["data"]))

    # 1. Player Flow by Phase (humans / test agents)
    print("\n--- 1. ФЛОУ ИГРОКОВ ПО ФАЗАМ (ИНТЕРАКТИВНОСТЬ И ПАУЗЫ) ---")
    ph_rows = []
    for ph_name, lo, hi in PHASES:
        pmins = phase_player_secs[ph_name] / 60.0
        if pmins <= 0.05:
            continue
        b_cnt = phase_actions[(ph_name, "build")]
        u_cnt = phase_actions[(ph_name, "upgrade")]
        r_cnt = phase_actions[(ph_name, "rally")] + phase_actions[(ph_name, "defend")]
        tp_cnt = phase_actions[(ph_name, "tp")]
        tot = b_cnt + u_cnt + r_cnt + tp_cnt

        # Average idle energy & zero-energy stall in phase
        snaps_in_ph = [
            p
            for key, plist in player_snaps.items()
            if not players_meta[key]["is_bot"]
            for s, p in plist
            if lo <= s < hi
        ]
        n_snaps = max(1, len(snaps_in_ph))
        low_e_pct = 100.0 * sum(1 for p in snaps_in_ph if p["energy"] < 55) / n_snaps
        hoard_e_pct = 100.0 * sum(1 for p in snaps_in_ph if p["energy"] >= 300 and p["hq_lvl"] >= 2) / n_snaps

        inc = phase_income[ph_name]
        inc_sum = max(1.0, sum(inc))
        inc_mix = f"Я:{100*inc[0]/inc_sum:.0f}% Д:{100*inc[1]/inc_sum:.0f}% U:{100*inc[2]/inc_sum:.0f}% P:{100*(inc[3]+inc[4])/inc_sum:.0f}%"

        ph_rows.append([
            ph_name,
            f"{tot/pmins:.1f}/м",
            f"{b_cnt/pmins:.1f}",
            f"{u_cnt/pmins:.1f}",
            f"{r_cnt/pmins:.1f}",
            f"{low_e_pct:.0f}%",
            f"{hoard_e_pct:.0f}%",
            inc_mix,
        ])
    if ph_rows:
        print(fmt_table(["Фаза", "Действ/м", "Строй", "Апгр", "Армия", "МалоЭн<55", "Залёж>=300", "Доход(Ядро/Дроны/Узлы/Бой)"], ph_rows))

    # 2. Strategy Viability Comparison (AFK Turtle vs Active Strategies!)
    print("\n--- 2. СРАВНЕНИЕ СТРАТЕГИЙ ИГРОКА (AFK ЧЕРЕПАХА vs АКТИВНАЯ ИГРА) ---")
    strat_agg = defaultdict(lambda: {
        "lives": 0, "deaths": 0, "sec": 0.0, "score": 0, "kills": 0, "hq3": 0,
        "acts": 0, "max_gaps": [], "avg_gaps": [], "inc": [0.0]*5, "max_r": 0
    })
    death_set = {(f, d["pid"]): d for f, _, d in deaths}

    for key, meta in players_meta.items():
        if meta["is_bot"]:
            continue
        st = strategy_of(meta["name"])
        s = strat_agg[st]
        s["lives"] += 1
        snaps = player_snaps[key]
        if snaps:
            life_sec = snaps[-1][0] - snaps[0][0] + 5.0
            s["sec"] += max(5.0, life_sec)
            s["score"] += max(p["score"] for _, p in snaps)
            s["kills"] += max(p["kills"] for _, p in snaps)
            s["max_r"] = max(s["max_r"], max(p["r"] for _, p in snaps))
            if any(p["hq_lvl"] >= 3 for _, p in snaps):
                s["hq3"] += 1
            acts = [t for t, _ in player_actions[key]]
            s["acts"] += len(acts)
            if len(acts) >= 2:
                gaps = [acts[i+1] - acts[i] for i in range(len(acts) - 1)]
                s["max_gaps"].append(max(gaps))
                s["avg_gaps"].append(sum(gaps) / len(gaps))
        if key in death_set:
            s["deaths"] += 1
        for i in range(5):
            s["inc"][i] += player_incomes[key][i]

    st_rows = []
    for st, d in sorted(strat_agg.items(), key=lambda kv: -(kv[1]["score"] / max(1.0, kv[1]["sec"] / 60.0))):
        mins = max(0.1, d["sec"] / 60.0)
        tot_inc = max(1.0, sum(d["inc"]))
        avg_gap = sum(d["avg_gaps"]) / len(d["avg_gaps"]) if d["avg_gaps"] else 0.0
        max_gap = max(d["max_gaps"]) if d["max_gaps"] else 0.0
        st_rows.append([
            st,
            f"{d['lives']}/{d['deaths']}",
            f"{d['sec']/max(1, d['lives']):.0f}с",
            round(d["score"] / mins, 1),
            round(d["kills"] / mins, 1),
            d["hq3"],
            f"{d['acts']/mins:.1f}",
            f"{avg_gap:.0f}/{max_gap:.0f}с",
            f"{100*d['inc'][2]/tot_inc:.0f}% / {100*(d['inc'][3]+d['inc'][4])/tot_inc:.0f}%",
        ])
    if st_rows:
        print(fmt_table(["Стратегия", "Жиз/См", "Ср.жизнь", "Очки/м", "Килл/м", "HQ3", "Дейст/м", "Пауза(ср/макс)", "Доход(POI/Бой)"], st_rows))

    # 3. Participant tiers & archetypes (all participants)
    print("\n--- 3. ВСЕ УЧАСТНИКИ ПО ТИРАМ (БОТЫ + СИМ) ---")
    role_stats = defaultdict(lambda: {"n": 0, "score": 0, "kills": 0, "hq2": 0, "hq3": 0, "deaths": 0})
    for key, snaps in player_snaps.items():
        meta = players_meta.get(key, {})
        tier_k = meta.get("tier", 1) if meta.get("is_bot") else 0
        st = role_stats[tier_k]
        st["n"] += 1
        st["score"] += max((p["score"] for _, p in snaps), default=0)
        st["kills"] += max((p["kills"] for _, p in snaps), default=0)
        if any(p["hq_lvl"] >= 2 for _, p in snaps):
            st["hq2"] += 1
        if any(p["hq_lvl"] >= 3 for _, p in snaps):
            st["hq3"] += 1
        if key in death_set:
            st["deaths"] += 1
    role_rows = []
    for rk, st in sorted(role_stats.items()):
        n = max(1, st["n"])
        role_rows.append([
            ROLE_NAMES.get(rk, f"Tier{rk}"),
            st["n"],
            round(st["score"] / n, 1),
            round(st["kills"] / n, 1),
            st["hq2"],
            st["hq3"],
            st["deaths"],
        ])
    print(fmt_table(["Группа", "Кол-во", "Ср.очки", "Ср.киллы", "HQ2+", "HQ3", "Смерти"], role_rows))

    # 4. Ring progression
    print("\n--- 4. РАСПРЕДЕЛЕНИЕ ПО КОЛЬЦАМ (СУММА ПО ПРОГОНАМ) ---")
    cp_rows = []
    for cp in checkpoints:
        c = ring_by_cp.get(cp)
        if c:
            cp_rows.append([f"{cp//60} мин", c[1], c[2], c[3]])
    if cp_rows:
        print(fmt_table(["Время", "Ring 1", "Ring 2", "Ring 3"], cp_rows))

    # 5. Combat matrix (top 18)
    print("\n--- 5. БОЕВАЯ МАТРИЦА (ТОП-18 ПО УРОНУ) ---")
    c_rows = [
        [src, "->", dst, dmg, kills]
        for (src, dst), (dmg, kills) in sorted(pair_agg.items(), key=lambda x: -x[1][0])[:18]
    ]
    if c_rows:
        print(fmt_table(["Источник", "", "Цель", "Урон", "Киллов"], c_rows))

    # 6. Radiation & Ring 1 deaths
    print("\n--- 6. ДАВЛЕНИЕ НА RING 1, РАДИАЦИЯ И РЕЙДЫ ---")
    if rad_agg:
        r_rows = [[dst, v[0], v[1]] for dst, v in sorted(rad_agg.items(), key=lambda x: -x[1][0])]
        print(fmt_table(["Жертва радиации", "Урон", "Смертей"], r_rows))
    if rally_matrix:
        print("Рейды ботов (from -> to):", dict(sorted(rally_matrix.items())))
    if mutations:
        print("Взятые Lv3 мутации {(bkind, mut): count}:", dict(sorted(mutations.items())))

    early60 = sum(1 for _, _, d in deaths if d.get("survival_sec", 999) < 60)
    early120 = sum(1 for _, _, d in deaths if d.get("survival_sec", 999) < 120)
    r1_deaths = [d for _, _, d in deaths if d.get("ring") == 1]
    r1_by_high = sum(1 for d in r1_deaths if (d.get("killer_ring") or 1) >= 2)
    print(
        f"Смертей Ядра: {len(deaths)} | <60с: {early60} | <120с: {early120} | "
        f"В Ring 1: {len(r1_deaths)} (от Ring 2/3: {r1_by_high})"
    )


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python scripts/analyze_balance.py <file1.jsonl> [file2.jsonl ...]")
        sys.exit(1)
    analyze(sys.argv[1:])
