import { useEffect, useRef, useState } from 'react';
import { BrowserRouter, Link, Routes, Route } from 'react-router-dom';
import Sidebar from './components/Sidebar';
import VideoPlayer from './components/VideoPlayer';
import RecentRecordings from './pages/RecentRecordings';
import Live from './pages/Live';
import TVShows from './pages/TVShows';
import Movies from './pages/Movies';
import Library from './pages/Library';
import Search from './pages/Search';
import Settings from './pages/Settings';
import { useStore } from './store/useStore';
import { useKeyboardNav } from './lib/useKeyboardNav';
import './App.css';

interface UpdateInfo {
  latestVersion: string;
  latestUrl: string;
}

function parseVersionParts(version: string): number[] {
  return version
    .replace(/^v/i, '')
    .split(/[.-]/)
    .map((part) => Number.parseInt(part, 10))
    .map((value) => (Number.isFinite(value) ? value : 0));
}

function isVersionNewer(latest: string, current: string): boolean {
  const a = parseVersionParts(latest);
  const b = parseVersionParts(current);
  const maxLen = Math.max(a.length, b.length);
  for (let i = 0; i < maxLen; i += 1) {
    const av = a[i] ?? 0;
    const bv = b[i] ?? 0;
    if (av > bv) return true;
    if (av < bv) return false;
  }
  return false;
}

async function fetchLatestRelease(): Promise<UpdateInfo | null> {
  try {
    const response = await fetch('https://api.github.com/repos/jay3702/winchannels/releases/latest', {
      headers: {
        Accept: 'application/vnd.github+json',
      },
    });
    if (!response.ok) return null;
    const payload = (await response.json()) as {
      tag_name?: string;
      html_url?: string;
    };
    const latestVersion = String(payload.tag_name ?? '').trim();
    const latestUrl = String(payload.html_url ?? '').trim();
    if (!latestVersion || !latestUrl) return null;
    return { latestVersion, latestUrl };
  } catch {
    return null;
  }
}

function App() {
  const { activeServerId, serverChangeVersion, probeActiveServer, apiVersionApproved, theme } = useStore();
  useKeyboardNav();
  const [probing, setProbing] = useState(true);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const didCheckUpdateRef = useRef(false);

  // Apply theme data attribute to <html> element.
  // 'system' removes the attribute so the CSS media query handles OS preference.
  useEffect(() => {
    const html = document.documentElement;
    if (theme === 'system') {
      html.removeAttribute('data-theme');
    } else {
      html.dataset.theme = theme;
    }
  }, [theme]);

  // On startup and whenever the active server changes, probe the LAN URL and
  // automatically fall back to the Tailscale URL if the LAN is unreachable.
  // Block route rendering until the probe resolves so pages always fetch with
  // the correct server URL.
  useEffect(() => {
    setProbing(true);
    probeActiveServer().finally(() => setProbing(false));
  }, [activeServerId, serverChangeVersion]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (didCheckUpdateRef.current) return;
    didCheckUpdateRef.current = true;
    void (async () => {
      const latest = await fetchLatestRelease();
      if (!latest) return;
      if (isVersionNewer(latest.latestVersion, __APP_VERSION__)) {
        setUpdateInfo(latest);
      }
    })();
  }, []);

  return (
    <BrowserRouter>
      <div className="app-shell">
        <Sidebar />
        <main className="app-main">
          {updateInfo && (
            <div className="app-update-banner" role="status">
              <span>
                New version available: <strong>{updateInfo.latestVersion}</strong> (current: v{__APP_VERSION__})
              </span>
              <a
                className="app-update-banner__link"
                href={updateInfo.latestUrl}
                target="_blank"
                rel="noreferrer"
              >
                View release
              </a>
            </div>
          )}
          {!probing && !apiVersionApproved && (
            <div className="api-version-banner" role="alert">
              <span>⚠ Server/API version changed and is not yet approved in the repository compatibility list. Continue with caution.</span>
              <Link to="/settings" className="api-version-banner__link">Review in Settings</Link>
            </div>
          )}
          {probing ? (
            <div className="app-connecting">Connecting…</div>
          ) : (
            <Routes>
              <Route path="/"         element={<RecentRecordings />} />
              <Route path="/live"     element={<Live />} />
              <Route path="/tv"       element={<TVShows />} />
              <Route path="/movies"   element={<Movies />} />
              <Route path="/library"  element={<Library />} />
              <Route path="/search"   element={<Search />} />
              <Route path="/settings" element={<Settings />} />
            </Routes>
          )}
        </main>
      </div>
      {/* Full-screen overlay when a video is playing */}
      <VideoPlayer />
    </BrowserRouter>
  );
}

export default App;
