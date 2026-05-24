import { create } from 'zustand';
import {
  fetchCompatibilityMatrix,
  fetchServerVersionInfo,
  getServerUrl,
  isVersionVerified,
  normalizeServerUrl,
  probeUrl,
  setServerUrl,
} from '../api/client';


const SHARE_KEY = 'dvr_storage_share';
const SERVERS_KEY = 'dvr_servers';
const ACTIVE_SERVER_KEY = 'dvr_active_server_id';
const PLAYBACK_REMUX_KEY = 'playback_prefer_remux';
const DIAGNOSTICS_ENABLED_KEY = 'diagnostics_enabled';
const LIVE_SHOW_HIDDEN_KEY = 'live_show_hidden_channels';
const KEYBINDINGS_KEY = 'player_keybindings';
const SKIP_INTERVALS_KEY = 'player_skip_intervals';
const THEME_KEY = 'app_theme';
const WINDOW_ALWAYS_ON_TOP_KEY = 'window_always_on_top';

export type AppTheme = 'system' | 'dark' | 'light';

// Reasonable defaults for keybindings and skip intervals
export const DEFAULT_KEYBINDINGS = {
  skipForward: ['ArrowRight', 'l'],
  skipBack: ['ArrowLeft', 'j'],
  fastForward: ['Shift+ArrowRight'],
  fastReverse: ['Shift+ArrowLeft'],
  playPause: [' ', 'k'],
  close: ['Escape'],
};

export const DEFAULT_SKIP_INTERVALS = {
  skipForward: 30,
  skipBack: 10,
  fastForward: 60,
  fastReverse: 60,
};

export interface ServerOption {
  id: string;
  name: string;
  /** LAN/primary URL configured by the user. */
  url: string;
  /** Optional Tailscale URL used when the LAN address is unreachable. */
  tailscaleUrl?: string;
}

