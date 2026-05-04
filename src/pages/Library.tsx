import { useEffect, useMemo, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { fetchVideoGroups, fetchVideosInGroup } from '../api/library';
import type { VideoGroup, Video } from '../api/types';
import MediaCard from '../components/MediaCard';
import { useResizableSidebar } from '../lib/useResizableSidebar';
import { useStore } from '../store/useStore';
import './Page.css';

type SortField = 'title' | 'id' | 'date-added' | 'date-updated';
const LIBRARY_SORT_STATE_KEY = 'winchannels_library_sort_state_v1';

function defaultOrderFor(field: SortField): 'asc' | 'desc' {
  return field === 'title' ? 'asc' : 'desc';
}

function loadLibrarySortState(): {
  groupSort: SortField;
  groupSortOrder: 'asc' | 'desc';
  videoSort: SortField;
  videoSortOrder: 'asc' | 'desc';
} {
  try {
    const raw = localStorage.getItem(LIBRARY_SORT_STATE_KEY);
    if (!raw) {
      return {
        groupSort: 'title',
        groupSortOrder: defaultOrderFor('title'),
        videoSort: 'date-added',
        videoSortOrder: defaultOrderFor('date-added'),
      };
    }
    const parsed = JSON.parse(raw) as Partial<{
      groupSort: SortField;
      groupSortOrder: 'asc' | 'desc';
      videoSort: SortField;
      videoSortOrder: 'asc' | 'desc';
    }>;
    const isField = (value: unknown): value is SortField =>
      value === 'title' || value === 'id' || value === 'date-added' || value === 'date-updated';
    const groupSort = isField(parsed.groupSort) ? parsed.groupSort : 'title';
    const videoSort = isField(parsed.videoSort) ? parsed.videoSort : 'date-added';
    return {
      groupSort,
      groupSortOrder: parsed.groupSortOrder === 'asc' || parsed.groupSortOrder === 'desc'
        ? parsed.groupSortOrder
        : defaultOrderFor(groupSort),
      videoSort,
      videoSortOrder: parsed.videoSortOrder === 'asc' || parsed.videoSortOrder === 'desc'
        ? parsed.videoSortOrder
        : defaultOrderFor(videoSort),
    };
  } catch {
    return {
      groupSort: 'title',
      groupSortOrder: defaultOrderFor('title'),
      videoSort: 'date-added',
      videoSortOrder: defaultOrderFor('date-added'),
    };
  }
}

export default function Library() {
  const initialSortState = loadLibrarySortState();
  const [groups, setGroups] = useState<VideoGroup[]>([]);
  const [selectedGroup, setSelectedGroup] = useState<VideoGroup | null>(null);
  const [selectedVideoId, setSelectedVideoId] = useState<string | null>(null);
  const [videos, setVideos] = useState<Video[]>([]);
  const [groupSort, setGroupSort] = useState<SortField>(initialSortState.groupSort);
  const [groupSortOrder, setGroupSortOrder] = useState<'asc' | 'desc'>(initialSortState.groupSortOrder);
  const [videoSort, setVideoSort] = useState<SortField>(initialSortState.videoSort);
  const [videoSortOrder, setVideoSortOrder] = useState<'asc' | 'desc'>(initialSortState.videoSortOrder);
  const [loadingGroups, setLoadingGroups] = useState(true);
  const [loadingVideos, setLoadingVideos] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const serverChangeVersion = useStore((s) => s.serverChangeVersion);
  const playItem = useStore((s) => s.playItem);
  const { width: sidebarWidth, isResizing, handleMouseDown } = useResizableSidebar({
    initialWidth: 220,
    minWidth: 180,
    maxWidth: 520,
  });
  const [searchParams] = useSearchParams();
  const requestedGroupId = searchParams.get('groupId');
  const requestedVideoId = searchParams.get('videoId');

  useEffect(() => {
    localStorage.setItem(
      LIBRARY_SORT_STATE_KEY,
      JSON.stringify({ groupSort, groupSortOrder, videoSort, videoSortOrder }),
    );
  }, [groupSort, groupSortOrder, videoSort, videoSortOrder]);

  const sortedGroups = useMemo(() => {
    const list = [...groups];
    switch (groupSort) {
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
    }
    if (groupSortOrder === 'desc') list.reverse();
    return list;
  }, [groups, groupSort, groupSortOrder]);

  const sortedVideos = useMemo(() => {
    const list = [...videos];
    switch (videoSort) {
      case 'title':
        list.sort((a, b) => a.video_title.localeCompare(b.video_title, undefined, { sensitivity: 'base' }));
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
    if (videoSortOrder === 'desc') list.reverse();
    return list;
  }, [videos, videoSort, videoSortOrder]);

  useEffect(() => {
    setLoadingGroups(true);
    setError(null);
    setSelectedGroup(null);
    setSelectedVideoId(null);
    setVideos([]);
    fetchVideoGroups()
      .then((loaded) => {
        setGroups(loaded);
        if (requestedGroupId) {
          const match = loaded.find((g) => g.id === requestedGroupId);
          if (match) selectGroup(match, requestedVideoId ?? undefined);
        }
      })
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoadingGroups(false));
  }, [requestedGroupId, requestedVideoId, serverChangeVersion]);

  function selectGroup(group: VideoGroup, preferredVideoId?: string) {
    setSelectedGroup(group);
    setSelectedVideoId(null);
    setVideos([]);
    setLoadingVideos(true);
    fetchVideosInGroup(group.id)
      .then((loaded) => {
        setVideos(loaded);
        if (preferredVideoId) {
          const preferred = loaded.find((video) => video.id === preferredVideoId);
          if (preferred) setSelectedVideoId(preferred.id);
        }
      })
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoadingVideos(false));
  }

  return (
    <div className={`page page--split ${isResizing ? 'resizing' : ''}`}>
      <aside className="show-list" style={{ width: `${sidebarWidth}px`, minWidth: `${sidebarWidth}px` }}>
        <h2 className="show-list__title">Videos</h2>
        <div className="show-list__sort">
          <select
            className="page-sort-select"
            value={groupSort}
            onChange={(e) => {
              const next = e.target.value as SortField;
              setGroupSort(next);
              setGroupSortOrder(defaultOrderFor(next));
            }}
            aria-label="Sort library list"
          >
            <option value="title">Title</option>
            <option value="id">ID</option>
            <option value="date-added">Date Added</option>
            <option value="date-updated">Date Updated</option>
          </select>
          <div className="page-sort-order-stack" role="group" aria-label="Library list sort direction">
            <button
              type="button"
              className={`page-sort-order-btn page-sort-order-btn--up ${groupSortOrder === 'asc' ? 'page-sort-order-btn--active' : ''}`}
              onClick={() => setGroupSortOrder('asc')}
              aria-pressed={groupSortOrder === 'asc'}
              aria-label="Sort library list ascending"
              title="Ascending"
            >
              ▲
            </button>
            <button
              type="button"
              className={`page-sort-order-btn page-sort-order-btn--down ${groupSortOrder === 'desc' ? 'page-sort-order-btn--active' : ''}`}
              onClick={() => setGroupSortOrder('desc')}
              aria-pressed={groupSortOrder === 'desc'}
              aria-label="Sort library list descending"
              title="Descending"
            >
              ▼
            </button>
          </div>
        </div>
        {loadingGroups && <p className="page__status">Loading…</p>}
        {error && <p className="page__error">⚠ {error}</p>}
        <ul className="show-list__items">
          {sortedGroups.map((g) => (
            <li key={g.id}>
              <button
                className={`show-item ${selectedGroup?.id === g.id ? 'show-item--active' : ''}`}
                onClick={() => selectGroup(g)}
              >
                {g.name}
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <div
        className={`resize-handle ${isResizing ? 'resize-handle--active' : ''}`}
        onMouseDown={handleMouseDown}
      />

      <div className="page__content">
        {selectedGroup ? (
          <>
            <header className="page__header">
              <h1 className="page__title">{selectedGroup.name}</h1>
              <div className="page__filters">
                <select
                  className="page-sort-select"
                  value={videoSort}
                  onChange={(e) => {
                    const next = e.target.value as SortField;
                    setVideoSort(next);
                    setVideoSortOrder(defaultOrderFor(next));
                  }}
                  aria-label="Sort library grid"
                >
                  <option value="title">Title</option>
                  <option value="id">ID</option>
                  <option value="date-added">Date Added</option>
                  <option value="date-updated">Date Updated</option>
                </select>
                <div className="page-sort-order-stack" role="group" aria-label="Library grid sort direction">
                  <button
                    type="button"
                    className={`page-sort-order-btn page-sort-order-btn--up ${videoSortOrder === 'asc' ? 'page-sort-order-btn--active' : ''}`}
                    onClick={() => setVideoSortOrder('asc')}
                    aria-pressed={videoSortOrder === 'asc'}
                    aria-label="Sort library grid ascending"
                    title="Ascending"
                  >
                    ▲
                  </button>
                  <button
                    type="button"
                    className={`page-sort-order-btn page-sort-order-btn--down ${videoSortOrder === 'desc' ? 'page-sort-order-btn--active' : ''}`}
                    onClick={() => setVideoSortOrder('desc')}
                    aria-pressed={videoSortOrder === 'desc'}
                    aria-label="Sort library grid descending"
                    title="Descending"
                  >
                    ▼
                  </button>
                </div>
              </div>
            </header>
            {loadingVideos && <p className="page__status">Loading videos…</p>}
            <div className="media-grid">
              {sortedVideos.map((v) => (
                <MediaCard
                  key={v.id}
                  id={v.id}
                  title={v.video_title}
                  imageUrl={v.image_url}
                  thumbnailUrl={v.thumbnail_url}
                  duration={v.duration}
                  watched={v.watched}
                  playbackTime={v.playback_time}
                  onClick={() => setSelectedVideoId(v.id)}
                  selected={selectedVideoId === v.id}
                  ariaLabel={`Select ${v.video_title}`}
                  onPlayAction={() => playItem(v.id, v.video_title, undefined, [], undefined, v.playback_time, null)}
                  playActionLabel="Play video"
                />
              ))}
            </div>
          </>
        ) : (
          <div className="page__empty">
            <p>Select a library section to browse videos.</p>
          </div>
        )}
      </div>
    </div>
  );
}
