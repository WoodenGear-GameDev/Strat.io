import {
  fetchLeaderboard,
  fetchMe,
  fetchUserProfile,
  getStoredToken,
  loginAccount,
  registerAccount,
  setStoredToken,
  type PublicUser,
} from "./game/api.ts";
import {
  getBuildingConfigByKind,
  getUnitConfigByKind,
  loadBalanceConfig,
} from "./game/balance.ts";
import { InputController } from "./game/input.ts";
import {
  decodeServerMessage,
  encodeConfigureBuilding,
  encodeJoin,
  encodePing,
  encodeSellBuilding,
  encodeUpgradeBuilding,
} from "./game/protocol.ts";
import { CanvasRenderer } from "./game/renderer.ts";
import { ClientGameState } from "./game/state.ts";
import { WsTransport } from "./transport/wsTransport.ts";

const ERROR_MESSAGES: Record<number, string> = {
  1: "Игрок не найден",
  2: "Недостаточно Энергии",
  3: "Стройка за пределами зоны",
  4: "Место занято другой постройкой",
  5: "Достигнут лимит построек",
  6: "Достигнут максимальный уровень",
  7: "Постройка не найдена",
  8: "Ядро нельзя продать",
  9: "Юнит ещё не открыт на этом уровне Дронов",
  11: "Телепорт на перезарядке",
  12: "Нельзя телепортироваться в ядро или за край",
  13: "Нельзя телепортироваться на чужую зону",
  14: "Маяк доступен с Ядра Ур. 3",
  15: "Недостаточно Трофеев",
  16: "Место разрушено — стройка заблокирована (8 с)",
  17: "В бою нельзя строить Маяки и улучшать до Ур. 3",
  18: "Телепорт возможен только в зоне взора (у дронов или базы)",
};

function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

const canvas = document.getElementById("game-canvas") as HTMLCanvasElement;
const lobbyOverlay = document.getElementById("lobby-overlay") as HTMLDivElement;
const hudOverlay = document.getElementById("hud-overlay") as HTMLDivElement;
const gameOverOverlay = document.getElementById("gameover-overlay") as HTMLDivElement;

const accountBar = document.getElementById("account-bar") as HTMLDivElement;
const accountAvatar = document.getElementById("account-avatar") as HTMLElement;
const accountUsername = document.getElementById("account-username") as HTMLElement;
const guestNameWrap = document.getElementById("guest-name-wrap") as HTMLDivElement;
const openProfileBtn = document.getElementById("open-profile-btn") as HTMLButtonElement;
const logoutBtn = document.getElementById("logout-btn") as HTMLButtonElement;
const nameInput = document.getElementById("player-name") as HTMLInputElement;
const playBtn = document.getElementById("play-btn") as HTMLButtonElement;
const openAuthBtn = document.getElementById("open-auth-btn") as HTMLButtonElement;
const openLbBtn = document.getElementById("open-lb-btn") as HTMLButtonElement;
const lobbyStatus = document.getElementById("lobby-status") as HTMLDivElement;

const replayBtn = document.getElementById("replay-btn") as HTMLButtonElement;
const goLobbyBtn = document.getElementById("go-lobby-btn") as HTMLButtonElement;

const hudEnergy = document.getElementById("hud-energy") as HTMLElement;
const hudTrophies = document.getElementById("hud-trophies") as HTMLElement;
const trophyChip = document.querySelector(".trophy-chip") as HTMLElement | null;
const hudHqBtn = document.getElementById("hud-hq-btn") as HTMLButtonElement;
const hudHqLevel = document.getElementById("hud-hq-level") as HTMLElement;
const hudHqReady = document.getElementById("hud-hq-ready") as HTMLElement;
const hudUnits = document.getElementById("hud-units") as HTMLElement;
const hudLeaderboard = document.getElementById("hud-leaderboard") as HTMLOListElement;
const debugToggleBtn = document.getElementById("debug-toggle-btn") as HTMLButtonElement;
const debugPanel = document.getElementById("debug-panel") as HTMLDivElement;
const hudTick = document.getElementById("hud-tick") as HTMLElement;
const hudFps = document.getElementById("hud-fps") as HTMLElement;
const hudTps = document.getElementById("hud-tps") as HTMLElement;
const hudPing = document.getElementById("hud-ping") as HTMLElement;
const hudEntities = document.getElementById("hud-entities") as HTMLElement;

