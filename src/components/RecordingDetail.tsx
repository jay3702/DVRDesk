import { useEffect, useRef } from 'react';
import { getServerUrl } from '../api/client';

// Minimal shape required to render the detail pane — satisfied by both
// Recording and Episode from the API.
export interface RecordingDetailItem {
  id: string;
  show_id?: string;
  program_id?: string;
  title: string;
  episode_title?: string;
  season_number?: number | null;
  episode_number?: number | null;
  thumbnail_url?: string;
  path: string;
  commercials?: number[];
  playback_time?: number;
  duration: number;
  watched: boolean;
  completed: boolean;
  tags?: string[];
  content_rating?: string;
  favorited: boolean;
  delayed: boolean;
  cancelled: boolean;
  corrupted: boolean;
  full_summary?: string;
  summary?: string;
  genres?: string[];
  created_at: number;
}

interface RecordingDetailProps {
  item: RecordingDetailItem;
  onPlay: () => void;
  /** If provided, shows a "← back" button at the top. */
  onBack?: () => void;
  backLabel?: string;
  /** If provided, shows the "View all episodes of …" link at the bottom. */
  onNavigateToShow?: () => void;
  /** Permanently delete the recording file. */
  onTrash?: () => void;
  /** Mark the program as not recorded so the DVR can re-record it. */
  onMarkNotRecorded?: () => void;
}

function formatDateTime(ms: number) {
  const d = new Date(ms);
  return (
    d.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric', year: 'numeric' }) +
    ' at ' +
    d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
  );
}

function formatDuration(seconds: number) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

function recLabel(item: RecordingDetailItem) {
  return item.episode_title ? `${item.title} — "${item.episode_title}"` : item.title;
}

function resolveThumb(item: RecordingDetailItem): string | undefined {
  const url = item.thumbnail_url;
  if (!url) return undefined;
  if (url.startsWith('http')) return url;
  return `${getServerUrl()}${url}`;
}

export default function RecordingDetail({
  item,
  onPlay,
  onBack,
  backLabel = '← Back',
  onNavigateToShow,
  onTrash,
  onMarkNotRecorded,
}: RecordingDetailProps) {
  const thumb = resolveThumb(item);
  const playBtnRef = useRef<HTMLButtonElement>(null);

  // Auto-focus the play button whenever a new item is shown so Enter triggers playback.
  useEffect(() => {
    playBtnRef.current?.focus({ preventScroll: true });
  }, [item.id]);

  return (
    <div className="rec-detail">
      {onBack && (
        <button className="tv-back-btn" onClick={onBack}>
          {backLabel}
        </button>
      )}

      {thumb ? (
        <button
          ref={playBtnRef}
          className="rec-detail__thumb"
          onClick={onPlay}
          title="Play recording"
        >
          <img src={thumb} alt={recLabel(item)} />
          <div className="rec-detail__play-icon">▶</div>
        </button>
      ) : (
        <button
          ref={playBtnRef}
          className="rec-detail__play-btn"
          onClick={onPlay}
          title="Play recording"
        >
          <span className="rec-detail__play-btn-icon">▶</span> Play
        </button>
      )}

      <div className="rec-detail__body">
        <h2 className="rec-detail__title">{item.title}</h2>

        {item.duration > 0 && !item.watched && (item.playback_time ?? 0) > 0 && (
          <span className="rec-detail__progress" aria-hidden="true">
            <span
              className="rec-detail__progress-fill"
              style={{ width: `${Math.min(((item.playback_time ?? 0) / item.duration) * 100, 100)}%` }}
            />
          </span>
        )}

        {item.episode_title && (
          <p className="rec-detail__episode">
            {item.season_number != null && item.episode_number != null
              ? `S${item.season_number}E${item.episode_number} — `
              : ''}
            {item.episode_title}
          </p>
        )}

        <p className="rec-detail__meta">
          <span>{formatDateTime(item.created_at)}</span>
          {item.tags && item.tags.map((tag) => (
            <span key={tag} className="rec-detail__badge">{tag}</span>
          ))}
          {item.content_rating && (
            <span className="rec-detail__badge">{item.content_rating}</span>
          )}
          {item.favorited && (
            <span className="rec-detail__badge rec-detail__badge--favorite">Favorited</span>
          )}
          {item.delayed && (
            <span className="rec-detail__badge rec-detail__badge--error">Delayed</span>
          )}
          {item.cancelled && (
            <span className="rec-detail__badge rec-detail__badge--error">Cancelled</span>
          )}
          {item.corrupted && (
            <span className="rec-detail__badge rec-detail__badge--error">Interrupted</span>
          )}
          {!item.completed ? (
            <span className="rec-detail__badge rec-detail__badge--recording">Recording</span>
          ) : (
            <span className="rec-detail__duration">{formatDuration(item.duration)}</span>
          )}
        </p>

        {(item.full_summary ?? item.summary) && (
          <p className="rec-detail__description">
            {item.full_summary ?? item.summary}
          </p>
        )}

        <p className="rec-detail__path">Path: {item.path}</p>

        {item.genres && item.genres.length > 0 && (
          <p className="rec-detail__genres">{item.genres.join(' · ')}</p>
        )}

        {onNavigateToShow && (
          <button className="rec-detail__show-link" onClick={onNavigateToShow}>
            View all episodes of {item.title} →
          </button>
        )}

        {(onMarkNotRecorded || onTrash) && (
          <div className="rec-detail__actions">
            {onMarkNotRecorded && (
              <button
                className="rec-detail__action-btn rec-detail__action-btn--secondary"
                onClick={onMarkNotRecorded}
              >
                ✕ Mark as Not Recorded
              </button>
            )}
            {onTrash && (
              <button
                className="rec-detail__action-btn rec-detail__action-btn--danger"
                onClick={() => {
                  if (window.confirm(`Send "${item.episode_title ?? item.title}" to trash?`)) {
                    onTrash();
                  }
                }}
              >
                🗑 Trash
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
