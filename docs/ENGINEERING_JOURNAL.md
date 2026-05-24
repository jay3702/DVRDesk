### 2026-04-17 - Progress bar on Recent Recordings detail pane

- Request: show playback progress bar on the detail pane after selecting a recording from the Recent Recordings list, matching the list row behavior.
- Rationale: users expect to see resume progress consistently, not just in the list view.
- Symptoms discovered:
  - progress bar was visible in the Recent Recordings list but missing from the detail pane
- Solution:
  - added the same progress bar logic to the detail pane for in-progress, unwatched items
- Validation:
  - TypeScript diagnostics clean; production build run after update
# Engineering Journal

This file adds the decision context that is usually missing from commit messages and GitHub activity history. Entries should stay concise and focus on why a change was made, what symptoms were observed, and how the solution was validated.

## Unreleased

## v1.10.0

### 2026-05-20 - Rebrand to DVRDesk

- Request: rename app from WinChannels to DVRDesk per request from Fancy Bits.
- Changes: productName, window title, bundle identifier display name, Cargo package/lib names, GitHub repo URLs, bug/playback report titles, sidebar brand label, Settings About section, README, copilot instructions.
- Identifier kept as `com.jay.winchannels` so existing installs retain their WebView localStorage (server URL, settings). Changing the identifier would reset all user configuration on upgrade.
- localStorage sort-state keys left unchanged for the same reason.

### 2026-05-20 - API compatibility test suite

- Request: need a repeatable way to validate all Channels DVR API endpoints after server version upgrades before approving the new version in the compatibility matrix.
- Solution: `tests/api-compatibility.test.ts` (Vitest, node environment). Covers all 28 endpoints the app uses: shape validation for all reads, watch/unwatch roundtrip mutation (fully restoring original state), playback_time no-op, destructive endpoint presence checks via fake IDs (no real data deleted), and live HLS probe.
- Run with: `$env:CHANNELS_DVR_URL = "http://<server>:8089"; npm run test:api`
- Server version `2026.05.18.1824` approved after all 28 tests passed.

### 2026-05-18 - Rebuild Index action — deferred (no confirmed API endpoint)

- Attempted to add a "Rebuild Index" button to recording/movie detail panes to trigger HLS streaming index regeneration via the Channels DVR API.
- Tried `DELETE /dvr/files/{id}/index` (404) and `PUT /dvr/files/{id}/m3u8` (404, sourced from a 2019 developer forum post — endpoint has since been removed or relocated to the proprietary API).
- Feature reverted; no code remains. Can be re-added once the correct API endpoint is confirmed via HAR capture from the Channels DVR web UI.
- Workaround: delete the `Metadata/{FILE_ID}/` folder from the Channels DVR admin interface.

### 2026-05-18 - Improve HLS buffer-gap resilience for recordings with fragmented indexes

- Request: community user reported repeated buffering circles during recording playback; WinChannels log showed non-fatal `bufferStalledError`, `bufferSeekOverHole`, and `fragParsingError` events. Deleting the recording's video index (Channels DVR feature) resolved the issue, confirming a corrupted/stale index caused buffer holes.
- Root cause: HLS.js auto-nudges the playhead past buffer holes, but the default `maxBufferHole` threshold (0.1s) and `nudgeMaxRetry` (3) were too conservative for larger gaps produced by fragmented recording indexes, causing a visible buffering spinner on each recovery cycle. Non-fatal `fragParsingError` events also left decoder state confused without explicit recovery.
- Solution:
  - Raised `maxBufferHole` from 0.1s to 0.5s in `hlsConfig` so HLS.js auto-skips gaps up to 0.5s without stalling.
  - Raised `nudgeMaxRetry` from 3 to 5 so HLS.js makes more attempts before escalating.
  - Added non-fatal `fragParsingError` handler that calls `hls.recoverMediaError()` immediately to clear confused decoder state rather than waiting for a downstream stall cycle.
- Validation: TypeScript diagnostics clean.

## v1.9.1

### 2026-05-16 - WinChannels: stop live stream immediately when player is closed

- Request: CC4C stream stays alive after pressing X (closing the player) unless the user changes channels or closes the Tauri app.
- Root cause: `VideoPlayer` is a persistent overlay (rendered outside `<Routes>` in `App.tsx`); it never unmounts on navigation. The HLS setup `useEffect` has deps `[nowPlayingKey, preferRemux]`. `stopPlayback()` clears `nowPlayingId` but does not change `nowPlayingKey`, so the effect cleanup never ran. HLS.js kept fetching segments from Channels DVR, which held the Channels DVR session (and thus the CC4C ffmpeg pipeline) open until Channels DVR's session inactivity timeout.
- Solution: added a `useEffect([nowPlayingId])` in `VideoPlayer.tsx` that fires when `nowPlayingId` becomes null (stopPlayback called). It destroys the HLS.js instance and calls `stopLiveDvrSession()`, which fetches `GET /api/v1/sessions`, finds the session matching the channel in the manifest URL, and sends `DELETE /api/v1/sessions/{ID}`. Session termination is best-effort (errors are caught and logged).
- Validation: TypeScript diagnostics clean.

## v1.9.0

### 2026-05-13 - Live TV HLS URL and remux improvements

- WinChannels code improvements:
  - `encoder=remux` query parameter is no longer appended to live channel URLs (`nowPlayingRecordingKind === null`); remux is only meaningful for DVR file playback, not live HLS streams.
  - Live manifest URL candidate order changed to prefer `/hls/master.m3u8` over bare `/hls`, ensuring a full multi-variant master playlist is requested.
  - URL probe in `resolveLiveManifestUrl` switched from browser `fetch` (GET) to Tauri HTTP `HEAD` to avoid initiating a transcoding session on Channels DVR before HLS.js connects.

## v1.8.0

### 2026-05-09 - Fix SRT path construction on Linux/POSIX share paths

- Request: SRT sidecar captions failed on Ubuntu with "Invalid argument (os error 22)".
- Symptoms: console showed path like `/mnt/channels/Channels\\TV\\Show\\ep.srt` — POSIX base path joined with Windows backslash separators.
- Root cause: `VideoPlayer.tsx` always used `\\` as the path separator and called `.replace(/\//g, '\\')` on the relative portion, regardless of OS.
- Solution: detect path style from `storageSharePath` (POSIX if it starts with `/`, Windows UNC/drive-letter otherwise); use the appropriate separator and normalization throughout.
- Validation: TypeScript diagnostics clean.

## v1.7.0

### 2026-05-08 - Episode sort by Season/Episode and First Aired; Movie sort by Release Date

- Request: add Season/Episode and First Aired sort options for TV show episodes; add Release Date sort for movies.
- Solution:
  - `TVShows.tsx`: added `'season-episode'` and `'air-date'` to `EpisodeSortField`; `season-episode` sorts by `season_number` then `episode_number`; `air-date` sorts by `original_air_date` (ISO string, lexicographic); episodes with no air date sort last; both default to ascending. Updated localStorage validator and `defaultOrderFor`.
  - `Movies.tsx`: added `'release-date'` to `SortField`; sorts by `release_date` (ISO string) falling back to `release_year` as a string; movies with neither field sort last; defaults to ascending. Updated localStorage validator and `defaultOrderFor`.
- Validation: TypeScript diagnostics clean.

## v1.6.0

### 2026-05-08 - Double-click to play, auto-hide controls, fullscreen aspect ratio fix

- Request: double-click a recording in Recent Recordings to play directly; auto-hide player controls after a few seconds of inactivity; fix fullscreen video clipping on ultrawide monitors.
- Rationale: two-click-to-play was friction for mouse users; persistent controls obstruct the video; custom fullscreen on 32:9 screens clipped the bottom of 16:9 content.
- Solution:
  - `RecentRecordings.tsx`: added `onDoubleClick` to each list item button that calls `playItem()` directly, bypassing the detail pane.
  - `VideoPlayer.tsx`: added `controlsVisible` state and `resetHideTimer` callback; mouse movement on the overlay resets a 3-second inactivity timer; `video-overlay--controls-hidden` class applied when timer fires.
  - `VideoPlayer.css`: added `transition: opacity 0.3s ease` to `.video-header` and `.video-ad-bar`; `.video-overlay--controls-hidden` fades them out (`opacity: 0; pointer-events: none`) and sets `cursor: none`.
  - `VideoPlayer.css`: added `min-height: 0` and `object-fit: contain` to `.video-element` so the video is letterboxed within its flex container instead of overflowing on non-16:9 displays.
- Validation: TypeScript diagnostics clean; dev build running.

## v1.5.0

### 2026-05-07 - Theme support: System, Dark, Light

- Request: add theme selection to Settings, including a system default option that follows OS preference.
- Rationale: dark-only UI excluded users who prefer light mode or rely on OS-level accessibility settings.
- Solution:
  - Created `src/themes.css` defining ~40 `--wc-*` CSS custom properties for dark (default) and light variants, plus a `@media (prefers-color-scheme: light)` block for the system auto case.
  - Converted all six CSS files (`reset.css`, `App.css`, `Sidebar.css`, `MediaCard.css`, `Page.css`, `VideoPlayer.css`) from hardcoded hex values to `var(--wc-*)` references.
  - Added `AppTheme = 'system' | 'dark' | 'light'` type, `theme` state, and `setTheme` action to `useStore.ts` with localStorage persistence under key `app_theme`.
  - `App.tsx` applies `data-theme="dark"` or `data-theme="light"` to `<html>` when an explicit theme is chosen; removes the attribute for system auto so the CSS media query resolves it.
  - Added an Appearance section at the top of Settings with three styled radio buttons (System default / Dark / Light).