let tpsStartTick = 0;
let tpsStartTime = 0;
let lastSnapTime = 0;

const buildingCard = document.getElementById("building-card") as HTMLDivElement;
const bcardName = document.getElementById("bcard-name") as HTMLElement;
const bcardLevel = document.getElementById("bcard-level") as HTMLElement;
const bcardHpFill = document.getElementById("bcard-hp-fill") as HTMLDivElement;
const bcardHpText = document.getElementById("bcard-hp-text") as HTMLElement;
const bcardBarracks = document.getElementById("bcard-barracks") as HTMLDivElement;
const bcardBarracksPause = document.getElementById("bcard-barracks-pause") as HTMLButtonElement;
const bcardUnitBtns = [
  document.getElementById("bcard-unit-0") as HTMLButtonElement,
  document.getElementById("bcard-unit-1") as HTMLButtonElement,
  document.getElementById("bcard-unit-2") as HTMLButtonElement,
];
const bcardUpgrade = document.getElementById("bcard-upgrade") as HTMLButtonElement;
const bcardUpgradeLabel = document.getElementById("bcard-upgrade-label") as HTMLElement;
const bcardSell = document.getElementById("bcard-sell") as HTMLButtonElement;
const bcardSellLabel = document.getElementById("bcard-sell-label") as HTMLElement;
const bcardClose = document.getElementById("bcard-close") as HTMLButtonElement;

const confirmBar = document.getElementById("confirm-bar") as HTMLDivElement;
const confirmOkBtn = document.getElementById("confirm-ok-btn") as HTMLButtonElement;
const confirmOkLabel = document.getElementById("confirm-ok-label") as HTMLElement;
const confirmCancelBtn = document.getElementById("confirm-cancel-btn") as HTMLButtonElement;

const btnActionUpgrade = document.getElementById("btn-action-upgrade") as HTMLButtonElement;
const btnActionSell = document.getElementById("btn-action-sell") as HTMLButtonElement;
const squadBar = document.getElementById("squad-bar") as HTMLDivElement;
const btnTeleport = document.getElementById("btn-teleport") as HTMLButtonElement;
const teleportLabel = document.getElementById("teleport-label") as HTMLElement;
const btnCenter = document.getElementById("btn-center") as HTMLButtonElement;
const camAnchorLabel = document.getElementById("cam-anchor-label") as HTMLElement;
const btnZoomIn = document.getElementById("btn-zoom-in") as HTMLButtonElement;
const btnZoomOut = document.getElementById("btn-zoom-out") as HTMLButtonElement;
const btnCancel = document.getElementById("btn-cancel") as HTMLButtonElement;
const toastEl = document.getElementById("toast") as HTMLDivElement;

const goScore = document.getElementById("go-score") as HTMLElement;
const goTime = document.getElementById("go-time") as HTMLElement;
const goKills = document.getElementById("go-kills") as HTMLElement;
const goBuildings = document.getElementById("go-buildings") as HTMLElement;

// Auth modal elements
const authModal = document.getElementById("auth-modal") as HTMLDivElement;
const tabLogin = document.getElementById("tab-login") as HTMLButtonElement;
const tabRegister = document.getElementById("tab-register") as HTMLButtonElement;
const authCloseBtn = document.getElementById("auth-close-btn") as HTMLButtonElement;
const authEmail = document.getElementById("auth-email") as HTMLInputElement;
const authUsername = document.getElementById("auth-username") as HTMLInputElement;
const authPassword = document.getElementById("auth-password") as HTMLInputElement;
const authSubmitBtn = document.getElementById("auth-submit-btn") as HTMLButtonElement;
const authError = document.getElementById("auth-error") as HTMLDivElement;

// Profile & Leaderboard modals
const profileModal = document.getElementById("profile-modal") as HTMLDivElement;
const profileTitle = document.getElementById("profile-title") as HTMLElement;
const profileContent = document.getElementById("profile-content") as HTMLDivElement;
const profileCloseBtn = document.getElementById("profile-close-btn") as HTMLButtonElement;

const lbModal = document.getElementById("lb-modal") as HTMLDivElement;
const lbContent = document.getElementById("lb-content") as HTMLDivElement;
const lbCloseBtn = document.getElementById("lb-close-btn") as HTMLButtonElement;

