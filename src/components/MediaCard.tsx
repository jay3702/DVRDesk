import { getServerUrl } from '../api/client';
import './MediaCard.css';

interface MediaCardBadge {
  label: string;
  type: 'default' | 'favorite' | 'error';
}

interface MediaCardProps {
  id: string;
  title: string;
  subtitle?: string;
  imageUrl?: string;
  thumbnailUrl?: string;
  duration: number;
  watched: boolean;
  playbackTime?: number;
  badge?: string; // legacy, single badge
  badges?: MediaCardBadge[]; // new, multiple badges
  commercials?: number[];
  filePath?: string;
  recordedAt?: number;
  recordedAtFormat?: 'time' | 'datetime';
  onClick?: () => void;
  selected?: boolean;
  ariaLabel?: string;
  onPlayAction?: () => void;
  playActionLabel?: string;
  onToggleWatched?: () => void;
  watchedActionLabel?: string;
  watchedActionBusy?: boolean;
  recordingKind?: 'episode' | 'movie' | null;
  completed?: boolean;
}

function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
}

function formatDateTime(ms: number): string {
  const d = new Date(ms);
  return (
    d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' }) +
    ' · ' +
    d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
  );
}

export default function MediaCard({
  id,
  title,
  subtitle,
  imageUrl,
  thumbnailUrl,
  duration,
  watched,
  playbackTime = 0,
  badge,
  recordedAt,
  recordedAtFormat = 'time',
  onClick,
  selected = false,
  ariaLabel,
  onPlayAction,
  playActionLabel = 'Play',
  onToggleWatched,
  watchedActionLabel,
  watchedActionBusy = false,
  badges = [],
  completed = true, // default to true for backward compatibility
}: MediaCardProps) {
  // Always use an array for badges
  const safeBadges = badges ?? [];

  // Resolve relative thumbnail URLs against the server
  const resolvedThumb = thumbnailUrl?.startsWith('http') ? thumbnailUrl
    : thumbnailUrl ? `${getServerUrl()}${thumbnailUrl}`
    : imageUrl ?? null;

  const toggleLabel = watchedActionLabel ?? (watched ? 'Mark unwatched' : 'Mark watched');

  // Calculate progress bar (if needed)
  const progress = duration > 0 && playbackTime > 0 ? Math.min(100, Math.round((playbackTime / duration) * 100)) : 0;

  return (
    <div
      className={`media-card ${watched ? 'media-card--watched' : ''} ${selected ? 'media-card--selected' : ''}`}
      role="button"
      tabIndex={0}
      data-media-id={id}
      onClick={onClick}
      aria-label={ariaLabel}
      onKeyDown={e => {
        if (!onClick) return;
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onClick();
        }
      }}
    >
      <div className="media-card__art">
        {resolvedThumb ? (
          <img src={resolvedThumb} alt="" draggable={false} />
        ) : (
          <div className="media-card__art-placeholder">▶</div>
        )}
        {onPlayAction && (
          <button
            className="media-card__play-action"
            onClick={e => {
              e.stopPropagation();
              onPlayAction();
            }}
            aria-label={`${playActionLabel} ${title}`}
            title={playActionLabel}
          >
            ▶
          </button>
        )}
        {onToggleWatched && (
          <button
            className="media-card__watch-action"
            onClick={e => {
              e.stopPropagation();
              if (!watchedActionBusy) onToggleWatched();
            }}
            aria-label={`${toggleLabel} ${title}`}
            title={toggleLabel}
            disabled={watchedActionBusy}
          >
            {watchedActionBusy ? '…' : watched ? '↺' : '✓'}
          </button>
        )}
      </div>
      <div className="media-card__info">
        <span className="media-card__title">{title}</span>
        {subtitle && <span className="media-card__subtitle">{subtitle}</span>}
        <span className="media-card__badges">
          {safeBadges.length > 0
            ? safeBadges.map((b, i) => (
                <span
                  key={b.label + i}
                  className={`media-card__badge${b.type !== 'default' ? ` media-card__badge--${b.type}` : ''}`}
                >
                  {b.label}
                </span>
              ))
            : badge && <span className="media-card__badge">{badge}</span>}
        </span>
        <div className="media-card__meta-row">
          {recordedAt != null && (
            <span className="media-card__time">
              {recordedAtFormat === 'datetime'
                ? formatDateTime(recordedAt)
                : formatTime(recordedAt)}
            </span>
          )}
          <span className="media-card__duration">
            {completed === false ? 'Recording' : formatDuration(duration)}
          </span>
        </div>
      </div>
      {(watched || progress > 0) && (
        <div className="media-card__progress">
          <div
            className={`media-card__progress-fill${watched ? ' media-card__progress-fill--watched' : ''}`}
            style={{ width: watched ? '100%' : `${progress}%` }}
          />
        </div>
      )}
    </div>
  );
}
