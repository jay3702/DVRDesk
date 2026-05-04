import { useEffect, useRef } from 'react';
import type React from 'react';
import { useStore } from '../store/useStore';

// ── Selectors ────────────────────────────────────────────────────────────────

const SIDEBAR_SEL = '.sidebar__nav a.sidebar__link';
// List-style items: show/channel rows, recent recordings rows
const LIST_SEL = 'button.show-item, button.rec-item';
// Card grid items
const GRID_SEL = '.media-card[tabindex="0"]';

// Maximum pixel difference between two items' tops to be considered the same row
const ROW_TOLERANCE = 16;

// ── Types ────────────────────────────────────────────────────────────────────

type NavZone = 'sidebar' | 'list' | 'grid' | 'detail';

// ── Helpers ──────────────────────────────────────────────────────────────────

function getItems(sel: string): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>(sel)).filter(
    (el) => el.offsetParent !== null, // visible only
  );
}

function getZone(el: Element | null): NavZone | null {
  if (!el || !(el instanceof HTMLElement)) return null;
  if (el.matches('a.sidebar__link')) return 'sidebar';
  if (el.matches('button.show-item, button.rec-item')) return 'list';
  if (el.matches('.media-card')) return 'grid';
  if (el.closest('.media-detail, .rec-detail')) return 'detail';
  return null;
}

/** Group elements into rows by their viewport-top offset. */
function toRows(items: HTMLElement[]): HTMLElement[][] {
  const rows: HTMLElement[][] = [];
  for (const item of items) {
    const top = item.getBoundingClientRect().top;
    const existing = rows.find(
      (r) => Math.abs(r[0].getBoundingClientRect().top - top) < ROW_TOLERANCE,
    );
    if (existing) {
      existing.push(item);
    } else {
      rows.push([item]);
    }
  }
  return rows;
}

/** Return the element in `row` whose horizontal center is closest to `target`'s. */
function closestHoriz(target: HTMLElement, row: HTMLElement[]): HTMLElement {
  const tRect = target.getBoundingClientRect();
  const tCenter = tRect.left + tRect.width / 2;
  return row.reduce((best, curr) => {
    const cRect = curr.getBoundingClientRect();
    const bRect = best.getBoundingClientRect();
    const cCenter = cRect.left + cRect.width / 2;
    const bCenter = bRect.left + bRect.width / 2;
    return Math.abs(cCenter - tCenter) < Math.abs(bCenter - tCenter) ? curr : best;
  });
}

function focusEl(el: HTMLElement) {
  el.focus({ preventScroll: true });
  el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}

function activeSidebarLink(sidebarItems: HTMLElement[]): HTMLElement {
  return (
    sidebarItems.find((a) => a.classList.contains('sidebar__link--active')) ??
    sidebarItems[0]
  );
}

/** Return the first visible button inside a .media-detail or .rec-detail container. */
function firstDetailButton(): HTMLElement | null {
  const container = document.querySelector<HTMLElement>('.media-detail, .rec-detail');
  if (!container || container.offsetParent === null) return null;
  return (
    Array.from(container.querySelectorAll<HTMLElement>('button')).find(
      (el) => el.offsetParent !== null,
    ) ?? null
  );
}

/** Focus helper used by both Escape and ArrowLeft: go to list or sidebar. */
function focusListOrSidebar(lastListFocusRef: React.MutableRefObject<HTMLElement | null>) {
  const listItems = getItems(LIST_SEL);
  if (listItems.length > 0) {
    const target =
      lastListFocusRef.current && listItems.includes(lastListFocusRef.current)
        ? lastListFocusRef.current
        : listItems[0];
    focusEl(target);
    lastListFocusRef.current = target;
  } else {
    const sidebarItems = getItems(SIDEBAR_SEL);
    if (sidebarItems.length > 0) focusEl(activeSidebarLink(sidebarItems));
  }
}

// ── Hook ─────────────────────────────────────────────────────────────────────

/**
 * Global D-pad keyboard navigation for the app shell.
 *
 * Zones (left → right):
 *   sidebar  →  list (show-item / rec-item)  →  grid (media-card)
 *
 * Detail zone: buttons inside .media-detail or .rec-detail — Up/Down navigates.
 * Escape clicks the back button (.media-detail__back / .tv-back-btn) if visible.
 */