const state = new ClientGameState();
const renderer = new CanvasRenderer(canvas);
const transport = new WsTransport();

let currentUser: PublicUser | null = null;
let authMode: "login" | "register" = "login";
let currentLbSort: "best_score" | "total_kills" | "total_score" = "best_score";

let toastTimer: number | undefined;
function showToast(text: string): void {
  toastEl.textContent = text;
  toastEl.classList.remove("hidden");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toastEl.classList.add("hidden"), 2300);
}

function updateHqStatusUi(): void {
  const hqLevel = state.getHqLevel();
  hudHqLevel.textContent = String(hqLevel);
  const ready = state.canUpgradeHq();
  hudHqBtn.classList.toggle("hq-ready", ready);
  hudHqReady.classList.toggle("hidden", !ready);
  if (ready) {
    hudHqReady.textContent = `⬆ Ур.${hqLevel + 1}`;
  }
}

function getEffectiveBuildCost(kind: number): number {
  const cfg = getBuildingConfigByKind(state.balance, kind);
  if (!cfg) return 0;
  return cfg.levels[0]?.cost ?? 0;
}

function updateAccountUi(): void {
  if (currentUser) {
    accountBar.classList.remove("hidden");
    accountUsername.textContent = currentUser.username;
    accountAvatar.textContent = (currentUser.username.trim()[0] ?? "S").toUpperCase();
    guestNameWrap.classList.add("hidden");
    openAuthBtn.classList.add("hidden");
  } else {
    accountBar.classList.add("hidden");
    guestNameWrap.classList.remove("hidden");
    openAuthBtn.classList.remove("hidden");
  }
}

