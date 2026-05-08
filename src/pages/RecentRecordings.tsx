import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { fetchChannels, fetchRecordings, trashRecording, markAsNotRecorded, fetchDvrFile } from '../api/recordings';
import type { Channel, Recording } from '../api/types';
import { useStore } from '../store/useStore';
import type { AppState } from '../store/useStore';
import { applyLogoFallback, buildChannelLogoMap, logoForChannelKey } from '../lib/channelLogos';
import { useResizableSidebar } from '../lib/useResizableSidebar';
import RecordingDetail from '../components/RecordingDetail';
import './Page.css';

interface RecentRecordingsCacheEntry {
  recordings: Recording[];
  channelLogos: Record<string, string>;
  newestCreatedAt: number;
  groups: ReturnType<typeof groupByDayTime>;
}

const recentRecordingsCache = new Map<string, RecentRecordingsCacheEntry>();
const INITIAL_VISIBLE_DAY_GROUPS = 3;
const VISIBLE_DAY_GROUPS_STEP = 2;

// ── Helpers ────────────────────────────────────────────────────────────────

function newestCreatedAt(recordings: Recording[]): number {
  return recordings.reduce((max, rec) => Math.max(max, rec.created_at ?? 0), 0);
}

function hasRecordingFeedChanged(previous: Recording[], next: Recording[]): boolean {
  if (previous.length !== next.length) return true;
  for (let i = 0; i < previous.length; i += 1) {
    if (previous[i]?.id !== next[i]?.id) return true;
  }
  return false;
}

function writeRecentRecordingsCache(key: string, recordings: Recording[], channelLogos: Record<string, string>) {
  recentRecordingsCache.set(key, {
    recordings,
    channelLogos,
    newestCreatedAt: newestCreatedAt(recordings),
    groups: groupByDayTime(recordings),
  });
}

