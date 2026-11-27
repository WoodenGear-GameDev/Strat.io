import { loadBalanceConfig } from "./game/balance.ts";
import { InputController } from "./game/input.ts";
import { decodeServerMessage, encodeJoin, encodePing } from "./game/protocol.ts";
import { CanvasRenderer } from "./game/renderer.ts";
import { ClientGameState } from "./game/state.ts";
import { WsTransport } from "./transport/wsTransport.ts";
import { AccountController } from "./ui/account.ts";
import { HudController } from "./ui/hud.ts";

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

const canvas = document.getElementById("game-canvas") as HTMLCanvasElement;
const lobbyOverlay = document.getElementById("lobby-overlay") as HTMLDivElement;
const hudOverlay = document.getElementById("hud-overlay") as HTMLDivElement;
const gameOverOverlay = document.getElementById("gameover-overlay") as HTMLDivElement;

const nameInput = document.getElementById("player-name") as HTMLInputElement;
const playBtn = document.getElementById("play-btn") as HTMLButtonElement;
const lobbyStatus = document.getElementById("lobby-status") as HTMLDivElement;

const replayBtn = document.getElementById("replay-btn") as HTMLButtonElement;
const goLobbyBtn = document.getElementById("go-lobby-btn") as HTMLButtonElement;

const state = new ClientGameState();
const renderer = new CanvasRenderer(canvas);
const transport = new WsTransport();
const account = new AccountController();
const hud = new HudController(state, transport, renderer);

const input = new InputController(canvas, state, renderer, transport, {
  onUiChange: () => hud.updateToolbarUi(),
  onToast: (text) => hud.showToast(text),
});
hud.bindControls(input);

let tpsStartTick = 0;
let tpsStartTime = 0;
let lastSnapTime = 0;

window.addEventListener("resize", () => renderer.resize());

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
  const { payload, skinId } = account.getJoinPayload(nameInput.value);
  transport.send(encodeJoin(payload, skinId, 0));
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
      hud.updateHqStatusUi();
      hud.updateToolbarUi();
      break;

    case "PlayerMeta":
      state.playerNames.clear();
      for (const p of msg.players) {
        state.playerNames.set(p.playerId, p.name);
      }
      break;

    case "MatchLeaderboard":
      state.leaderboard = msg.entries;
      hud.updateMatchLeaderboard(msg.entries);
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
          hud.setTps(tps);
          tpsStartTick = msg.tick;
          tpsStartTime = snapNow;
        }
      }
      state.applySnapshot(msg);
      hud.updateSnapshotStats();
      break;
    }

    case "ActionError":
      hud.showToast(ERROR_MESSAGES[msg.code] ?? `Ошибка #${msg.code}`);
      break;

    case "Pong":
      state.pingMs = Math.max(0, Math.round(performance.now() - msg.clientTimeMs));
      hud.setPing(state.pingMs);
      break;

    case "GameOver":
      state.reset();
      tpsStartTime = 0;
      lastSnapTime = 0;
      hud.setTps(0);
      hudOverlay.classList.add("hidden");
      hud.setGameOverStats(msg.score, msg.survivalSec, msg.kills, msg.buildingsDestroyed);
      gameOverOverlay.classList.remove("hidden");
      break;
  }
});

transport.onClose(() => {
  if (gameOverOverlay.classList.contains("hidden")) {
    state.reset();
    tpsStartTime = 0;
    lastSnapTime = 0;
    hud.setTps(0);
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

hud.updateHqStatusUi();
hud.updateToolbarUi();

loadBalanceConfig().then((cfg) => {
  state.balance = cfg;
  hud.updateHqStatusUi();
  hud.updateToolbarUi();
});

account.restoreSession();

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
    hud.setFps(frames);
    frames = 0;
    lastFpsTime = now;
    if (state.playerId !== null && lastSnapTime > 0 && now - lastSnapTime > 1500) {
      hud.setTps(0);
    }
  }

  state.stepEffects(dt);
  renderer.render(state, now);
  requestAnimationFrame(loop);
}

requestAnimationFrame(loop);
