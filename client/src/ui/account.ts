import {
  fetchLeaderboard,
  fetchMe,
  fetchUserProfile,
  getStoredToken,
  loginAccount,
  registerAccount,
  setStoredToken,
  type PublicUser,
} from "../game/api.ts";

function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export class AccountController {
  private accountBar = document.getElementById("account-bar") as HTMLDivElement;
  private accountAvatar = document.getElementById("account-avatar") as HTMLElement;
  private accountUsername = document.getElementById("account-username") as HTMLElement;
  private guestNameWrap = document.getElementById("guest-name-wrap") as HTMLDivElement;
  private openProfileBtn = document.getElementById("open-profile-btn") as HTMLButtonElement;
  private logoutBtn = document.getElementById("logout-btn") as HTMLButtonElement;
  private openAuthBtn = document.getElementById("open-auth-btn") as HTMLButtonElement;
  private openLbBtn = document.getElementById("open-lb-btn") as HTMLButtonElement;

  private authModal = document.getElementById("auth-modal") as HTMLDivElement;
  private tabLogin = document.getElementById("tab-login") as HTMLButtonElement;
  private tabRegister = document.getElementById("tab-register") as HTMLButtonElement;
  private authCloseBtn = document.getElementById("auth-close-btn") as HTMLButtonElement;
  private authEmail = document.getElementById("auth-email") as HTMLInputElement;
  private authUsername = document.getElementById("auth-username") as HTMLInputElement;
  private authPassword = document.getElementById("auth-password") as HTMLInputElement;
  private authSubmitBtn = document.getElementById("auth-submit-btn") as HTMLButtonElement;
  private authError = document.getElementById("auth-error") as HTMLDivElement;

  private profileModal = document.getElementById("profile-modal") as HTMLDivElement;
  private profileTitle = document.getElementById("profile-title") as HTMLElement;
  private profileContent = document.getElementById("profile-content") as HTMLDivElement;
  private profileCloseBtn = document.getElementById("profile-close-btn") as HTMLButtonElement;

  private lbModal = document.getElementById("lb-modal") as HTMLDivElement;
  private lbContent = document.getElementById("lb-content") as HTMLDivElement;
  private lbCloseBtn = document.getElementById("lb-close-btn") as HTMLButtonElement;

  currentUser: PublicUser | null = null;
  private authMode: "login" | "register" = "login";
  private currentLbSort: "best_score" | "total_kills" | "total_score" = "best_score";

  constructor() {
    this.bindEvents();
  }

  private bindEvents(): void {
    this.openAuthBtn.addEventListener("click", () => {
      this.setAuthMode("login");
      this.authModal.classList.remove("hidden");
    });
    this.authCloseBtn.addEventListener("click", () => this.authModal.classList.add("hidden"));
    this.authModal.addEventListener("click", (e) => {
      if (e.target === this.authModal) this.authModal.classList.add("hidden");
    });
    this.tabLogin.addEventListener("click", () => this.setAuthMode("login"));
    this.tabRegister.addEventListener("click", () => this.setAuthMode("register"));

    this.authSubmitBtn.addEventListener("click", () => void this.submitAuth());
    this.authPassword.addEventListener("keydown", (e) => {
      if (e.key === "Enter") void this.submitAuth();
    });

    this.logoutBtn.addEventListener("click", () => {
      setStoredToken(null);
      this.currentUser = null;
      this.updateAccountUi();
    });

    this.openProfileBtn.addEventListener("click", () => {
      if (this.currentUser) void this.openProfile(this.currentUser.username);
    });
    this.profileCloseBtn.addEventListener("click", () => this.profileModal.classList.add("hidden"));
    this.profileModal.addEventListener("click", (e) => {
      if (e.target === this.profileModal) this.profileModal.classList.add("hidden");
    });

    this.openLbBtn.addEventListener("click", () => {
      this.lbModal.classList.remove("hidden");
      void this.loadGlobalLeaderboard();
    });
    this.lbCloseBtn.addEventListener("click", () => this.lbModal.classList.add("hidden"));
    this.lbModal.addEventListener("click", (e) => {
      if (e.target === this.lbModal) this.lbModal.classList.add("hidden");
    });
    for (const btn of document.querySelectorAll<HTMLButtonElement>(".lb-sort-btn")) {
      btn.addEventListener("click", () => {
        for (const b of document.querySelectorAll(".lb-sort-btn")) b.classList.remove("active");
        btn.classList.add("active");
        this.currentLbSort = (btn.dataset.sort as typeof this.currentLbSort) || "best_score";
        void this.loadGlobalLeaderboard();
      });
    }
  }

  restoreSession(): void {
    const savedToken = getStoredToken();
    if (savedToken) {
      fetchMe(savedToken)
        .then((res) => {
          this.currentUser = res.user;
          this.updateAccountUi();
        })
        .catch(() => {
          setStoredToken(null);
          this.updateAccountUi();
        });
    } else {
      this.updateAccountUi();
    }
  }

  getJoinPayload(guestName: string): { payload: string; skinId: number } {
    const token = getStoredToken();
    const payload =
      this.currentUser && token ? `jwt:${token}` : guestName.trim() || "Guest";
    return {
      payload,
      skinId: this.currentUser?.selected_skin_id ?? 0,
    };
  }

  updateAccountUi(): void {
    if (this.currentUser) {
      this.accountBar.classList.remove("hidden");
      this.accountUsername.textContent = this.currentUser.username;
      this.accountAvatar.textContent = (this.currentUser.username.trim()[0] ?? "S").toUpperCase();
      this.guestNameWrap.classList.add("hidden");
      this.openAuthBtn.classList.add("hidden");
    } else {
      this.accountBar.classList.add("hidden");
      this.guestNameWrap.classList.remove("hidden");
      this.openAuthBtn.classList.remove("hidden");
    }
  }

  private setAuthMode(mode: "login" | "register"): void {
    this.authMode = mode;
    this.tabLogin.classList.toggle("active", mode === "login");
    this.tabRegister.classList.toggle("active", mode === "register");
    this.authUsername.classList.toggle("hidden", mode === "login");
    this.authSubmitBtn.textContent = mode === "login" ? "Войти" : "Создать аккаунт";
    this.authError.textContent = "";
  }

  private async submitAuth(): Promise<void> {
    this.authError.textContent = "";
    try {
      const res =
        this.authMode === "login"
          ? await loginAccount(this.authEmail.value, this.authPassword.value)
          : await registerAccount(
              this.authEmail.value,
              this.authUsername.value,
              this.authPassword.value,
            );
      setStoredToken(res.token);
      this.currentUser = res.user;
      this.updateAccountUi();
      this.authModal.classList.add("hidden");
    } catch (err) {
      this.authError.textContent = err instanceof Error ? err.message : "Ошибка авторизации";
    }
  }

  private async openProfile(username: string): Promise<void> {
    this.profileTitle.textContent = `Профиль: ${username}`;
    this.profileContent.textContent = "Загрузка...";
    this.profileModal.classList.remove("hidden");
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

      this.profileContent.innerHTML = `
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
      this.profileContent.textContent = err instanceof Error ? err.message : "Ошибка загрузки профиля";
    }
  }

  private async loadGlobalLeaderboard(): Promise<void> {
    this.lbContent.textContent = "Загрузка...";
    try {
      const rows = await fetchLeaderboard(this.currentLbSort, 50);
      if (rows.length === 0) {
        this.lbContent.textContent = "Лидерборд пока пуст";
        return;
      }
      this.lbContent.innerHTML = `
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
      for (const btn of this.lbContent.querySelectorAll<HTMLButtonElement>(".lb-user-link")) {
        btn.addEventListener("click", () => {
          const uname = btn.dataset.user;
          if (uname) void this.openProfile(uname);
        });
      }
    } catch (err) {
      this.lbContent.textContent = err instanceof Error ? err.message : "Ошибка загрузки лидерборда";
    }
  }
}
