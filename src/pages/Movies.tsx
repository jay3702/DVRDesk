import { useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { fetchMovies, setMovieWatched } from '../api/recordings';
import type { Movie } from '../api/types';
import MediaCard from '../components/MediaCard';
import { useStore } from '../store/useStore';
import './Page.css';

type SortField = 'title' | 'id' | 'date-added' | 'date-updated' | 'release-date';
const INITIAL_VISIBLE_MOVIES = 120;
const VISIBLE_MOVIES_STEP = 80;
const MOVIES_SORT_STATE_KEY = 'winchannels_movies_sort_state_v1';

const moviesCache = new Map<string, Movie[]>();

function defaultOrderFor(field: SortField): 'asc' | 'desc' {
  if (field === 'title' || field === 'release-date') return 'asc';
  return 'desc';
}

function loadMoviesSortState(): { sort: SortField; sortOrder: 'asc' | 'desc' } {
  try {
    const raw = localStorage.getItem(MOVIES_SORT_STATE_KEY);
    if (!raw) return { sort: 'date-added', sortOrder: defaultOrderFor('date-added') };
    const parsed = JSON.parse(raw) as Partial<{ sort: SortField; sortOrder: 'asc' | 'desc' }>;
    const sort = parsed.sort === 'title' || parsed.sort === 'id' || parsed.sort === 'date-added' || parsed.sort === 'date-updated' || parsed.sort === 'release-date'
      ? parsed.sort
      : 'date-added';
    return {
      sort,
      sortOrder: parsed.sortOrder === 'asc' || parsed.sortOrder === 'desc'
        ? parsed.sortOrder
        : defaultOrderFor(sort),
    };
  } catch {
    return { sort: 'date-added', sortOrder: defaultOrderFor('date-added') };
  }
}

function labelKey(key: string): string {
  return key
    .replace(/_/g, ' ')
    .replace(/\b\w/g, (m) => m.toUpperCase());
}

function formatValue(value: unknown): string {
  if (value == null) return '';
  if (typeof value === 'boolean') return value ? 'Yes' : 'No';
  if (Array.isArray(value)) return value.join(', ');
  if (typeof value === 'number' && String(value).length >= 12) {
    return new Date(value).toLocaleString('en-US');
  }
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

function movieAttributes(movie: Movie): Array<{ key: string; label: string; value: string }> {
  const hidden = new Set(['image_url', 'thumbnail_url', 'commercials', 'path']);
  return Object.entries(movie)
    .filter(([key, value]) => !hidden.has(key) && value != null && value !== '')
    .map(([key, value]) => ({ key, label: labelKey(key), value: formatValue(value) }))
    .sort((a, b) => a.label.localeCompare(b.label));
}

/** Build MediaCard badges for a movie (mirrors the flags shown in RecordingDetail). */
function movieBadges(movie: Movie): { label: string; type: 'default' | 'favorite' | 'error' }[] {
  const out: { label: string; type: 'default' | 'favorite' | 'error' }[] = [];
  movie.tags?.forEach((t) => out.push({ label: t, type: 'default' }));
  if (movie.content_rating) out.push({ label: movie.content_rating, type: 'default' });
  if (movie.favorited)  out.push({ label: 'Favorited',   type: 'favorite' });
  if (movie.delayed)    out.push({ label: 'Delayed',     type: 'error' });
  if (movie.cancelled)  out.push({ label: 'Cancelled',   type: 'error' });
  if (movie.corrupted)  out.push({ label: 'Interrupted', type: 'error' });
  if (!movie.completed) out.push({ label: 'Recording',   type: 'error' });
  return out;
}

export default function Movies() {
  const initialSortState = loadMoviesSortState();
  const activeServerId = useStore((s) => s.activeServerId);
  const cacheKey = activeServerId;
  const cachedMovies = moviesCache.get(cacheKey);
  const [movies, setMovies] = useState<Movie[]>(cachedMovies ?? []);
  const [selectedMovie, setSelectedMovie] = useState<Movie | null>(null);
  const [loading, setLoading] = useState(!cachedMovies);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [watchBusyId, setWatchBusyId] = useState<string | null>(null);
  const [filter, setFilter] = useState<'all' | 'unwatched'>('all');
  const [sort, setSort] = useState<SortField>(initialSortState.sort);
  const [sortOrder, setSortOrder] = useState<'asc' | 'desc'>(initialSortState.sortOrder);
  const [visibleMovieCount, setVisibleMovieCount] = useState(INITIAL_VISIBLE_MOVIES);
  const serverChangeVersion = useStore((s) => s.serverChangeVersion);
  const playItem = useStore((s) => s.playItem);
  const [searchParams] = useSearchParams();
  const requestedMovieId = searchParams.get('movieId');
  const pageRef = useRef<HTMLDivElement | null>(null);
  const moviePlayBtnRef = useRef<HTMLButtonElement>(null);
  const prevMovieIdRef = useRef<string | null>(null);

  // Auto-focus the play button when detail opens; restore focus to the card when it closes.
  useEffect(() => {
    if (selectedMovie) {
      prevMovieIdRef.current = selectedMovie.id;
      moviePlayBtnRef.current?.focus({ preventScroll: true });
    } else if (prevMovieIdRef.current) {
      const id = prevMovieIdRef.current;
      prevMovieIdRef.current = null;
      requestAnimationFrame(() => {
        const card = document.querySelector<HTMLElement>(`[data-media-id="${id}"]`);
        if (card) {
          card.focus({ preventScroll: true });
          card.scrollIntoView({ block: 'nearest' });
        }
      });
    }
  }, [selectedMovie?.id]);

  useEffect(() => {
    localStorage.setItem(MOVIES_SORT_STATE_KEY, JSON.stringify({ sort, sortOrder }));
  }, [sort, sortOrder]);

  useEffect(() => {
    const cached = moviesCache.get(cacheKey);
    setLoading(!cached);
    setError(null);
    setActionError(null);
    if (cached) {
      setMovies(cached);
      setSelectedMovie((prev) => {
        if (requestedMovieId) return cached.find((m) => m.id === requestedMovieId) ?? null;
        return prev ? cached.find((m) => m.id === prev.id) ?? prev : null;
      });
    }

    fetchMovies({ sort: 'date_added', order: 'desc' })
      .then((loaded) => {
        moviesCache.set(cacheKey, loaded);
        setMovies(loaded);
        setSelectedMovie((prev) => {
          if (requestedMovieId) return loaded.find((m) => m.id === requestedMovieId) ?? null;
          return prev ? loaded.find((m) => m.id === prev.id) ?? prev : null;
        });
      })
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoading(false));
  }, [cacheKey, requestedMovieId, serverChangeVersion]);

  const displayed = filter === 'unwatched' ? movies.filter((m) => !m.watched) : movies;
  const sortedDisplayed = useMemo(() => {
    const list = [...displayed];
    switch (sort) {
      case 'title':
        list.sort((a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: 'base' }));
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
      case 'release-date':
        list.sort((a, b) => {
          // Prefer release_date (ISO string); fall back to release_year as a string
          const ad = a.release_date ?? (a.release_year != null ? String(a.release_year) : '');
          const bd = b.release_date ?? (b.release_year != null ? String(b.release_year) : '');
          if (!ad && !bd) return 0;
          if (!ad) return 1;
          if (!bd) return -1;
          return ad.localeCompare(bd);
        });
        break;
    }
    if (sortOrder === 'desc') list.reverse();
    return list;
  }, [displayed, sort, sortOrder]);

  useEffect(() => {
    setVisibleMovieCount(INITIAL_VISIBLE_MOVIES);
  }, [filter, sort, sortOrder, sortedDisplayed.length]);

  useEffect(() => {
    if (selectedMovie) return;
    const node = pageRef.current;
    if (!node) return;

    const handleScroll = () => {
      const remaining = node.scrollHeight - node.scrollTop - node.clientHeight;
      if (remaining > 260) return;
      setVisibleMovieCount((current) => {
        if (current >= sortedDisplayed.length) return current;
        return Math.min(current + VISIBLE_MOVIES_STEP, sortedDisplayed.length);
      });
    };

    handleScroll();
    node.addEventListener('scroll', handleScroll);
    return () => node.removeEventListener('scroll', handleScroll);
  }, [selectedMovie, sortedDisplayed.length, visibleMovieCount]);

  const displayedMovies = useMemo(() => {
    return sortedDisplayed.slice(0, visibleMovieCount);
  }, [sortedDisplayed, visibleMovieCount]);

  async function toggleWatched(movie: Movie) {
    const nextWatched = !movie.watched;
    const previous = movies;

    setActionError(null);
    setWatchBusyId(movie.id);
    setMovies((list) => list.map((m) => (m.id === movie.id ? { ...m, watched: nextWatched } : m)));
    setSelectedMovie((prev) => (prev?.id === movie.id ? { ...prev, watched: nextWatched } : prev));

    try {
      await setMovieWatched(movie.id, nextWatched);
    } catch (e) {
      setMovies(previous);
      setSelectedMovie((prev) => {
        if (!prev || prev.id !== movie.id) return prev;
        const restored = previous.find((m) => m.id === prev.id);
        return restored ?? prev;
      });
      const msg = e instanceof Error ? e.message : String(e);
      setActionError(`Failed to update watched state: ${msg}`);
    } finally {
      setWatchBusyId(null);
    }
  }

  return (
    <div className="page" ref={pageRef}>
      <header className="page__header">
        <h1 className="page__title">Movies</h1>
        <div className="page__filters">
          <button
            className={`filter-btn ${filter === 'all' ? 'filter-btn--active' : ''}`}
            onClick={() => setFilter('all')}
          >
            All
          </button>
          <button
            className={`filter-btn ${filter === 'unwatched' ? 'filter-btn--active' : ''}`}
            onClick={() => setFilter('unwatched')}
          >
            Unwatched
          </button>
          <select
            className="page-sort-select"
            value={sort}
            onChange={(e) => {
              const next = e.target.value as SortField;
              setSort(next);
              setSortOrder(defaultOrderFor(next));
            }}
            aria-label="Sort movies"
          >
            <option value="release-date">Release Date</option>
            <option value="date-added">Date Added</option>
            <option value="date-updated">Date Updated</option>
            <option value="title">Title</option>
            <option value="id">ID</option>
          </select>
          <div className="page-sort-order-stack" role="group" aria-label="Movie sort direction">
            <button
              type="button"
              className={`page-sort-order-btn page-sort-order-btn--up ${sortOrder === 'asc' ? 'page-sort-order-btn--active' : ''}`}
              onClick={() => setSortOrder('asc')}
              aria-label="Sort movies ascending"
              title="Ascending"
            >
              ▲
            </button>
            <button
              type="button"
              className={`page-sort-order-btn page-sort-order-btn--down ${sortOrder === 'desc' ? 'page-sort-order-btn--active' : ''}`}
              onClick={() => setSortOrder('desc')}
              aria-label="Sort movies descending"
              title="Descending"
            >
              ▼
            </button>
          </div>
        </div>
      </header>

      {loading && <p className="page__status">Loading…</p>}
      {error && <p className="page__error">⚠ {error}</p>}
      {actionError && <p className="page__error">⚠ {actionError}</p>}

      {!loading && !error && (
        <>
          {selectedMovie && (
            <section className="media-detail media-detail--full">
              <button className="media-detail__back" onClick={() => setSelectedMovie(null)}>
                ← Back to movie list
              </button>
              <button
                ref={moviePlayBtnRef}
                className="media-detail__thumb"
                onClick={() => playItem(
                  selectedMovie.id,
                  selectedMovie.title,
                  selectedMovie.path,
                  selectedMovie.commercials,
                  '',
                  selectedMovie.playback_time,
                  'movie'
                )}
                title="Play movie"
              >
                <img src={selectedMovie.thumbnail_url || selectedMovie.image_url || ''} alt={selectedMovie.title} />
                <div className="media-detail__play-icon">▶</div>
              </button>
              <h2 className="media-detail__title">{selectedMovie.title}</h2>
              <p className="media-detail__meta">
                {new Date(selectedMovie.created_at).toLocaleDateString('en-US', {
                  month: 'short', day: 'numeric', year: 'numeric',
                })}
                {' · '}
                {new Date(selectedMovie.created_at).toLocaleTimeString('en-US', {
                  hour: 'numeric', minute: '2-digit',
                })}
                {selectedMovie.content_rating ? ` · ${selectedMovie.content_rating}` : ''}
              </p>
              {(selectedMovie.full_summary || selectedMovie.summary) && (
                <p className="media-detail__description">{selectedMovie.full_summary || selectedMovie.summary}</p>
              )}
              <p className="media-detail__path">Path: {selectedMovie.path}</p>
              <dl className="media-attrs" aria-label="Movie details">
                {movieAttributes(selectedMovie).map((attr) => (
                  <div key={attr.key} className="media-attrs__row">
                    <dt>{attr.label}</dt>
                    <dd>{attr.value}</dd>
                  </div>
                ))}
              </dl>
            </section>
          )}

          {!selectedMovie && (
            <div className="media-grid">
              {displayedMovies.map((movie) => (
                <MediaCard
                  key={movie.id}
                  id={movie.id}
                  title={movie.title}
                  subtitle={movie.release_year ? String(movie.release_year) : undefined}
                  imageUrl={movie.image_url}
                  thumbnailUrl={movie.thumbnail_url}
                  duration={movie.duration}
                  watched={movie.watched}
                  playbackTime={movie.playback_time}
                  badges={movieBadges(movie)}
                  completed={movie.completed}
                  commercials={movie.commercials}
                  filePath={movie.path}
                  recordedAt={movie.created_at}
                  recordedAtFormat="datetime"
                  onClick={() => setSelectedMovie(movie)}
                  onToggleWatched={() => {
                    void toggleWatched(movie);
                  }}
                  watchedActionBusy={watchBusyId === movie.id}
                  recordingKind="movie"
                  selected={false}
                  ariaLabel={`Select ${movie.title}`}
                />
              ))}
              {sortedDisplayed.length === 0 && <p className="page__status">No movies found.</p>}
              {displayedMovies.length < sortedDisplayed.length && (
                <p className="page__status">Scroll for more movies…</p>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}