export function useKeyboardNav() {
  // Remember last-focused list item so returning from the grid lands on it.
  const lastListFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    function handle(e: KeyboardEvent) {
      // Video player owns the keyboard when playing.
      if (useStore.getState().nowPlayingId) return;

      const active = document.activeElement as HTMLElement | null;

      // Don't intercept inside modals.
      if (active?.closest('.media-modal')) return;

      const dir = e.key;
      if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Escape'].includes(dir)) return;

      // ── Escape ──────────────────────────────────────────────────────────────
      if (dir === 'Escape') {
        // From a search input: go back to the active sidebar link.
        if (active?.matches('.search-panel__input')) {
          e.preventDefault();
          const sidebarItems = getItems(SIDEBAR_SEL);
          if (sidebarItems.length > 0) focusEl(activeSidebarLink(sidebarItems));
          return;
        }
        // Click a visible explicit back button, then restore focus to the selected card.
        const backBtn = document.querySelector<HTMLButtonElement>(
          'button.media-detail__back, button.tv-back-btn',
        );
        if (backBtn && backBtn.offsetParent !== null) {
          e.preventDefault();
          const selectedCard = document.querySelector<HTMLElement>('.media-card--selected');
          backBtn.click();
          // Restore focus after React re-renders (keyed lists reuse DOM nodes).
          requestAnimationFrame(() => {
            if (selectedCard) {
              selectedCard.focus({ preventScroll: true });
              selectedCard.scrollIntoView({ block: 'nearest' });
            } else if (lastListFocusRef.current?.offsetParent !== null) {
              focusEl(lastListFocusRef.current!);
            }
          });
          return;
        }
        // No explicit back button: if focus is inside a detail pane, go to list/sidebar.
        if (active?.closest('.media-detail, .rec-detail')) {
          e.preventDefault();
          focusListOrSidebar(lastListFocusRef);
          return;
        }
        return;
      }

      // Don't intercept arrow keys on text inputs / selects.
      if (active) {
        const tag = active.tagName.toLowerCase();
        if (tag === 'input' || tag === 'textarea' || tag === 'select') return;
      }

      const zone = getZone(active);

      // If nothing nav-relevant is focused, any arrow focuses the first sidebar link.
      if (!zone) {
        const items = getItems(SIDEBAR_SEL);
        if (items.length > 0) {
          e.preventDefault();
          focusEl(items[0]);
        }
        return;
      }

      e.preventDefault();

      // ── Sidebar ────────────────────────────────────────────────────────────
      if (zone === 'sidebar') {
        const items = getItems(SIDEBAR_SEL);
        const idx = items.indexOf(active!);

        if (dir === 'ArrowUp' && idx > 0) {
          focusEl(items[idx - 1]);
        } else if (dir === 'ArrowDown' && idx < items.length - 1) {
          focusEl(items[idx + 1]);
        } else if (dir === 'ArrowRight') {
          // Search page: go straight to the keyword input.
          const searchInput = document.querySelector<HTMLElement>('.search-panel__input');
          if (searchInput && searchInput.offsetParent !== null) {
            focusEl(searchInput);
            return;
          }
          // Prefer list zone, then grid, then detail.
          const listItems = getItems(LIST_SEL);
          if (listItems.length > 0) {
            const target =
              lastListFocusRef.current && listItems.includes(lastListFocusRef.current)
                ? lastListFocusRef.current
                : listItems[0];
            focusEl(target);
            lastListFocusRef.current = target;
          } else {
            const gridItems = getItems(GRID_SEL);
            if (gridItems.length > 0) {
              focusEl(gridItems[0]);
            } else {
              const detailBtn = firstDetailButton();
              if (detailBtn) focusEl(detailBtn);
            }
          }
        }
        // ArrowLeft in sidebar: nowhere to go.
      }

      // ── List zone ──────────────────────────────────────────────────────────
      else if (zone === 'list') {
        lastListFocusRef.current = active;
        const items = getItems(LIST_SEL);
        const idx = items.indexOf(active!);

        if (dir === 'ArrowUp' && idx > 0) {
          focusEl(items[idx - 1]);
          lastListFocusRef.current = items[idx - 1];
        } else if (dir === 'ArrowDown' && idx < items.length - 1) {
          focusEl(items[idx + 1]);
          lastListFocusRef.current = items[idx + 1];
        } else if (dir === 'ArrowLeft') {
          const sidebarItems = getItems(SIDEBAR_SEL);
          if (sidebarItems.length > 0) focusEl(activeSidebarLink(sidebarItems));
        } else if (dir === 'ArrowRight') {
          // Prefer grid; fall back to detail zone if no grid is visible.
          const gridItems = getItems(GRID_SEL);
          if (gridItems.length > 0) {
            focusEl(gridItems[0]);
          } else {
            const detailBtn = firstDetailButton();
            if (detailBtn) focusEl(detailBtn);
          }
        }
      }

      // ── Grid zone ──────────────────────────────────────────────────────────
      else if (zone === 'grid') {
        const items = getItems(GRID_SEL);
        if (items.length === 0) return;

        const rows = toRows(items);
        const ri = rows.findIndex((r) => r.includes(active!));
        if (ri === -1) return;

        const row = rows[ri];
        const ci = row.indexOf(active!);

        if (dir === 'ArrowRight') {
          // Move right within row; stop at end.
          if (ci < row.length - 1) focusEl(row[ci + 1]);
        } else if (dir === 'ArrowLeft') {
          if (ci > 0) {
            focusEl(row[ci - 1]);
          } else {
            focusListOrSidebar(lastListFocusRef);
          }
        } else if (dir === 'ArrowDown') {
          if (ri < rows.length - 1) {
            focusEl(closestHoriz(active!, rows[ri + 1]));
          }
        } else if (dir === 'ArrowUp') {
          if (ri > 0) {
            focusEl(closestHoriz(active!, rows[ri - 1]));
          } else {
            // Top row: go back to list or sidebar.
            focusListOrSidebar(lastListFocusRef);
          }
        }
      }

      // ── Detail zone ────────────────────────────────────────────────────────
      else if (zone === 'detail') {
        const container = active!.closest<HTMLElement>('.media-detail, .rec-detail');
        if (!container) return;
        const items = Array.from(container.querySelectorAll<HTMLElement>('button')).filter(
          (el) => el.offsetParent !== null,
        );
        const idx = items.indexOf(active as HTMLElement);
        if (idx === -1) return;

        if (dir === 'ArrowUp' && idx > 0) {
          focusEl(items[idx - 1]);
        } else if (dir === 'ArrowDown' && idx < items.length - 1) {
          focusEl(items[idx + 1]);
        } else if (dir === 'ArrowLeft') {
          // Go back to list or sidebar.
          focusListOrSidebar(lastListFocusRef);
        }
        // ArrowRight in detail zone: no-op.
      }
    }

    window.addEventListener('keydown', handle, true);
    return () => window.removeEventListener('keydown', handle, true);
  }, []);
}