function updateToolbarUi(): void {
  const hqLevel = state.getHqLevel();
  const inCombat = state.isInCombat();
  const buildBtns = document.querySelectorAll<HTMLButtonElement>(".build-btn");
  for (const btn of buildBtns) {
    const kind = Number(btn.dataset.kind);
    const beaconLocked = kind === 5 && hqLevel < 3;
    const combatLocked = kind === 5 && !beaconLocked && inCombat;
    btn.disabled = beaconLocked;
    btn.classList.toggle("active", !beaconLocked && state.buildKind === kind);
    const c = getEffectiveBuildCost(kind);
    const tc = state.dynamicBuildTrophyCost(kind);
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
      ? "Маяк [4] (нужно Ядро Ур. 3)"
      : tc > 0
        ? `${getBuildingConfigByKind(state.balance, kind)?.name ?? "Здание"} [${kind === 5 ? 4 : kind}] (${c}⚡ + ${tc}⚔)`
        : `${getBuildingConfigByKind(state.balance, kind)?.name ?? "Здание"} [${kind === 5 ? 4 : kind}]`;
    btn.classList.toggle(
      "cant-afford",
      !beaconLocked && (combatLocked || (c > 0 && state.energy < c) || (tc > 0 && state.trophies < tc)),
    );
  }

  btnActionUpgrade.classList.toggle("active", state.actionMode === "upgrade");
  btnActionSell.classList.toggle("active", state.actionMode === "sell");
  btnTeleport.classList.toggle("active", state.teleportSelectMode);
  btnTeleport.disabled = state.teleportCdSec > 0;
  teleportLabel.textContent =
    state.teleportCdSec > 0 ? `${state.teleportCdSec}с` : "Телепорт";
  btnCenter.classList.toggle("active", state.cameraAnchor === "drones");
  camAnchorLabel.textContent =
    state.cameraAnchor === "drones" && state.activeSquad > 0
      ? `#${state.activeSquad}`
      : state.cameraAnchor === "drones"
        ? "Дроны"
        : "Ядро";
  btnCancel.disabled =
    state.buildKind === null &&
    state.actionMode === null &&
    !state.rallySelectMode &&
    !state.teleportSelectMode;

  renderSquadBar();

  if (state.pendingTeleport && state.teleportSelectMode) {
    const cost = state.computeTeleportCost(state.pendingTeleport.x, state.pendingTeleport.y);
    confirmOkBtn.disabled = state.energy < cost;
    confirmOkLabel.textContent = `Телепорт (${cost})`;
    confirmBar.classList.remove("hidden");
  } else {
    confirmOkBtn.disabled = false;
    confirmBar.classList.add("hidden");
  }

  if (state.selectedBuildingId !== null) {
    const b = state.buildings.get(state.selectedBuildingId);
    if (b) {
      const cfg = getBuildingConfigByKind(state.balance, b.kind);
      bcardName.textContent = cfg?.name ?? "Здание";
      bcardLevel.textContent = `Ур. ${b.level}`;

      const hpRatio = Math.max(0, Math.min(1, b.hp / Math.max(1, b.maxHp)));
      bcardHpFill.style.width = `${(hpRatio * 100).toFixed(1)}%`;
      bcardHpFill.classList.toggle("low", hpRatio < 0.4);
      bcardHpText.textContent = `${Math.ceil(b.hp)} / ${Math.ceil(b.maxHp)} HP`;

      const nextLvl = cfg?.levels[b.level];
      const tierBlocked = b.kind !== 0 && b.level >= hqLevel;
      const showBarracks = b.kind === 3;

      bcardBarracks.classList.toggle("hidden", !showBarracks);
      bcardUpgrade.classList.remove("hidden");

      if (showBarracks) {
        const isPaused = (b.meta & 0x01) !== 0;
        const unitType = (b.meta >> 2) & 0x07;
        bcardBarracksPause.textContent = isPaused ? "▶ Пуск" : "⏸ Пауза";
        bcardBarracksPause.classList.toggle("paused", isPaused);

        bcardUnitBtns.forEach((btn, idx) => {
          const uCfg = getUnitConfigByKind(state.balance, idx);
          if (uCfg) {
            const sup = uCfg.supply_cost ?? (idx === 2 ? 3 : 1);
            const uCost = Math.round(uCfg.cost ?? 0);
            const costStr = uCost > 0 ? ` ${uCost}⚡` : "";
            btn.textContent = sup > 1 ? `${uCfg.name}${costStr} [${sup}]` : `${uCfg.name}${costStr}`;
          }
          btn.classList.toggle("active", unitType === idx);
          btn.disabled = (idx === 1 && b.level < 2) || (idx === 2 && b.level < 3);
        });
      }

      if (tierBlocked) {
        bcardUpgrade.disabled = true;
        bcardUpgradeLabel.textContent = `Нужно Ядро Ур.${b.level + 1}`;
      } else if (nextLvl) {
        const tCost = nextLvl.trophy_cost ?? 0;
        const eCost = nextLvl.cost ?? 0;
        const combatBlockedL3 = inCombat && b.kind !== 0 && b.level + 1 >= 3;
        if (combatBlockedL3) {
          bcardUpgrade.disabled = true;
          bcardUpgradeLabel.textContent = "В бою (Ур. 3 блок)";
        } else if (tCost > 0 && eCost <= 0) {
          bcardUpgrade.disabled = state.trophies < tCost;
          bcardUpgradeLabel.textContent = `Улучшить (${tCost} ⚔)`;
        } else if (tCost > 0) {
          bcardUpgrade.disabled = state.energy < eCost || state.trophies < tCost;
          bcardUpgradeLabel.textContent = `Улучшить (${eCost}⚡/${tCost}⚔)`;
        } else {
          bcardUpgrade.disabled = state.energy < eCost;
          bcardUpgradeLabel.textContent = `Улучшить (${eCost}⚡)`;
        }
      } else {
        bcardUpgrade.disabled = true;
        bcardUpgradeLabel.textContent = "Макс. ур.";
      }

      bcardSell.classList.toggle("hidden", b.kind === 0);
      if (b.kind !== 0) {
        const eRef = state.computeSellRefund(b);
        const tRef = state.computeSellTrophyRefund(b);
        bcardSellLabel.textContent = tRef > 0 ? `Продать (+${eRef}⚡/+${tRef}⚔)` : `Продать (+${eRef}⚡)`;
      }

      buildingCard.classList.remove("hidden");
    } else {
      buildingCard.classList.add("hidden");
    }
  } else {
    buildingCard.classList.add("hidden");
  }
}

const input = new InputController(canvas, state, renderer, transport, {
  onUiChange: updateToolbarUi,
  onToast: showToast,
});

window.addEventListener("resize", () => renderer.resize());

debugToggleBtn.addEventListener("click", () => {
  const isHidden = debugPanel.classList.toggle("hidden");
  debugToggleBtn.classList.toggle("active", !isHidden);
  debugToggleBtn.setAttribute("aria-pressed", String(!isHidden));
});

