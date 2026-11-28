import { getBuildingConfigByKind, getUnitConfigByKind } from "../game/balance.ts";
import type { InputController } from "../game/input.ts";
import {
  encodeConfigureBuilding,
  encodeSellBuilding,
  encodeToggleBaseRegen,
  encodeUpgradeBuilding,
  type LeaderboardEntry,
} from "../game/protocol.ts";
import type { CanvasRenderer } from "../game/renderer.ts";
import type { ClientGameState } from "../game/state.ts";
import type { WsTransport } from "../transport/wsTransport.ts";

export class HudController {
  private hudEnergy = document.getElementById("hud-energy") as HTMLElement;
  private hudEnergyRate = document.getElementById("hud-energy-rate") as HTMLElement;
  private hudTrophies = document.getElementById("hud-trophies") as HTMLElement;
  private trophyChip = document.querySelector(".trophy-chip") as HTMLElement | null;
  private hudHqBtn = document.getElementById("hud-hq-btn") as HTMLButtonElement;
  private hudHqLevel = document.getElementById("hud-hq-level") as HTMLElement;
  private hudUnits = document.getElementById("hud-units") as HTMLElement;
  private hudLeaderboard = document.getElementById("hud-leaderboard") as HTMLOListElement;
  private debugToggleBtn = document.getElementById("debug-toggle-btn") as HTMLButtonElement;
  private debugPanel = document.getElementById("debug-panel") as HTMLDivElement;
  private hudTick = document.getElementById("hud-tick") as HTMLElement;
  private hudFps = document.getElementById("hud-fps") as HTMLElement;
  private hudTps = document.getElementById("hud-tps") as HTMLElement;
  private hudPing = document.getElementById("hud-ping") as HTMLElement;
  private hudEntities = document.getElementById("hud-entities") as HTMLElement;

  private buildingCard = document.getElementById("building-card") as HTMLDivElement;
  private bcardName = document.getElementById("bcard-name") as HTMLElement;
  private bcardLevel = document.getElementById("bcard-level") as HTMLElement;
  private bcardHpFill = document.getElementById("bcard-hp-fill") as HTMLDivElement;
  private bcardHpText = document.getElementById("bcard-hp-text") as HTMLElement;
  private bcardHq = document.getElementById("bcard-hq") as HTMLDivElement;
  private bcardHqRegen = document.getElementById("bcard-hq-regen") as HTMLButtonElement;
  private bcardBarracks = document.getElementById("bcard-barracks") as HTMLDivElement;
  private bcardBarracksPause = document.getElementById("bcard-barracks-pause") as HTMLButtonElement;
  private bcardUnitBtns = [
    document.getElementById("bcard-unit-0") as HTMLButtonElement,
    document.getElementById("bcard-unit-1") as HTMLButtonElement,
    document.getElementById("bcard-unit-2") as HTMLButtonElement,
  ];
  private bcardUpgrade = document.getElementById("bcard-upgrade") as HTMLButtonElement;
  private bcardUpgradeLabel = document.getElementById("bcard-upgrade-label") as HTMLElement;
  private bcardSell = document.getElementById("bcard-sell") as HTMLButtonElement;
  private bcardSellLabel = document.getElementById("bcard-sell-label") as HTMLElement;
  private bcardClose = document.getElementById("bcard-close") as HTMLButtonElement;

  private confirmBar = document.getElementById("confirm-bar") as HTMLDivElement;
  private confirmOkBtn = document.getElementById("confirm-ok-btn") as HTMLButtonElement;
  private confirmOkLabel = document.getElementById("confirm-ok-label") as HTMLElement;
  private confirmCancelBtn = document.getElementById("confirm-cancel-btn") as HTMLButtonElement;

  private btnActionUpgrade = document.getElementById("btn-action-upgrade") as HTMLButtonElement;
  private btnActionSell = document.getElementById("btn-action-sell") as HTMLButtonElement;
  private squadBar = document.getElementById("squad-bar") as HTMLDivElement;
  private btnTeleport = document.getElementById("btn-teleport") as HTMLButtonElement;
  private teleportLabel = document.getElementById("teleport-label") as HTMLElement;
  private btnCenter = document.getElementById("btn-center") as HTMLButtonElement;
  private camAnchorLabel = document.getElementById("cam-anchor-label") as HTMLElement;
  private toastEl = document.getElementById("toast") as HTMLDivElement;