function makeServerId(): string {
  return `srv_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
}

function parseServers(raw: string | null): ServerOption[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    const out: ServerOption[] = [];
    for (const item of parsed) {
      if (!item || typeof item !== 'object') continue;
      const name = String((item as { name?: unknown }).name ?? '').trim();
      const url = normalizeServerUrl(String((item as { url?: unknown }).url ?? ''));
      const idRaw = String((item as { id?: unknown }).id ?? '').trim();
      const tailscaleRaw = String((item as { tailscaleUrl?: unknown }).tailscaleUrl ?? '').trim();
      const tailscaleUrl = tailscaleRaw ? normalizeServerUrl(tailscaleRaw) : undefined;
      if (!name || !url) continue;
      out.push({ id: idRaw || makeServerId(), name, url, ...(tailscaleUrl ? { tailscaleUrl } : {}) });
    }
    return out;
  } catch {
    return [];
  }
}

function bootstrapServers(): { servers: ServerOption[]; activeServerId: string } {
  const legacyUrl = getServerUrl();
  const parsed = parseServers(localStorage.getItem(SERVERS_KEY));
  const servers = parsed.length > 0
    ? parsed
    : [{ id: 'default', name: 'Default', url: legacyUrl }];

  const requestedActive = (localStorage.getItem(ACTIVE_SERVER_KEY) ?? '').trim();
  const activeServerId = servers.some((s) => s.id === requestedActive)
    ? requestedActive
    : servers[0].id;

  return { servers, activeServerId };
}

function persistServers(servers: ServerOption[], activeServerId: string): void {
  localStorage.setItem(SERVERS_KEY, JSON.stringify(servers));
  localStorage.setItem(ACTIVE_SERVER_KEY, activeServerId);
  const active = servers.find((s) => s.id === activeServerId) ?? servers[0];
  if (active) setServerUrl(active.url);
}

const boot = bootstrapServers();
const bootActive = boot.servers.find((s) => s.id === boot.activeServerId) ?? boot.servers[0];
persistServers(boot.servers, bootActive.id);

export interface KeybindingsConfig {
  skipForward: string[];
  skipBack: string[];
  fastForward: string[];
  fastReverse: string[];
  playPause: string[];
  close: string[];
}

export interface SkipIntervalsConfig {
  skipForward: number;
  skipBack: number;
  fastForward: number;
  fastReverse: number;
}

export interface AppState {
    // Player keybindings and skip intervals
    keybindings: KeybindingsConfig;
    setKeybindings: (bindings: KeybindingsConfig) => void;
    skipIntervals: SkipIntervalsConfig;
    setSkipIntervals: (intervals: SkipIntervalsConfig) => void;
  servers: ServerOption[];
  activeServerId: string;
  serverUrl: string;
  serverChangeVersion: number;

  setActiveServer: (id: string) => void;
  setServers: (servers: ServerOption[]) => void;
  setServerUrl: (url: string) => void;
  /** Probe the active server's LAN URL; if unreachable and a Tailscale URL is configured, switch to it. */
  probeActiveServer: () => Promise<void>;

  /** Internal server version string reported by the active server status endpoint. */
  apiVersion: string | null;
  /** Public API version string reported by the active server status endpoint. */
  apiPublicVersion: string | null;
  /** True when the detected version pair matches the repository approval matrix. */
  apiVersionApproved: boolean;
  /** Additional compatibility status context for UI. */
  apiCompatibilityNote: string | null;

  // UNC / local path to the root of the DVR storage share, e.g.
  // e.g. \\192.168.x.x\AllMedia\Channels  — used to find SRT sidecar files.
  storageSharePath: string;
  setStorageSharePath: (path: string) => void;

  preferRemux: boolean;
  setPreferRemux: (value: boolean) => void;

  diagnosticsEnabled: boolean;
  setDiagnosticsEnabled: (value: boolean) => void;

  showHiddenLiveChannels: boolean;
  setShowHiddenLiveChannels: (value: boolean) => void;

  theme: AppTheme;
  setTheme: (theme: AppTheme) => void;

  windowAlwaysOnTop: boolean;
  setWindowAlwaysOnTop: (value: boolean) => void;

  // Currently playing item – fileId drives the VideoPlayer
  nowPlayingId: string | null;
  nowPlayingKey: number;            // increments on every playItem call so the effect re-fires even for the same id
  nowPlayingTitle: string;
  nowPlayingFilePath: string;       // relative path from DVR API, e.g. TV/Show/Episode.mpg
  nowPlayingCommercials: number[];   // flat [start, end, start, end, …] in seconds
  nowPlayingManifestUrl: string;
  nowPlayingResumeTime: number;
  nowPlayingRecordingKind: 'episode' | 'movie' | null;
  playItem: (
    fileId: string,
    title: string,
    filePath?: string,
    commercials?: number[],
    manifestUrl?: string,
    resumeTime?: number,
    recordingKind?: 'episode' | 'movie' | null
  ) => void;
  stopPlayback: () => void;
}

function loadKeybindings(): KeybindingsConfig {
  try {
    const raw = localStorage.getItem(KEYBINDINGS_KEY);
    if (raw) return { ...DEFAULT_KEYBINDINGS, ...JSON.parse(raw) };
  } catch {}
  return { ...DEFAULT_KEYBINDINGS };
}

function loadSkipIntervals(): SkipIntervalsConfig {
  try {
    const raw = localStorage.getItem(SKIP_INTERVALS_KEY);
    if (raw) return { ...DEFAULT_SKIP_INTERVALS, ...JSON.parse(raw) };
  } catch {}
  return { ...DEFAULT_SKIP_INTERVALS };
}

function loadTheme(): AppTheme {
  const raw = (localStorage.getItem(THEME_KEY) ?? '').trim();
  if (raw === 'dark' || raw === 'light' || raw === 'system') return raw;
  return 'system';
}

export const useStore = create<AppState>((set) => ({
    keybindings: loadKeybindings(),
    setKeybindings: (bindings) => {
      localStorage.setItem(KEYBINDINGS_KEY, JSON.stringify(bindings));
      set({ keybindings: bindings });
    },
    skipIntervals: loadSkipIntervals(),
    setSkipIntervals: (intervals) => {
      localStorage.setItem(SKIP_INTERVALS_KEY, JSON.stringify(intervals));
      set({ skipIntervals: intervals });
    },
  servers: boot.servers,
  activeServerId: bootActive.id,
  serverUrl: bootActive.url,
  serverChangeVersion: 0,
  apiVersion: null,
  apiPublicVersion: null,
  apiVersionApproved: true,
  apiCompatibilityNote: null,

  setActiveServer: (id: string) => {
    set((state) => {
      const next = state.servers.find((s) => s.id === id);
      if (!next || next.id === state.activeServerId) return state;
      persistServers(state.servers, next.id);
      return {
        activeServerId: next.id,
        serverUrl: next.url,
        serverChangeVersion: state.serverChangeVersion + 1,
        apiVersion: null,
        apiPublicVersion: null,
        apiVersionApproved: true,
        apiCompatibilityNote: null,
        nowPlayingId: null,
        nowPlayingTitle: '',
        nowPlayingFilePath: '',
        nowPlayingCommercials: [],
        nowPlayingManifestUrl: '',
        nowPlayingResumeTime: 0,
        nowPlayingRecordingKind: null,
      };
    });
  },

  setServers: (servers: ServerOption[]) => {
    const cleaned = servers
      .map((s) => ({
        id: s.id?.trim() || makeServerId(),
        name: s.name.trim(),
        url: normalizeServerUrl(s.url),
        ...(s.tailscaleUrl?.trim() ? { tailscaleUrl: normalizeServerUrl(s.tailscaleUrl.trim()) } : {}),
      }))
      .filter((s) => s.name && s.url);

    if (cleaned.length === 0) return;

    set((state) => {
      const activeServerId = cleaned.some((s) => s.id === state.activeServerId)
        ? state.activeServerId
        : cleaned[0].id;
      const active = cleaned.find((s) => s.id === activeServerId) ?? cleaned[0];
      persistServers(cleaned, active.id);
      return {
        servers: cleaned,
        activeServerId: active.id,
        serverUrl: active.url,
        serverChangeVersion: state.serverChangeVersion + 1,
        apiVersion: null,
        apiPublicVersion: null,
        apiVersionApproved: true,
        apiCompatibilityNote: null,
        nowPlayingId: null,
        nowPlayingTitle: '',
        nowPlayingFilePath: '',
        nowPlayingCommercials: [],
        nowPlayingManifestUrl: '',
        nowPlayingResumeTime: 0,
        nowPlayingRecordingKind: null,
      };
    });
  },

  setServerUrl: (url: string) => {
    const normalized = normalizeServerUrl(url);
    if (!normalized) return;
    set((state) => {
      const updatedServers = state.servers.map((s) =>
        s.id === state.activeServerId ? { ...s, url: normalized } : s
      );
      persistServers(updatedServers, state.activeServerId);
      return {
        servers: updatedServers,
        serverUrl: normalized,
        serverChangeVersion: state.serverChangeVersion + 1,
        apiVersion: null,
        apiPublicVersion: null,
        apiVersionApproved: true,
        apiCompatibilityNote: null,
        nowPlayingId: null,
        nowPlayingTitle: '',
        nowPlayingFilePath: '',
        nowPlayingCommercials: [],
        nowPlayingManifestUrl: '',
        nowPlayingResumeTime: 0,
        nowPlayingRecordingKind: null,
      };
    });
  },

  probeActiveServer: async () => {
    const { servers, activeServerId } = useStore.getState();
    const targetServerId = activeServerId;
    const active = servers.find((s) => s.id === targetServerId);
    if (!active) return;

    let resolvedUrl = normalizeServerUrl(active.url);
    let reachable = false;

    const lanReachable = await probeUrl(active.url);
    if (lanReachable) {
      setServerUrl(active.url);
      set({ serverUrl: normalizeServerUrl(active.url) });
      resolvedUrl = normalizeServerUrl(active.url);
      reachable = true;
    } else if (active.tailscaleUrl) {
      const tailscaleReachable = await probeUrl(active.tailscaleUrl);
      if (tailscaleReachable) {
        setServerUrl(active.tailscaleUrl);
        set({ serverUrl: normalizeServerUrl(active.tailscaleUrl!) });
        resolvedUrl = normalizeServerUrl(active.tailscaleUrl);
        reachable = true;
        console.info(`[Network] LAN unreachable; switched to Tailscale URL for "${active.name}"`);
      }
    }

    if (!reachable) {
      // Neither URL responded — keep the configured LAN URL (connection will surface its own error)
      setServerUrl(active.url);
      if (useStore.getState().activeServerId === targetServerId) {
        set({
          serverUrl: normalizeServerUrl(active.url),
          apiVersion: null,
          apiPublicVersion: null,
          apiVersionApproved: true,
          apiCompatibilityNote: null,
        });
      }
      return;
    }

    const detected = await fetchServerVersionInfo(resolvedUrl);
    if (useStore.getState().activeServerId !== targetServerId) {
      return;
    }
    if (!detected || !detected.serverVersion) {
      set({
        apiVersion: null,
        apiPublicVersion: detected?.publicApiVersion ?? null,
        apiVersionApproved: true,
        apiCompatibilityNote: 'Unable to detect the active server version from status endpoints.',
      });
      return;
    }

    const matrix = await fetchCompatibilityMatrix();
    if (useStore.getState().activeServerId !== targetServerId) {
      return;
    }
    if (!matrix) {
      set({
        apiVersion: detected.serverVersion,
        apiPublicVersion: detected.publicApiVersion,
        apiVersionApproved: false,
        apiCompatibilityNote: 'Compatibility approvals could not be loaded from the repository.',
      });
      return;
    }

    const approved = isVersionVerified(matrix, detected);
    set({
      apiVersion: detected.serverVersion,
      apiPublicVersion: detected.publicApiVersion,
      apiVersionApproved: approved,
      apiCompatibilityNote: approved
        ? null
        : 'Detected version is not approved in the repository compatibility list yet.',
    });
  },

  storageSharePath: localStorage.getItem(SHARE_KEY) ?? '',
  setStorageSharePath: (path: string) => {
    const trimmed = path.trim().replace(/[/\\]+$/, '');
    localStorage.setItem(SHARE_KEY, trimmed);
    set({ storageSharePath: trimmed });
  },

  preferRemux: localStorage.getItem(PLAYBACK_REMUX_KEY) !== 'false',
  setPreferRemux: (value: boolean) => {
    localStorage.setItem(PLAYBACK_REMUX_KEY, String(value));
    set({ preferRemux: value });
  },

  diagnosticsEnabled: localStorage.getItem(DIAGNOSTICS_ENABLED_KEY) === 'true',
  setDiagnosticsEnabled: (value: boolean) => {
    localStorage.setItem(DIAGNOSTICS_ENABLED_KEY, String(value));
    set({ diagnosticsEnabled: value });
  },

  showHiddenLiveChannels: localStorage.getItem(LIVE_SHOW_HIDDEN_KEY) === 'true',
  setShowHiddenLiveChannels: (value: boolean) => {
    localStorage.setItem(LIVE_SHOW_HIDDEN_KEY, String(value));
    set({ showHiddenLiveChannels: value });
  },

  theme: loadTheme(),
  setTheme: (theme: AppTheme) => {
    localStorage.setItem(THEME_KEY, theme);
    set({ theme });
  },

  windowAlwaysOnTop: localStorage.getItem(WINDOW_ALWAYS_ON_TOP_KEY) === 'true',
  setWindowAlwaysOnTop: (value: boolean) => {
    localStorage.setItem(WINDOW_ALWAYS_ON_TOP_KEY, String(value));
    set({ windowAlwaysOnTop: value });
  },

  nowPlayingId: null,
  nowPlayingKey: 0,
  nowPlayingTitle: '',
  nowPlayingFilePath: '',
  nowPlayingCommercials: [],
  nowPlayingManifestUrl: '',
  nowPlayingResumeTime: 0,
  nowPlayingRecordingKind: null,
  playItem: (fileId, title, filePath = '', commercials = [], manifestUrl = '', resumeTime = 0, recordingKind = null) =>
    set(s => ({
      nowPlayingId: fileId,
      nowPlayingKey: s.nowPlayingKey + 1,
      nowPlayingTitle: title,
      nowPlayingFilePath: filePath,
      nowPlayingCommercials: commercials,
      nowPlayingManifestUrl: manifestUrl,
      nowPlayingResumeTime: Math.max(0, Math.floor(resumeTime)),
      nowPlayingRecordingKind: recordingKind,
    })),
  stopPlayback: () => set({
    nowPlayingId: null,
    nowPlayingTitle: '',
    nowPlayingFilePath: '',
    nowPlayingCommercials: [],
    nowPlayingManifestUrl: '',
    nowPlayingResumeTime: 0,
    nowPlayingRecordingKind: null,
  }),
}));