hudHqBtn.addEventListener("click", () => {
  const hqs = state.collectBuildingsByKind(0);
  if (hqs.length === 0) return;
  const hq = hqs[0];
  if (state.canUpgradeHq()) {
    transport.send(encodeUpgradeBuilding(hq.id));
  } else {
    state.selectedBuildingId = hq.id;
    updateToolbarUi();
  }
});

for (const btn of document.querySelectorAll<HTMLButtonElement>(".build-btn")) {
  btn.addEventListener("click", () => {
    input.toggleBuild(Number(btn.dataset.kind));
  });
}

let lastSquadBarKey = "";
function renderSquadBar(): void {
  if (state.playerId === null) {
    if (lastSquadBarKey !== "none") {
      lastSquadBarKey = "none";
      squadBar.innerHTML = "";
    }
    return;
  }
  const list = state.getSquadSummaries();
  const key =
    `${state.cameraAnchor}:${state.activeSquad}:${state.reinforceSquad}|` +
    list.map((s) => `${s.squad}:${s.count}`).join(",");
  if (key === lastSquadBarKey) return;
  lastSquadBarKey = key;

  squadBar.innerHTML = "";
  let combatSlot = 0;
  for (const s of list) {
    const btn = document.createElement("button");
    btn.type = "button";
    const isCombat = s.squad > 0;
    const isSelected = isCombat
      ? state.cameraAnchor === "drones" && state.activeSquad === s.squad
      : state.cameraAnchor === "hq" || state.activeSquad === 0;
    btn.className = `squad-pill${isCombat ? " combat" : ""}${isSelected ? " active" : ""}`;
    if (isCombat) {
      const hk = combatSlot < 5 ? ` [${5 + combatSlot}]` : "";
      combatSlot++;
      const rf = state.reinforceSquad === s.squad ? " ↻" : "";
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
      input.selectSquad(s.squad);
    });
    squadBar.appendChild(btn);
  }

  if (state.activeSquad > 0) {
    const rfBtn = document.createElement("button");
    rfBtn.type = "button";
    const on = state.reinforceSquad === state.activeSquad;
    rfBtn.className = `squad-pill combat${on ? " active" : ""}`;
    rfBtn.textContent = on ? "↻ Подкреп: ВКЛ" : "↻ Подкреп";
    rfBtn.title = "Новые дроны из казарм автоматически летят к этому отряду";
    rfBtn.addEventListener("pointerdown", (e) => e.stopPropagation());
    rfBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      rfBtn.blur();
      input.toggleReinforce(state.activeSquad);
    });
    squadBar.appendChild(rfBtn);
  }
}

btnActionUpgrade.addEventListener("click", () => input.toggleActionMode("upgrade"));
btnActionSell.addEventListener("click", () => input.toggleActionMode("sell"));
btnTeleport.addEventListener("click", () => input.toggleTeleportMode());
btnCenter.addEventListener("click", () => input.centerOnBase());
btnZoomIn.addEventListener("click", () =>
  renderer.zoomAt(1.2, window.innerWidth / 2, window.innerHeight / 2),
);
btnZoomOut.addEventListener("click", () =>
  renderer.zoomAt(0.83, window.innerWidth / 2, window.innerHeight / 2),
);
btnCancel.addEventListener("click", () => input.cancelModes());

bcardUpgrade.addEventListener("click", () => {
  if (state.selectedBuildingId !== null) {
    transport.send(encodeUpgradeBuilding(state.selectedBuildingId));
  }
});

bcardBarracksPause.addEventListener("click", () => {
  if (state.selectedBuildingId === null) return;
  const b = state.buildings.get(state.selectedBuildingId);
  if (!b || b.kind !== 3) return;
  transport.send(encodeConfigureBuilding(b.id, b.meta ^ 0x01));
});

bcardUnitBtns.forEach((btn, idx) => {
  btn.addEventListener("click", () => {
    if (state.selectedBuildingId === null) return;
    const b = state.buildings.get(state.selectedBuildingId);
    if (!b || b.kind !== 3) return;
    const newMeta = (b.meta & ~0x1c) | ((idx & 0x07) << 2);
    transport.send(encodeConfigureBuilding(b.id, newMeta));
  });
});

bcardSell.addEventListener("click", () => {
  if (state.selectedBuildingId !== null) {
    transport.send(encodeSellBuilding(state.selectedBuildingId));
    state.selectedBuildingId = null;
    updateToolbarUi();
  }
});

