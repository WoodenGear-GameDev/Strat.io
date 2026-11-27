export interface PublicUser {
  id: number;
  username: string;
  selected_skin_id: number;
  created_at: number;
}

export interface UserStats {
  games_played: number;
  best_score: number;
  total_score: number;
  total_kills: number;
  total_buildings_destroyed: number;
  total_survival_sec: number;
}

export interface MatchResult {
  id: number;
  mode: string;
  score: number;
  kills: number;
  buildings_destroyed: number;
  survival_sec: number;
  finished_at: number;
}

export interface AuthResponse {
  token: string;
  user: PublicUser;
}

export interface ProfileResponse {
  user: PublicUser;
  stats: UserStats | null;
  recent_matches?: MatchResult[];
}

export interface LeaderboardRow {
  username: string;
  selected_skin_id: number;
  games_played: number;
  best_score: number;
  total_score: number;
  total_kills: number;
}

const TOKEN_KEY = "stratio_token";

function apiBase(): string {
  return (import.meta.env.VITE_API_BASE_URL as string | undefined)?.replace(/\/$/, "") ?? "";
}

export function getStoredToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setStoredToken(token: string | null): void {
  if (token) localStorage.setItem(TOKEN_KEY, token);
  else localStorage.removeItem(TOKEN_KEY);
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${apiBase()}${path}`, init);
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new Error((data as { error?: string }).error ?? `HTTP ${res.status}`);
  }
  return data as T;
}

export function registerAccount(
  email: string,
  username: string,
  password: string,
): Promise<AuthResponse> {
  return apiFetch<AuthResponse>("/api/auth/register", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, username, password }),
  });
}

export function loginAccount(email: string, password: string): Promise<AuthResponse> {
  return apiFetch<AuthResponse>("/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
}

export function fetchMe(token: string): Promise<ProfileResponse> {
  return apiFetch<ProfileResponse>("/api/auth/me", {
    headers: { Authorization: `Bearer ${token}` },
  });
}

export function fetchUserProfile(username: string): Promise<ProfileResponse> {
  return apiFetch<ProfileResponse>(`/api/users/${encodeURIComponent(username)}`);
}

export function fetchLeaderboard(
  sort: "best_score" | "total_kills" | "total_score" = "best_score",
  limit = 50,
): Promise<LeaderboardRow[]> {
  return apiFetch<LeaderboardRow[]>(`/api/leaderboard?sort=${sort}&limit=${limit}`);
}