- Validation: TypeScript diagnostics clean; production build succeeded.

## v1.4.1

### 2026-05-03 - Auto-focus first episode card when TV show is selected

- Request: when a TV show is selected for the first time (or switched), automatically focus the first episode card in the grid so keyboard navigation can begin without a manual click.
- Rationale: episodes load asynchronously after show selection; focus cannot be set until the episode list has rendered.
- Solution:
  - `TVShows.tsx`: added a `useEffect` on `[loadingEps, selectedShow?.id]` that fires when `loadingEps` transitions to `false` with a show selected and no episode detail open; uses `requestAnimationFrame` to focus `.media-grid .media-card[tabindex="0"]` after the grid has painted.
  - Naturally re-fires when the user switches shows, giving each new show grid its own auto-focus.
- Validation: TypeScript diagnostics clean.

### 2026-05-05 - Keyboard D-pad navigation across sidebar / list / grid zones

- Request: add arrow-key navigation so the app can be operated without a mouse — left/right arrows move between sidebar, list, and grid zones; up/down navigate within a zone; Enter selection uses native element behavior.
- Rationale: TV-style apps should be fully keyboard-navigable; existing elements (`a.sidebar__link`, `button.show-item`, `button.rec-item`, `.media-card[tabindex="0"]`) are already focusable.
- Solution:
  - `src/lib/useKeyboardNav.ts` (new): global `keydown` capture listener on `window`; detects active zone from `document.activeElement` using `matches()`; implements per-zone directional logic:
    - Sidebar: Up/Down within `a.sidebar__link` items; Right → list zone (or grid if no list), remembers last list focus.
    - List zone: Up/Down within `button.show-item, button.rec-item`; Left → active sidebar link; Right → first grid item; remembers last position.
    - Grid zone: Left/Right within a row; Up/Down to closest-horizontal item in adjacent row via `getBoundingClientRect` row-grouping; Up from top row / Left from leftmost item → list or sidebar.
    - No-op when `nowPlayingId` is set (player owns keyboard), when focus is inside `input`/`textarea`/`select`, or inside `.media-modal`.
    - Any arrow when nothing nav-relevant is focused → focuses first sidebar link.
  - `src/App.tsx`: added `useKeyboardNav()` call inside `App()`.
  - `src/pages/Page.css`: added `:focus-visible` outline (2px solid #0078d4) for `.show-item` and `.rec-item`.
  - `src/components/Sidebar.css`: added `:focus-visible` outline for `.sidebar__link`.
  - `src/components/MediaCard.css`: already had `:focus-visible` outline — no change needed.
- Validation: TypeScript diagnostics clean; no errors in `useKeyboardNav.ts` or `App.tsx`.

### 2026-05-05 - Configurable keybindings and skip intervals for the video player

- Request: add configurable keyboard shortcuts for all player commands (skip forward/back, fast forward/reverse, play/pause, close), with configurable skip interval durations, and reasonable defaults.
- Solution:
  - `useStore.ts`: added `KeybindingsConfig` and `SkipIntervalsConfig` interfaces, `DEFAULT_KEYBINDINGS` and `DEFAULT_SKIP_INTERVALS` constants, and corresponding `keybindings`/`setKeybindings`/`skipIntervals`/`setSkipIntervals` store state persisted via `localStorage`.
  - `VideoPlayer.tsx`: replaced hardcoded `SKIP_FWD`/`SKIP_BACK` constants with store-driven `skipIntervals`; skip buttons display live interval values; unified `onPlayerKeyDown` handler uses `keybindings` from store to match keys for all commands; imports updated for `useCallback`.
  - `Settings.tsx`: added "Player Keybindings & Skip Intervals" section with a table allowing per-action key (comma-separated) and skip duration (seconds) configuration; saved to store on button press with a `✓ Saved` confirmation.
- Recovery: Settings.tsx was completely mangled by earlier failed patch attempts (duplicate `export default`, import statements inside function bodies, floating JSX outside return). The file was reconstructed in full via `replace_string_in_file` after trimming it to a clean 13-line header with `Set-Content`.
- Validation: TypeScript diagnostics clean for all three modified files.

### 2026-04-26 - Local bug report composer with persistent 48-hour client error log

- Request: replace the GitHub-dependent report flow with a local bug report composer suitable for Channels Community posts, and include a persistent client-side error log that retains the last 48 hours.
- Symptoms discovered:
  - the prior `Open Bug Report` and `Open Feature Request` actions assumed GitHub authentication.
  - users had no persistent client log to include alongside server diagnostics when posting to the forum.
- Solution:
  - added `src/lib/clientErrorLog.ts` to persist client-side error entries in localStorage, prune entries older than 48 hours, and capture `console.error`, `window.error`, and `unhandledrejection` events.
  - `main.tsx`: installs client error logging at startup so errors persist across app restarts.
  - `Settings.tsx`: replaced GitHub issue links with a single `Open Bug Report` action that opens a local editor prefilled with environment details, latest Test Connection output, and the fenced client error log ready for Channels Community.
  - `Page.css`: added bug-report composer modal and textarea styling consistent with the existing app modal pattern.
- Validation:
  - Production build completed successfully.

### 2026-04-25 - Reusable report template workflow for bug/feature requests

- Request: add a report template that can be used for any bug or feature request and direct users to use it when API compatibility shows unapproved versions.
- Symptoms discovered:
  - users had no guided way to package diagnostics for compatibility mismatches.
  - incompatibility warnings lacked a direct action path for reporting findings.
- Solution:
  - `Settings.tsx`: added a `Report Template` section with:
    - `Copy Report Template` (clipboard)
    - `Open Bug Report` (prefilled GitHub issue)
    - `Open Feature Request` (prefilled GitHub issue)
  - Template now includes app version, active server info, detected internal/public API versions, compatibility state, and latest Test Connection output.
  - API Compatibility warning now explicitly directs users to use the report template when the detected version is not approved.
- Validation:
  - TypeScript diagnostics clean for modified file.
  - Production build completed successfully.

### 2026-04-25 - Repository-hosted API compatibility approvals (server + public API versions)

- Request: move compatibility approvals from client-local state to the repository, track both server and public API version numbers, keep warnings non-blocking, and improve confusing compatibility messaging.
- Symptoms discovered:
  - local per-server approval wording (`not yet approved for this server`) was confusing.
  - API Compatibility section could still show `Connect to a server first` even after Test Connection checks.
  - write actions were blocked on version mismatch, conflicting with desired warn-only policy.
- Solution:
  - Added `.github/api-version-compatibility.json` as a repository-managed approval matrix with entries for `serverVersion`, optional `publicApiVersion`, and `approved`.
  - `client.ts`: added status parsing for both internal server version and public API version; added repository matrix fetch and version-pair approval matcher.
  - `useStore.ts`: replaced localStorage approval logic with repository matrix checks; now stores detected `apiVersion` + `apiPublicVersion` and a compatibility note.
  - `Settings.tsx`: Test Connection now evaluates versions against repository approvals and reports clearer per-server results (`approved in repository`, `not approved in repository yet`, or approvals unavailable).
  - `Settings.tsx`: API Compatibility messaging updated to clarify active-server detection and repository approval status; About section now also shows newer client version notice with releases link.
  - `Movies.tsx`, `RecentRecordings.tsx`, `TVShows.tsx`: removed hard mutation blocks on unapproved version; behavior is warning-only.
  - `App.tsx`: updated top warning banner copy to reflect non-blocking caution.
- Validation:
  - TypeScript diagnostics clean for modified files.
  - Production build completed successfully after removing an unused selector.

### 2026-04-25 - Test Connection now reports per-server API compatibility

- Request: make the Settings `Test Connection` button check API version on each configured server and report compatibility problems individually.
- Symptoms discovered:
  - Connection test only reported endpoint reachability/counts and did not evaluate API version compatibility per server.
  - Users with multiple servers needed server-by-server confirmation of version approval mismatches.
- Solution:
  - `Settings.tsx`: added per-server `/api/v1/status` checks during Test Connection.
  - Extracted reported API version (`version`/`Version`/`build`) and compared it to the stored approved version for each server id.
  - Output now shows one of: approved version, not-yet-approved version, version mismatch, or missing version field for each server.
  - Test summary now includes total compatibility issue count in addition to reachability.
- Validation:
  - TypeScript diagnostics clean for modified file.

### 2026-04-25 - Status endpoint fallback for API version detection

- Request: user reported Test Connection showing `0/N reachable` due to `404 GET /api/v1/status` on otherwise reachable servers.
- Symptoms discovered:
  - Test Connection grouped `/api/v1/status` into the same `Promise.all` as episodes/movies/shows, so a status 404 failed the entire server connectivity check.
  - Version detection and probe logic assumed `/api/v1/status` existed on all server variants.
- Solution:
  - `Settings.tsx`: decoupled connectivity checks from version checks; reachability now comes from content endpoints, while version uses `fetchServerVersion` separately.
  - `client.ts`: added status endpoint fallback candidates (`/api/v1/status`, `/api/status`, `/status`) for both probe and version detection.
  - Compatibility output now flags undetected version as a per-server compatibility warning rather than reporting the server as unreachable.
- Validation:
  - TypeScript diagnostics clean for modified files.
  - Production build completed successfully.

### 2026-04-25 - API compatibility probe reliability across multi-port servers

- Request: investigate odd API compatibility behavior reported on a setup with multiple DVR servers sharing one IP but using different ports.
- Symptoms discovered:
  - API compatibility probe in `App.tsx` only reran when `activeServerId` changed.
  - Editing active server URL/port could leave compatibility state stale (`Version not yet detected` or prior value) until a full server switch/restart.
  - Async probe completion could apply version results to the wrong active server if selection changed during the request.
- Solution:
  - `App.tsx`: made startup/server probe effect depend on both `activeServerId` and `serverChangeVersion` so URL/port edits trigger a fresh version probe.
  - `useStore.ts`: captured probe target server id, ignored stale async results when active server changed mid-probe, and reset compatibility state when detection fails or server context changes.
  - `useStore.ts`: reset `apiVersion`/`apiVersionApproved` on active server change and server URL updates to avoid stale display.
- Validation:
  - TypeScript diagnostics clean for modified files (`App.tsx`, `useStore.ts`).

### 2026-04-24 - Startup check for newer WinChannels release

- Request: check on startup whether a newer app version exists.
- Rationale: users should see available updates proactively instead of manually checking GitHub releases.
- Solution:
  - `App.tsx`: added a one-time startup check against GitHub Releases (`/repos/jay3702/winchannels/releases/latest`).
  - Added semantic version comparison logic so only newer versions trigger a notice.
  - Added a top-of-app update banner with latest version and a direct release link.
  - Guarded the check with a ref so React StrictMode does not duplicate requests in development.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-23 - TV Shows default sort set to Last Recorded

- Request: make Last Recorded the default sort in the TV show list.
- Rationale: users expect the most recently recorded shows to surface first without changing sort controls.
- Solution:
  - `TVShows.tsx`: changed initial and fallback show sort defaults from `title` to `last-recorded`.
  - Sort direction default remains field-based, so `last-recorded` defaults to descending.
  - Existing persisted sort selections remain intact; the new default applies when no saved TV show sort state exists.
- Validation:
  - TypeScript diagnostics clean for modified file.

### 2026-04-23 - TV Shows list sort by last_recorded_at

- Request: add a TV show list sort option for `last_recorded_at`.
- Rationale: users want to prioritize shows by most recently recorded activity rather than created/updated metadata.
- Solution:
  - `TVShows.tsx`: added a show-list sort field `last-recorded`, wired to `Show.last_recorded_at`, and exposed it in the TV show sort dropdown as `Last Recorded`.
  - Kept episode sort fields unchanged to avoid introducing an unsupported `last-recorded` mode where episode data does not provide that key.
  - Updated sort-state parsing/validation to persist and restore the new show sort field safely.
  - `types.ts`: added optional `last_recorded_at?: number` to `Show`.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-23 - Progressive list auto-fill loop: missing scrollbar when content fits

- Request: user reported no scrollbar in Recent Recordings list despite seeing 5 recordings and "Scroll for more recordings" message.
- Symptoms: `rec-list__items` had `overflow-y: auto` but no scrollbar; the "Scroll for older recordings…" sentinel was visible, meaning more groups existed, but there was nothing to scroll.
- Root cause: the scroll `useEffect` depended only on `[groups.length]`. On mount it called `handleScroll()` once, which expanded from 3 → 5 day groups (step = 2). After that one expansion the effect did not re-run, so if the 5 groups' content still fit within the container height without overflowing, the scrollbar never appeared and the user had no way to trigger further expansion.
- Solution: added the visible-count state variable (`visibleDayGroups`, `visibleChannelCount`, `visibleMovieCount`, `visibleShowCount`) to the scroll effect's dependency array in all four pages. Now each expansion triggers a re-run of the effect which calls `handleScroll()` again, continuing to fill until either overflow exists or all items are shown. The `current >= total` guard already prevents infinite expansion.
- Validation: TypeScript diagnostics clean for all modified files; pushed as v1.3.3.

### 2026-04-23 - Fix CI build failure: unused recordings state variable

- Request: CI builds failed on all four platform targets after v1.3.1 push.
- Symptoms: TypeScript strict mode flagged `recordings` in `RecentRecordings.tsx` line 91 as "declared but its value is never read", causing all platform builds to exit with code 1.
- Root cause: the `recordings` read side of `useState` was kept during the cache refactor even though rendering uses the pre-grouped `groups` state and mutations use the functional-update form of `setRecordings`.
- Solution: changed `const [recordings, setRecordings]` to `const [, setRecordings]` to omit the unused read binding.
- Validation: TypeScript diagnostics clean; pushed as v1.3.2.

### 2026-04-23 - Recent Recordings cache with background refresh

- Request: avoid blocking every time Recent Recordings is revisited by caching the last loaded list, then refreshing in the background and updating only when new items or feed changes are detected.
- Symptoms discovered:
  - Recent Recordings remounted on route changes and always re-ran the initial fetch before showing content.
  - The list felt blocking even when the user had just visited it moments earlier.
  - Initial cache hydration done in `useEffect` still delayed visible rendering until after mount, so the page could appear blank before the cached list appeared.
- Solution:
  - `RecentRecordings.tsx`: added a per-server in-memory cache keyed by active server and server-change version.
  - Cached recordings and channel logos now initialize component state synchronously during the first render, so revisiting the page paints immediately from cache.
  - A background refresh still runs on every visit; it updates the visible list only if newer recordings are present or the feed order/content changed.
  - Trash actions now update the cache immediately so revisiting the page does not re-show deleted items.
  - To reduce mount cost, the page now renders only about three day groups initially and appends older groups as the user scrolls near the bottom.
- Validation:
  - TypeScript diagnostics clean for modified file.

### 2026-04-23 - Progressive list rendering for Live, TV Shows, and Movies

- Request: apply the same quick-initial-render behavior to Live, TV Shows, and Movies lists.
- Rationale: reduce mount-time DOM cost for large lists by rendering an initial slice, then appending older items during scroll.
- Solution:
  - `Live.tsx`: render an initial subset of filtered/sorted channel rows and append more near list bottom.
  - `TVShows.tsx`: render an initial subset of sorted shows in the sidebar and append more on scroll; keep selected/deep-linked show visible by expanding the slice when needed.
  - `Movies.tsx`: render an initial subset of movie cards in list view and append more as the page scroll nears the bottom.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-23 - Cache-first route revisit rendering for Live, TV Shows, and Movies

- Request: remove perceptible delay when navigating back to Live, TV Shows, and Movies (especially on higher-latency remote server connections).
- Symptoms discovered:
  - each page still waited on fresh network responses before list rendering, unlike Recent Recordings.
  - delay was much more visible when using the Hawaii server.
- Solution:
  - `Live.tsx`, `TVShows.tsx`, `Movies.tsx`: added per-server in-memory caches and cache-first state initialization so the previous list paints immediately on revisit.
  - Each page still performs a background refresh and updates the visible list when fresh data arrives.
  - Existing progressive list slicing remains in place to reduce mount-time DOM work.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-23 - Persisted sort selections with field-based default direction

- Request: persist sort selections across sessions and use default direction rules: ascending for alphabetical sorts; descending for ID and date sorts.
- Solution:
  - `Live.tsx`: persisted live list and diagnostics sort mode selections.
  - `TVShows.tsx`, `Movies.tsx`, `Library.tsx`: persisted sort field + direction selections via localStorage.
  - Sort field changes now reset direction using field defaults (`title` → ascending, `id`/`date-*` → descending), while still allowing user override via arrow controls.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-23 - Preserve Search criteria across server switches

- Request: keep search criteria/results when switching servers.
- Solution:
  - `Search.tsx`: removed server-switch clearing logic so `keyword` and `submittedKeyword` persist across server changes.
  - Existing behavior remains: users can clear manually via the clear action.
- Validation:
  - TypeScript diagnostics clean for modified file.

### 2026-04-23 - Resizable split lists and non-jumping divider drag

- Request: keep the default sort direction visibly selected on first render and make split-view list columns resizable like Recent Recordings, without the drag-jump quirk.
- Symptoms discovered:
  - Recent Recordings resize used absolute mouse X for width, which caused the divider to jump when the drag started away from the exact sidebar edge.
  - TV Shows and Library split views had fixed-width list panes.
- Solution:
  - Added `useResizableSidebar` to compute width from drag delta (`startWidth + currentX - startX`) so resizing tracks smoothly from the initial click point.
  - `RecentRecordings.tsx`: switched to the shared resize hook, removing the divider jump.
  - `TVShows.tsx` and `Library.tsx`: added the same resizable sidebar behavior and splitter handle used by Recent Recordings.
  - Added `aria-pressed` on active sort direction buttons so the default sort direction is explicitly marked active from initial render.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-23 - Sort direction switched to stacked up/down triangle buttons

- Request: replace the single direction toggle with two vertically stacked triangle buttons (up and down).
- Rationale: explicit direction controls are faster to scan and avoid ambiguous toggle state.
- Solution:
  - `TVShows.tsx`, `Movies.tsx`, `Library.tsx`: replaced each single sort-order toggle with a two-button stack that sets `asc` and `desc` directly.
  - `Page.css`: added `.page-sort-order-stack` plus button variants/active styling for compact stacked triangle controls.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-23 - Added Date Updated and separate sort direction toggle

- Request: add `Date Updated` to sort options and use a separate asc/desc control next to each sort dropdown instead of encoding direction in dropdown choices.
- Rationale: cleaner sort model and easier direction changes without reopening the field menu.
- Solution:
  - `TVShows.tsx`, `Movies.tsx`, `Library.tsx`: sort dropdowns now select field only (`Title`, `ID`, `Date Added`, `Date Updated`) and a dedicated adjacent button toggles direction (`↑`/`↓`).
  - `Page.css`: added shared `.page-sort-order-btn` styles and updated side-list sort row layout to place dropdown and toggle on the same line.
- Validation:
  - TypeScript diagnostics clean for all modified files.

### 2026-04-23 - Unified dropdown sorting with ID and created-date options

- Request: revert date behavior to created-date sorting and replace sort buttons with dropdowns that include ID and Date Added options across all sortable views.
- Symptoms discovered:
  - toggle-button sorting made it harder to discover all sort modes.
  - previous show-level date behavior using `last_recorded_at` did not match expected results.
- Solution:
  - `TVShows.tsx`: replaced show/episode sort buttons with dropdowns; added `ID` and `Date Added` options (plus title A-Z/Z-A); date sorts now use `created_at` ordering.
  - `Movies.tsx`: replaced sort buttons with dropdown containing title/ID/date-added options; date-added uses `created_at` ordering.
  - `Library.tsx`: expanded existing dropdowns (group and video) to include title/ID/date-added options and ascending/descending variants.
  - `types.ts`: removed temporary `last_recorded_at` typing from `Show` after reverting strategy.
- Validation:
  - TypeScript diagnostics clean for all modified files.

### 2026-04-22 - Date sort aligned to Date Added semantics

- Request: clarify and correct date sort behavior to better match Channels' Date Added expectations.
- Symptoms discovered:
  - UI labels said `Date`, which was ambiguous against Date Added / Date Updated / Date Released terminology in Channels tools.
  - TV show list date ordering could feel wrong when series were sorted by `created_at` only.
- Solution:
  - `TVShows.tsx`: renamed date sort labels to `Date Added` (show list and episode list).
  - TV show date sort now prefers `last_recorded_at`, then falls back to `created_at`, then `updated_at`.
  - Episode date sort uses `created_at` with fallback to `updated_at`.
  - `Movies.tsx`: renamed date sort label to `Date Added` (behavior remains `created_at`-based).
  - `types.ts`: added optional `last_recorded_at` to `Show` type for API parity.
- Validation:
  - TypeScript diagnostics clean for modified files.

## v1.3.0 — 2026-04-22

### 2026-04-22 - Search UI polish: icons and inline clear button

- Request: add icons to Search, Trash, and Mark as Not Recorded buttons; move the ✕ clear button inside the search textbox on the right edge; style it red.
- Rationale: visual affordance for destructive/utility actions; inline clear button follows common browser/address-bar pattern.
- Solution:
  - `Search.tsx`: wrapped input in `.search-panel__input-wrap` (relative container); absolutely positioned `.search-panel__clear-btn` at right edge; button only renders when keyword is non-empty.
  - Added 🔍 prefix to Search button text.
  - `RecordingDetail.tsx`: added 🗑️ prefix to Trash button and ✕ prefix to Mark as Not Recorded button.
  - `Page.css`: `.search-panel__input-wrap` sets `position: relative`; `.search-panel__clear-btn` uses `position: absolute; right: 8px; z-index: 1` with no background/border; input gets `z-index: 0` so button overlays it. `padding-right: 36px` keeps text clear of the button.
- Validation:
  - TypeScript diagnostics clean.

### 2026-04-22 - Search persistence, clear action, and series-first ordering

- Request: keep search results when navigating away/back, add a clear-search button, and order results with matching series first then episodes by created date.
- Rationale: users often inspect multiple hits by navigating to details and returning; rerunning the query each time is friction.
- Solution:
  - `Search.tsx`: persisted `keyword`, `submittedKeyword`, and `searchType` to localStorage and restored them on page mount.
  - Added Clear button (`✕`) beside Search to reset keyword and results quickly.
  - Updated result ordering to rank by type (`TV Series` first, then `TV Episode`, then `Movie`, then `Video`) and sort within each type by `created_at` descending.
- Validation:
  - TypeScript diagnostics clean for modified file.

### 2026-04-22 - Explicit Enter key execution for Search

- Request: ensure pressing Enter in the Search textbox executes the search.
- Rationale: keyboard-first flow should not depend only on button clicks.
- Solution:
  - `Search.tsx`: added explicit `onKeyDown` handling on the keyword input to execute search when Enter is pressed.
  - Search form submit behavior remains in place; this makes Enter execution explicit and reliable.
- Validation:
  - TypeScript diagnostics clean for modified file.

### 2026-04-22 - Search UX: explicit Search button and episode title matching for Title mode

- Request: stop incremental filtering; execute search only on explicit action, but re-run when search type changes. Also fix Title mode so an episode with matching `episode_title` is returned.
- Symptoms discovered:
  - keyword typing updated results immediately (incremental behavior).
  - `Title` mode did not return episodes when only `episode_title` matched (for example: "One Last Call").
- Solution:
  - `Search.tsx`: introduced `submittedKeyword` state so results are computed from the last executed keyword.
  - Added a Search button and submit handler; Enter key now submits via form.
  - Added auto re-execute on search-type change by re-submitting the current keyword when radio selection changes.
  - Updated TV episode Title-mode matching to include both `title` and `episode_title`.
  - `Page.css`: added `.search-panel__input-row` layout for textbox + Search button.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-22 - Show file path in episode and movie details

- Request: add the recording file path to the episode details page and movie detail page.
- Rationale: path visibility helps with troubleshooting source files and share-path mapping.
- Solution:
  - `RecordingDetail.tsx`: added a dedicated `Path: ...` line in the episode/recording detail body.
  - `Movies.tsx`: added a dedicated `Path: ...` line in movie details and removed duplicate `path` from the generic attributes list.
  - `Page.css`: added `.rec-detail__path` and `.media-detail__path` styles with wrapping for long paths.
- Validation:
  - TypeScript diagnostics clean for modified files.

### 2026-04-22 - Add cross-library Search page with typed matching and result navigation

- Request: add a Search page in the main menu (above Settings) with a keyword textbox, search-type radio options, and a persistent results table. Support targets: `show_id`, `program_id`, title fields, and summary fields, with a special series-name mode.
- Rationale: users need a direct way to find recordings and library items by IDs and text metadata, then jump straight to the relevant detail view.
- Solution:
  - Added `/search` route and Sidebar navigation item.
  - Added new `Search.tsx` page that loads shows, episodes, movies, and videos, then filters by search type:
    - Any: matches across ID/title/summary text fields (excludes season/episode numbers as requested).
    - Title: matches only `title`.
    - Summary: matches `summary` and `full_summary`.
    - Series Name: resolves matching show names, then returns rows for those series plus episodes whose `show_id` maps to matched series.
  - Added persistent results table columns: Type, Created Date, Modified Date, Title, Episode Title, Summary, Full Summary.
  - Added row-click navigation targets:
    - TV Episode -> `/tv?showId=...&episodeId=...`
    - TV Series -> `/tv?showId=...`
    - Movie -> `/movies?movieId=...`
    - Video -> `/library?groupId=...&videoId=...`
  - Added deep-link query handling:
    - `TVShows.tsx` now supports `episodeId` preselection after loading show episodes.
    - `Movies.tsx` now supports `movieId` preselection.
    - `Library.tsx` now supports `groupId` and `videoId` preselection/highlight.
  - Added Search-specific styles to `Page.css` for form controls and table layout.
- Validation:
  - TypeScript diagnostics clean across all modified files.

### 2026-04-22 - Use created date for TV show/episode Date sort

- Request: stop sorting TV shows/episodes by modified timestamp for the Date sort; use created date instead.
- Rationale: modified timestamps can drift from recording chronology and produced ordering that did not match user expectations.
- Solution:
  - Updated `TVShows.tsx` Date sort logic for show list and episode list to use `created_at` instead of `updated_at`.
  - Preserved existing sort toggle behavior and Date label.
- Validation:
  - TypeScript diagnostics clean for updated file.

## v1.2.2 — 2026-04-21

### 2026-04-21 - Sort toggle buttons; Live TV title wrap fix

- Request: replace sort dropdowns with toggle buttons showing sort direction; clicking the active sort reverses it; rename "Date Added" to "Date". Prevent "Live TV" heading from wrapping when source filter buttons overflow.
- Rationale: toggle buttons are more compact and discoverable; ▲/▼ arrows eliminate ambiguity about sort order. The heading wrap was a cosmetic regression when many sources are configured.
- Solution:
  - `TVShows.tsx`: added `showSortOrder` and `episodeSortOrder` state; replaced both `<select>` controls with paired `sort-btn` buttons; sort `useMemo` respects direction.
  - `Movies.tsx`: same pattern with `sortOrder` state.
  - `Page.css`: added `.sort-btn` and `.sort-btn--active` rules matching `.filter-btn` family.
  - `Live.tsx`: added `whiteSpace: nowrap` and `alignSelf: flex-start` to the `<h1>`.
- Validation: TypeScript diagnostics clean on all modified files.

## v1.2.1 — 2026-04-21

### 2026-04-21 - Gate "Mark as Not Recorded" on DVR rule association

- Request: "Mark as Not Recorded" only makes sense for recordings created by a DVR pass/rule. Disable the button for manually-recorded or otherwise unassociated files.
- Rationale: the action removes the program record so the DVR can re-record it — meaningless unless a rule is watching for it. Showing it unconditionally would confuse users.
- Solution:
  - Added `DvrFile` interface to `types.ts` with `ID`, `RuleID`, and companion fields from the `/dvr/files/{id}` response.
  - Added `fetchDvrFile(id)` to `recordings.ts` that GETs `/dvr/files/{id}`.
  - In `TVShows.tsx`: added `selectedEpisodeRuleId` state and a `selectEpisode(ep)` helper that calls `setSelectedEpisode` then fires `fetchDvrFile` in the background, updating `selectedEpisodeRuleId` on resolve. `onMarkNotRecorded` is now only passed when both `selectedEpisodeRuleId` is non-empty and `program_id` is present.
  - In `RecentRecordings.tsx`: same pattern with `selectedRuleId` state and `selectRecording(rec)` helper. State is also cleared on server reload (`useEffect` dependency on `serverChangeVersion`).
  - The `fetchDvrFile` call is fire-and-forget from the user's perspective — the button simply appears after the detail pane loads, with no visible loading state.
- Validation:
  - TypeScript diagnostics clean across all modified files.

### 2026-04-21 - Trash and Mark as Not Recorded actions on episode detail

- Request: add two mutation buttons to the episode/recording detail pane — "Trash" (permanently delete the file) and "Mark as Not Recorded" (remove the program record so the DVR can re-record it).
- Rationale: common DVR housekeeping tasks; the curl equivalents are `DELETE /dvr/files/{id}` and `DELETE /dvr/programs/{program_id}`.
- Solution:
  - Added `trashRecording(id)` (`DELETE /dvr/files/{id}`) and `markAsNotRecorded(programId)` (`DELETE /dvr/programs/{programId}`) to `src/api/recordings.ts` using the existing `runMutationCandidates` plumbing.
  - Added `program_id?: string` to `RecordingDetailItem` so callers can pass the EPxxxxxx identifier.
  - Added `onTrash` and `onMarkNotRecorded` optional callback props to `RecordingDetail`; the Trash button wraps its callback in a `window.confirm` to prevent accidental deletion.
  - Added `.rec-detail__actions` flex container and `.rec-detail__action-btn--secondary` / `--danger` styles to `Page.css`.
  - `TVShows.tsx`: added `handleTrashEpisode` and `handleMarkNotRecorded` handlers that call the API, optimistically remove the episode from the list, and deselect it on success. Both respect the `apiVersionApproved` guard.
  - `RecentRecordings.tsx`: same handlers; added `actionError` state displayed above the detail pane; wired `apiVersionApproved` from the store.
  - `onMarkNotRecorded` is only passed when `program_id` is present on the item.
- Validation:
  - TypeScript diagnostics clean across all modified files.

### 2026-04-21 - API version guard: block write actions on server update

- Request: Channels DVR occasionally ships breaking API changes. After a server update, mutations that worked before could produce unintended side effects. Implement a version check that blocks or warns about write actions until the user explicitly confirms the new version is safe.
- Rationale: The DVR API is undocumented and subject to change at any release. A silent break is worse than a visible prompt.
- Solution:
  - Added `fetchServerVersion(serverUrl)` to `client.ts` — hits `/api/v1/status` and extracts a version string. Returns `null` gracefully on any failure.
  - Extended `probeActiveServer()` in `useStore.ts` to fetch the version after a successful URL probe, then compare against a per-server approved version map stored in `localStorage` under `dvr_api_approved_versions`.
  - First connection to a server auto-approves the current version (no false alarm for new installs).
  - Added `apiVersion: string | null` and `apiVersionApproved: boolean` to store state; `approveApiVersion()` action saves the current version as approved for the active server.
  - `App.tsx` renders a persistent amber banner when `!probing && !apiVersionApproved`, with a link to Settings.
  - Settings page gains an "API Compatibility" section showing the detected version, approval status, and an "Approve v{version}" button.
  - `TVShows.toggleWatched` and `Movies.toggleWatched` check `apiVersionApproved` and surface an inline error rather than calling the API when blocked.
  - VideoPlayer automatic playback-position writeback is intentionally not blocked — it is low-risk and blocking it mid-playback would be disruptive.
- Validation:
  - TypeScript diagnostics clean across all six modified files.

### 2026-04-21 - Fix v1.2.0 CI build: MediaCard TypeScript errors

- Request: v1.2.0 GitHub Actions release workflow failed on all four targets.
- Symptoms: 36 TypeScript errors across all build jobs; `completed` prop used in component body and passed by callers but missing from `MediaCardProps`; unused destructured params (`id`, `commercials`, `filePath`, `recordingKind`) and unused `useStore` import caused `noUnusedLocals` errors.
- Root cause: the "Restore flags/badges" commit (2026-04-20) added `completed={…}` to `MediaCard` callers and used it inside the component but forgot to declare `completed?: boolean` in `MediaCardProps`, and left no-longer-needed destructured params in the function signature.
- Solution:
  - Added `completed?: boolean` to `MediaCardProps` interface.
  - Removed `useStore` import (unused since RecordingDetail extraction).
  - Removed `id`, `commercials`, `filePath`, `recordingKind` from the function destructuring (still in the interface; callers may pass them).
- Validation:
  - TypeScript diagnostics clean across `MediaCard.tsx`, `TVShows.tsx`, `Movies.tsx`.

### 2026-04-21 - MediaCard badge row wraps instead of scrolling

- Request: episode/movie cards with many badges were showing a horizontal scrollbar; wrap to a second line instead.
- Solution:
  - Changed `.media-card__badges` from `flex-wrap: nowrap; overflow-x: auto` to `flex-wrap: wrap` and removed the overflow scroll.
  - Tightened gap from `6px` to `4px` so wrapped rows stay compact.
- Validation:
  - CSS-only change; TypeScript diagnostics unaffected.

### 2026-04-21 - Fix Tailscale fallback: block page mounts until probe resolves

- Request: app would not load content when launched off-LAN even with a Tailscale URL configured.
- Symptoms: pages fetched data immediately on mount using the LAN URL before the async probe had a chance to switch to Tailscale, causing all API calls to fail.
- Root causes:
  1. Route components mounted and fired `useEffect` data fetches before `probeActiveServer()` finished.
  2. `probeActiveServer` did not increment `serverChangeVersion` when switching to Tailscale, so pages already mounted would not retry.
- Solution:
  - `App.tsx` now tracks a `probing` boolean; while `true`, routes are replaced with a "Connecting…" placeholder so no page mounts until the correct URL is confirmed.
  - `probeActiveServer` in `useStore.ts` now increments `serverChangeVersion` alongside `serverUrl` when switching to the Tailscale address, triggering a re-fetch on any mounted page.
- Validation:
  - TypeScript diagnostics clean across `App.tsx` and `useStore.ts`.

### 2026-04-20 - Restore flags/badges on TV Shows and Movies cards

- Request: badge indicators (favorited, delayed, cancelled, interrupted, content rating, tags) were not appearing on episode or movie cards.
- Symptoms: cards showed no badges at all despite the data being present on the API objects.
- Root cause: `MediaCard` calls in `TVShows.tsx` and `Movies.tsx` passed neither `badges` nor `completed` props; the earlier refactoring that removed the show-attributes modal did not re-add the badge-building logic.
- Solution:
  - Added `epBadges(ep)` helper to `TVShows.tsx` mirroring the flags shown in `RecordingDetail`.
  - Added `movieBadges(movie)` helper to `Movies.tsx` with identical logic.
  - Both `MediaCard` calls now pass `badges={…}` and `completed={…}` instead of the legacy single `badge` prop.
- Validation:
  - TypeScript diagnostics clean across both page files.

### 2026-04-20 - Tailscale dual-address support with automatic LAN probe

- Request: support two addresses per server (LAN + Tailscale); on startup probe which is reachable and use LAN if available, Tailscale otherwise.
- Rationale: users run Channels DVR on a home LAN but also need remote access via Tailscale; the app should auto-select the best URL without manual intervention.
- Solution:
  - Added optional `tailscaleUrl` field to `ServerOption` in `useStore.ts` and `parseServers`.
  - Added `probeUrl(url, timeoutMs)` to `client.ts` — uses `Promise.race` to enforce a 2.5 s timeout; returns `true` if any HTTP response is received (even 4xx), `false` on network error or timeout.
  - Added `probeActiveServer()` async action to the store — probes the LAN URL, switches `serverUrl` (and `dvr_server_url` in localStorage) to the Tailscale address if LAN is unreachable; falls back to LAN if neither responds.
  - Added a "Tailscale URL" column to the Settings servers table (optional, validated only if non-empty).
  - `testConnection` in Settings now probes LAN first and reports which address was used (LAN / Tailscale) when testing.
  - `App.tsx` calls `probeActiveServer()` on mount and whenever `activeServerId` changes.
- Validation:
  - TypeScript diagnostics clean across all four modified files.

### 2026-04-20 - Extract shared RecordingDetail component

- Request: episode detail page in TV Shows should be the identical component rendered in Recent Recordings, not a similar-looking duplicate.
- Rationale: the two pages were maintaining separate but functionally identical JSX for the detail pane, causing divergence and confusion.
- Solution:
  - Created `src/components/RecordingDetail.tsx` with a `RecordingDetailItem` interface satisfied by both `Recording` and `Episode`.
  - Component accepts `onPlay`, optional `onBack`/`backLabel`, and optional `onNavigateToShow` (renders "View all episodes" link only when provided).
  - `RecentRecordings` now renders `<RecordingDetail>` with `onNavigateToShow` pointing to `/tv?showId=…`.
  - `TVShows` now renders `<RecordingDetail>` with `onBack` returning to the series view; no "View all episodes" link (already on that page).
  - Removed all duplicate inline detail JSX from both pages.
  - Removed dead helpers from TVShows (`labelKey`, `formatValue`, `showAttributes`, `formatDateTime`, `formatDuration`, `getServerUrl` import) that were only used by the now-deleted show-attributes modal.
- Validation:
  - TypeScript diagnostics clean across all three files.

- Request: simplify TV Shows layout to two levels: (1) select show → see series image + description + episode grid; (2) click episode → episode detail view identical to RecentRecordings, with a back button.
- Rationale: previous layout had three side-by-side columns (show list, episode grid, detail pane) which was cramped and confusing; user wanted a single detail at a time.
- Solution:
  - `page__content` now has three conditional branches: episode detail view, series detail view, empty state.
  - Episode detail view reuses all `rec-detail__*` CSS classes and adds a `← Back to episodes` button calling `setSelectedEpisode(null)`.
  - Series detail view shows series image + description in a `tv-series-info` flex row, then the episode grid.
  - Removed third `ep-detail-panel` column entirely.
  - Removed `showMetaOpen` state and the show-attributes modal.
  - Removed `useNavigate` (unused), removed auto-select of first episode on show load.
  - Added `.tv-back-btn` and `.tv-series-info` CSS classes.
- Validation:
  - TypeScript diagnostics clean.

- Request: clicking an episode card in the TV Shows page should show the same rec-detail panel used in Recent Recordings.
- Rationale: the episode grid gave no visual feedback when a card was clicked because `setSelectedEpisode` was called but no detail UI was rendered; the user expected the identical layout to Recent Recordings.
- Root cause: `selectedEpisode` state was updated on click but the JSX had no conditional branch rendering episode details.
- Solution:
  - Added a third flex column (`ep-detail-panel`) to the `page--split` layout in TVShows.tsx, rendered when `selectedEpisode !== null`.
  - Pane reuses all `rec-detail__*` CSS classes from Page.css (thumbnail, progress bar, meta badges, description, genres).
  - Added `useNavigate`, `getServerUrl`, `formatDateTime`, and `formatDuration` to TVShows.tsx.
  - Added `.ep-detail-panel` CSS class (fixed 360 px column, left border, independent scroll).
  - Empty-state placeholder shown when a show is selected but no episode has been clicked.
- Validation:
  - TypeScript diagnostics clean; no errors in TVShows.tsx or Page.css.

### 2026-04-17 - TV shows filter URL persistence

- Request: persist TV Shows `All`/`Unwatched` filter across refresh and navigation.
- Rationale: users should not lose filter context when reloading or navigating back to the TV Shows page.
- Symptoms discovered:
  - filter state was local-only and reset to `All` on page reload
- Solution:
  - wired TV Shows filter to `filter` query param using `useSearchParams`
  - hydrating component state from URL and updating the URL when filter changes
- Validation:
  - TypeScript diagnostics clean for updated page; production build run after update

### 2026-04-17 - TV episode unwatched filter and recent-list progress indicators

- Request: add All/Unwatched filtering on TV Shows page; keep Recent Recordings unfiltered but show playback progress bars.
- Rationale: watched-state workflows should match Movies for episodic browsing, while recent recordings should surface resume progress without additional filter complexity.
- Symptoms discovered:
  - TV Shows page only exposed sorting and lacked watched-state filtering controls
  - Recent Recordings list did not visualize partial playback despite available `playback_time`
- Solution:
  - added `All` and `Unwatched` filter controls to the TV Shows episode grid, integrated with existing sort behavior
  - added slim per-item progress bars to Recent Recordings list rows when `playback_time > 0` and item is not watched
- Validation:
  - production build succeeds after updates

### 2026-04-17 - HAR-confirmed unwatch mutation contract

- Request: confirm and wire the inverse watched mutation using a dedicated unwatch HAR capture.
- Rationale: previous implementation used a fallback set for unwatch pending evidence of the exact server route.
- Symptoms discovered:
  - broad fallback variants for unwatch were no longer necessary after capture-driven endpoint discovery
- Solution:
  - switched unwatched action to exact `PUT /dvr/files/:id/unwatch` route with empty body
  - retained watched action as `PUT /dvr/files/:id/watch`
- Validation:
  - HAR parse confirms `PUT /dvr/files/65740/unwatch` returned `200`; production build run after code update

### 2026-04-17 - HAR-confirmed watched and playback-time mutations

- Request: replace endpoint guessing with exact mutation contracts captured from Channels web client HAR traces.
- Rationale: runtime probing was noisy and unreliable; captured traffic now provides definitive request paths.
- Symptoms discovered:
  - watched writes only succeeded in web client via a specific DVR route shape
  - playback resume writes used a path-parameter endpoint rather than JSON/form payload mutation
- Solution:
  - switched watched-on mutation to `PUT /dvr/files/:id/watch` (empty body)
  - switched playback position writeback to `PUT /dvr/files/:id/playback_time/:seconds` (empty body)
  - kept a small unwatch fallback set pending a dedicated HAR capture for the inverse action
- Validation:
  - TypeScript diagnostics clean for updated API module; production build validation run after change

### 2026-04-17 - Watched toggle and resume-progress writeback prototype

- Request: evaluate feasibility of watched/unwatched and resume playback state updates for recordings.
- Rationale: current app consumed read-only DVR API fields (`watched`, `playback_time`) but offered no UI write path.
- Symptoms discovered:
  - API client only supported `GET` calls
  - playback start did not seek to saved `playback_time`
  - no mutation controls existed in movie/episode card UI
- Solution:
  - added mutation-capable API helper (`requestWithMethod`) and recording update wrappers with fallback endpoint/payload shapes
  - added watched/unwatched action button on `MediaCard` and wired it into Movies and TV Shows pages with optimistic UI + rollback on failure
  - extended playback store state to carry recording kind and resume time
  - updated player to seek to saved resume point and periodically attempt playback-time + watched writeback for episode/movie playback
- Validation:
  - TypeScript/Problems check run after edits; remaining verification requires live DVR endpoint confirmation for undocumented mutation routes

### 2026-04-17 - Playback diagnostics exposes winning mutation endpoint

- Request: show which write endpoint shape succeeds so undocumented API discovery is visible in-app.
- Rationale: endpoint guessing should be auditable during real playback/testing runs.
- Symptoms discovered:
  - write helpers attempted multiple candidate routes but success path was only visible in logs
- Solution:
  - tracked last successful mutation candidate in recording API helper
  - added `Last Mutation` row to player diagnostics panel and playback report output
- Validation:
  - local build passes; runtime confirmation requires exercising watched/resume actions against a live DVR server

### 2026-04-17 - Expanded writeback endpoint probes after 404 diagnostics

- Request: investigate `No compatible mutation endpoint found` failures when marking movies watched.
- Rationale: initial probes targeted only a narrow `/api/v1` shape and consistently returned `404`.
- Symptoms discovered:
  - watched toggle errors showed every attempted `/api/v1/movies/:id` mutation variant failing with `404`
  - `Last Mutation` remained `n/a`, indicating no successful write path discovered
- Solution:
  - expanded mutation candidates to include likely `/dvr/files/:id` and related `/watch`, `/watched`, `/playback`, `/resume`, `/set` paths
  - added alternate payload key/value shapes (`true/false`, `1/0`, playback aliases)
  - added session-level unsupported-signature cache to avoid repeatedly spamming the same failed candidate set during periodic playback sync
- Validation:
  - local build and type checks pass; live endpoint discovery still required against actual DVR server behavior

### 2026-04-17 - Mutation failure telemetry and form-body probes

- Request: make failed writeback attempts easier to inspect and try a lower-level body format on the promising `/dvr/files/:id` route.
- Rationale: `PUT /dvr/files/:id` returned `503` while most other candidates returned `404`, suggesting the route may exist but expect a different request shape.
- Symptoms discovered:
  - user-facing error text became very long and hard to compare across attempts
  - diagnostics only exposed successful mutations, not the most recent failure cause
- Solution:
  - added `application/x-www-form-urlencoded` body support in the API client
  - added form-body variants for `/dvr/files/:id` and related set paths
  - exposed `Last Failure` alongside `Last Mutation` in player diagnostics and copied playback report output
- Validation:
  - local build passes; next live test should reveal whether form-encoded requests change the `/dvr/files/:id` response behavior

### 2026-04-16 - Version metadata bump to 1.1.8 for hidden-channel release

- Request: cut a new release version that includes the Live hidden-channel visibility option.
- Rationale: `v1.1.7` did not include commit `64efa75`; release installers must include the new setting.
- Symptoms discovered:
  - hidden-channel setting was present on `main` but not in `v1.1.7` tagged artifacts
- Solution:
  - bumped version metadata to `1.1.8` in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `src-tauri/Cargo.lock`
  - prepared fresh `v1.1.8` tag from current `main`
- Validation:
  - local Problems check passes on changed files; tag release workflow will enforce version/tag parity

### 2026-04-16 - Live TV hidden-channel visibility setting

- Request: add a user option to hide/show channels that are marked hidden by source/DVR metadata in Live view.
- Rationale: hidden channels should not appear in normal Live browsing unless explicitly requested for diagnostics or edge cases.
- Symptoms discovered:
  - some users saw hidden HDHR/Channels DVR channels (including duplicates/SD/alternate language feeds) in Live list
- Solution:
  - added persisted app setting `showHiddenLiveChannels` (Settings -> Live TV -> Show hidden channels)
  - extended Live guide parsing to read hidden flags from guide channel metadata
  - added hidden-channel predicate that honors multiple API flag variants (`hidden`, `is_hidden`, `disabled`, `enabled=false`, `visible=false`) plus guide-hidden keys
  - applied hidden filter to the Live row set (affecting list, source counts, and diagnostics matrix) unless the new setting is enabled
- Validation:
  - local Problems check passes for touched files
  - change kept local per request; not pushed

### 2026-04-16 - Linux ARM64 AppImage bundling fix (`xdg-open`)

- Request: determine why Linux ARM64 failed and produce a corrected release.
- Rationale: ARM64 Linux artifact generation should fail only for real compile issues, not missing base tools in runner images.
- Symptoms discovered:
  - `Build (aarch64-unknown-linux-gnu)` failed in `Build Tauri app`
  - job logs showed `failed to bundle project xdg-open binary not found /usr/bin/xdg-open`
- Solution:
  - added `xdg-utils` to Linux dependency install step in release workflow
  - bumped release metadata to `1.1.7` across manifest files for a fresh, consistent tagged release
- Validation:
  - local Problems checks pass for updated workflow and version files; next tagged release run should include Linux ARM64 AppImage bundling prerequisites

### 2026-04-16 - v1.1.5 workflow failure recovery via version correction

- Request: fix release workflow failures after the new gating step blocked compilation.
- Rationale: the version gate should prevent mixed-version assets, but the next release tag must match manifests exactly so publishing can proceed.
- Symptoms discovered:
  - all `v1.1.5` build jobs failed in `Verify tag version matches manifests`
  - manifests were still `1.1.4` while the pushed tag was `v1.1.5`
- Solution:
  - bumped release-driving metadata to `1.1.6` in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `src-tauri/Cargo.lock`
  - prepared next release to use a fresh matching tag rather than mutating the failed `v1.1.5` run again
- Validation:
  - local problems check to confirm updated files remain valid before retriggering release

### 2026-04-16 - Latest release pointer recovery and resilient release gating

- Request: ensure `/releases/latest` points to a release with install packages instead of stale source-only `v1.1.2`.
- Rationale: failed newer release runs should not prevent publishing valid installers and updating the latest release pointer.
- Symptoms discovered:
  - `v1.1.4` workflow failed in Windows verify step due shell-sensitive inline Node command
  - Linux ARM64 leg failure blocked publish job, leaving no newer release than `v1.1.2`
- Solution:
  - replaced inline verification commands with shared script `.github/scripts/verify-tag-version.mjs` used by Windows and Linux jobs
  - made Linux ARM64 matrix leg non-blocking (`continue-on-error` with `allow_failure: true`) so publish can proceed with available artifacts
- Validation:
  - local problems check passes; next tagged run should publish assets and advance latest release

### 2026-04-16 - Enforced build/version increment consistency

- Request: ensure build number increments on all release assets.
- Rationale: release tags and generated installer/package filenames must stay in sync to avoid mixed-version artifacts.
- Symptoms discovered:
  - previous tags could produce assets with older embedded app version when metadata had not been bumped first
- Solution:
  - bumped app version metadata from `1.1.3` to `1.1.4` in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `src-tauri/Cargo.lock`
  - added release workflow gate in both Windows and Linux build jobs to verify `GITHUB_REF_NAME` tag version matches manifest versions before building
- Validation:
  - local problems check passes for updated files; next release tag will fail fast if version metadata is not incremented consistently

### 2026-04-16 - Linux release pipeline for x64 and ARM64

- Request: attempt Linux builds for x64 and ARM64 to support Debian/Ubuntu-style distributions.
- Rationale: publish native Linux install assets alongside Windows to broaden supported client platforms.
- Symptoms discovered:
  - existing release workflow only produced Windows (`msi`/`nsis`) artifacts
- Solution:
  - split release build jobs into `build-windows` and `build-linux`
  - added Linux matrix targets for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`
  - installed required Linux Tauri build dependencies on runner
  - added Linux artifact upload (`.deb`, `.AppImage`) and expanded publish globs to include Linux assets
- Validation:
  - workflow syntax/Problems checks pass locally; release run required to confirm runner availability and resulting artifacts

### 2026-04-16 - Version metadata bump to 1.1.3 for installer filenames

- Request: ensure release installer filenames show `1.1.3` instead of `1.1.1`.
- Rationale: tagged release version and generated installer version should match for support and distribution clarity.
- Symptoms discovered:
  - `v1.1.3` release assets were published but filenames were still `WinChannels_1.1.1_...`
- Solution:
  - bumped version metadata to `1.1.3` in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and root package entry in `src-tauri/Cargo.lock`
- Validation:
  - Problems check passes for updated files; ready for release rebuild via tag push

### 2026-04-16 - Release asset upload path fix

- Request: fix tagged releases that completed workflow runs but still showed source-only assets.
- Rationale: successful build jobs should always result in attached installer assets on the GitHub Release.
- Symptoms discovered:
  - latest `Release` workflow runs reported success across build/upload/download/publish steps
  - `v1.1.2` release still had zero attached assets
- Solution:
  - updated release publish globs to recursive patterns (`release-artifacts/**/*.msi`, `release-artifacts/**/*-setup.exe`) to match downloaded artifact subfolder layouts
  - added explicit artifact listing step before publish for visibility in workflow logs
- Validation:
  - workflow syntax/Problems checks pass locally; ready for retrigger on next tag

### 2026-04-16 - Release publish fix for source-only tag releases

- Request: resolve repeated release workflow failures where tagged releases showed only source archives.
- Rationale: installer assets must publish reliably for both x64 and ARM64 builds.
- Symptoms discovered:
  - `Publish GitHub Release` step failed with `Resource not accessible by integration` on release notes generation API
  - release page remained with source-only assets
- Solution:
  - updated release workflow to stop calling `generate_release_notes` and use explicit `GITHUB_TOKEN` in `softprops/action-gh-release`
- Validation:
  - workflow file updated; ready for retrigger via tag push

### 2026-04-15 - Fullscreen label and stats hotkey toggle corrections

- Request: fix fullscreen button text showing `Exit Fullscreen` at startup and make stats hotkey toggle persist instead of only while key is held.
- Rationale: diagnostics controls should accurately reflect state and behave predictably during playback.
- Symptoms discovered:
  - fullscreen control could render the exit label before entering fullscreen
  - stats overlay toggled on keydown and back off on keyup, causing press-and-hold behavior
- Solution:
  - changed overlay fullscreen state check to require a non-null fullscreen element match
  - removed keyup listener from stats hotkey path so `Shift+S` toggles only on keydown
- Validation:
  - TypeScript/Problems check reports no errors in updated files

### 2026-04-15 - Fullscreen-safe stats hotkey and player fullscreen control

- Request: make `Shift+S` reliably toggle stats while fullscreen.
- Rationale: native video fullscreen can intercept some keys before app-level handlers, causing diagnostics hotkeys to appear broken.
- Symptoms discovered:
  - even after expanding listeners, `Shift+S` still did not always fire when using native video fullscreen controls
- Solution:
  - hardened hotkey matching (`code === KeyS` with shift fallback to key value) and added capture listeners on window/document
  - added app-controlled fullscreen toggle on the player overlay so keyboard events stay inside app-managed DOM focus
  - focused overlay container on entering overlay fullscreen to keep shortcut handling reliable
- Validation:
  - TypeScript/Problems check reports no errors in updated files

### 2026-04-15 - Stats hotkey support in fullscreen playback

- Request: make the diagnostics stats hotkey work while video is in fullscreen mode.
- Rationale: fullscreen troubleshooting should not require exiting fullscreen to toggle diagnostics.
- Symptoms discovered:
  - `Shift+S` worked in windowed playback but could miss key events in fullscreen focus contexts
- Solution:
  - expanded key listeners from window-only to window + document capture + video element listeners
  - retained diagnostics-enabled and active-playback gating for shortcut behavior
- Validation:
  - TypeScript/Problems check reports no errors in updated files

### 2026-04-15 - Stats overlay keyboard toggle

- Request: add a keyboard shortcut for toggling player diagnostics stats.
- Rationale: quick in-playback toggling is faster than targeting a small header button during troubleshooting.
- Symptoms discovered:
  - stats overlay could only be toggled via mouse click
- Solution:
  - added `Shift+S` key handling in `VideoPlayer` when diagnostics tools are enabled and playback is active
  - updated the Stats button tooltip to document the shortcut
- Validation:
  - TypeScript/Problems check reports no errors in updated files

### 2026-04-15 - Player diagnostics "Stats for Nerds" overlay

- Request: add a YouTube-style in-player diagnostics view to inspect live playback performance and stream selection.
- Rationale: playback quality and stutter reports are easier to debug when stream/video telemetry is visible during playback, not only in copied logs.
- Symptoms discovered:
  - current diagnostics exposed only a copy-to-clipboard report and no live metrics view
  - users needed immediate visibility for bitrate estimate, selected level, buffering, and dropped-frame behavior
- Solution:
  - added a diagnostics-only `Stats` toggle in `VideoPlayer` header controls
  - implemented a live overlay panel updating every second with state, resolution, selected level, bandwidth estimate, buffer ahead, dropped/decoded frames, dropped-frame percentage, playback rate, volume, and manifest URL
  - extended copied playback report to include the same runtime metrics snapshot
  - updated Settings diagnostics hint text to describe the new overlay tool
- Validation:
  - TypeScript/Problems check reports no errors in changed files

### 2026-04-15 - Release workflow adds Windows ARM64 artifacts

- Request: publish Windows ARM64 installers in addition to existing x64 artifacts so Snapdragon users can install native builds.
- Rationale: release automation should ship both supported Windows architectures from tagged builds.
- Symptoms discovered:
  - local ARM64 build attempt failed on missing ARM64 MSVC cross tools (`cl.exe`/`clang` not found), indicating architecture-specific toolchain requirements
  - existing release workflow built only one default Windows target and uploaded only x64 paths
- Solution:
  - changed `.github/workflows/release.yml` to a matrix build for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`
  - configured Rust setup with per-matrix target installation
  - uploaded per-target installer bundles as artifacts and added a single publish job that downloads and releases all installers together
- Validation:
  - workflow YAML updated with explicit per-target build commands and release file globs for both architectures

## Version 1.1.1 (2026-04-15)

### Fix duplicate channels in Favorites view

- Request: favorites list showed the same channel multiple times when it was present across multiple tuner sources.
- Rationale: `dedupeRows` was only applied for the `all` filter mode; the `favorites` filter left all per-source rows in the list.
- Solution: extended the `dedupeRows` call to also run when `filterMode === 'favorites'`.
- Validation: build passes; favorites view now shows each channel once.

### 2026-04-15 - Version bump to 1.1.0 for release parity

- Request: bump app version to `1.1` and ensure installer artifacts carry the same version.
- Rationale: shipped binaries and in-app/version metadata must stay aligned for supportability and release clarity.
- Solution:
  - updated `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `src-tauri/Cargo.lock` to `1.1.0`
  - prepared release by pushing matching git tag `v1.1.0` so CI-generated installers inherit the same version line
- Validation:
  - project build succeeds after version updates

### 2026-04-15 - Live diagnostics favorites and matching corrections

- Request: reduce false-positive favorites and align same channels (for example `TLC`) into the same comparison row across sources.
- Rationale: the diagnostics table should be trustworthy for source comparison and should not imply favorites that are not explicitly flagged by authoritative metadata.
- Symptoms discovered:
  - too many channels were marked favorite in Live diagnostics
  - same channel names could appear on separate rows across sources
- Solution:
  - tightened favorite detection to explicit channel/guide favorite flags only
  - changed diagnostics grouping key from channel number-only to normalized channel name
  - kept number/name sort controls while using representative minimum number for numeric ordering
- Validation:
  - build succeeds
  - same-name channels now align into shared rows across sources

### 2026-04-15 - Live source diagnostics matrix

- Request: add a diagnostics-only tool on Live showing a popup table of channels by source.
- Rationale: troubleshooting source differences (especially between similarly named sources) requires a single view that clearly shows channel membership per source instance.
- Symptoms discovered:
  - regular list filtering was useful for browsing but not ideal for side-by-side source inventory visibility
- Solution:
  - added a `Source Diagnostics` button on Live, shown only when diagnostics are enabled in settings
  - added modal with source-grouped channel tables including number, name, id, and favorite field state
- Validation:
  - build succeeds
  - diagnostics control is gated behind the existing diagnostics setting

### 2026-04-15 - Diagnostics matrix layout refactor

- Request: refactor diagnostics display to show sources across the top with channels listed in columns under each source.
- Rationale: source-by-source comparison is easier when all sources are aligned in one matrix rather than stacked as separate tables.
- Solution:
  - replaced per-source stacked tables with a single cross-source matrix keyed by channel number
  - sources are column headers and channel numbers are row headers
  - added diagnostics sort options for channel number and channel name
  - each cell stacks all matching entries for that source/number and shows logo, name, id, and favorite marker
- Validation:
  - build succeeds
  - matrix renders sources horizontally for direct comparison and keeps duplicate per-source matches visible within each cell

### 2026-04-15 - Live source split and favorites parity tuning

- Request: expose each real channel source instance separately at the top of Live (not merged by source name), while keeping `All Channels` and `Favorites` meta filters.
- Rationale: multiple HDHomeRun devices with the same source name can still differ in feed behavior, so source-instance visibility is needed for troubleshooting and comparison.
- Symptoms discovered:
  - source filters merged both HDHomeRun devices into one `HDHomeRun QUATRO` entry
  - some channels appeared as favorites in Channels UX context but were missing from Live favorites when `favorited` was absent in `/api/v1/channels`
- Solution:
  - changed source filters to use `source_name + source_id` labels (per-source-instance chips)
  - kept meta filters (`All Channels`, `Favorites`) as top-level options
  - expanded favorites detection to combine channel `favorited`, guide `Favorite` flags, and membership in `/dvr/guide/channels?source=favorites`
- Validation:
  - build succeeds
  - source chips now expose each real source instance separately
  - custom channels in the server favorites source can now be treated as favorites even when explicit flags are missing

### 2026-04-15 - VS Code build-task workflow approximation

- Request: approximate a Visual Studio-style Build menu experience in VS Code.
- Rationale: VS Code does not expose the same top-level Build menu model as Visual Studio, but default build tasks provide equivalent day-to-day behavior.
- Symptoms discovered:
  - F5 launch worked, but there was no obvious build-menu-style action
- Solution:
  - marked `WinChannels: Build` as the default build task in `.vscode/tasks.json`
  - this enables `Terminal -> Run Build Task...` and `Ctrl+Shift+B` for one-step builds
- Validation:
  - build task remains `npm run build` and continues to execute successfully

### 2026-04-15 - VS Code F5 launch configuration

- Request: make F5 start this project reliably from VS Code Run and Debug.
- Rationale: running through helper scripts worked, but F5 had no workspace launch configuration to invoke the app.
- Symptoms discovered:
  - pressing F5 did not start WinChannels debugging workflow
- Solution:
  - added `.vscode/launch.json` with a `node-terminal` launch profile that runs `npm run tauri dev`
  - added `.vscode/tasks.json` with named dev/build tasks for consistency
- Validation:
  - configuration files are present and valid JSON; project build remains successful

### 2026-04-15 - Convenience scripts for common commands

- Request: add root-level `run.cmd` and `build.cmd` wrappers for the common npm commands.
- Rationale: the full npm commands were easy to forget during routine local development.
- Symptoms discovered:
  - repeated need to recall or retype the exact dev and build commands
- Solution:
  - added `run.cmd` to call `npm run tauri dev`
  - added `build.cmd` to call `npm run build`
- Validation:
  - executed the build helper successfully

### 2026-04-15 - Live TV: source filters, dedupe, playback, logos

- Request: add a Live TV view with a list-style channel browser, sorting, filtering, and click-to-play behavior.
- Rationale: a list matched the existing TV Shows navigation model better than a dense guide grid and was sufficient for initial live playback support.
- Symptoms discovered:
  - initial list rows were inert because they had no play behavior
  - early filter rendering behaved incorrectly when duplicate channels were present
  - guessed live playback endpoints produced `manifestLoadError` 404s and `levelLoadError` 500s on some channels
  - custom 9000+ channels had no `logo_url` in the API even though Channels DVR web showed logos
- Solution:
  - added a Live page with channel sorting and filtering
  - changed source menus to use unique `source_name` values only
  - deduped the `All Channels` view while preserving source-specific filtering
  - resolved live playback against Channels DVR live HLS endpoints that match web playback behavior
  - centralized channel logo fallback logic using `station_id -> /tmsimg/assets/...`
  - added explicit list scrolling and kept duplicate-aware playback support
- Validation:
  - production build passes
  - live playback works with the verified `/devices/ANY/channels/{number}/hls` endpoint pattern
  - custom channel logos can be resolved from `station_id` fallback

### 2026-04-14 - TV and movie browsing UX refinement

- Request: make TV Shows and Movies behavior more consistent with Recent Recordings, then refine TV detail layout further.
- Rationale: the previous flows were inconsistent and exposed too much raw metadata in the TV detail view.
- Symptoms discovered:
  - TV details showed raw API output that was noisy and hard to scan
  - movie and TV navigation patterns differed more than necessary
- Solution:
  - reworked TV show list/detail flow to use artwork, summary, episode grid, and metadata popup
  - reworked movie browsing into a cleaner detail-first flow with back navigation
  - renamed Library to Videos for clearer navigation
- Validation:
  - UX changes were reviewed interactively and accepted during the session

### 2026-04-14 - Versioning, About page, and release wiring

- Request: surface version `1.0` in-app and make release packaging reflect it.
- Rationale: release builds and in-app UI needed a single visible version source for shipping and support.
- Solution:
  - bumped app, Tauri, and Rust package versions
  - added compile-time version constants and About section in Settings
  - ensured release workflow could publish tagged builds
- Validation:
  - tagged releases were created and pushed successfully

### 2026-04-14 - Playback quality investigation and remux preference

- Request: investigate blocky playback compared with Channels DVR web playback.
- Rationale: WinChannels should not force a visibly worse transcoding path than the native web client when the DVR can serve a better stream.
- Symptoms discovered:
  - DVR logs showed WinChannels requests triggering `h264_vaapi` while the web UI often used `remux`
  - HLS level selection was not reliably choosing the best variant
- Solution:
  - used absolute HLS URLs in Tauri contexts and routed playback through the Tauri HLS loader in dev and prod
  - added browser-like headers in the loader
  - added remux-preferred manifest requests with fallback to default manifests on load failure
  - switched level choice to highest bitrate rather than index order
  - added optional diagnostics and playback report capture
- Validation:
  - user confirmed playback quality improved and remux path activation was visible in DVR behavior

### 2026-04-14 - Multi-server support and connection diagnostics

- Request: support multiple DVR servers, allow switching in the sidebar, and test each configured server.
- Rationale: the app needed to work across several DVRs without editing config or restarting the app.
- Symptoms discovered:
  - connection tests initially used stale server state until settings were saved
  - server changes required restart-like behavior without an explicit refetch trigger
- Solution:
  - added persisted server table config with active-server selection
  - added sidebar dropdown for server choice
  - added server-change versioning to trigger refetch across pages
  - changed test flow to save first and then test all configured servers with per-server output
- Validation:
  - user reported all configured servers tested successfully

### 2026-04-14 - Tauri scope fix for non-default DVR ports

- Request: allow DVR access on ports other than `8089`.
- Rationale: the Tauri HTTP allow-list was too narrow and blocked legitimate DVR hosts on alternate ports.
- Symptoms discovered:
  - requests succeeded on `:8089` but failed on another DVR port due to scope restrictions
- Solution:
  - updated Tauri capability URL patterns to allow host + any port for HTTP and HTTPS
- Validation:
  - alternate-port DVR requests became reachable after the capability change