bcardClose.addEventListener("click", () => {
  state.selectedBuildingId = null;
  updateToolbarUi();
});

confirmOkBtn.addEventListener("click", () => input.confirmPendingAction());
confirmCancelBtn.addEventListener("click", () => {
  state.pendingTeleport = null;
  updateToolbarUi();
});

// Auth Modal handlers
function setAuthMode(mode: "login" | "register"): void {
  authMode = mode;
  tabLogin.classList.toggle("active", mode === "login");
  tabRegister.classList.toggle("active", mode === "register");
  authUsername.classList.toggle("hidden", mode === "login");
  authSubmitBtn.textContent = mode === "login" ? "Войти" : "Создать аккаунт";
  authError.textContent = "";
}

openAuthBtn.addEventListener("click", () => {
  setAuthMode("login");
  authModal.classList.remove("hidden");
});
authCloseBtn.addEventListener("click", () => authModal.classList.add("hidden"));
authModal.addEventListener("click", (e) => {
  if (e.target === authModal) authModal.classList.add("hidden");
});
tabLogin.addEventListener("click", () => setAuthMode("login"));
tabRegister.addEventListener("click", () => setAuthMode("register"));

async function submitAuth(): Promise<void> {
  authError.textContent = "";
  try {
    const res =
      authMode === "login"
        ? await loginAccount(authEmail.value, authPassword.value)
        : await registerAccount(authEmail.value, authUsername.value, authPassword.value);
    setStoredToken(res.token);
    currentUser = res.user;
    updateAccountUi();
    authModal.classList.add("hidden");
  } catch (err) {
    authError.textContent = err instanceof Error ? err.message : "Ошибка авторизации";
  }
}

authSubmitBtn.addEventListener("click", () => void submitAuth());
authPassword.addEventListener("keydown", (e) => {
  if (e.key === "Enter") void submitAuth();
});

logoutBtn.addEventListener("click", () => {
  setStoredToken(null);
  currentUser = null;
  updateAccountUi();
});

// Profile Modal
async function openProfile(username: string): Promise<void> {
  profileTitle.textContent = `Профиль: ${username}`;
  profileContent.textContent = "Загрузка...";
  profileModal.classList.remove("hidden");
  try {
    const data = await fetchUserProfile(username);
    const s = data.stats;
    const matchesHtml =
      data.recent_matches && data.recent_matches.length > 0
        ? `<table class="data-table">
            <thead><tr><th>Очки</th><th>Убийств</th><th>Зданий</th><th>Время</th></tr></thead>
            <tbody>${data.recent_matches
              .map(
                (m) =>
                  `<tr><td><strong>${m.score}</strong></td><td>${m.kills}</td><td>${m.buildings_destroyed}</td><td>${m.survival_sec} с</td></tr>`,
              )
              .join("")}</tbody>
          </table>`
        : `<p class="status-text">Матчей пока нет</p>`;

    profileContent.innerHTML = `
      <div class="profile-metrics">
        <div class="go-tile"><span>Игр сыграно</span><strong>${s?.games_played ?? 0}</strong></div>
        <div class="go-tile"><span>Лучший счёт</span><strong>${s?.best_score ?? 0}</strong></div>
        <div class="go-tile"><span>Всего очков</span><strong>${s?.total_score ?? 0}</strong></div>
        <div class="go-tile"><span>Убито юнитов</span><strong>${s?.total_kills ?? 0}</strong></div>
        <div class="go-tile"><span>Снесено зданий</span><strong>${s?.total_buildings_destroyed ?? 0}</strong></div>
      </div>
      <h4>Последние матчи</h4>
      ${matchesHtml}
    `;
  } catch (err) {
    profileContent.textContent = err instanceof Error ? err.message : "Ошибка загрузки профиля";
  }
}

openProfileBtn.addEventListener("click", () => {
  if (currentUser) void openProfile(currentUser.username);
});
profileCloseBtn.addEventListener("click", () => profileModal.classList.add("hidden"));
profileModal.addEventListener("click", (e) => {
  if (e.target === profileModal) profileModal.classList.add("hidden");
});