function groupByDayTime(recordings: Recording[]) {
  const groups: {
    dateKey: string;
    label: string;
    timeGroups: { timeKey: string; timeLabel: string; items: Recording[] }[];
  }[] = [];
  let lastKey = '';
  let lastTimeKey = '';
  for (const rec of recordings) {
    const d = new Date(rec.created_at);
    const key = `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    const timeLabel = d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
    const timeKey = `${key}-${timeLabel}`;
    if (key !== lastKey) {
      const label = d.toLocaleDateString('en-US', {
        weekday: 'long', month: 'long', day: 'numeric', year: 'numeric',
      });
      groups.push({ dateKey: key, label, timeGroups: [] });
      lastKey = key;
      lastTimeKey = '';
    }
    if (timeKey !== lastTimeKey) {
      groups[groups.length - 1].timeGroups.push({ timeKey, timeLabel, items: [] });
      lastTimeKey = timeKey;
    }
    groups[groups.length - 1].timeGroups[groups[groups.length - 1].timeGroups.length - 1].items.push(rec);
  }
  return groups;
}

function recLabel(rec: Recording) {
  return rec.episode_title ? `${rec.title} — "${rec.episode_title}"` : rec.title;
}

function logoForRecording(rec: Recording, logoMap: Record<string, string>): string | null {
  return logoForChannelKey(rec.channel, logoMap);
}

// ── Component ──────────────────────────────────────────────────────────────

export default function RecentRecordings() {
  const activeServerId = useStore((s: AppState) => s.activeServerId);
  const serverChangeVersion = useStore((s: AppState) => s.serverChangeVersion);
  const cacheKey = `${activeServerId}:${serverChangeVersion}`;
  const cachedEntry = recentRecordingsCache.get(cacheKey);
  const [, setRecordings] = useState<Recording[]>(cachedEntry?.recordings ?? []);
  const [groups, setGroups] = useState<ReturnType<typeof groupByDayTime>>(cachedEntry?.groups ?? []);
  const [channelLogos, setChannelLogos] = useState<Record<string, string>>(cachedEntry?.channelLogos ?? {});
  const [selected, setSelected] = useState<Recording | null>(null);
  const [selectedRuleId, setSelectedRuleId] = useState<string | null>(null);
  const [loading, setLoading] = useState(!cachedEntry);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [visibleDayGroups, setVisibleDayGroups] = useState(INITIAL_VISIBLE_DAY_GROUPS);
  const listRef = useRef<HTMLDivElement | null>(null);
  const { width: sidebarWidth, isResizing, handleMouseDown } = useResizableSidebar({
    initialWidth: 300,
    minWidth: 200,
    maxWidth: 600,
  });
  const navigate = useNavigate();
  const playItem = useStore((s: AppState) => s.playItem);
  function selectRecording(rec: Recording) {
    setSelected(rec);
    setSelectedRuleId(null);
    fetchDvrFile(rec.id)
      .then((file) => setSelectedRuleId(file.RuleID || null))
      .catch(() => setSelectedRuleId(null));
  }

  async function handleTrash(rec: Recording) {
    try {
      await trashRecording(rec.id);
      setRecordings((list) => {
        const next = list.filter((r) => r.id !== rec.id);
        writeRecentRecordingsCache(cacheKey, next, channelLogos);
        setGroups(groupByDayTime(next));
        return next;
      });
      setSelected(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setActionError(`Failed to trash recording: ${msg}`);
    }
  }

  async function handleMarkNotRecorded(rec: Recording) {
    if (!rec.program_id) {
      setActionError('Cannot mark as not recorded: program_id is missing.');
      return;
    }
    try {
      await markAsNotRecorded(rec.program_id);
      setSelected(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setActionError(`Failed to mark as not recorded: ${msg}`);
    }
  }

  useEffect(() => {
    let cancelled = false;
    const cached = recentRecordingsCache.get(cacheKey);
    setVisibleDayGroups(INITIAL_VISIBLE_DAY_GROUPS);
    setLoading(!cached);
    setError(null);
    setSelected(null);
    setSelectedRuleId(null);
    if (cached) {
      setRecordings(cached.recordings);
      setGroups(cached.groups);
      setChannelLogos(cached.channelLogos);
    } else {
      setRecordings([]);
      setGroups([]);
      setChannelLogos({});
    }

    void (async () => {
      try {
        const [loadedRecordings, loadedChannels] = await Promise.all([
          fetchRecordings(),
          fetchChannels().catch(() => [] as Channel[]),
        ]);
        if (cancelled) return;
        const nextLogos = buildChannelLogoMap(loadedChannels);
        const previousRecordings = cached?.recordings ?? [];
        const hasNewItems = loadedRecordings.some((rec) => rec.created_at > (cached?.newestCreatedAt ?? 0));
        const feedChanged = hasRecordingFeedChanged(previousRecordings, loadedRecordings);

        writeRecentRecordingsCache(cacheKey, loadedRecordings, nextLogos);

        if (!cached || hasNewItems || feedChanged) {
          setRecordings(loadedRecordings);
          setGroups(groupByDayTime(loadedRecordings));
          setChannelLogos(nextLogos);
        }
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [cacheKey]);

  useEffect(() => {
    const node = listRef.current;
    if (!node) return;

    const handleScroll = () => {
      const remaining = node.scrollHeight - node.scrollTop - node.clientHeight;
      if (remaining > 240) return;
      setVisibleDayGroups((current) => {
        if (current >= groups.length) return current;
        return Math.min(current + VISIBLE_DAY_GROUPS_STEP, groups.length);
      });
    };

    handleScroll();
    node.addEventListener('scroll', handleScroll);
    return () => node.removeEventListener('scroll', handleScroll);
  }, [groups.length, visibleDayGroups]);

  const displayedGroups = groups.slice(0, visibleDayGroups);

  return (
    <div className={`page page--split ${isResizing ? 'resizing' : ''}`}>
      {/* LEFT: recordings list */}
      <aside className="rec-list" style={{ width: `${sidebarWidth}px`, minWidth: `${sidebarWidth}px` }}>
        <h2 className="show-list__title">Recent Recordings</h2>
        {loading && <p className="page__status">Loading…</p>}
        {error && <p className="page__error">⚠ {error}</p>}
        <div className="rec-list__items" ref={listRef}>
          {displayedGroups.map((group) => (
            <div key={group.dateKey}>
              <div className="rec-day-header">{group.label}</div>
              {group.timeGroups.map((timeGroup) => (
                <div key={timeGroup.timeKey}>
                  <div className="rec-time-group-header">{timeGroup.timeLabel}</div>
                  {timeGroup.items.map((rec) => {
                    const logoUrl = logoForRecording(rec, channelLogos);
                    const progress = rec.duration > 0
                      ? Math.min(((rec.playback_time ?? 0) / rec.duration) * 100, 100)
                      : 0;
                    return (
                      <button
                        key={rec.id}
                        className={`rec-item ${selected?.id === rec.id ? 'rec-item--active' : ''}`}
                        onClick={() => selectRecording(rec)}
                        onDoubleClick={() => playItem(
                          rec.id,
                          recLabel(rec),
                          rec.path,
                          rec.commercials,
                          '',
                          rec.playback_time,
                          rec.show_id ? 'episode' : 'movie'
                        )}
                      >
                        {logoUrl && (
                          <img
                            src={logoUrl}
                            alt={rec.channel}
                            className="rec-item__logo"
                            onError={(e) => applyLogoFallback(e.currentTarget)}
                          />
                        )}
                        <span className="rec-item__main">
                          <span className="rec-item__title">{recLabel(rec)}</span>
                          {progress > 0 && !rec.watched && (
                            <span className="rec-item__progress" aria-hidden="true">
                              <span className="rec-item__progress-fill" style={{ width: `${progress}%` }} />
                            </span>
                          )}
                        </span>
                      </button>
                    );
                  })}
                </div>
              ))}
            </div>
          ))}
          {displayedGroups.length < groups.length && (
            <p className="page__status">Scroll for older recordings…</p>
          )}
        </div>
      </aside>

      {/* RESIZE HANDLE */}
      <div
        className={`resize-handle ${isResizing ? 'resize-handle--active' : ''}`}
        onMouseDown={handleMouseDown}
      />

      {/* RIGHT: detail pane */}
      <div className="page__content">
        {actionError && <p className="page__error">⚠ {actionError}</p>}
        {selected ? (
          <RecordingDetail
            item={selected}
            onPlay={() => playItem(
              selected.id,
              recLabel(selected),
              selected.path,
              selected.commercials,
              '',
              selected.playback_time,
              selected.show_id ? 'episode' : 'movie'
            )}
            onNavigateToShow={selected.show_id
              ? () => navigate(`/tv?showId=${selected.show_id}`)
              : undefined
            }
            onTrash={() => void handleTrash(selected)}
            onMarkNotRecorded={selectedRuleId && selected.program_id ? () => void handleMarkNotRecorded(selected) : undefined}
          />
        ) : (
          <div className="page__empty">
            <p>Select a recording to see details.</p>
          </div>
        )}
      </div>
    </div>
  );
}
