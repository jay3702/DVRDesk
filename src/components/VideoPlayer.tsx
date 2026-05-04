import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import Hls from 'hls.js';
import { invoke } from '@tauri-apps/api/core';
import request, { streamUrl } from '../api/client';
import {
  getLastRecordingMutationDebug,
  getLastRecordingMutationFailure,
  setEpisodePlaybackTime,
  setEpisodeWatched,
  setMoviePlaybackTime,
  setMovieWatched,
} from '../api/recordings';
import { useStore, DEFAULT_KEYBINDINGS, DEFAULT_SKIP_INTERVALS } from '../store/useStore';
import { buildTauriHlsLoader } from '../lib/hlsTauriLoader';
import './VideoPlayer.css';

// Cache the Tauri HLS loader at module level so the dynamic import
// only fires once and never hangs on subsequent play attempts.
let tauriLoaderCache: Awaited<ReturnType<typeof buildTauriHlsLoader>> | null = null;
let tauriLoaderPromise: Promise<void> | null = null;
function getTauriLoader() {
  if (tauriLoaderCache !== null) return Promise.resolve(tauriLoaderCache);
  if (!tauriLoaderPromise) {
    tauriLoaderPromise = buildTauriHlsLoader().then(l => { tauriLoaderCache = l ?? null; });
  }
  return tauriLoaderPromise.then(() => tauriLoaderCache);
}
// Pre-warm in production so the loader is ready before the first play attempt.
if (import.meta.env.PROD) void getTauriLoader();

// Cache the DVR server's storage root path (e.g. "/tank/AllMedia/Channels").
// Fetched once from /dvr and used to strip the absolute prefix from file paths
// before joining with the Windows share path.
let dvrStorageRootCache: string | null = null;
async function getDvrStorageRoot(): Promise<string> {
  if (dvrStorageRootCache !== null) return dvrStorageRootCache;
  try {
    const data = await request<{ path?: string }>('/dvr');
    dvrStorageRootCache = (data.path ?? '').replace(/\/+$/, '');
  } catch {
    dvrStorageRootCache = '';
  }
  return dvrStorageRootCache;
}

/** Convert an SRT string to WebVTT. Returns null if input is empty/blank. */
function srtToVtt(srt: string): string | null {
  const trimmed = srt.trim();
  if (!trimmed) return null;
  // Replace SRT timestamp commas with dots and prepend the WEBVTT header.
  // SRT: 00:01:23,456 --> 00:01:25,789
  // VTT: 00:01:23.456 --> 00:01:25.789
  const vtt = trimmed.replace(/(\d{2}:\d{2}:\d{2}),(\d{3})/g, '$1.$2');
  return 'WEBVTT\n\n' + vtt;
}

const RE_ENABLE_BEFORE = 30; // seek this many seconds before block start to re-enable auto-skip
type CaptionMode = 'off' | 'broadcast' | 'srt';

interface NerdStats {
  timestampIso: string;
  manifestUrl: string;
  playbackState: string;
  playbackRate: number;
  volumePct: number;
  muted: boolean;
  videoSize: string;
  currentLevel: string;
  bandwidthEstimate: string;
  bufferAheadSec: number;
  droppedFrames: number | null;
  decodedFrames: number | null;
  droppedPercent: number | null;
  readyState: number;
}

function formatBitrate(bitsPerSec: number | undefined): string {
  if (!bitsPerSec || !Number.isFinite(bitsPerSec) || bitsPerSec <= 0) return 'n/a';
  const mbps = bitsPerSec / 1_000_000;
  if (mbps >= 1) return `${mbps.toFixed(2)} Mbps`;
  return `${(bitsPerSec / 1000).toFixed(0)} kbps`;
}

function withQuery(url: string, key: string, value: string): string {
  const sep = url.includes('?') ? '&' : '?';
  return `${url}${sep}${encodeURIComponent(key)}=${encodeURIComponent(value)}`;
}

function getBufferAhead(video: HTMLVideoElement): number {
  const t = video.currentTime;
  const ranges = video.buffered;
  for (let i = 0; i < ranges.length; i += 1) {
    const start = ranges.start(i);
    const end = ranges.end(i);
    if (t >= start && t <= end) {
      return Math.max(0, end - t);
    }
  }
  return 0;
}

