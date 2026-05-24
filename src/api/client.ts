// ── Base API Client ────────────────────────────────────────────────────────
// Server URL is read from localStorage so Settings page can update it.

const DEFAULT_SERVER = 'http://localhost:8089';
const STATUS_PATH_CANDIDATES = ['/api/v1/status', '/api/status', '/status'] as const;
const COMPAT_MATRIX_PATH = '.github/api-version-compatibility.json';

export interface ServerVersionInfo {
  serverVersion: string | null;
  publicApiVersion: string | null;
}

export interface CompatibilityMatrixEntry {
  serverVersion: string;
  verified: boolean;
  testedOn?: string;
  notes?: string;
}

export interface CompatibilityMatrixFile {
  schemaVersion: number;
  entries: CompatibilityMatrixEntry[];
}

/**
 * Normalize a server URL: add http:// scheme if missing, fix single-slash
 * (http:/host → http://host), remove trailing slash.
 */
export function normalizeServerUrl(raw: string): string {
  let url = raw.trim();
  if (!url) return url;
  // Fix single-slash: http:/host → http://host
  url = url.replace(/^(https?:\/(?!\/))/, '$1/');
  // Add scheme if missing entirely
  if (!/^https?:\/\//.test(url)) url = 'http://' + url;
  // Remove trailing slash
  return url.replace(/\/$/, '');
}

export function getServerUrl(): string {
  const stored = localStorage.getItem('dvr_server_url');
  return stored ? normalizeServerUrl(stored) : DEFAULT_SERVER;
}

export function setServerUrl(url: string): void {
  localStorage.setItem('dvr_server_url', normalizeServerUrl(url));
}

/**
 * Returns true if the server at `url` responds within `timeoutMs`.
 * Uses lightweight status endpoint candidates.
 * Any HTTP response (even 4xx) counts as reachable — only a network error or
 * timeout counts as unreachable.
 */
export async function probeUrl(url: string, timeoutMs = 2500): Promise<boolean> {
  const normalized = normalizeServerUrl(url);
  const fetchProbe = (async () => {
    for (const path of STATUS_PATH_CANDIDATES) {
      try {
        await httpFetch(`${normalized}${path}`);
        return true;
      } catch {
        // Try next candidate.
      }
    }
    return false;
  })();
  const timeout = new Promise<false>((resolve) => setTimeout(() => resolve(false), timeoutMs));
  return Promise.race([fetchProbe, timeout]);
}

// Use Tauri's HTTP plugin fetch so requests go through Rust (bypasses CORS).
// Falls back to window.fetch when running outside Tauri (e.g. browser dev).
async function httpFetch(url: string, init?: RequestInit): Promise<Response> {
  if (window.__TAURI_INTERNALS__) {
    const { fetch: tauriFetch } = await import('@tauri-apps/plugin-http');
    return tauriFetch(url, init);
  }
  return fetch(url, init);
}

function extractVersionInfo(status: Record<string, unknown>): ServerVersionInfo {
  const serverRaw = status['version'] ?? status['Version'] ?? status['build'] ?? null;
  const publicRaw = status['api_version'] ?? status['apiVersion'] ?? status['public_api_version'] ?? status['publicApiVersion'] ?? null;
  return {
    serverVersion: typeof serverRaw === 'string' && serverRaw.trim().length > 0 ? serverRaw.trim() : null,
    publicApiVersion: typeof publicRaw === 'string' && publicRaw.trim().length > 0 ? publicRaw.trim() : null,
  };
}

function repositoryRawCompatibilityUrl(): string {
  // __APP_REPOSITORY_URL__ is expected to be like https://github.com/<owner>/<repo>
  const match = __APP_REPOSITORY_URL__.match(/github\.com\/([^/]+)\/([^/]+)/i);
  if (!match) {
    return `https://raw.githubusercontent.com/jay3702/dvrdesk/main/${COMPAT_MATRIX_PATH}`;
  }
  const owner = match[1];
  const repo = match[2].replace(/\.git$/i, '');
  return `https://raw.githubusercontent.com/${owner}/${repo}/main/${COMPAT_MATRIX_PATH}`;
}

export async function requestFromServer<T>(
  serverUrl: string,
  path: string,
  params?: Record<string, string>
): Promise<T> {
  return requestFromServerWithInit<T>(serverUrl, path, { params, method: 'GET' });
}

interface RequestWithInitOptions {
  method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  params?: Record<string, string>;
  body?: unknown;
  bodyEncoding?: 'json' | 'form';
}

async function requestFromServerWithInit<T>(
  serverUrl: string,
  path: string,
  options: RequestWithInitOptions
): Promise<T> {
  const base = normalizeServerUrl(serverUrl);
  const url = new URL(path, base);
  if (options.params) {
    Object.entries(options.params).forEach(([k, v]) => url.searchParams.set(k, v));
  }

  const init: RequestInit = { method: options.method };
  if (options.body !== undefined) {
    if (options.bodyEncoding === 'form') {
      const form = new URLSearchParams();
      for (const [key, value] of Object.entries(options.body as Record<string, unknown>)) {
        form.set(key, String(value));
      }
      init.headers = { 'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8' };
      init.body = form.toString();
    } else {
      init.headers = { 'Content-Type': 'application/json' };
      init.body = JSON.stringify(options.body);
    }
  }

  let res: Response;
  try {
    res = await httpFetch(url.toString(), init);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    throw new Error(`Network error reaching ${url.host}: ${msg}`);
  }
  if (!res.ok) {
    throw new Error(`API error ${res.status}: ${options.method} ${url.pathname}`);
  }

  let data: T;
  if (res.status === 204) {
    data = undefined as T;
  } else {
    const contentType = (res.headers.get('content-type') ?? '').toLowerCase();
    if (contentType.includes('application/json')) {
      data = await res.json() as T;
    } else {
      data = await res.text() as T;
    }
  }

  console.debug(`[API] ${url.pathname}`, data);
  return data;
}

async function request<T>(path: string, params?: Record<string, string>): Promise<T> {
  return requestFromServer<T>(getServerUrl(), path, params);
}

export async function requestWithMethod<T = unknown>(
  path: string,
  method: 'POST' | 'PUT' | 'PATCH' | 'DELETE',
  body?: unknown,
  params?: Record<string, string>,
  bodyEncoding: 'json' | 'form' = 'json'
): Promise<T> {
  return requestFromServerWithInit<T>(getServerUrl(), path, { method, body, params, bodyEncoding });
}

export default request;

// ── Streaming URL helpers ─────────────────────────────────────────────────
// In dev mode use a relative URL so Vite's proxy forwards it to the DVR
// server (same-origin → no CORS, hls.js default XHR loader works).
// In production use the full server URL with the TauriLoader.

export function streamUrl(fileId: string): string {
  // In browser-only dev, keep using Vite proxy for convenience.
  // In Tauri dev/prod, always use absolute URL so active server selection is honored.
  if (import.meta.env.DEV && !window.__TAURI_INTERNALS__) {
    return `/dvr/files/${fileId}/hls/master.m3u8`;
  }
  return `${getServerUrl()}/dvr/files/${fileId}/hls/master.m3u8`;
}

export function previewUrl(fileId: string): string {
  // Thumbnail images are always absolute (displayed via <img>, not fetch)
  return `${getServerUrl()}/dvr/files/${fileId}/preview.jpg`;
}

// ── Server version ────────────────────────────────────────────────────────

/**
 * Fetch version details from available status endpoints.
 * Returns null when status endpoints are unavailable.
 */
export async function fetchServerVersionInfo(serverUrl: string): Promise<ServerVersionInfo | null> {
  for (const path of STATUS_PATH_CANDIDATES) {
    try {
      const data = await requestFromServer<Record<string, unknown>>(serverUrl, path);
      const info = extractVersionInfo(data);
      if (info.serverVersion || info.publicApiVersion) return info;
    } catch {
      // Try next candidate.
    }
  }
  return null;
}

export async function fetchServerVersion(serverUrl: string): Promise<string | null> {
  const info = await fetchServerVersionInfo(serverUrl);
  return info?.serverVersion ?? null;
}

export async function fetchCompatibilityMatrix(): Promise<CompatibilityMatrixFile | null> {
  try {
    const response = await httpFetch(repositoryRawCompatibilityUrl(), {
      method: 'GET',
      headers: { Accept: 'application/json' },
    });
    if (!response.ok) return null;
    const payload = await response.json() as unknown;
    if (!payload || typeof payload !== 'object') return null;
    const schemaVersion = Number((payload as { schemaVersion?: unknown }).schemaVersion ?? 1);
    const rawEntries = (payload as { entries?: unknown }).entries;
    if (!Array.isArray(rawEntries)) return null;
    const entries: CompatibilityMatrixEntry[] = rawEntries
      .filter((entry) => entry && typeof entry === 'object')
      .map((entry) => ({
        serverVersion: String((entry as { serverVersion?: unknown }).serverVersion ?? '').trim(),
        verified: Boolean((entry as { verified?: unknown }).verified),
        ...(String((entry as { testedOn?: unknown }).testedOn ?? '').trim()
          ? { testedOn: String((entry as { testedOn?: unknown }).testedOn).trim() }
          : {}),
        ...(String((entry as { notes?: unknown }).notes ?? '').trim()
          ? { notes: String((entry as { notes?: unknown }).notes).trim() }
          : {}),
      }))
      .filter((entry) => entry.serverVersion.length > 0);
    return { schemaVersion, entries };
  } catch {
    return null;
  }
}

export function isVersionVerified(
  matrix: CompatibilityMatrixFile,
  detected: ServerVersionInfo,
): boolean {
  if (!detected.serverVersion) return false;
  return matrix.entries.some((entry) => entry.verified && entry.serverVersion === detected.serverVersion);
}