  private goScore = document.getElementById("go-score") as HTMLElement;
  private goTime = document.getElementById("go-time") as HTMLElement;
  private goKills = document.getElementById("go-kills") as HTMLElement;
  private goBuildings = document.getElementById("go-buildings") as HTMLElement;

  private toastTimer: number | undefined;
  private lastSquadBarKey = "";
  private input: InputController | null = null;

  constructor(
    private state: ClientGameState,
    private transport: WsTransport,
    _renderer: CanvasRenderer,
  ) {}

  bindControls(input: InputController): void {
    this.input = input;

    this.debugToggleBtn.addEventListener("click", () => {
      const isHidden = this.debugPanel.classList.toggle("hidden");
      this.debugToggleBtn.classList.toggle("active", !isHidden);
      this.debugToggleBtn.setAttribute("aria-pressed", String(!isHidden));
    });

    this.hudHqBtn.addEventListener("click", () => {
      const hqs = this.state.collectBuildingsByKind(0);
      if (hqs.length === 0) return;
      const hq = hqs[0];
      if (this.state.canUpgradeHq()) {
        this.transport.send(encodeUpgradeBuilding(hq.id));
      } else {
        this.state.selectedBuildingId = hq.id;
        this.updateToolbarUi();
      }
    });

    for (const btn of document.querySelectorAll<HTMLButtonElement>(".build-btn")) {
      btn.addEventListener("click", () => {
        input.toggleBuild(Number(btn.dataset.kind));
      });
    }

    this.btnActionUpgrade.addEventListener("click", () => input.toggleActionMode("upgrade"));
    this.btnActionSell.addEventListener("click", () => input.toggleActionMode("sell"));
    this.btnTeleport.addEventListener("click", () => input.toggleTeleportMode());
    this.btnCenter.addEventListener("click", () => input.centerOnBase());

    this.bcardUpgrade.addEventListener("click", () => {
      if (this.state.selectedBuildingId !== null) {
        this.transport.send(encodeUpgradeBuilding(this.state.selectedBuildingId));
      }
    });

    this.bcardHqRegen.addEventListener("click", () => {
      this.transport.send(encodeToggleBaseRegen());
    });

    this.bcardBarracksPause.addEventListener("click", () => {
      if (this.state.selectedBuildingId === null) return;
      const b = this.state.buildings.get(this.state.selectedBuildingId);
      if (!b || b.kind !== 3) return;
      this.transport.send(encodeConfigureBuilding(b.id, b.meta ^ 0x01));
    });

    this.bcardUnitBtns.forEach((btn, idx) => {
      btn.addEventListener("click", () => {
        if (this.state.selectedBuildingId === null) return;
        const b = this.state.buildings.get(this.state.selectedBuildingId);
        if (!b || b.kind !== 3) return;
        const newMeta = (b.meta & ~0x1c) | ((idx & 0x07) << 2);
        this.transport.send(encodeConfigureBuilding(b.id, newMeta));
      });
    });

    this.bcardSell.addEventListener("click", () => {
      if (this.state.selectedBuildingId !== null) {
        this.transport.send(encodeSellBuilding(this.state.selectedBuildingId));
        this.state.selectedBuildingId = null;
        this.updateToolbarUi();
      }
    });

    this.bcardClose.addEventListener("click", () => {
      this.state.selectedBuildingId = null;
      this.updateToolbarUi();
    });

    this.confirmOkBtn.addEventListener("click", () => input.confirmPendingAction());
    this.confirmCancelBtn.addEventListener("click", () => {
      this.state.pendingTeleport = null;
      this.updateToolbarUi();
    });
  }

  showToast(text: string): void {
    this.toastEl.textContent = text;
    this.toastEl.classList.remove("hidden");
    window.clearTimeout(this.toastTimer);
    this.toastTimer = window.setTimeout(() => this.toastEl.classList.add("hidden"), 2300);
  }

  updateHqStatusUi(): void {
    const hqLevel = this.state.getHqLevel();
    this.hudHqLevel.textContent = String(hqLevel);
    const ready = this.state.canUpgradeHq();
    this.hudHqBtn.classList.toggle("hq-ready", ready);
  }

