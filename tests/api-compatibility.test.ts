// Integration tests against a live Channels DVR server.
// Run with: CHANNELS_DVR_URL=http://192.168.x.x:8089 npm run test:api
//
// These tests validate that every API endpoint DVRDesk depends on still exists,
// accepts the expected HTTP method, and returns the expected response shape.
// Pass a new server version through these tests before approving it in
// .github/api-version-compatibility.json.

import { beforeAll, describe, expect, it } from 'vitest';

const BASE = (process.env.CHANNELS_DVR_URL ?? 'http://localhost:8089').replace(/\/$/, '');

// ── HTTP helpers ──────────────────────────────────────────────────────────────

async function get<T>(path: string, params?: Record<string, string>): Promise<T> {
  const url = new URL(BASE + path);
  if (params) Object.entries(params).forEach(([k, v]) => url.searchParams.set(k, v));
  const res = await fetch(url.toString());
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}: GET ${path}`);
  return res.json() as Promise<T>;
}

async function method(verb: string, path: string): Promise<Response> {
  return fetch(`${BASE}${path}`, { method: verb });
}

// ── Types (mirrors src/api/types.ts — intentionally duplicated so tests catch field renames) ──

interface Recording {
  id: string; show_id?: string; program_id: string; path: string; channel: string;
  title: string; episode_title?: string; thumbnail_url: string;
  duration: number; playback_time: number;
  watched: boolean; favorited: boolean; delayed: boolean; cancelled: boolean;
  corrupted: boolean; completed: boolean; processed: boolean;
  created_at: number; updated_at: number;
}
interface Show { id: string; name: string; episode_count?: number; updated_at?: number; }
interface Episode {
  id: string; show_id: string; program_id: string; path: string; channel: string;
  title: string; episode_title: string;
  duration: number; playback_time: number;
  watched: boolean; favorited: boolean; created_at: number; updated_at: number;
}
interface Movie {
  id: string; title: string; program_id: string; path: string; channel: string;
  duration: number; playback_time: number;
  watched: boolean; favorited: boolean; created_at: number; updated_at: number;
}
interface Channel { id: string; name: string; number: string; }
interface VideoGroup { id: string; name: string; }
interface Video {
  id: string; video_group_id: string; title: string; video_title: string;
  duration: number; playback_time: number;
  watched: boolean; favorited: boolean; created_at: number; updated_at: number;
}
interface DvrFile {
  ID: string; RuleID: string; GroupID: string; JobID: string;
  Path: string; CreatedAt: number; Duration: number;
}
type SessionsPayload =
  | { ID: string; Channel?: { Number?: string; ID?: string } }[]
  | { live?: { ID: string }[] };

// ── Test fixtures — loaded once before all tests ──────────────────────────────

let recording: Recording | null = null;
let show: Show | null = null;
let movie: Movie | null = null;
let channel: Channel | null = null;
let videoGroup: VideoGroup | null = null;

beforeAll(async () => {
  const tryGet = async <T>(path: string, params?: Record<string, string>): Promise<T[]> => {
    try { return await get<T[]>(path, params); } catch { return []; }
  };
  // Prefer a completed, non-corrupted recording so duration is populated and the file is on disk.
  const allRecordings = await tryGet<Recording>('/api/v1/all', { sort: 'date_added', order: 'desc', source: 'recordings' });
  recording = allRecordings.find(r => r.completed && !r.corrupted) ?? allRecordings[0] ?? null;
  [show]      = await tryGet<Show>('/api/v1/shows');
  [movie]     = await tryGet<Movie>('/api/v1/movies');
  [channel]   = await tryGet<Channel>('/api/v1/channels');
  [videoGroup] = await tryGet<VideoGroup>('/api/v1/video_groups');
}, 30_000);

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('Channels DVR API Compatibility', () => {

  // ── Connectivity ─────────────────────────────────────────────────────────────

  describe('connectivity', () => {
    it('server is reachable via a status endpoint', async () => {
      let reached = false;
      for (const p of ['/api/v1/status', '/api/status', '/status']) {
        try {
          const res = await fetch(`${BASE}${p}`);
          if (res.status < 500) { reached = true; break; }
        } catch { /* try next candidate */ }
      }
      expect(reached, `No status endpoint responded at ${BASE}`).toBe(true);
    });
  });

  // ── Recent Recordings (/api/v1/all) ──────────────────────────────────────────

  describe('GET /api/v1/all?source=recordings', () => {
    it('returns an array', async () => {
      const data = await get<unknown[]>('/api/v1/all', { sort: 'date_added', order: 'desc', source: 'recordings' });
      expect(Array.isArray(data)).toBe(true);
    });

    it('items have all required fields with correct types', () => {
      if (!recording) return;
      const r = recording;
      expect(typeof r.id).toBe('string');
      expect(typeof r.program_id).toBe('string');
      expect(typeof r.path).toBe('string');
      expect(typeof r.channel).toBe('string');
      expect(typeof r.title).toBe('string');
      expect(typeof r.thumbnail_url).toBe('string');
      expect(typeof r.duration).toBe('number');
      expect(typeof r.playback_time).toBe('number');
      expect(typeof r.watched).toBe('boolean');
      expect(typeof r.favorited).toBe('boolean');
      expect(typeof r.delayed).toBe('boolean');
      expect(typeof r.cancelled).toBe('boolean');
      expect(typeof r.corrupted).toBe('boolean');
      expect(typeof r.completed).toBe('boolean');
      expect(typeof r.processed).toBe('boolean');
      expect(typeof r.created_at).toBe('number');
      expect(typeof r.updated_at).toBe('number');
    });
  });

  // ── Shows ─────────────────────────────────────────────────────────────────────

  describe('GET /api/v1/shows', () => {
    it('returns an array', async () => {
      expect(Array.isArray(await get<unknown[]>('/api/v1/shows'))).toBe(true);
    });

    it('items use "name" (not "title") for the show name', () => {
      if (!show) return;
      expect(typeof show.id).toBe('string');
      expect(typeof show.name).toBe('string');
      expect((show as unknown as Record<string, unknown>)['title']).toBeUndefined();
    });
  });

  describe('GET /api/v1/shows/:id', () => {
    it('returns the correct show', async () => {
      if (!show) return;
      const data = await get<Show>(`/api/v1/shows/${encodeURIComponent(show.id)}`);
      expect(data.id).toBe(show.id);
      expect(typeof data.name).toBe('string');
    });
  });

  describe('GET /api/v1/shows/:id/episodes', () => {
    it('returns an array with required fields', async () => {
      if (!show) return;
      const eps = await get<Episode[]>(
        `/api/v1/shows/${encodeURIComponent(show.id)}/episodes`,
        { sort: 'date_added', order: 'desc' },
      );
      expect(Array.isArray(eps)).toBe(true);
      if (!eps.length) return;
      const ep = eps[0];
      expect(typeof ep.id).toBe('string');
      expect(typeof ep.show_id).toBe('string');
      expect(typeof ep.program_id).toBe('string');
      expect(typeof ep.title).toBe('string');
      expect(typeof ep.episode_title).toBe('string');
      expect(typeof ep.duration).toBe('number');
      expect(typeof ep.playback_time).toBe('number');
      expect(typeof ep.watched).toBe('boolean');
      expect(typeof ep.created_at).toBe('number');
      expect(typeof ep.updated_at).toBe('number');
    });
  });

  // ── Movies ────────────────────────────────────────────────────────────────────

  describe('GET /api/v1/movies', () => {
    it('returns an array', async () => {
      expect(Array.isArray(await get<unknown[]>('/api/v1/movies'))).toBe(true);
    });

    it('items have required fields', () => {
      if (!movie) return;
      expect(typeof movie.id).toBe('string');
      expect(typeof movie.title).toBe('string');
      expect(typeof movie.program_id).toBe('string');
      expect(typeof movie.duration).toBe('number');
      expect(typeof movie.playback_time).toBe('number');
      expect(typeof movie.watched).toBe('boolean');
      expect(typeof movie.favorited).toBe('boolean');
      expect(typeof movie.created_at).toBe('number');
      expect(typeof movie.updated_at).toBe('number');
    });
  });

  describe('GET /api/v1/movies/:id', () => {
    it('returns the correct movie', async () => {
      if (!movie) return;
      const data = await get<Movie>(`/api/v1/movies/${encodeURIComponent(movie.id)}`);
      expect(data.id).toBe(movie.id);
      expect(typeof data.title).toBe('string');
    });
  });

  // ── Channels ──────────────────────────────────────────────────────────────────

  describe('GET /api/v1/channels', () => {
    it('returns an array', async () => {
      expect(Array.isArray(await get<unknown[]>('/api/v1/channels'))).toBe(true);
    });

    it('items have id, name, and number as strings', () => {
      if (!channel) return;
      expect(typeof channel.id).toBe('string');
      expect(typeof channel.name).toBe('string');
      expect(typeof channel.number).toBe('string');
    });
  });

  // ── Live Guide ────────────────────────────────────────────────────────────────

  describe('GET /dvr/guide/channels', () => {
    it('returns 200 with parseable JSON', async () => {
      const res = await fetch(`${BASE}/dvr/guide/channels`);
      expect(res.ok, `Expected 200, got ${res.status}`).toBe(true);
      expect(await res.json()).toBeDefined();
    });
  });

  // ── Library ───────────────────────────────────────────────────────────────────

  describe('GET /api/v1/video_groups', () => {
    it('returns an array', async () => {
      expect(Array.isArray(await get<unknown[]>('/api/v1/video_groups'))).toBe(true);
    });

    it('items use "name" (not "title") for the group name', () => {
      if (!videoGroup) return;
      expect(typeof videoGroup.id).toBe('string');
      expect(typeof videoGroup.name).toBe('string');
      expect((videoGroup as unknown as Record<string, unknown>)['title']).toBeUndefined();
    });
  });

  describe('GET /api/v1/video_groups/:id/videos', () => {
    it('returns an array with required fields', async () => {
      if (!videoGroup) return;
      const videos = await get<Video[]>(`/api/v1/video_groups/${videoGroup.id}/videos`);
      expect(Array.isArray(videos)).toBe(true);
      if (!videos.length) return;
      const v = videos[0];
      expect(typeof v.id).toBe('string');
      expect(typeof v.video_group_id).toBe('string');  // not "group_id"
      expect(typeof v.video_title).toBe('string');     // individual title
      expect(typeof v.title).toBe('string');           // group name
      expect(typeof v.duration).toBe('number');
      expect(typeof v.watched).toBe('boolean');
      expect(typeof v.created_at).toBe('number');
      expect(typeof v.updated_at).toBe('number');
    });
  });

  // ── DVR Files ─────────────────────────────────────────────────────────────────

  describe('GET /dvr', () => {
    it('returns an object with an optional path field', async () => {
      const res = await fetch(`${BASE}/dvr`);
      expect(res.ok, `Expected 200, got ${res.status}`).toBe(true);
      const data = await res.json() as Record<string, unknown>;
      expect(typeof data).toBe('object');
      if ('path' in data) expect(typeof data['path']).toBe('string');
    });
  });

  describe('GET /dvr/files/:id', () => {
    it('returns DvrFile with PascalCase required fields', async () => {
      if (!recording) return;
      const file = await get<DvrFile>(`/dvr/files/${encodeURIComponent(recording.id)}`);
      expect(typeof file.ID).toBe('string');
      expect(typeof file.RuleID).toBe('string');
      expect(typeof file.GroupID).toBe('string');
      expect(typeof file.Path).toBe('string');
      expect(typeof file.Duration).toBe('number');
    });
  });

  describe('GET /dvr/files/:id/hls/master.m3u8', () => {
    it('returns 200 with M3U8 content for a known recording', async () => {
      if (!recording) return;
      const res = await fetch(`${BASE}/dvr/files/${encodeURIComponent(recording.id)}/hls/master.m3u8`);
      expect(res.ok, `HLS stream returned ${res.status}`).toBe(true);
      const text = await res.text();
      expect(text.trimStart().startsWith('#EXTM3U'), 'Response body is not M3U8').toBe(true);
    });
  });

  // ── Sessions ──────────────────────────────────────────────────────────────────

  describe('GET /api/v1/sessions', () => {
    it('returns an array or { live: array } (404 acceptable when no sessions are active)', async () => {
      const res = await fetch(`${BASE}/api/v1/sessions`);
      // 404 is normal when no sessions are active — the endpoint may not respond until in use.
      if (res.status === 404) return;
      expect(res.ok, `Sessions endpoint returned unexpected ${res.status}`).toBe(true);
      const data = await res.json() as SessionsPayload;
      const sessions = Array.isArray(data) ? data : (data.live ?? []);
      expect(Array.isArray(sessions)).toBe(true);
    });
  });

  // ── Live HLS probe ────────────────────────────────────────────────────────────

  describe('HEAD /devices/ANY/channels/:number/hls/master.m3u8', () => {
    it('endpoint exists and accepts HEAD (no 405)', async () => {
      if (!channel) return;
      const res = await method(
        'HEAD',
        `/devices/ANY/channels/${encodeURIComponent(channel.number)}/hls/master.m3u8`,
      );
      // 200 = available, 404/503 = tuner busy — all acceptable
      // 405 = method changed (breaking), 500 = unexpected server error
      expect(res.status, 'HEAD returned 405 — method may have changed').not.toBe(405);
      expect(res.status, 'Unexpected server error on live HLS probe').not.toBe(500);
    });
  });

  // ── Safe Mutations ────────────────────────────────────────────────────────────
  // Each test leaves the recording in its original state.

  describe('watch / unwatch', () => {
    it('PUT /dvr/files/:id/watch returns 2xx', async () => {
      if (!recording) return;
      const res = await method('PUT', `/dvr/files/${encodeURIComponent(recording.id)}/watch`);
      expect(res.ok, `watch returned ${res.status}`).toBe(true);
    });

    it('PUT /dvr/files/:id/unwatch returns 2xx', async () => {
      if (!recording) return;
      const res = await method('PUT', `/dvr/files/${encodeURIComponent(recording.id)}/unwatch`);
      expect(res.ok, `unwatch returned ${res.status}`).toBe(true);
    });

    it('original watched state is restored', async () => {
      if (!recording) return;
      const restore = recording.watched ? 'watch' : 'unwatch';
      const res = await method('PUT', `/dvr/files/${encodeURIComponent(recording.id)}/${restore}`);
      expect(res.ok).toBe(true);
    });
  });

  describe('playback_time', () => {
    it('PUT /dvr/files/:id/playback_time/:seconds returns 2xx', async () => {
      if (!recording) return;
      // Use the existing playback position — this is a no-op for the user.
      const seconds = Math.floor(recording.playback_time);
      const res = await method('PUT', `/dvr/files/${encodeURIComponent(recording.id)}/playback_time/${seconds}`);
      expect(res.ok, `playback_time returned ${res.status}`).toBe(true);
    });
  });

  // ── Destructive Endpoint Presence ─────────────────────────────────────────────
  // No real data is deleted. A nonexistent ID is used so the server returns 404,
  // which confirms the endpoint exists and accepts DELETE. A 405 would mean the
  // method changed; that is the breaking signal we are watching for.

  describe('destructive endpoints (presence check — no actual deletion)', () => {
    it('DELETE /dvr/files/:id accepts DELETE method', async () => {
      const res = await method('DELETE', '/dvr/files/dvrdesk-compat-probe');
      expect(res.status, 'Got 405 — DELETE /dvr/files/:id may no longer accept DELETE').not.toBe(405);
      expect(res.status, 'Unexpected 500 on probe ID').not.toBe(500);
    });

    it('DELETE /dvr/programs/:programId accepts DELETE method', async () => {
      const res = await method('DELETE', '/dvr/programs/dvrdesk-compat-probe');
      expect(res.status, 'Got 405 — DELETE /dvr/programs/:id may no longer accept DELETE').not.toBe(405);
      expect(res.status, 'Unexpected 500 on probe ID').not.toBe(500);
    });

    it('DELETE /api/v1/sessions/:id accepts DELETE method', async () => {
      const res = await method('DELETE', '/api/v1/sessions/dvrdesk-compat-probe');
      expect(res.status, 'Got 405 — DELETE /api/v1/sessions/:id may no longer accept DELETE').not.toBe(405);
      expect(res.status, 'Unexpected 500 on probe ID').not.toBe(500);
    });
  });

});
