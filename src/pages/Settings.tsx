import { useEffect, useState } from 'react';
import { useStore, type ServerOption, DEFAULT_KEYBINDINGS, DEFAULT_SKIP_INTERVALS, type KeybindingsConfig, type SkipIntervalsConfig, type AppTheme } from '../store/useStore';
import {
  fetchCompatibilityMatrix,
  fetchServerVersionInfo,
  isVersionVerified,
  normalizeServerUrl,
  requestFromServer,
  probeUrl,
} from '../api/client';
import { getRecentClientErrorLogText } from '../lib/clientErrorLog';
import './Page.css';

function makeServerId(): string {
  return `srv_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
}

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
    const response = await fetch('https://api.github.com/repos/jay3702/dvrdesk/releases/latest', {
      headers: { Accept: 'application/vnd.github+json' },
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

interface BuildBugReportDraftInput {
  activeServerName: string;
  apiVersion: string | null;
  apiPublicVersion: string | null;
  apiVersionApproved: boolean;
  apiCompatibilityNote: string | null;
  testResult: string | null;
  errorLogText: string;
}

function buildBugReportDraft(input: BuildBugReportDraftInput): string {
  const compatibility = input.apiVersionApproved ? 'verified' : 'not-verified';
  const details = input.testResult ? input.testResult : 'Test Connection has not been run yet.';
  const appMode = window.__TAURI_INTERNALS__ ? 'Tauri desktop' : 'Browser development';

  return [
    '# DVRDesk Bug Report',
    '',
    'Please include a concise summary and exact steps to reproduce before posting this to Channels Community.',
    'Remove or edit any private hostnames or IP addresses if you do not want them included in a public post.',
    '',
    '## Environment',
    `- DVRDesk version: ${__APP_VERSION__}`,
    `- Client mode: ${appMode}`,
    `- User agent: ${navigator.userAgent}`,
    `- Active server: ${input.activeServerName}`,
    `- Internal server version: ${input.apiVersion ?? 'not detected'}`,
    `- Public API version: ${input.apiPublicVersion ?? 'not detected'}`,
    `- Compatibility status: ${compatibility}`,
    `- Compatibility note: ${input.apiCompatibilityNote ?? 'none'}`,
    '',
    '## Summary',
    '<Describe the problem you saw>',
    '',
    '## Steps To Reproduce',
    '1. <Step one>',
    '2. <Step two>',
    '3. <Expected result>',
    '4. <Actual result>',
    '',
    '## Server Check Output',
    '```',
    details,
    '```',
    '',
    '## Client Error Log (last 48 hours)',
    '```',
    input.errorLogText,
    '```',
    '',
    '## Additional Notes',
    '<Optional screenshots, timing notes, or other context>',
  ].join('\n');
}

function redactServerUrls(text: string, serverList: ServerOption[]): string {
  let out = text;
  for (const server of serverList) {
    const urls = [server.url, server.tailscaleUrl].filter(Boolean) as string[];
    for (const url of urls) {
      const escaped = url.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      out = out.replace(new RegExp(escaped, 'g'), `[${server.name}]`);
      try {
        const { host } = new URL(url);
        const escapedHost = host.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        out = out.replace(new RegExp(escapedHost, 'g'), `[${server.name}]`);
      } catch {
        // Ignore malformed URLs.
      }
    }
  }
  return out;
}