  private getEffectiveBuildCost(kind: number): number {
    const cfg = getBuildingConfigByKind(this.state.balance, kind);
    if (!cfg) return 0;
    return cfg.levels[0]?.cost ?? 0;
  }

  updateToolbarUi(): void {
    const hqLevel = this.state.getHqLevel();
    const buildBtns = document.querySelectorAll<HTMLButtonElement>(".build-btn");
    for (const btn of buildBtns) {
      const kind = Number(btn.dataset.kind);
      const beaconLocked = kind === 5 && hqLevel < 3;
      btn.disabled = beaconLocked;
      btn.classList.toggle("active", !beaconLocked && this.state.buildKind === kind);
      const c = this.getEffectiveBuildCost(kind);
      const tc = this.state.dynamicBuildTrophyCost(kind);
      const costEl = document.getElementById(`cost-${kind}`);
      if (costEl) {
        if (beaconLocked) {
          costEl.textContent = "Ядро 3";
        } else if (tc > 0) {
          costEl.textContent = `${c}/${tc}⚔`;
        } else {
          costEl.textContent = c > 0 ? String(c) : "—";
        }
      }
      btn.title = beaconLocked
        ? "Узел [4] (нужно Ядро Ур. 3)"
        : tc > 0
          ? `${getBuildingConfigByKind(this.state.balance, kind)?.name ?? "Здание"} [${kind === 5 ? 4 : kind}] (${c}⚡ + ${tc}⚔)`
          : `${getBuildingConfigByKind(this.state.balance, kind)?.name ?? "Здание"} [${kind === 5 ? 4 : kind}]`;
      btn.classList.toggle(
        "cant-afford",
        !beaconLocked &&
          ((c > 0 && this.state.energy < c) || (tc > 0 && this.state.trophies < tc)),
      );
    }

    this.btnActionUpgrade.classList.toggle("active", this.state.actionMode === "upgrade");
    this.btnActionSell.classList.toggle("active", this.state.actionMode === "sell");
    this.btnTeleport.classList.toggle("active", this.state.teleportSelectMode);
    this.btnTeleport.disabled = this.state.teleportCdSec > 0;
    this.teleportLabel.textContent =
      this.state.teleportCdSec > 0 ? `${this.state.teleportCdSec}с` : "Телепорт";
    this.btnCenter.classList.toggle("active", this.state.cameraAnchor === "drones");
    this.camAnchorLabel.textContent =
      this.state.cameraAnchor === "drones" && this.state.activeSquad > 0
        ? `#${this.state.activeSquad}`
        : this.state.cameraAnchor === "drones"
          ? "Дроны"
          : "Ядро";

    this.renderSquadBar();

    if (this.state.pendingTeleport && this.state.teleportSelectMode) {
      const cost = this.state.computeTeleportCost(
        this.state.pendingTeleport.x,
        this.state.pendingTeleport.y,
      );
      this.confirmOkBtn.disabled = this.state.energy < cost;
      this.confirmOkLabel.textContent = `Телепорт (${cost})`;
      this.confirmBar.classList.remove("hidden");
    } else {
      this.confirmOkBtn.disabled = false;
      this.confirmBar.classList.add("hidden");
    }

    if (this.state.selectedBuildingId !== null) {
      const b = this.state.buildings.get(this.state.selectedBuildingId);
      if (b) {
        const cfg = getBuildingConfigByKind(this.state.balance, b.kind);
        this.bcardName.textContent = cfg?.name ?? "Здание";
        this.bcardLevel.textContent = `Ур. ${b.level}`;

        const hpRatio = Math.max(0, Math.min(1, b.hp / Math.max(1, b.maxHp)));
        this.bcardHpFill.style.width = `${(hpRatio * 100).toFixed(1)}%`;
        this.bcardHpFill.classList.toggle("low", hpRatio < 0.4);
        this.bcardHpText.textContent = `${Math.ceil(b.hp)} / ${Math.ceil(b.maxHp)} HP`;

        const nextLvl = cfg?.levels[b.level];
        const maxAllowedLvl = Math.min(3, hqLevel + 1);
        const tierBlocked = b.kind !== 0 && b.level >= maxAllowedLvl;
        const showHq = b.kind === 0;
        const showBarracks = b.kind === 3;

        this.bcardHq.classList.toggle("hidden", !showHq);
        this.bcardBarracks.classList.toggle("hidden", !showBarracks);
        this.bcardUpgrade.classList.remove("hidden");

        if (showHq) {
          const regenOn = this.state.isBaseRegenEnabled();
          this.bcardHqRegen.classList.toggle("active", regenOn);
          this.bcardHqRegen.textContent = regenOn ? "Реген базы: ВКЛ" : "Реген базы: ВЫКЛ";
        }

        if (showBarracks) {
          const isPaused = (b.meta & 0x01) !== 0;
          const unitType = (b.meta >> 2) & 0x07;
          this.bcardBarracksPause.textContent = isPaused ? "▶ Пуск" : "⏸ Пауза";
          this.bcardBarracksPause.classList.toggle("paused", isPaused);

          this.bcardUnitBtns.forEach((btn, idx) => {
            const uCfg = getUnitConfigByKind(this.state.balance, idx);
            if (uCfg) {
              const sup = uCfg.supply_cost ?? (idx === 2 ? 3 : 1);
              const uCost = Math.round(uCfg.cost ?? 0);
              const costStr = uCost > 0 ? ` ${uCost}⚡` : "";
              btn.textContent =
                sup > 1 ? `${uCfg.name}${costStr} [${sup}]` : `${uCfg.name}${costStr}`;
            }
            btn.classList.toggle("active", unitType === idx);
            btn.disabled = (idx === 1 && b.level < 2) || (idx === 2 && b.level < 3);
          });
        }

        if (tierBlocked) {
          this.bcardUpgrade.disabled = true;
          this.bcardUpgradeLabel.textContent = `Нужно Ядро Ур.${b.level}`;
        } else if (nextLvl) {
          const tCost = nextLvl.trophy_cost ?? 0;
          const eCost = nextLvl.cost ?? 0;
          if (tCost > 0 && eCost <= 0) {
            this.bcardUpgrade.disabled = this.state.trophies < tCost;
            this.bcardUpgradeLabel.textContent = `Улучшить (${tCost} ⚔)`;
          } else if (tCost > 0) {
            this.bcardUpgrade.disabled =
              this.state.energy < eCost || this.state.trophies < tCost;
            this.bcardUpgradeLabel.textContent = `Улучшить (${eCost}⚡/${tCost}⚔)`;
          } else {
            this.bcardUpgrade.disabled = this.state.energy < eCost;
            this.bcardUpgradeLabel.textContent = `Улучшить (${eCost}⚡)`;
          }
        } else {
          this.bcardUpgrade.disabled = true;
          this.bcardUpgradeLabel.textContent = "Макс. ур.";
        }

        this.bcardSell.classList.toggle("hidden", b.kind === 0);
        if (b.kind !== 0) {
          const eRef = this.state.computeSellRefund(b);
          const tRef = this.state.computeSellTrophyRefund(b);
          this.bcardSellLabel.textContent =
            tRef > 0 ? `Продать (+${eRef}⚡/+${tRef}⚔)` : `Продать (+${eRef}⚡)`;
        }

        this.buildingCard.classList.remove("hidden");
      } else {
        this.buildingCard.classList.add("hidden");
      }
    } else {
      this.buildingCard.classList.add("hidden");
    }
  }