// Global Leaderboard Modal
async function loadGlobalLeaderboard(): Promise<void> {
  lbContent.textContent = "Загрузка...";
  try {
    const rows = await fetchLeaderboard(currentLbSort, 50);
    if (rows.length === 0) {
      lbContent.textContent = "Лидерборд пока пуст";
      return;
    }
    lbContent.innerHTML = `
      <table class="data-table">
        <thead><tr><th>#</th><th>Игрок</th><th>Рекорд</th><th>Убийства</th><th>Игр</th></tr></thead>
        <tbody>${rows
          .map((r, i) => {
            const rankClass = i < 3 ? ` rank-${i + 1}` : "";
            const safeName = escapeHtml(r.username);
            return `<tr>
              <td><span class="rank-badge${rankClass}">${i + 1}</span></td>
              <td><button class="link-btn lb-user-link" data-user="${safeName}">${safeName}</button></td>
              <td><strong>${r.best_score}</strong></td>
              <td>${r.total_kills}</td>
              <td>${r.games_played}</td>
            </tr>`;
          })
          .join("")}</tbody>
      </table>
    `;
    for (const btn of lbContent.querySelectorAll<HTMLButtonElement>(".lb-user-link")) {
      btn.addEventListener("click", () => {
        const uname = btn.dataset.user;
        if (uname) void openProfile(uname);
      });
    }
  } catch (err) {
    lbContent.textContent = err instanceof Error ? err.message : "Ошибка загрузки лидерборда";
  }
}

openLbBtn.addEventListener("click", () => {
  lbModal.classList.remove("hidden");
  void loadGlobalLeaderboard();
});
lbCloseBtn.addEventListener("click", () => lbModal.classList.add("hidden"));
lbModal.addEventListener("click", (e) => {
  if (e.target === lbModal) lbModal.classList.add("hidden");
});
for (const btn of document.querySelectorAll<HTMLButtonElement>(".lb-sort-btn")) {
  btn.addEventListener("click", () => {
    for (const b of document.querySelectorAll(".lb-sort-btn")) b.classList.remove("active");
    btn.classList.add("active");
    currentLbSort = (btn.dataset.sort as typeof currentLbSort) || "best_score";
    void loadGlobalLeaderboard();
  });
}