function areSameServers(a: ServerOption[], b: ServerOption[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((left, i) => {
    const right = b[i];
    return left.id === right.id && left.name === right.name && left.url === right.url && left.tailscaleUrl === right.tailscaleUrl;
  });
}

const ACTION_LABELS: Record<keyof KeybindingsConfig, string> = {
  skipForward: 'Skip Forward',
  skipBack: 'Skip Back',
  fastForward: 'Fast Forward',
  fastReverse: 'Fast Reverse',
  playPause: 'Play / Pause',
  close: 'Close Player',
};

export default function Settings() {
  const {
    servers,
    setServers,
    storageSharePath,
    setStorageSharePath,
    preferRemux,
    setPreferRemux,
    diagnosticsEnabled,
    setDiagnosticsEnabled,
    showHiddenLiveChannels,
    setShowHiddenLiveChannels,
    apiVersion,
    apiPublicVersion,
    apiVersionApproved,
    apiCompatibilityNote,
    keybindings,
    setKeybindings,
    skipIntervals,
    setSkipIntervals,
    theme,
    setTheme,
    windowAlwaysOnTop,
    setWindowAlwaysOnTop,
  } = useStore();

  const [draftServers, setDraftServers] = useState<ServerOption[]>(servers);
  const [shareDraft, setShareDraft] = useState(storageSharePath);
  const [saved, setSaved] = useState(false);
  const [serverSaveError, setServerSaveError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [preparingReport, setPreparingReport] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [bugReportOpen, setBugReportOpen] = useState(false);
  const [bugReportDraft, setBugReportDraft] = useState('');
  const [logViewOpen, setLogViewOpen] = useState(false);
  const [logViewText, setLogViewText] = useState('');
  const [reportCopyMessage, setReportCopyMessage] = useState<string | null>(null);
  const [bindingsDraft, setBindingsDraft] = useState<KeybindingsConfig>(keybindings);
  const [intervalsDraft, setIntervalsDraft] = useState<SkipIntervalsConfig>(skipIntervals);
  const [playerSettingsSaved, setPlayerSettingsSaved] = useState(false);

  useEffect(() => { setDraftServers(servers); }, [servers]);
  useEffect(() => { setBindingsDraft(keybindings); }, [keybindings]);
  useEffect(() => { setIntervalsDraft(skipIntervals); }, [skipIntervals]);

  useEffect(() => {
    void (async () => {
      const latest = await fetchLatestRelease();
      if (!latest) return;
      if (isVersionNewer(latest.latestVersion, __APP_VERSION__)) {
        setUpdateInfo(latest);
      }
    })();
  }, []);

  function handleKeybindingChange(action: keyof KeybindingsConfig, value: string) {
    setBindingsDraft((prev) => ({ ...prev, [action]: value.split(',').map(s => s.trim()).filter(Boolean) }));
  }

  function handleIntervalChange(action: keyof SkipIntervalsConfig, value: string) {
    const num = Number(value);
    if (!isNaN(num) && num > 0) {
      setIntervalsDraft((prev) => ({ ...prev, [action]: num }));
    }
  }

  function savePlayerSettings() {
    setKeybindings(bindingsDraft);
    setSkipIntervals(intervalsDraft);
    setPlayerSettingsSaved(true);
    setTimeout(() => setPlayerSettingsSaved(false), 2000);
  }

  function updateServer(serverId: string, field: 'name' | 'url' | 'tailscaleUrl', value: string) {
    setDraftServers((prev) =>
      prev.map((server) =>
        server.id === serverId ? { ...server, [field]: value } : server
      )
    );
    setSaved(false);
    setServerSaveError(null);
  }

  function addServerRow() {
    setDraftServers((prev) => [...prev, { id: makeServerId(), name: '', url: '' }]);
    setSaved(false);
    setServerSaveError(null);
  }

  function removeServerRow(serverId: string) {
    setDraftServers((prev) => prev.filter((server) => server.id !== serverId));
    setSaved(false);
    setServerSaveError(null);
  }

  function validateServerDrafts(): ServerOption[] | null {
    const validated: ServerOption[] = [];

    for (const server of draftServers) {
      const name = server.name.trim();
      const rawUrl = server.url.trim();
      if (!name && !rawUrl) continue;
      if (!name || !rawUrl) {
        setServerSaveError('Each server row must include both Name and URL.');
        return null;
      }

      const normalized = normalizeServerUrl(rawUrl);
      if (!/^https?:\/\/.+:\d+$/.test(normalized)) {
        setServerSaveError(`Invalid URL for "${name}". Include scheme and port, e.g. http://192.168.1.4:8189`);
        return null;
      }
      const rawTailscale = (server.tailscaleUrl ?? '').trim();
      let tailscaleUrl: string | undefined;
      if (rawTailscale) {
        const normalizedTs = normalizeServerUrl(rawTailscale);
        if (!/^https?:\/\/.+:\d+$/.test(normalizedTs)) {
          setServerSaveError(`Invalid Tailscale URL for "${name}". Include scheme and port, e.g. http://100.64.0.1:8189`);
          return null;
        }
        tailscaleUrl = normalizedTs;
      }
      validated.push({ id: server.id || makeServerId(), name, url: normalized, ...(tailscaleUrl ? { tailscaleUrl } : {}) });
    }

    if (validated.length === 0) {
      setServerSaveError('Add at least one server with Name and URL.');
      return null;
    }

    return validated;
  }

  function saveServers(showSavedState = true): ServerOption[] | null {
    const validated = validateServerDrafts();
    if (!validated) return null;

    if (!areSameServers(validated, servers)) {
      setServers(validated);
    }
    setServerSaveError(null);
    if (showSavedState) {
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    }
    setTestResult(null);
    return validated;
  }

  async function runConnectionTest(validatedServers: ServerOption[]): Promise<string> {
    const lines: string[] = [];
    let connectivityPassCount = 0;
    let compatibilityIssueCount = 0;
    const matrix = await fetchCompatibilityMatrix();
    const matrixAvailable = Boolean(matrix);

    for (const server of validatedServers) {
      let testUrl = server.url;
      let urlLabel = 'LAN';
      if (server.tailscaleUrl) {
        const lanOk = await probeUrl(server.url);
        if (!lanOk) {
          testUrl = server.tailscaleUrl;
          urlLabel = 'Tailscale';
        }
      }
      lines.push(`[${server.name}] ${testUrl} (${urlLabel})`);
      try {
        const [episodes, movies, shows] = await Promise.all([
          requestFromServer<unknown[]>(testUrl, '/api/v1/episodes'),
          requestFromServer<unknown[]>(testUrl, '/api/v1/movies'),
          requestFromServer<unknown[]>(testUrl, '/api/v1/shows'),
        ]);

        const ep = Array.isArray(episodes)
          ? episodes.length
          : `bad format -> ${JSON.stringify(episodes).slice(0, 80)}`;
        const mv = Array.isArray(movies) ? movies.length : 'bad format';
        const sh = Array.isArray(shows) ? shows.length : 'bad format';
        const detected = await fetchServerVersionInfo(testUrl);

        lines.push(`  ✓ Episodes: ${ep}  Movies: ${mv}  Shows: ${sh}`);
        connectivityPassCount += 1;

        if (!detected?.serverVersion) {
          lines.push('  ⚠ API Version: unable to detect from status endpoints');
          compatibilityIssueCount += 1;
        } else if (!matrixAvailable || !matrix) {
          lines.push(`  ⚠ API Version: ${detected.serverVersion} (repository approvals unavailable)`);
          compatibilityIssueCount += 1;
        } else if (!isVersionVerified(matrix, detected)) {
          const publicPart = detected.publicApiVersion ? `, public API ${detected.publicApiVersion}` : '';
          lines.push(`  ⚠ API Version: server ${detected.serverVersion}${publicPart} (not verified in repository yet)`);
          compatibilityIssueCount += 1;
        } else {
          const publicPart = detected.publicApiVersion ? `, public API ${detected.publicApiVersion}` : '';
          lines.push(`  ✓ API Version: server ${detected.serverVersion}${publicPart} (verified in repository)`);
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        lines.push(`  ✗ ${msg}`);
        compatibilityIssueCount += 1;
      }
      lines.push('');
    }

    const allConnectionsOk = connectivityPassCount === validatedServers.length;
    const header = allConnectionsOk
      ? compatibilityIssueCount === 0
        ? `✓ Server checks complete (${connectivityPassCount}/${validatedServers.length} reachable, 0 compatibility issues)`
        : `⚠ Server checks complete (${connectivityPassCount}/${validatedServers.length} reachable, ${compatibilityIssueCount} compatibility issue${compatibilityIssueCount === 1 ? '' : 's'})`
      : `✗ Server checks complete (${connectivityPassCount}/${validatedServers.length} reachable, ${compatibilityIssueCount} compatibility issue${compatibilityIssueCount === 1 ? '' : 's'})`;
    return [header, '', ...lines].join('\n').trim();
  }

  async function testConnection() {
    const validatedServers = saveServers(false);
    if (!validatedServers) {
      setTestResult('✗ Fix server table errors, then test again.');
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      const result = await runConnectionTest(validatedServers);
      setTestResult(result);
    } finally {
      setTesting(false);
    }
  }

  async function openBugReportComposer() {
    const validatedServers = saveServers(false);
    if (!validatedServers) return;
    setPreparingReport(true);
    try {
      const freshTestResult = await runConnectionTest(validatedServers);
      setTestResult(freshTestResult);
      const active = servers.find((server) => server.id === useStore.getState().activeServerId) ?? servers[0];
      const raw = buildBugReportDraft({
        activeServerName: active?.name ?? 'Unknown',
        apiVersion,
        apiPublicVersion,
        apiVersionApproved,
        apiCompatibilityNote,
        testResult: freshTestResult,
        errorLogText: getRecentClientErrorLogText(),
      });
      const template = redactServerUrls(raw, servers);
      setBugReportDraft(template);
      setReportCopyMessage(null);
      setBugReportOpen(true);
    } finally {
      setPreparingReport(false);
    }
  }

  async function copyBugReportDraft() {
    try {
      await navigator.clipboard.writeText(bugReportDraft);
      setReportCopyMessage('Bug report copied to clipboard.');
    } catch {
      setReportCopyMessage('Unable to copy automatically. Select the text and copy it manually.');
    }
    setTimeout(() => setReportCopyMessage(null), 3500);
  }

  return (
    <div className="page">
      <header className="page__header">
        <h1 className="page__title">Settings</h1>
      </header>

      <div className="settings-form">
        <section className="settings-section">
          <h2 className="settings-section__title">Appearance</h2>
          <div className="settings-theme-options">
            {(['system', 'dark', 'light'] as AppTheme[]).map((t) => (
              <label key={t} className="settings-theme-option">
                <input
                  type="radio"
                  name="app-theme"
                  value={t}
                  checked={theme === t}
                  onChange={() => setTheme(t)}
                />
                <span>
                  {t === 'system' ? 'System default' : t.charAt(0).toUpperCase() + t.slice(1)}
                </span>
              </label>
            ))}
          </div>
          <p className="settings-hint">
            <em>System default</em> follows your operating system&apos;s light or dark preference.
          </p>
          {window.__TAURI_INTERNALS__ && (
            <>
              <label className="settings-toggle settings-toggle--top-gap">
                <input
                  type="checkbox"
                  checked={windowAlwaysOnTop}
                  onChange={(e) => setWindowAlwaysOnTop(e.target.checked)}
                />
                <span>Keep window on top when not maximized</span>
              </label>
              <p className="settings-hint">
                When enabled, the app window floats above other windows while in windowed mode. Disabled automatically when maximized.
              </p>
            </>
          )}
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">Servers</h2>
          <table className="settings-table" aria-label="Configured DVR servers">
            <thead>
              <tr>
                <th>Name</th>
                <th>LAN URL</th>
                <th>Tailscale URL</th>
              </tr>
            </thead>
            <tbody>
              {draftServers.map((server) => (
                <tr key={server.id}>
                  <td>
                    <input
                      className="settings-input"
                      type="text"
                      value={server.name}
                      onChange={(e) => updateServer(server.id, 'name', e.target.value)}
                      placeholder="Living Room"
                      spellCheck={false}
                    />
                  </td>
                  <td>
                    <div className="settings-table-url-cell">
                      <input
                        className="settings-input"
                        type="url"
                        value={server.url}
                        onChange={(e) => updateServer(server.id, 'url', e.target.value)}
                        placeholder="http://192.168.1.4:8189"
                        spellCheck={false}
                      />
                      <button
                        className="settings-row-delete-btn"
                        onClick={() => removeServerRow(server.id)}
                        aria-label={`Remove ${server.name || 'server'} row`}
                        title="Remove row"
                      >
                        Remove
                      </button>
                    </div>
                  </td>
                  <td>
                    <input
                      className="settings-input"
                      type="url"
                      value={server.tailscaleUrl ?? ''}
                      onChange={(e) => updateServer(server.id, 'tailscaleUrl', e.target.value)}
                      placeholder="http://100.64.0.1:8189 (optional)"
                      spellCheck={false}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="settings-row settings-row--top-gap">
            <button className="settings-save-btn settings-save-btn--secondary" onClick={addServerRow}>
              Add Server
            </button>
            <button className="settings-save-btn" onClick={() => saveServers()}>
              {saved ? '✓ Saved' : 'Save Servers'}
            </button>
          </div>
          {serverSaveError && <p className="settings-hint settings-hint--warn">{serverSaveError}</p>}
          <p className="settings-hint">
            Add as many servers as you like. Use the server dropdown in the left sidebar to switch instantly.
            The <strong>Tailscale URL</strong> is optional — if provided, DVRDesk will probe the LAN URL on startup
            and fall back to the Tailscale address when it's unreachable.
          </p>
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">Subtitles</h2>
          <label className="settings-label" htmlFor="share-path">
            Storage Share Path
          </label>
          <div className="settings-row">
            <input
              id="share-path"
              className="settings-input"
              type="text"
              value={shareDraft}
              onChange={(e) => setShareDraft(e.target.value)}
              placeholder="\\192.168.3.150\AllMedia\Channels"
              spellCheck={false}
            />
            <button
              className="settings-save-btn"
              onClick={() => { setStorageSharePath(shareDraft); }}
            >
              Save
            </button>
          </div>
          <p className="settings-hint">
            UNC or local path to the root of your DVR storage (where recordings are saved).
            Used to load <code>.srt</code> subtitle files alongside recordings.
            Leave blank to disable subtitle loading.
          </p>
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">Playback</h2>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={preferRemux}
              onChange={(e) => setPreferRemux(e.target.checked)}
            />
            <span>Prefer remux stream (best quality on local LAN)</span>
          </label>
          <p className="settings-hint">
            Turn this off for remote internet playback if startup/reliability is better with standard transcode behavior.
          </p>
          <label className="settings-toggle settings-toggle--top-gap">
            <input
              type="checkbox"
              checked={diagnosticsEnabled}
              onChange={(e) => setDiagnosticsEnabled(e.target.checked)}
            />
            <span>Enable diagnostics tools in player</span>
          </label>
          <p className="settings-hint">
            Shows in-player diagnostics tools, including a live stats overlay and report copy button for troubleshooting playback.
          </p>
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">Player Keybindings &amp; Skip Intervals</h2>
          <table className="settings-table" aria-label="Player keybindings">
            <thead>
              <tr>
                <th>Action</th>
                <th>Key(s) <span style={{ fontWeight: 400 }}>(comma-separated)</span></th>
                <th>Skip (s)</th>
              </tr>
            </thead>
            <tbody>
              {(Object.keys(DEFAULT_KEYBINDINGS) as (keyof KeybindingsConfig)[]).map((action) => (
                <tr key={action}>
                  <td>{ACTION_LABELS[action]}</td>
                  <td>
                    <input
                      className="settings-input"
                      type="text"
                      value={bindingsDraft[action]?.join(', ') ?? ''}
                      onChange={(e) => handleKeybindingChange(action, e.target.value)}
                      placeholder={DEFAULT_KEYBINDINGS[action].join(', ')}
                      spellCheck={false}
                    />
                  </td>
                  <td>
                    {action in DEFAULT_SKIP_INTERVALS ? (
                      <input
                        className="settings-input"
                        type="number"
                        min={1}
                        value={intervalsDraft[action as keyof SkipIntervalsConfig] ?? ''}
                        onChange={(e) => handleIntervalChange(action as keyof SkipIntervalsConfig, e.target.value)}
                        placeholder={String(DEFAULT_SKIP_INTERVALS[action as keyof SkipIntervalsConfig])}
                        style={{ width: '5em' }}
                      />
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="settings-row settings-row--top-gap">
            <button className="settings-save-btn" onClick={savePlayerSettings}>
              {playerSettingsSaved ? '✓ Saved' : 'Save Player Settings'}
            </button>
          </div>
          <p className="settings-hint">
            Configure keyboard shortcuts for the video player. Use key names like <code>ArrowRight</code>, <code>l</code>, <code>Shift+ArrowLeft</code>, <code>Space</code>, <code>Escape</code>. Separate multiple keys with commas.
            Skip interval only applies to skip/fast-forward/reverse actions.
          </p>
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">Live TV</h2>
          <label className="settings-toggle">
            <input
              type="checkbox"
              checked={showHiddenLiveChannels}
              onChange={(e) => setShowHiddenLiveChannels(e.target.checked)}
            />
            <span>Show hidden channels</span>
          </label>
          <p className="settings-hint">
            When off, Live TV filters out channels marked hidden by your channel source or Channels DVR metadata.
          </p>
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">Diagnostics</h2>
          <button className="settings-save-btn" onClick={testConnection} disabled={testing}>
            {testing ? 'Testing…' : 'Test Connection'}
          </button>
          {testResult && (
            <pre className={`settings-test-result ${testResult.startsWith('✓') ? 'settings-test-result--ok' : 'settings-test-result--err'}`}>
              {testResult}
            </pre>
          )}
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">API Compatibility</h2>
          {apiVersion ? (
            <>
              <p className="settings-hint">
                Detected server version: <strong>{apiVersion}</strong>
              </p>
              {apiPublicVersion && (
                <p className="settings-hint">
                  Detected public API version: <strong>{apiPublicVersion}</strong>
                </p>
              )}
              {apiVersionApproved ? (
                <p className="settings-hint settings-hint--ok">✓ This server version is verified by the repository compatibility list.</p>
              ) : (
                <>
                  <p className="settings-hint settings-hint--warn">
                    ⚠ {apiCompatibilityNote ?? 'Detected version is not verified in the repository compatibility list yet.'}
                  </p>
                  <p className="settings-hint settings-hint--warn">
                    Please use the bug report composer below to package your diagnostics for Channels Community.
                  </p>
                </>
              )}
            </>
          ) : (
            <p className="settings-hint">Active server version not detected yet. Use Test Connection to inspect all configured servers.</p>
          )}
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">Bug Report</h2>
          <div className="settings-row">
            <button className="settings-save-btn" onClick={() => { void openBugReportComposer(); }} disabled={preparingReport || testing}>
              {preparingReport ? 'Preparing…' : 'Open Bug Report'}
            </button>
            <button className="settings-save-btn settings-save-btn--secondary" onClick={() => { setLogViewText(getRecentClientErrorLogText()); setLogViewOpen(true); }}>
              View Error Log
            </button>
          </div>
          <p className="settings-hint">
            Opens a local editor prefilled for Channels Community. It includes environment details, the latest Test Connection output, and the persistent client error log from the last 48 hours.
          </p>
        </section>

        <section className="settings-section">
          <h2 className="settings-section__title">About</h2>
          <p className="settings-hint settings-hint--about">
            <strong>DVRDesk</strong> v{__APP_VERSION__}
          </p>
          <p className="settings-hint settings-hint--about">
            Repository: <a className="settings-link" href={__APP_REPOSITORY_URL__} target="_blank" rel="noreferrer">{__APP_REPOSITORY_URL__}</a>
          </p>
          {updateInfo && (
            <p className="settings-hint settings-hint--warn settings-hint--about">
              New DVRDesk client version available: <strong>{updateInfo.latestVersion}</strong>.{' '}
              <a className="settings-link" href={updateInfo.latestUrl} target="_blank" rel="noreferrer">Open releases</a>
            </p>
          )}
        </section>
      </div>

      {bugReportOpen && (
        <div className="media-modal-backdrop" onClick={() => setBugReportOpen(false)}>
          <div className="media-modal settings-report-modal" onClick={(e) => e.stopPropagation()}>
            <div className="media-modal__header">
              <h3>Bug Report Composer</h3>
              <div className="settings-row">
                <button className="settings-save-btn settings-save-btn--secondary" onClick={() => setBugReportOpen(false)}>
                  Close
                </button>
              </div>
            </div>
            <p className="settings-hint settings-report-help">
              This text is ready to paste into the Channels Community editor. It uses plain text formatting that works with Markdown, BBCode, and HTML-style forum posts.
            </p>
            <p className="settings-hint settings-report-help">
              Add your summary and reproduction steps before posting. The log section is fenced in triple backticks so it stays preformatted.
            </p>
            <textarea
              className="settings-report-editor"
              value={bugReportDraft}
              onChange={(e) => setBugReportDraft(e.target.value)}
              spellCheck={false}
              aria-label="Bug report draft"
            />
            <div className="settings-row settings-row--top-gap settings-report-actions">
              <button className="settings-save-btn" onClick={() => { void copyBugReportDraft(); }}>
                Copy To Clipboard
              </button>
              <button className="settings-save-btn settings-save-btn--secondary" onClick={() => setBugReportOpen(false)}>
                Close
              </button>
            </div>
            {reportCopyMessage && <p className="settings-hint settings-hint--ok">{reportCopyMessage}</p>}
          </div>
        </div>
      )}

      {logViewOpen && (
        <div className="media-modal-backdrop" onClick={() => setLogViewOpen(false)}>
          <div className="media-modal settings-report-modal" onClick={(e) => e.stopPropagation()}>
            <div className="media-modal__header">
              <h3>Client Error Log (last 48 hours)</h3>
              <button className="media-modal__close" onClick={() => setLogViewOpen(false)}>
                Close
              </button>
            </div>
            <pre className="settings-log-view">{logViewText}</pre>
          </div>
        </div>
      )}
    </div>
  );
}