function collectNerdStats(video: HTMLVideoElement | null, hls: Hls | null, fallbackManifestUrl: string): NerdStats {
  const nowIso = new Date().toISOString();
  if (!video) {
    return {
      timestampIso: nowIso,
      manifestUrl: fallbackManifestUrl,
      playbackState: 'no-video-element',
      playbackRate: 1,
      volumePct: 0,
      muted: false,
      videoSize: 'n/a',
      currentLevel: 'n/a',
      bandwidthEstimate: 'n/a',
      bufferAheadSec: 0,
      droppedFrames: null,
      decodedFrames: null,
      droppedPercent: null,
      readyState: 0,
    };
  }

  const quality = typeof video.getVideoPlaybackQuality === 'function'
    ? video.getVideoPlaybackQuality()
    : null;
  const legacyVideo = video as HTMLVideoElement & {
    webkitDroppedFrameCount?: number;
    webkitDecodedFrameCount?: number;
  };
  const droppedFrames = quality?.droppedVideoFrames ?? legacyVideo.webkitDroppedFrameCount ?? null;
  const decodedFrames = quality?.totalVideoFrames ?? legacyVideo.webkitDecodedFrameCount ?? null;
  const droppedPercent =
    droppedFrames !== null && decodedFrames !== null && decodedFrames > 0
      ? (droppedFrames / decodedFrames) * 100
      : null;

  let currentLevel = 'n/a';
  let bandwidthEstimate = 'n/a';
  if (hls) {
    const selected = hls.currentLevel;
    if (selected >= 0 && selected < hls.levels.length) {
      const level = hls.levels[selected];
      currentLevel = `${selected} (${level.width || '?'}x${level.height || '?'} @ ${formatBitrate(level.bitrate)})`;
    } else {
      currentLevel = 'auto';
    }
    bandwidthEstimate = formatBitrate(hls.bandwidthEstimate);
  }

  return {
    timestampIso: nowIso,
    manifestUrl: fallbackManifestUrl,
    playbackState: video.paused ? 'paused' : (video.ended ? 'ended' : 'playing'),
    playbackRate: video.playbackRate,
    volumePct: Math.round(video.volume * 100),
    muted: video.muted,
    videoSize: `${video.videoWidth || '?'}x${video.videoHeight || '?'}`,
    currentLevel,
    bandwidthEstimate,
    bufferAheadSec: getBufferAhead(video),
    droppedFrames,
    decodedFrames,
    droppedPercent,
    readyState: video.readyState,
  };
}

