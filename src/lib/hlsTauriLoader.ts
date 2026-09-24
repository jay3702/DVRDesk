/**
 * Custom hls.js loader that uses @tauri-apps/plugin-http for all requests.
 * This routes HLS manifest and segment fetches through Rust, bypassing
 * WebView2's CORS restrictions for cross-origin streams.
 */

import type {
  HlsConfig,
  Loader,
  LoaderCallbacks,
  LoaderConfiguration,
  LoaderContext,
  LoaderStats,
} from 'hls.js';

function makeStats(): LoaderStats {
  return {
    aborted: false,
    loaded: 0,
    retry: 0,
    total: 0,
    chunkCount: 0,
    bwEstimate: 0,
    loading: { start: 0, first: 0, end: 0 },
    parsing: { start: 0, end: 0 },
    buffering: { start: 0, first: 0, end: 0 },
  };
}

type LoaderClass = new (config: HlsConfig) => Loader<LoaderContext>;

/**
 * Returns a hls.js-compatible Loader class that uses the Tauri HTTP plugin.
 * Returns undefined when running outside Tauri (falls back to hls.js default).
 */
export async function buildTauriHlsLoader(): Promise<LoaderClass | undefined> {
  if (!window.__TAURI_INTERNALS__) return undefined;

  const { fetch: tauriFetch } = await import('@tauri-apps/plugin-http');

  class TauriLoader implements Loader<LoaderContext> {
    context: LoaderContext | null = null;
    stats: LoaderStats = makeStats();
    private aborted = false;
    private controller: AbortController | null = null;

    // hls.js requires constructor to accept HlsConfig even if unused
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    constructor(_config: HlsConfig) {}

    load(
      context: LoaderContext,
      config: LoaderConfiguration,
      callbacks: LoaderCallbacks<LoaderContext>,
    ): void {
      this.context = context;
      this.stats = makeStats();
      this.aborted = false;
      this.stats.loading.start = performance.now();

      const headers: Record<string, string> = { ...(context.headers ?? {}) };

      // Channels DVR chooses HLS encoder strategy based in part on client request
      // fingerprinting. Mimic browser requests so server-side logic aligns with
      // Channels Web UI playback behavior (often remux on capable clients).
      if (!headers['User-Agent']) headers['User-Agent'] = navigator.userAgent;
      if (!headers['Accept']) {
        headers['Accept'] = context.responseType === 'arraybuffer'
          ? '*/*'
          : 'application/vnd.apple.mpegurl, application/x-mpegURL, */*';
      }

      // NOTE: Channels DVR does not support byte-range requests on TS segments
      // and returns HTTP 416. Omit the Range header entirely — HLS over TS
      // never requires byte ranges, so this is safe.
      // (fMP4 streams that genuinely need byte ranges would also need the DVR
      // server to support them, which it doesn't.)

      // hls.js's own retry policy (manifestLoadPolicy/fragLoadPolicy) re-invokes
      // load() on error, but that only helps if onError actually fires. Without
      // a timeout, a request that hangs at the network level (dropped
      // connection, misbehaving proxy, etc.) leaves this promise pending
      // forever — the player just spins with no error and no retry. Use
      // hls.js's own per-load-type timeout (falling back to a sane default)
      // to turn a hang into a normal, retryable onError.
      const timeoutMs = config.timeout > 0 ? config.timeout : 20_000;
      // A real AbortController, not just the `aborted` flag: when hls.js calls
      // abort() (e.g. rapid retries during a buffer-hole nudge storm), the
      // underlying Tauri fetch must actually be cancelled so its native-side
      // response resource is released promptly. Leaving it to resolve/reject
      // on its own after the loader has moved on is what was producing the
      // "resource id N is invalid" rejections under heavy retry churn.
      const controller = new AbortController();
      this.controller = controller;
      const signal = AbortSignal.any([controller.signal, AbortSignal.timeout(timeoutMs)]);

      void (async () => {
        try {
          const res = await tauriFetch(context.url, { method: 'GET', headers, signal });

          if (this.aborted) return;

          if (!res.ok) {
            callbacks.onError(
              { code: res.status, text: res.statusText },
              context,
              null,
              this.stats,
            );
            return;
          }

          this.stats.loading.first = performance.now();

          let data: string | ArrayBuffer;
          if (context.responseType === 'arraybuffer') {
            data = await res.arrayBuffer();
          } else {
            data = await res.text();
          }

          if (this.aborted) return;

          const size = typeof data === 'string' ? data.length : data.byteLength;
          this.stats.loaded = size;
          this.stats.total = size;
          this.stats.loading.end = performance.now();

          callbacks.onSuccess(
            { data, url: context.url },
            this.stats,
            context,
            null,
          );
        } catch (e) {
          if (this.aborted) return;
          const text = signal.aborted
            ? `Request timed out after ${timeoutMs}ms`
            : String(e);
          callbacks.onError(
            { code: 0, text },
            context,
            null,
            this.stats,
          );
        }
      })();
    }

    abort(): void {
      this.aborted = true;
      this.stats.aborted = true;
      this.controller?.abort();
    }

    destroy(): void {
      this.abort();
    }
  }

  return TauriLoader;
}