function getWsUrl(): string {
  const envUrl = import.meta.env.VITE_GAME_WS_URL as string | undefined;
  if (envUrl) return envUrl;
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}/ws`;
}

function connectMatch(): void {
  lobbyStatus.textContent = "Подключение...";
  gameOverOverlay.classList.add("hidden");
  transport.connect(getWsUrl());
}

transport.onOpen(() => {
  lobbyStatus.textContent = "Вход в комнату...";
  const token = getStoredToken();
  const joinPayload =
    currentUser && token ? `jwt:${token}` : nameInput.value.trim() || "Guest";
  transport.send(encodeJoin(joinPayload, currentUser?.selected_skin_id ?? 0, 0));
});

transport.onMessage((bytes) => {
  const msg = decodeServerMessage(bytes);
  if (!msg) return;

  switch (msg.type) {
    case "Welcome":
      state.playerId = msg.playerId;
      state.mapRadius = msg.mapRadius;
      state.tickRate = msg.tickRate;
      renderer.resetCamera();
      lobbyOverlay.classList.add("hidden");
      gameOverOverlay.classList.add("hidden");
      hudOverlay.classList.remove("hidden");
      lobbyStatus.textContent = "";
      updateHqStatusUi();
      updateToolbarUi();
      break;

    case "PlayerMeta":
      state.playerNames.clear();
      for (const p of msg.players) {
        state.playerNames.set(p.playerId, p.name);
      }
      break;

    case "MatchLeaderboard":
      state.leaderboard = msg.entries;
      hudLeaderboard.innerHTML = "";
      msg.entries.slice(0, 5).forEach((entry, idx) => {
        const li = document.createElement("li");
        li.className = "lb-item";
        const isSelf = entry.playerId === state.playerId;
        if (isSelf) li.classList.add("is-self");

        const left = document.createElement("span");
        left.className = "lb-left";

        const rank = document.createElement("span");
        rank.className = "lb-rank";
        rank.textContent = String(idx + 1);

        const nameEl = document.createElement("span");
        nameEl.className = "lb-name";
        nameEl.textContent = state.playerNames.get(entry.playerId) ?? `#${entry.playerId}`;

        const scoreEl = document.createElement("span");
        scoreEl.className = "lb-score";
        scoreEl.textContent = String(entry.score);

        left.appendChild(rank);
        left.appendChild(nameEl);
        li.appendChild(left);
        li.appendChild(scoreEl);
        hudLeaderboard.appendChild(li);
      });
      break;

    case "Snapshot": {
      const snapNow = performance.now();
      lastSnapTime = snapNow;
      if (tpsStartTime === 0 || msg.tick < tpsStartTick) {
        tpsStartTick = msg.tick;
        tpsStartTime = snapNow;
      } else {
        const elapsedMs = snapNow - tpsStartTime;
        if (elapsedMs >= 1000) {
          const tps = Math.round(((msg.tick - tpsStartTick) * 1000) / elapsedMs);
          hudTps.textContent = String(tps);
          tpsStartTick = msg.tick;
          tpsStartTime = snapNow;
        }
      }
      state.applySnapshot(msg);
      const eCap = state.getEnergyCap();
      const tCap = state.getTrophyCap();
      hudEnergy.textContent = `${Math.floor(state.energy)} / ${Math.floor(eCap)}`;
      hudTrophies.textContent = `${Math.floor(state.trophies)} / ${Math.floor(tCap)}`;
      trophyChip?.classList.toggle("at-cap", state.trophies >= tCap - 1);
      updateHqStatusUi();
      hudUnits.textContent = `${state.unitCount} / ${state.unitCap}`;
      hudTick.textContent = state.tick.toString();
      hudEntities.textContent = String(
        state.buildings.size + state.units.size + state.projectiles.size,
      );
      updateToolbarUi();
      break;
    }

    case "ActionError":
      showToast(ERROR_MESSAGES[msg.code] ?? `Ошибка #${msg.code}`);
      break;

    case "Pong":
      state.pingMs = Math.max(0, Math.round(performance.now() - msg.clientTimeMs));
      hudPing.textContent = state.pingMs.toString();
      break;

    case "GameOver":
      state.reset();
      tpsStartTime = 0;
      lastSnapTime = 0;
      hudTps.textContent = "0";
      hudOverlay.classList.add("hidden");
      goScore.textContent = msg.score.toString();
      goTime.textContent = `${msg.survivalSec} с`;
      goKills.textContent = msg.kills.toString();
      goBuildings.textContent = msg.buildingsDestroyed.toString();
      gameOverOverlay.classList.remove("hidden");
      break;
  }
});

transport.onClose(() => {
  if (gameOverOverlay.classList.contains("hidden")) {
    state.reset();
    tpsStartTime = 0;
    lastSnapTime = 0;
    hudTps.textContent = "0";
    hudOverlay.classList.add("hidden");
    lobbyOverlay.classList.remove("hidden");
    lobbyStatus.textContent = "Соединение закрыто";
  }
});

playBtn.addEventListener("click", connectMatch);
nameInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") connectMatch();
});
replayBtn.addEventListener("click", connectMatch);
goLobbyBtn.addEventListener("click", () => {
  gameOverOverlay.classList.add("hidden");
  lobbyOverlay.classList.remove("hidden");
});

updateHqStatusUi();
updateToolbarUi();

loadBalanceConfig().then((cfg) => {
  state.balance = cfg;
  updateHqStatusUi();
  updateToolbarUi();
});

const savedToken = getStoredToken();
if (savedToken) {
  fetchMe(savedToken)
    .then((res) => {
      currentUser = res.user;
      updateAccountUi();
    })
    .catch(() => {
      setStoredToken(null);
      updateAccountUi();
    });
} else {
  updateAccountUi();
}

window.setInterval(() => {
  if (state.playerId !== null) {
    transport.send(encodePing(performance.now()));
  }
}, 2000);

let frames = 0;
let lastFpsTime = performance.now();
let lastFrameTime = performance.now();

function loop(now: number): void {
  const dt = Math.min(0.1, (now - lastFrameTime) / 1000);
  lastFrameTime = now;

  frames++;
  if (now - lastFpsTime >= 1000) {
    hudFps.textContent = frames.toString();
    frames = 0;
    lastFpsTime = now;
    if (state.playerId !== null && lastSnapTime > 0 && now - lastSnapTime > 1500) {
      hudTps.textContent = "0";
    }
  }

  state.stepEffects(dt);
  renderer.render(state, now);
  requestAnimationFrame(loop);
}

requestAnimationFrame(loop);