  renderSquadBar(): void {
    if (this.state.playerId === null) {
      if (this.lastSquadBarKey !== "none") {
        this.lastSquadBarKey = "none";
        this.squadBar.innerHTML = "";
      }
      return;
    }
    const list = this.state.getSquadSummaries();
    const key =
      `${this.state.cameraAnchor}:${this.state.activeSquad}:${this.state.reinforceSquad}|` +
      list.map((s) => `${s.squad}:${s.count}`).join(",");
    if (key === this.lastSquadBarKey) return;
    this.lastSquadBarKey = key;

    this.squadBar.innerHTML = "";
    let combatSlot = 0;
    for (const s of list) {
      const btn = document.createElement("button");
      btn.type = "button";
      const isCombat = s.squad > 0;
      const isSelected = isCombat
        ? this.state.cameraAnchor === "drones" && this.state.activeSquad === s.squad
        : this.state.cameraAnchor === "hq" || this.state.activeSquad === 0;
      btn.className = `squad-pill${isCombat ? " combat" : ""}${isSelected ? " active" : ""}`;
      if (isCombat) {
        const hk = combatSlot < 5 ? ` [${5 + combatSlot}]` : "";
        combatSlot++;
        const rf = this.state.reinforceSquad === s.squad ? " ↻" : "";
        btn.textContent = `#${s.squad} · ${s.count}${rf}${hk}`;
        btn.title = `Отряд #${s.squad} [Q / E / Tab${hk}]`;
      } else {
        btn.textContent = `Ядро · ${s.count}`;
        btn.title = "Ядро / Гарнизон [~ / Пробел]";
      }
      btn.addEventListener("pointerdown", (e) => e.stopPropagation());
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        btn.blur();
        this.input?.selectSquad(s.squad);
      });
      this.squadBar.appendChild(btn);
    }

    if (this.state.activeSquad > 0) {
      const rfBtn = document.createElement("button");
      rfBtn.type = "button";
      const on = this.state.reinforceSquad === this.state.activeSquad;
      rfBtn.className = `squad-pill combat${on ? " active" : ""}`;
      rfBtn.textContent = on ? "↻ Подкреп: ВКЛ" : "↻ Подкреп";
      rfBtn.title = "Новые дроны из казарм автоматически летят к этому отряду";
      rfBtn.addEventListener("pointerdown", (e) => e.stopPropagation());
      rfBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        rfBtn.blur();
        this.input?.toggleReinforce(this.state.activeSquad);
      });
      this.squadBar.appendChild(rfBtn);
    }
  }

  updateMatchLeaderboard(entries: LeaderboardEntry[]): void {
    this.hudLeaderboard.innerHTML = "";
    entries.slice(0, 5).forEach((entry, idx) => {
      const li = document.createElement("li");
      li.className = "lb-item";
      const isSelf = entry.playerId === this.state.playerId;
      if (isSelf) li.classList.add("is-self");

      const left = document.createElement("span");
      left.className = "lb-left";

      const rank = document.createElement("span");
      rank.className = "lb-rank";
      rank.textContent = String(idx + 1);

      const nameEl = document.createElement("span");
      nameEl.className = "lb-name";
      nameEl.textContent = this.state.playerNames.get(entry.playerId) ?? `#${entry.playerId}`;

      const scoreEl = document.createElement("span");
      scoreEl.className = "lb-score";
      scoreEl.textContent = String(entry.score);

      left.appendChild(rank);
      left.appendChild(nameEl);
      li.appendChild(left);
      li.appendChild(scoreEl);
      this.hudLeaderboard.appendChild(li);
    });
  }

  updateSnapshotStats(): void {
    const eCap = this.state.getEnergyCap();
    let income = 0;
    if (this.state.playerId !== null) {
      for (const b of this.state.buildings.values()) {
        if (b.ownerId !== this.state.playerId) continue;
        const cfg = getBuildingConfigByKind(this.state.balance, b.kind);
        income += cfg?.levels[b.level - 1]?.income_per_sec ?? 0;
      }
    }
    this.hudEnergy.textContent = `${Math.floor(this.state.energy)} / ${Math.floor(eCap)}`;
    this.hudEnergyRate.textContent = `+${Math.round(income)}/с`;
    this.hudTrophies.textContent = `${Math.floor(this.state.trophies)}`;
    this.trophyChip?.classList.remove("at-cap");
    this.updateHqStatusUi();
    this.hudUnits.textContent = `${this.state.unitCount} / ${this.state.unitCap}`;
    this.hudTick.textContent = this.state.tick.toString();
    this.hudEntities.textContent = String(
      this.state.buildings.size + this.state.units.size + this.state.projectiles.size,
    );
    this.updateToolbarUi();
  }

  setFps(fps: number): void {
    this.hudFps.textContent = fps.toString();
  }

  setTps(tps: number): void {
    this.hudTps.textContent = String(tps);
  }

  setPing(pingMs: number): void {
    this.hudPing.textContent = pingMs.toString();
  }

  setGameOverStats(score: number, survivalSec: number, kills: number, buildingsDestroyed: number): void {
    this.goScore.textContent = score.toString();
    this.goTime.textContent = `${survivalSec} с`;
    this.goKills.textContent = kills.toString();
    this.goBuildings.textContent = buildingsDestroyed.toString();
  }
}
