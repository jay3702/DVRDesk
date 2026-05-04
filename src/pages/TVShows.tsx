import { useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { fetchShows, fetchEpisodesForShow, setEpisodeWatched, trashRecording, markAsNotRecorded, fetchDvrFile } from '../api/recordings';
import type { Show, Episode } from '../api/types';
import MediaCard from '../components/MediaCard';
import RecordingDetail from '../components/RecordingDetail';
import { useResizableSidebar } from '../lib/useResizableSidebar';
import { useStore } from '../store/useStore';
import './Page.css';

type ShowSortField = 'title' | 'id' | 'date-added' | 'date-updated' | 'last-recorded';
type EpisodeSortField = 'title' | 'id' | 'date-added' | 'date-updated';
const INITIAL_VISIBLE_SHOWS = 90;
const VISIBLE_SHOWS_STEP = 60;
const TV_SHOWS_SORT_STATE_KEY = 'winchannels_tvshows_sort_state_v1';

const tvShowsCache = new Map<string, Show[]>();

function defaultOrderFor(field: ShowSortField | EpisodeSortField): 'asc' | 'desc' {
  return field === 'title' ? 'asc' : 'desc';
}

function loadTvShowsSortState(): {
  showSort: ShowSortField;
  showSortOrder: 'asc' | 'desc';
  episodeSort: EpisodeSortField;
  episodeSortOrder: 'asc' | 'desc';
} {
  try {
    const raw = localStorage.getItem(TV_SHOWS_SORT_STATE_KEY);
    if (!raw) {
      return {
        showSort: 'last-recorded',
        showSortOrder: defaultOrderFor('last-recorded'),
        episodeSort: 'date-added',
        episodeSortOrder: defaultOrderFor('date-added'),
      };
    }
    const parsed = JSON.parse(raw) as Partial<{
      showSort: ShowSortField;
      showSortOrder: 'asc' | 'desc';
      episodeSort: EpisodeSortField;
      episodeSortOrder: 'asc' | 'desc';
    }>;
    const isShowField = (value: unknown): value is ShowSortField =>
      value === 'title' || value === 'id' || value === 'date-added' || value === 'date-updated' || value === 'last-recorded';
    const isEpisodeField = (value: unknown): value is EpisodeSortField =>
      value === 'title' || value === 'id' || value === 'date-added' || value === 'date-updated';
    const showSort = isShowField(parsed.showSort) ? parsed.showSort : 'last-recorded';
    const episodeSort = isEpisodeField(parsed.episodeSort) ? parsed.episodeSort : 'date-added';
    return {
      showSort,
      showSortOrder: parsed.showSortOrder === 'asc' || parsed.showSortOrder === 'desc'
        ? parsed.showSortOrder
        : defaultOrderFor(showSort),
      episodeSort,
      episodeSortOrder: parsed.episodeSortOrder === 'asc' || parsed.episodeSortOrder === 'desc'
        ? parsed.episodeSortOrder
        : defaultOrderFor(episodeSort),
    };
  } catch {
    return {
      showSort: 'last-recorded',
      showSortOrder: defaultOrderFor('last-recorded'),
      episodeSort: 'date-added',
      episodeSortOrder: defaultOrderFor('date-added'),
    };
  }
}

function showIconUrl(show: Show): string | undefined {
  const raw = (
    (show as Show & { PreferredImage?: string }).PreferredImage ||
    (show as Show & { Image?: string }).Image ||
    (show as Show & { preferred_image?: string }).preferred_image ||
    (show as Show & { image?: string }).image ||
    show.image_url
  );
  if (!raw) return undefined;
  try {
    const url = new URL(raw);
    url.searchParams.set('w', '80');
    url.searchParams.set('h', '60');
    return url.toString();
  } catch {
    return raw;
  }
}

/** Build card title/subtitle with fallbacks for sparse DVR metadata. */
function epLabel(ep: Episode): { title: string; subtitle?: string } {
  const hasS = ep.season_number != null && Number.isFinite(Number(ep.season_number));
  const hasE = ep.episode_number != null && Number.isFinite(Number(ep.episode_number));
  const sub = ep.episode_title || ep.title || undefined;
  // When no season/episode info exists, use the recorded date — it's the only
  // field that uniquely identifies a specific airing (original_air_date is the
  // same for every re-run of the same old episode).
  const recordedLabel = ep.created_at
    ? new Date(ep.created_at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' })
    : 'Unknown';
  if (hasS && hasE) return { title: `S${ep.season_number}E${ep.episode_number}`, subtitle: sub };
  if (hasS) return { title: `S${ep.season_number}`, subtitle: sub };
  if (hasE) return { title: `E${ep.episode_number}`, subtitle: sub };
  return { title: recordedLabel, subtitle: sub };
}

/** Build MediaCard badges for an episode (mirrors the flags shown in RecordingDetail). */
function epBadges(ep: Episode): { label: string; type: 'default' | 'favorite' | 'error' }[] {
  const out: { label: string; type: 'default' | 'favorite' | 'error' }[] = [];
  ep.tags?.forEach((t) => out.push({ label: t, type: 'default' }));
  if (ep.content_rating) out.push({ label: ep.content_rating, type: 'default' });
  if (ep.favorited)  out.push({ label: 'Favorited',   type: 'favorite' });
  if (ep.delayed)    out.push({ label: 'Delayed',     type: 'error' });
  if (ep.cancelled)  out.push({ label: 'Cancelled',   type: 'error' });
  if (ep.corrupted)  out.push({ label: 'Interrupted', type: 'error' });
  if (!ep.completed) out.push({ label: 'Recording',   type: 'error' });
  return out;
}

export default function TVShows() {
  const initialSortState = loadTvShowsSortState();
  const activeServerId = useStore((s) => s.activeServerId);
  const cacheKey = activeServerId;
  const cachedShows = tvShowsCache.get(cacheKey);
  const [shows, setShows] = useState<Show[]>(cachedShows ?? []);
  const [selectedShow, setSelectedShow] = useState<Show | null>(null);
  const [episodes, setEpisodes] = useState<Episode[]>([]);
  const [selectedEpisode, setSelectedEpisode] = useState<Episode | null>(null);
  const [showSort, setShowSort] = useState<ShowSortField>(initialSortState.showSort);
  const [showSortOrder, setShowSortOrder] = useState<'asc' | 'desc'>(initialSortState.showSortOrder);
  const [episodeSort, setEpisodeSort] = useState<EpisodeSortField>(initialSortState.episodeSort);
  const [episodeSortOrder, setEpisodeSortOrder] = useState<'asc' | 'desc'>(initialSortState.episodeSortOrder);
  const [filter, setFilter] = useState<'all' | 'unwatched'>('all');
  const [loadingShows, setLoadingShows] = useState(!cachedShows);
  const [loadingEps, setLoadingEps] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [epError, setEpError] = useState<string | null>(null);
  const [watchBusyId, setWatchBusyId] = useState<string | null>(null);
  const [selectedEpisodeRuleId, setSelectedEpisodeRuleId] = useState<string | null>(null);
  const [visibleShowCount, setVisibleShowCount] = useState(INITIAL_VISIBLE_SHOWS);
  const [searchParams, setSearchParams] = useSearchParams();
  const serverChangeVersion = useStore((s) => s.serverChangeVersion);
  const playItem = useStore((s) => s.playItem);
  const { width: sidebarWidth, isResizing, handleMouseDown } = useResizableSidebar({
    initialWidth: 220,
    minWidth: 180,
    maxWidth: 520,
  });
  const requestedShowId = searchParams.get('showId');
  const requestedEpisodeId = searchParams.get('episodeId');
  const prevEpisodeIdRef = useRef<string | null>(null);

  // Restore focus to the episode card when the detail pane closes.
  useEffect(() => {
    if (selectedEpisode) {
      prevEpisodeIdRef.current = selectedEpisode.id;
    } else if (prevEpisodeIdRef.current) {
      const id = prevEpisodeIdRef.current;
      prevEpisodeIdRef.current = null;
      requestAnimationFrame(() => {
        const card = document.querySelector<HTMLElement>(`[data-media-id="${id}"]`);
        if (card) {
          card.focus({ preventScroll: true });
          card.scrollIntoView({ block: 'nearest' });
        }
      });
    }
  }, [selectedEpisode?.id]);

  // When a show's episodes finish loading with no pre-selected episode, focus the first card.
  useEffect(() => {
    if (loadingEps || !selectedShow || selectedEpisode) return;
    requestAnimationFrame(() => {
      const first = document.querySelector<HTMLElement>('.media-grid .media-card[tabindex="0"]');
      if (first) first.focus({ preventScroll: true });
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadingEps, selectedShow?.id]);
  const showListRef = useRef<HTMLUListElement | null>(null);

  useEffect(() => {
    localStorage.setItem(
      TV_SHOWS_SORT_STATE_KEY,
      JSON.stringify({ showSort, showSortOrder, episodeSort, episodeSortOrder }),
    );
  }, [showSort, showSortOrder, episodeSort, episodeSortOrder]);

  useEffect(() => {
    const rawFilter = searchParams.get('filter');
    const nextFilter: 'all' | 'unwatched' = rawFilter === 'unwatched' ? 'unwatched' : 'all';
    setFilter((prev) => (prev === nextFilter ? prev : nextFilter));
  }, [searchParams]);

  function updateFilter(next: 'all' | 'unwatched') {
    setFilter(next);
    const params = new URLSearchParams(searchParams);
    if (next === 'all') {
      params.delete('filter');
    } else {
      params.set('filter', next);
    }
    setSearchParams(params, { replace: true });
  }

  const sortedShows = useMemo(() => {
    const list = [...shows];
    switch (showSort) {
      case 'title':
        list.sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }));
        break;
      case 'id':
        list.sort((a, b) => a.id.localeCompare(b.id, undefined, { sensitivity: 'base' }));
        break;
      case 'date-added':
        list.sort((a, b) => (a.created_at ?? 0) - (b.created_at ?? 0));
        break;
      case 'date-updated':
        list.sort((a, b) => (a.updated_at ?? 0) - (b.updated_at ?? 0));
        break;
      case 'last-recorded':
        list.sort((a, b) => (a.last_recorded_at ?? 0) - (b.last_recorded_at ?? 0));
        break;
    }
    if (showSortOrder === 'desc') list.reverse();
    return list;
  }, [shows, showSort, showSortOrder]);

  const sortedEpisodes = useMemo(() => {
    const list = filter === 'unwatched' ? episodes.filter((ep) => !ep.watched) : [...episodes];
    switch (episodeSort) {
      case 'title':
        list.sort((a, b) => {
          const ak = (a.episode_title || a.title || '').trim();
          const bk = (b.episode_title || b.title || '').trim();
          return ak.localeCompare(bk, undefined, { sensitivity: 'base' });
        });
        break;
      case 'id':
        list.sort((a, b) => a.id.localeCompare(b.id, undefined, { sensitivity: 'base' }));
        break;
      case 'date-added':
        list.sort((a, b) => (a.created_at ?? 0) - (b.created_at ?? 0));
        break;
      case 'date-updated':
        list.sort((a, b) => (a.updated_at ?? 0) - (b.updated_at ?? 0));
        break;
    }
    if (episodeSortOrder === 'desc') list.reverse();
    return list;
  }, [episodes, filter, episodeSort, episodeSortOrder]);

  useEffect(() => {
    setVisibleShowCount(INITIAL_VISIBLE_SHOWS);
  }, [showSort, showSortOrder, sortedShows.length]);

  useEffect(() => {
    const node = showListRef.current;
    if (!node) return;

    const handleScroll = () => {
      const remaining = node.scrollHeight - node.scrollTop - node.clientHeight;
      if (remaining > 220) return;
      setVisibleShowCount((current) => {
        if (current >= sortedShows.length) return current;
        return Math.min(current + VISIBLE_SHOWS_STEP, sortedShows.length);
      });
    };

    handleScroll();
    node.addEventListener('scroll', handleScroll);
    return () => node.removeEventListener('scroll', handleScroll);
  }, [sortedShows.length, visibleShowCount]);

  useEffect(() => {
    if (!selectedShow) return;
    const index = sortedShows.findIndex((show) => show.id === selectedShow.id);
    if (index < 0 || index < visibleShowCount) return;
    setVisibleShowCount(index + 1);
  }, [selectedShow, sortedShows, visibleShowCount]);

  const displayedShows = useMemo(() => {
    return sortedShows.slice(0, visibleShowCount);
  }, [sortedShows, visibleShowCount]);

  useEffect(() => {
    const cached = tvShowsCache.get(cacheKey);
    setLoadingShows(!cached);
    setError(null);
    setEpError(null);
    setSelectedShow(null);
    setEpisodes([]);
    setSelectedEpisode(null);
    if (cached) {
      setShows(cached);
    }

    fetchShows()
      .then((loaded) => {
        tvShowsCache.set(cacheKey, loaded);
        setShows(loaded);
        if (requestedShowId) {
          const match = loaded.find((s) => s.id === requestedShowId);
          if (match) selectShow(match, requestedEpisodeId ?? undefined);
        }
      })
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoadingShows(false));
  }, [cacheKey, requestedEpisodeId, requestedShowId, serverChangeVersion]);

  function selectShow(show: Show, preferredEpisodeId?: string) {
    console.log('[selectShow] called with:', show);
    setSelectedShow(show);
    setEpisodes([]);
    setSelectedEpisode(null);
    setSelectedEpisodeRuleId(null);
    setEpError(null);
    setLoadingEps(true);
    fetchEpisodesForShow(String(show.id))
      .then((eps) => {
        console.log('[selectShow] episodes loaded:', eps);
        setEpisodes(eps);
        if (preferredEpisodeId) {
          const preferred = eps.find((ep) => ep.id === preferredEpisodeId);
          if (preferred) {
            selectEpisode(preferred);
            return;
          }
        }
        setSelectedEpisode(null);
      })
      .catch((e: unknown) => {
        const msg = e instanceof Error ? e.message : String(e);
        console.error('[selectShow] error loading episodes:', msg);
        setEpError(msg);
      })
      .finally(() => {
        setLoadingEps(false);
        console.log('[selectShow] done');
      });
  }

  async function toggleWatched(episode: Episode) {
    const nextWatched = !episode.watched;
    const previous = episodes;

    setEpError(null);
    setWatchBusyId(episode.id);
    setEpisodes((list) => list.map((ep) => (ep.id === episode.id ? { ...ep, watched: nextWatched } : ep)));
    setSelectedEpisode((prev) => (prev?.id === episode.id ? { ...prev, watched: nextWatched } : prev));

    try {
      await setEpisodeWatched(episode.id, nextWatched);
    } catch (e) {
      setEpisodes(previous);
      setSelectedEpisode((prev) => {
        if (!prev || prev.id !== episode.id) return prev;
        const restored = previous.find((ep) => ep.id === prev.id);
        return restored ?? prev;
      });
      const msg = e instanceof Error ? e.message : String(e);
      setEpError(`Failed to update watched state: ${msg}`);
    } finally {
      setWatchBusyId(null);
    }
  }

  function selectEpisode(ep: Episode) {
    setSelectedEpisode(ep);
    setSelectedEpisodeRuleId(null);
    fetchDvrFile(ep.id)
      .then((file) => setSelectedEpisodeRuleId(file.RuleID || null))
      .catch(() => setSelectedEpisodeRuleId(null));
  }

  async function handleTrashEpisode(episode: Episode) {
    try {
      await trashRecording(episode.id);
      setEpisodes((list) => list.filter((ep) => ep.id !== episode.id));
      setSelectedEpisode(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setEpError(`Failed to trash recording: ${msg}`);
    }
  }

  async function handleMarkNotRecorded(episode: Episode) {
    const programId = episode.program_id;
    if (!programId) {
      setEpError('Cannot mark as not recorded: program_id is missing.');
      return;
    }
    try {
      await markAsNotRecorded(programId);
      setSelectedEpisode(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setEpError(`Failed to mark as not recorded: ${msg}`);
    }
  }

  return (
    <div className={`page page--split ${isResizing ? 'resizing' : ''}`}>
      {/* Show list */}
      <aside className="show-list" style={{ width: `${sidebarWidth}px`, minWidth: `${sidebarWidth}px` }}>
        <h2 className="show-list__title">TV Shows</h2>
        <div className="show-list__sort">
          <select
            className="page-sort-select"
            value={showSort}
            onChange={(e) => {
              const next = e.target.value as ShowSortField;
              setShowSort(next);
              setShowSortOrder(defaultOrderFor(next));
            }}
            aria-label="Sort TV show list"
          >
            <option value="title">Title</option>
            <option value="id">ID</option>
            <option value="date-added">Date Added</option>
            <option value="date-updated">Date Updated</option>
            <option value="last-recorded">Last Recorded</option>
          </select>
          <div className="page-sort-order-stack" role="group" aria-label="TV show list sort direction">
            <button
              type="button"
              className={`page-sort-order-btn page-sort-order-btn--up ${showSortOrder === 'asc' ? 'page-sort-order-btn--active' : ''}`}
              onClick={() => setShowSortOrder('asc')}
              aria-pressed={showSortOrder === 'asc'}
              aria-label="Sort TV show list ascending"
              title="Ascending"
            >
              ▲
            </button>
            <button
              type="button"
              className={`page-sort-order-btn page-sort-order-btn--down ${showSortOrder === 'desc' ? 'page-sort-order-btn--active' : ''}`}
              onClick={() => setShowSortOrder('desc')}
              aria-pressed={showSortOrder === 'desc'}
              aria-label="Sort TV show list descending"
              title="Descending"
            >
              ▼
            </button>
          </div>
        </div>
        {loadingShows && <p className="page__status">Loading…</p>}
        {error && <p className="page__error">⚠ {error}</p>}
        <ul className="show-list__items" ref={showListRef}>
          {displayedShows.map((show) => (
            <li key={show.id}>
              <button
                className={`show-item ${selectedShow?.id === show.id ? 'show-item--active' : ''}`}
                onClick={() => {
                  console.log('[show card] clicked', show);
                  selectShow(show);
                }}
              >
                {showIconUrl(show) ? (
                  <img className="show-item__thumb" src={showIconUrl(show)} alt="" aria-hidden="true" />
                ) : (
                  <span className="show-item__icon" aria-hidden="true">📺</span>
                )}
                <span className="show-item__name">{show.name}</span>
              </button>
            </li>
          ))}
          {displayedShows.length < sortedShows.length && (
            <li>
              <p className="page__status">Scroll for more shows…</p>
            </li>
          )}
        </ul>
      </aside>

      <div
        className={`resize-handle ${isResizing ? 'resize-handle--active' : ''}`}
        onMouseDown={handleMouseDown}
      />

      {/* Main content */}
      <div className="page__content">
        {selectedEpisode ? (
          /* ── Episode detail view ── */
          <RecordingDetail
            item={selectedEpisode}
            onPlay={() => {
              const { title, subtitle } = epLabel(selectedEpisode);
              const label = subtitle ? `${title} – ${subtitle}` : title;
              playItem(selectedEpisode.id, label, selectedEpisode.path, selectedEpisode.commercials, '', selectedEpisode.playback_time, 'episode');
            }}
            onBack={() => setSelectedEpisode(null)}
            backLabel="← Back to episodes"
            onTrash={() => void handleTrashEpisode(selectedEpisode)}
            onMarkNotRecorded={selectedEpisodeRuleId && selectedEpisode.program_id ? () => void handleMarkNotRecorded(selectedEpisode) : undefined}
          />
        ) : selectedShow ? (
          /* ── Series detail view ── */
          <>
            <header className="page__header">
              <h1 className="page__title">{selectedShow.name}</h1>
              <div className="page__filters">
                <button
                  className={`filter-btn ${filter === 'all' ? 'filter-btn--active' : ''}`}
                  onClick={() => updateFilter('all')}
                >
                  All
                </button>
                <button
                  className={`filter-btn ${filter === 'unwatched' ? 'filter-btn--active' : ''}`}
                  onClick={() => updateFilter('unwatched')}
                >
                  Unwatched
                </button>
                <select
                  className="page-sort-select"
                  value={episodeSort}
                  onChange={(e) => {
                    const next = e.target.value as EpisodeSortField;
                    setEpisodeSort(next);
                    setEpisodeSortOrder(defaultOrderFor(next));
                  }}
                  aria-label="Sort episodes"
                >
                  <option value="title">Title</option>
                  <option value="id">ID</option>
                  <option value="date-added">Date Added</option>
                  <option value="date-updated">Date Updated</option>
                </select>
                <div className="page-sort-order-stack" role="group" aria-label="Episode sort direction">
                  <button
                    type="button"
                    className={`page-sort-order-btn page-sort-order-btn--up ${episodeSortOrder === 'asc' ? 'page-sort-order-btn--active' : ''}`}
                    onClick={() => setEpisodeSortOrder('asc')}
                    aria-pressed={episodeSortOrder === 'asc'}
                    aria-label="Sort episodes ascending"
                    title="Ascending"
                  >
                    ▲
                  </button>
                  <button
                    type="button"
                    className={`page-sort-order-btn page-sort-order-btn--down ${episodeSortOrder === 'desc' ? 'page-sort-order-btn--active' : ''}`}
                    onClick={() => setEpisodeSortOrder('desc')}
                    aria-pressed={episodeSortOrder === 'desc'}
                    aria-label="Sort episodes descending"
                    title="Descending"
                  >
                    ▼
                  </button>
                </div>
              </div>
            </header>
            {loadingEps && <p className="page__status">Loading episodes…</p>}
            {epError && <p className="page__error">⚠ {epError}</p>}

            {(selectedShow.image_url || selectedShow.summary) && (
              <section className="tv-series-info">
                {selectedShow.image_url && (
                  <img className="tv-series-info__image" src={selectedShow.image_url} alt={selectedShow.name} />
                )}
                {selectedShow.summary && (
                  <p className="tv-series-info__description">{selectedShow.summary}</p>
                )}
              </section>
            )}

            <div className="media-grid">
              {sortedEpisodes.map((ep) => {
                const { title, subtitle } = epLabel(ep);
                return (
                  <MediaCard
                    key={ep.id}
                    id={ep.id}
                    title={title}
                    subtitle={subtitle}
                    imageUrl={ep.image_url}
                    thumbnailUrl={ep.thumbnail_url}
                    duration={ep.duration}
                    watched={ep.watched}
                    playbackTime={ep.playback_time}
                    badges={epBadges(ep)}
                    completed={ep.completed}
                    commercials={ep.commercials}
                    filePath={ep.path}
                    recordedAt={ep.created_at}
                    recordedAtFormat="datetime"
                    onClick={() => selectEpisode(ep)}
                    onPlayAction={() => {
                      const { title, subtitle } = epLabel(ep);
                      const label = subtitle ? `${title} – ${subtitle}` : title;
                      playItem(ep.id, label, ep.path, ep.commercials, '', ep.playback_time, 'episode');
                    }}
                    playActionLabel="Play episode"
                    onToggleWatched={() => {
                      void toggleWatched(ep);
                    }}
                    watchedActionBusy={watchBusyId === ep.id}
                    recordingKind="episode"
                    ariaLabel={`Select ${subtitle ? `${title} – ${subtitle}` : title}`}
                  />
                );
              })}
              {sortedEpisodes.length === 0 && <p className="page__status">No episodes found.</p>}
            </div>
          </>
        ) : (
          /* ── Empty state ── */
          <div className="page__empty">
            <p>Select a show to see its episodes.</p>
          </div>
        )}
      </div>
    </div>
  );
}