export default function VideoPlayer() {
  const {
    nowPlayingId,
    nowPlayingKey,
    nowPlayingTitle,
    nowPlayingCommercials,
    nowPlayingFilePath,
    nowPlayingManifestUrl,
    nowPlayingResumeTime,
    nowPlayingRecordingKind,
    storageSharePath,
    stopPlayback,
    serverChangeVersion,
    preferRemux,
    diagnosticsEnabled,
    keybindings,
    skipIntervals,
  } = useStore();
  const videoRef = useRef<HTMLVideoElement>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const hlsRef = useRef<Hls | null>(null);
  const subtitleBlobUrl = useRef<string | null>(null);
  const activeManifestUrlRef = useRef<string>('');
  // Flag to distinguish programmatic seeks from user seeks
  const isAutoSeekRef = useRef(false);

  const [error, setError] = useState<string | null>(null);
  const [skipAds, setSkipAds] = useState(true);
  const [skipping, setSkipping] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  // Indices of blocks where auto-skip has been manually overridden
  const [disabledBlocks, setDisabledBlocks] = useState<Set<number>>(new Set());
  const [hasSrt, setHasSrt] = useState(false);
  const [hasBroadcast, setHasBroadcast] = useState(false);
  const [captionMode, setCaptionMode] = useState<CaptionMode>('off');
  const [reportCopied, setReportCopied] = useState(false);
  const [showStats, setShowStats] = useState(false);
  const [nerdStats, setNerdStats] = useState<NerdStats | null>(null);
  const [lastMutationDebug, setLastMutationDebug] = useState<string>('n/a');
  const [lastMutationFailure, setLastMutationFailure] = useState<string>('n/a');
  const [isOverlayFullscreen, setIsOverlayFullscreen] = useState(false);
  const captionModeRef = useRef<CaptionMode>('off');
  const hasAppliedResumeRef = useRef(false);
  const hasMarkedWatchedRef = useRef(false);
  const saveInFlightRef = useRef(false);
  const pendingSaveRef = useRef<number | null>(null);
  const pendingMarkWatchedRef = useRef(false);
  const lastSavedPlaybackRef = useRef(0);
  captionModeRef.current = captionMode;

  useEffect(() => {
    dvrStorageRootCache = null;
  }, [serverChangeVersion]);

  useEffect(() => {
    hasAppliedResumeRef.current = false;
    hasMarkedWatchedRef.current = false;
    saveInFlightRef.current = false;
    pendingSaveRef.current = null;
    pendingMarkWatchedRef.current = false;
    lastSavedPlaybackRef.current = Math.max(0, Math.floor(nowPlayingResumeTime));
  }, [nowPlayingKey, nowPlayingResumeTime]);

  function canSyncPlayback(): boolean {
    return Boolean(nowPlayingId && nowPlayingRecordingKind);
  }

  async function persistPlaybackUpdate(playbackTime: number, markWatched: boolean): Promise<void> {
    if (!nowPlayingId || !nowPlayingRecordingKind) return;

    const clamped = Math.max(0, Math.floor(playbackTime));
    if (nowPlayingRecordingKind === 'episode') {
      await setEpisodePlaybackTime(nowPlayingId, clamped);
      if (markWatched) await setEpisodeWatched(nowPlayingId, true);
      return;
    }

    await setMoviePlaybackTime(nowPlayingId, clamped);
    if (markWatched) await setMovieWatched(nowPlayingId, true);
  }

  function flushPendingPlaybackUpdate() {
    if (saveInFlightRef.current) return;
    if (!canSyncPlayback()) return;

    const pendingPlayback = pendingSaveRef.current;
    const pendingWatched = pendingMarkWatchedRef.current;
    if (pendingPlayback == null && !pendingWatched) return;

    const playbackToWrite = pendingPlayback ?? lastSavedPlaybackRef.current;
    pendingSaveRef.current = null;
    pendingMarkWatchedRef.current = false;

    saveInFlightRef.current = true;
    void persistPlaybackUpdate(playbackToWrite, pendingWatched)
      .then(() => {
        lastSavedPlaybackRef.current = Math.max(0, Math.floor(playbackToWrite));
      })
      .catch((e) => {
        // Mutation endpoints are best-effort; continue playback if unsupported.
        console.warn('[PlaybackSync] Could not persist playback update', e);
      })
      .finally(() => {
        saveInFlightRef.current = false;
        if (pendingSaveRef.current != null || pendingMarkWatchedRef.current) {
          flushPendingPlaybackUpdate();
        }
      });
  }

  function queuePlaybackUpdate(playbackTime: number, force = false, markWatched = false) {
    if (!canSyncPlayback()) return;
    const clamped = Math.max(0, Math.floor(playbackTime));
    const delta = Math.abs(clamped - lastSavedPlaybackRef.current);
    if (!force && delta < 20) return;

    pendingSaveRef.current = clamped;
    if (markWatched) pendingMarkWatchedRef.current = true;
    flushPendingPlaybackUpdate();
  }

  function getCaptionTracks(video: HTMLVideoElement) {
    const tracks = Array.from(video.textTracks);
    const srt = tracks.find((t) => t.label === 'Subtitles' && t.kind === 'subtitles') ?? null;
    const broadcast = tracks.find((t) => t !== srt && (t.kind === 'captions' || t.kind === 'subtitles')) ?? null;
    return { tracks, srt, broadcast };
  }

  function applyCaptionMode(video: HTMLVideoElement, mode: CaptionMode) {
    const { tracks, srt, broadcast } = getCaptionTracks(video);
    tracks.forEach((t) => {
      if (t.kind === 'captions' || t.kind === 'subtitles') t.mode = 'hidden';
    });
    if (mode === 'srt' && srt) srt.mode = 'showing';
    if (mode === 'broadcast' && broadcast) broadcast.mode = 'showing';
  }

  function syncCaptionState(video: HTMLVideoElement) {
    const { srt, broadcast } = getCaptionTracks(video);
    setHasSrt(Boolean(srt));
    setHasBroadcast(Boolean(broadcast));

    let next = captionModeRef.current;
    if (next === 'srt' && !srt) next = broadcast ? 'broadcast' : 'off';
    if (next === 'broadcast' && !broadcast) next = srt ? 'srt' : 'off';

    if (next !== captionModeRef.current) setCaptionMode(next);
    applyCaptionMode(video, next);
  }

  // Refs so event handlers always see current values without re-registration
  const skipAdsRef = useRef(skipAds);
  const disabledBlocksRef = useRef(disabledBlocks);
  skipAdsRef.current = skipAds;
  disabledBlocksRef.current = disabledBlocks;

  const adBlocks = useMemo<[number, number][]>(() => {
    const blocks: [number, number][] = [];
    for (let i = 0; i + 1 < nowPlayingCommercials.length; i += 2) {
      blocks.push([nowPlayingCommercials[i], nowPlayingCommercials[i + 1]]);
    }
    return blocks;
  }, [nowPlayingCommercials]);

  const adBlocksRef = useRef(adBlocks);
  adBlocksRef.current = adBlocks;

  // Reset UI state when item changes
  useEffect(() => {
    setHasSrt(false);
    setHasBroadcast(false);
    setCaptionMode('off');
    setCurrentTime(0);
    setDuration(0);
    setDisabledBlocks(new Set());
    setSkipping(false);
    setShowStats(false);
    setNerdStats(null);
  }, [nowPlayingKey]);

  useEffect(() => {
    if (!diagnosticsEnabled || !nowPlayingId) {
      setShowStats(false);
      setNerdStats(null);
      return;
    }

    const updateStats = () => {
      const manifest = activeManifestUrlRef.current || nowPlayingManifestUrl || streamUrl(nowPlayingId);
      setNerdStats(collectNerdStats(videoRef.current, hlsRef.current, manifest));
      setLastMutationDebug(getLastRecordingMutationDebug() ?? 'n/a');
      setLastMutationFailure(getLastRecordingMutationFailure() ?? 'n/a');
    };

    updateStats();
    const id = window.setInterval(updateStats, 1000);
    return () => window.clearInterval(id);
  }, [diagnosticsEnabled, nowPlayingId, nowPlayingManifestUrl]);

  // Unified skip/seek function
  const skipBy = useCallback((delta: number) => {
    const video = videoRef.current;
    if (!video) return;
    video.currentTime = Math.max(0, Math.min(video.duration || 0, video.currentTime + delta));
  }, []);

  // Play/pause toggle
  const togglePlayPause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) video.play();
    else video.pause();
  }, []);

  // Keydown handler for all player commands
  const onPlayerKeyDown = useCallback((e: KeyboardEvent) => {
    // Helper to match keybindings (supports Shift+Key, etc)
    function matchBinding(binding: string): boolean {
      if (binding.startsWith('Shift+')) {
        return e.shiftKey && (e.key === binding.slice(6) || e.code === binding.slice(6));
      }
      if (binding === ' ' || binding === 'Space') return e.key === ' ' || e.code === 'Space';
      return e.key === binding || e.code === binding;
    }
    // Use settings or fallback to defaults
    const binds = keybindings || DEFAULT_KEYBINDINGS;
    const intervals = skipIntervals || DEFAULT_SKIP_INTERVALS;
    // Skip Forward
    if (binds.skipForward.some(matchBinding)) {
      e.preventDefault();
      skipBy(intervals.skipForward);
      return;
    }
    // Skip Back
    if (binds.skipBack.some(matchBinding)) {
      e.preventDefault();
      skipBy(-intervals.skipBack);
      return;
    }
    // Fast Forward
    if (binds.fastForward.some(matchBinding)) {
      e.preventDefault();
      skipBy(intervals.fastForward);
      return;
    }
    // Fast Reverse
    if (binds.fastReverse.some(matchBinding)) {
      e.preventDefault();
      skipBy(-intervals.fastReverse);
      return;
    }
    // Play/Pause
    if (binds.playPause.some(matchBinding)) {
      e.preventDefault();
      togglePlayPause();
      return;
    }
    // Close Player
    if (binds.close.some(matchBinding)) {
      e.preventDefault();
      stopPlayback();
      return;
    }
  }, [keybindings, skipIntervals, skipBy, togglePlayPause, stopPlayback]);

  useEffect(() => {
    if (!nowPlayingId) return;
    // Attach keydown handler for player commands
    const handler = (e: KeyboardEvent) => {
      if (e.repeat) return;
      onPlayerKeyDown(e);
      // Also handle stats overlay toggle (Shift+S)
      if (diagnosticsEnabled && e.shiftKey && (e.key === 'S' || e.key === 's' || e.code === 'KeyS')) {
        e.preventDefault();
        setShowStats((v) => !v);
      }
    };
    window.addEventListener('keydown', handler, true);
    return () => {
      window.removeEventListener('keydown', handler, true);
    };
  }, [nowPlayingId, onPlayerKeyDown, diagnosticsEnabled]);

  useEffect(() => {
    function onFullscreenChange() {
      const fullscreenEl = document.fullscreenElement;
      setIsOverlayFullscreen(Boolean(fullscreenEl) && fullscreenEl === overlayRef.current);
      if (fullscreenEl === overlayRef.current) {
        overlayRef.current?.focus();
      }
    }

    document.addEventListener('fullscreenchange', onFullscreenChange);
    onFullscreenChange();
    return () => document.removeEventListener('fullscreenchange', onFullscreenChange);
  }, []);

  // HLS setup
  useEffect(() => {
    const video = videoRef.current;
    // Always clear the error so the video element stays visible and the
    // ref is available even if a previous play attempt failed.
    setError(null);
    if (!video || !nowPlayingId) return;

    const src = nowPlayingManifestUrl || streamUrl(nowPlayingId);
    const remuxSrc = withQuery(src, 'encoder', 'remux');
    activeManifestUrlRef.current = preferRemux ? remuxSrc : src;

    if (Hls.isSupported()) {
      let cancelled = false;

      void (async () => {
        // Use the Tauri loader whenever we're running inside Tauri (dev or prod)
        // so HLS requests bypass browser CORS restrictions.
        const tauriLoader = window.__TAURI_INTERNALS__ ? await getTauriLoader() : undefined;
        // Fetch DVR storage root for SRT path stripping (cached after first call)
        const dvrStorageRoot = storageSharePath ? await getDvrStorageRoot() : '';

        if (cancelled) return;

        // Local-network DVR: assume high bandwidth from the start so ABR
        // picks the highest quality tier immediately instead of ramping up.
        // enableWorker:false — hls.js workers fail to load scripts under the
        // tauri:// custom protocol used in production builds.
        const hlsConfig = {
          enableWorker: false,
          testBandwidth: false,
          startLevel: 999,
          capLevelToPlayerSize: false,
          abrEwmaDefaultEstimate: 20_000_000,
          maxBufferLength: 60,
          maxMaxBufferLength: 120,
          abrBandWidthFactor: 0.98,
          abrBandWidthUpFactor: 0.5,
          // Extract CEA-608/708 closed captions embedded in TS segments and
          // expose them as native video text tracks (selectable via the
          // browser's built-in CC button in the video controls bar).
          enableCEA708Captions: true,
          ...(tauriLoader ? { loader: tauriLoader as any } : {}),
        };
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const hls = new Hls(hlsConfig as any);
        hlsRef.current = hls;
        let usedRemuxManifest = preferRemux;

        hls.on(Hls.Events.ERROR, (_event, data) => {
          console.error('HLS error:', data.type, data.details, data.fatal,
            'url:', data.url, 'response:', data.response);

          // Some DVR/server combinations may not honor encoder=remux. If the
          // remux-preferred manifest fails to load, retry once with default URL.
          if (
            data.fatal &&
            usedRemuxManifest &&
            data.details === 'manifestLoadError'
          ) {
            usedRemuxManifest = false;
            activeManifestUrlRef.current = src;
            hls.loadSource(src);
            return;
          }

          if (data.fatal && !cancelled) {
            const status = data.response?.code ? ` (HTTP ${data.response.code})` : '';
            const errUrl = data.url ?? activeManifestUrlRef.current;
            setError(
              `Playback failed\n` +
              `Type: ${data.type}\n` +
              `Detail: ${data.details}${status}\n` +
              `URL: ${errUrl}`
            );
          }
        });

        hls.loadSource(preferRemux ? remuxSrc : src);
        hls.attachMedia(video);
        hls.on(Hls.Events.MANIFEST_PARSED, (_event, data) => {
          if (!cancelled) {
            // Lock to the best available quality level by bitrate.
            // ABR relies on bandwidth samples from the loader; the Tauri HTTP
            // loader buffers the full response before returning it, so timing
            // data is unreliable and ABR incorrectly downgrades quality.
            // On a local network there's no reason to use ABR at all.
            const bestLevelIndex = data.levels.reduce((bestIdx, level, idx, arr) => {
              const best = arr[bestIdx];
              const byBitrate = (level.bitrate ?? 0) - (best.bitrate ?? 0);
              if (byBitrate !== 0) return byBitrate > 0 ? idx : bestIdx;

              const levelPixels = (level.width ?? 0) * (level.height ?? 0);
              const bestPixels = (best.width ?? 0) * (best.height ?? 0);
              return levelPixels > bestPixels ? idx : bestIdx;
            }, 0);
            hls.currentLevel = bestLevelIndex;
            video.play().catch((e: Error) => { if (!cancelled) setError(e.message); });
            syncCaptionState(video);

            if (
              !hasAppliedResumeRef.current &&
              nowPlayingResumeTime > 5 &&
              Number.isFinite(video.duration) &&
              nowPlayingResumeTime < Math.max(video.duration - 10, 10)
            ) {
              hasAppliedResumeRef.current = true;
              video.currentTime = nowPlayingResumeTime;
            }

            // Try to load a sidecar .srt subtitle file if a share path is configured.
            if (storageSharePath && nowPlayingFilePath) {
              // Strip the DVR server's absolute storage root prefix so we get
              // a path relative to the share root, e.g.:
              //   /tank/AllMedia/Channels/TV/Show/ep.mpg → TV\Show\ep.srt
              let relPath = nowPlayingFilePath;
              if (dvrStorageRoot && relPath.startsWith(dvrStorageRoot)) {
                relPath = relPath.slice(dvrStorageRoot.length);
              }
              const rel = relPath.replace(/^[\/\\]+/, '').replace(/\//g, '\\').replace(/\.[^.\\]+$/, '.srt');
              const base = storageSharePath.replace(/[/\\]+$/, '');
              const srtPath = `${base}\\${rel}`;
              (invoke('read_text_file', { path: srtPath }) as Promise<string>)
                .then((srt) => {
                  if (cancelled) return;
                  const vtt = srtToVtt(srt);
                  if (!vtt) return;
                  // Revoke any previous blob URL
                  if (subtitleBlobUrl.current) URL.revokeObjectURL(subtitleBlobUrl.current);
                  const blob = new Blob([vtt], { type: 'text/vtt' });
                  const blobUrl = URL.createObjectURL(blob);
                  subtitleBlobUrl.current = blobUrl;
                  // Remove any existing sidecar tracks, then inject the new one
                  const existing = video.querySelectorAll('track[data-srt]');
                  existing.forEach((t) => t.remove());
                  const track = document.createElement('track');
                  track.kind = 'subtitles';
                  track.label = 'Subtitles';
                  track.srclang = 'en';
                  track.default = true;
                  track.src = blobUrl;
                  track.dataset.srt = '1';
                  track.addEventListener('load', () => {
                    syncCaptionState(video);
                  });
                  video.appendChild(track);
                })
                .catch((e: unknown) => {
                  console.warn('[SRT] Could not load subtitle file:', srtPath, e);
                });
            }
          }
        });
      })();

      return () => {
        cancelled = true;
        hlsRef.current?.destroy();
        hlsRef.current = null;
        if (subtitleBlobUrl.current) {
          URL.revokeObjectURL(subtitleBlobUrl.current);
          subtitleBlobUrl.current = null;
        }
        // Remove injected subtitle track from video element
        const video2 = videoRef.current;
        if (video2) {
          video2.querySelectorAll('track[data-srt]').forEach((t) => t.remove());
        }
      };
    } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
      video.src = src;
      video.play().catch((e: Error) => setError(e.message));
      return () => { /* nothing hls-specific to destroy */ };
    } else {
      setError('HLS playback is not supported in this environment.');
    }
  }, [nowPlayingKey, preferRemux]);

  // Track additions/removals happen asynchronously (especially broadcast CEA tracks).
  // Keep availability + current mode in sync whenever TextTracks changes.
  useEffect(() => {
    const video = videoRef.current;
    if (!video || !nowPlayingId) return;

    const list = video.textTracks as TextTrackList & {
      onaddtrack?: ((this: TextTrackList, ev: TrackEvent) => any) | null;
      onremovetrack?: ((this: TextTrackList, ev: TrackEvent) => any) | null;
    };

    const onChange = () => syncCaptionState(video);
    const prevAdd = list.onaddtrack ?? null;
    const prevRemove = list.onremovetrack ?? null;

    list.onaddtrack = () => onChange();
    list.onremovetrack = () => onChange();
    onChange();

    return () => {
      list.onaddtrack = prevAdd;
      list.onremovetrack = prevRemove;
    };
  }, [nowPlayingId]);

  // Video event listeners — set up once per item, read mutable state via refs
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    function onTimeUpdate() {
      if (!video) return;
      const t = video.currentTime;
      setCurrentTime(t);
      queuePlaybackUpdate(t);

      if (
        !hasMarkedWatchedRef.current &&
        Number.isFinite(video.duration) &&
        video.duration > 0 &&
        t / video.duration >= 0.9
      ) {
        hasMarkedWatchedRef.current = true;
        queuePlaybackUpdate(t, true, true);
      }

      if (!skipAdsRef.current) return;
      const blocks = adBlocksRef.current;
      const disabled = disabledBlocksRef.current;
      for (let i = 0; i < blocks.length; i++) {
        if (disabled.has(i)) continue;
        const [start, end] = blocks[i];
        if (t >= start && t < end) {
          isAutoSeekRef.current = true;
          video.currentTime = end;
          setSkipping(true);
          setTimeout(() => setSkipping(false), 1500);
          break;
        }
      }
    }

    function onPause() {
      if (!video) return;
      queuePlaybackUpdate(video.currentTime, true);
    }

    function onEnded() {
      if (!video) return;
      hasMarkedWatchedRef.current = true;
      queuePlaybackUpdate(video.currentTime, true, true);
    }

    function onLoadedMetadata() {
      if (!video) return;
      setDuration(video.duration);
      if (
        !hasAppliedResumeRef.current &&
        nowPlayingResumeTime > 5 &&
        Number.isFinite(video.duration) &&
        nowPlayingResumeTime < Math.max(video.duration - 10, 10)
      ) {
        hasAppliedResumeRef.current = true;
        video.currentTime = nowPlayingResumeTime;
      }
    }

    function onSeeked() {
      // Ignore programmatic auto-skips
      if (isAutoSeekRef.current) {
        isAutoSeekRef.current = false;
        return;
      }
      const t = video!.currentTime;
      const blocks = adBlocksRef.current;
      setDisabledBlocks((prev) => {
        const next = new Set(prev);
        blocks.forEach(([start, end], i) => {
          if (t < start - RE_ENABLE_BEFORE) {
            // Well before the block — re-enable auto-skip
            next.delete(i);
          } else if (t >= start - 5 && t <= end) {
            // Seeked into or just before the commercial zone — disable auto-skip
            next.add(i);
          }
        });
        return next;
      });
    }

    video.addEventListener('timeupdate', onTimeUpdate);
    video.addEventListener('loadedmetadata', onLoadedMetadata);
    video.addEventListener('seeked', onSeeked);
    video.addEventListener('pause', onPause);
    video.addEventListener('ended', onEnded);
    return () => {
      queuePlaybackUpdate(video.currentTime, true);
      video.removeEventListener('timeupdate', onTimeUpdate);
      video.removeEventListener('loadedmetadata', onLoadedMetadata);
      video.removeEventListener('seeked', onSeeked);
      video.removeEventListener('pause', onPause);
      video.removeEventListener('ended', onEnded);
    };
  }, [nowPlayingKey, nowPlayingRecordingKind, nowPlayingResumeTime]);

  function setCaptionModeAndApply(mode: CaptionMode) {
    setCaptionMode(mode);
    const video = videoRef.current;
    if (!video) return;
    applyCaptionMode(video, mode);
  }

  async function toggleOverlayFullscreen() {
    const overlay = overlayRef.current;
    if (!overlay) return;

    if (document.fullscreenElement === overlay) {
      await document.exitFullscreen();
      return;
    }

    await overlay.requestFullscreen();
    overlay.focus();
  }

  async function copyPlaybackReport() {
    if (!nowPlayingId) return;
    const hls = hlsRef.current;
    const video = videoRef.current;
    const manifest = activeManifestUrlRef.current || nowPlayingManifestUrl || streamUrl(nowPlayingId);
    const stats = collectNerdStats(video, hls, manifest);
    const levelLines: string[] = [];
    let selectedLine = 'Selected level: n/a';
    let bandwidthLine = 'Estimated bandwidth: n/a';

    if (hls) {
      bandwidthLine = `Estimated bandwidth: ${formatBitrate(hls.bandwidthEstimate)}`;
      const selected = hls.currentLevel;
      selectedLine = `Selected level: ${selected >= 0 ? selected : 'auto'}`;

      hls.levels.forEach((level, idx) => {
        const levelDesc = [
          `${idx}:`,
          `${level.width || '?'}x${level.height || '?'}`,
          formatBitrate(level.bitrate),
          level.videoCodec ? `v=${level.videoCodec}` : '',
          level.audioCodec ? `a=${level.audioCodec}` : '',
        ].filter(Boolean).join(' ');
        levelLines.push(levelDesc);
      });

      if (selected >= 0 && selected < hls.levels.length) {
        const l = hls.levels[selected];
        selectedLine = `Selected level: ${selected} (${l.width || '?'}x${l.height || '?'} @ ${formatBitrate(l.bitrate)})`;
      }
    }

    const report = [
      'WinChannels Playback Report',
      `Time: ${new Date().toISOString()}`,
      `Title: ${nowPlayingTitle}`,
      `File ID: ${nowPlayingId}`,
      `Prefer remux: ${preferRemux ? 'on' : 'off'}`,
      `Manifest URL: ${manifest}`,
      `Last mutation endpoint: ${getLastRecordingMutationDebug() ?? 'n/a'}`,
      `Last mutation failure: ${getLastRecordingMutationFailure() ?? 'n/a'}`,
      `Video element: ${stats.videoSize}`,
      `Current time: ${Math.floor(currentTime)}s / ${Math.floor(duration)}s`,
      `Playback state: ${stats.playbackState}`,
      `Playback rate: ${stats.playbackRate.toFixed(2)}x`,
      `Volume: ${stats.muted ? 'muted' : `${stats.volumePct}%`}`,
      `Buffer ahead: ${stats.bufferAheadSec.toFixed(2)}s`,
      `Dropped frames: ${stats.droppedFrames ?? 'n/a'}`,
      `Decoded frames: ${stats.decodedFrames ?? 'n/a'}`,
      `Dropped frame %: ${stats.droppedPercent !== null ? `${stats.droppedPercent.toFixed(2)}%` : 'n/a'}`,
      `Ready state: ${stats.readyState}`,
      selectedLine,
      bandwidthLine,
      'Levels:',
      ...(levelLines.length > 0 ? levelLines : ['n/a']),
    ].join('\n');

    try {
      await navigator.clipboard.writeText(report);
      setReportCopied(true);
      setTimeout(() => setReportCopied(false), 1800);
      console.info(report);
    } catch {
      console.info(report);
      setError('Could not copy report to clipboard. Report was printed to console.');
    }
  }

  if (!nowPlayingId) return null;

  return (
    <div className="video-overlay" ref={overlayRef} tabIndex={0}>
      <div className="video-header">
        <span className="video-title">{nowPlayingTitle}</span>
        <div className="video-header__controls">
          {(hasBroadcast || hasSrt) && (
            <label className="video-cc-wrap" title="Caption track selection">
              <span className="video-cc-label">CC</span>
              <select
                className="video-cc-select"
                value={captionMode}
                onChange={(e) => setCaptionModeAndApply(e.target.value as CaptionMode)}
              >
                <option value="off">Off</option>
                {hasBroadcast && <option value="broadcast">Broadcast</option>}
                {hasSrt && <option value="srt">Py-Captions (SRT)</option>}
              </select>
            </label>
          )}
          {adBlocks.length > 0 && (
            <button
              className={`video-skip-toggle ${skipAds ? 'video-skip-toggle--on' : ''}`}
              onClick={() => setSkipAds((v) => !v)}
              title={skipAds ? 'Commercial skipping ON — click to disable' : 'Commercial skipping OFF — click to enable'}
            >
              {skipAds ? '⏭ Skip Ads: On' : '⏭ Skip Ads: Off'}
            </button>
          )}
          <button className="video-jump-btn" onClick={() => skipBy(-(skipIntervals?.skipBack ?? DEFAULT_SKIP_INTERVALS.skipBack))} title={`Back ${(skipIntervals?.skipBack ?? DEFAULT_SKIP_INTERVALS.skipBack)} seconds`}>
            ↺ {skipIntervals?.skipBack ?? DEFAULT_SKIP_INTERVALS.skipBack}s
          </button>
          <button className="video-jump-btn" onClick={() => skipBy(skipIntervals?.skipForward ?? DEFAULT_SKIP_INTERVALS.skipForward)} title={`Forward ${(skipIntervals?.skipForward ?? DEFAULT_SKIP_INTERVALS.skipForward)} seconds`}>
            {skipIntervals?.skipForward ?? DEFAULT_SKIP_INTERVALS.skipForward}s ↻
          </button>
          {diagnosticsEnabled && (
            <button
              className={`video-report-btn ${showStats ? 'video-report-btn--active' : ''}`}
              onClick={() => setShowStats((v) => !v)}
              title="Toggle live playback stats overlay (Shift+S)"
            >
              {showStats ? 'Hide Stats' : 'Stats'}
            </button>
          )}
          {diagnosticsEnabled && (
            <button className="video-report-btn" onClick={copyPlaybackReport} title="Copy playback diagnostics report">
              {reportCopied ? 'Copied' : 'Copy Report'}
            </button>
          )}
          <button
            className="video-jump-btn"
            onClick={() => { void toggleOverlayFullscreen(); }}
            title={isOverlayFullscreen ? 'Exit fullscreen' : 'Enter fullscreen'}
          >
            {isOverlayFullscreen ? '⤢ Exit Fullscreen' : '⤢ Fullscreen'}
          </button>
          <button className="video-close" onClick={stopPlayback} aria-label="Close player">
            ✕
          </button>
        </div>
      </div>

      {/* Commercial indicator bar */}
      {adBlocks.length > 0 && duration > 0 && (
        <div className="video-ad-bar" aria-label="Timeline with commercial markers">
          {adBlocks.map(([start, end], i) => (
            <div
              key={i}
              className={`video-ad-segment ${disabledBlocks.has(i) ? 'video-ad-segment--disabled' : ''}`}
              style={{
                left: `${(start / duration) * 100}%`,
                width: `${Math.max(0.4, ((end - start) / duration) * 100)}%`,
              }}
              title={disabledBlocks.has(i)
                ? `Commercial block ${i + 1} — auto-skip disabled (seeked manually)`
                : `Commercial block ${i + 1}`}
            />
          ))}
          <div
            className="video-ad-bar__playhead"
            style={{ left: `${(currentTime / duration) * 100}%` }}
          />
        </div>
      )}

      {skipping && <div className="video-skip-toast">Skipping commercial…</div>}

      {diagnosticsEnabled && showStats && nerdStats && (
        <div className="video-nerd-panel" aria-live="polite">
          <div className="video-nerd-panel__title">Stats for Nerds</div>
          <div className="video-nerd-panel__row"><span>State</span><strong>{nerdStats.playbackState}</strong></div>
          <div className="video-nerd-panel__row"><span>Video</span><strong>{nerdStats.videoSize}</strong></div>
          <div className="video-nerd-panel__row"><span>Level</span><strong>{nerdStats.currentLevel}</strong></div>
          <div className="video-nerd-panel__row"><span>BW Estimate</span><strong>{nerdStats.bandwidthEstimate}</strong></div>
          <div className="video-nerd-panel__row"><span>Buffer Ahead</span><strong>{nerdStats.bufferAheadSec.toFixed(2)}s</strong></div>
          <div className="video-nerd-panel__row"><span>Dropped/Decoded</span><strong>{nerdStats.droppedFrames ?? 'n/a'} / {nerdStats.decodedFrames ?? 'n/a'}</strong></div>
          <div className="video-nerd-panel__row"><span>Dropped %</span><strong>{nerdStats.droppedPercent !== null ? `${nerdStats.droppedPercent.toFixed(2)}%` : 'n/a'}</strong></div>
          <div className="video-nerd-panel__row"><span>Ready State</span><strong>{nerdStats.readyState}</strong></div>
          <div className="video-nerd-panel__row"><span>Rate</span><strong>{nerdStats.playbackRate.toFixed(2)}x</strong></div>
          <div className="video-nerd-panel__row"><span>Volume</span><strong>{nerdStats.muted ? 'muted' : `${nerdStats.volumePct}%`}</strong></div>
          <div className="video-nerd-panel__row"><span>Last Mutation</span><strong>{lastMutationDebug}</strong></div>
          <div className="video-nerd-panel__row"><span>Last Failure</span><strong>{lastMutationFailure}</strong></div>
          <div className="video-nerd-panel__small" title={nerdStats.manifestUrl}>Manifest: {nerdStats.manifestUrl}</div>
          <div className="video-nerd-panel__small">Updated: {nerdStats.timestampIso}</div>
        </div>
      )}

      {error ? (
        <div className="video-error">
          <span className="video-error__icon">⚠</span>
          <p className="video-error__msg">{error}</p>
          <button className="video-error__close" onClick={stopPlayback}>Close</button>
        </div>
      ) : null}
      <video
        ref={videoRef}
        className="video-element"
        style={error ? { visibility: 'hidden' } : undefined}
        controls
        onEnded={stopPlayback}
      />
    </div>
  );
}
