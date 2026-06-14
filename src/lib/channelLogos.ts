import { getServerUrl } from '../api/client';
import type { Channel } from '../api/types';

const TMS_CDN_PREFIX = 'https://tmsimg.fancybits.co/assets/';
const TMS_LOCAL_PATH = '/tmsimg/assets/';

export function channelLogoUrl(channel: Pick<Channel, 'logo_url' | 'station_id'>): string | undefined {
  const direct = channel.logo_url?.trim();
  if (direct) return direct;
  const station = (channel.station_id || '').trim();
  if (!station) return undefined;
  const server = getServerUrl();
  return `${server}/tmsimg/assets/s${station}_ll_h15_ab.png?w=360&h=270`;
}

export function nextLogoVariant(url: string): string | null {
  const m = url.match(/_ll_h15_(ab|ac|aa)\.png\?w=360&h=270$/i);
  if (!m) return null;
  const current = m[1].toLowerCase();
  const next = current === 'ab' ? 'ac' : current === 'ac' ? 'aa' : null;
  return next ? url.replace(/_ll_h15_(ab|ac|aa)\.png\?w=360&h=270$/i, `_ll_h15_${next}.png?w=360&h=270`) : null;
}

export function applyLogoFallback(img: HTMLImageElement): void {
  const next = nextLogoVariant(img.src);
  if (next && next !== img.src) {
    img.src = next;
    return;
  }
  img.style.display = 'none';
}

export function buildChannelLogoMap(channels: Channel[]): Record<string, string> {
  const map: Record<string, string> = {};
  for (const ch of channels) {
    const logo = channelLogoUrl(ch);
    if (!logo) continue;
    const keys = [ch.id, ch.name, ch.number]
      .filter((k): k is string => Boolean(k && k.trim()))
      .map((k) => k.trim());
    for (const key of keys) {
      map[key] = logo;
      map[key.toLowerCase()] = logo;
    }
  }
  return map;
}

export function logoForChannelKey(key: string | null | undefined, logoMap: Record<string, string>): string | null {
  const clean = (key || '').trim();
  if (!clean) return null;
  return logoMap[clean] ?? logoMap[clean.toLowerCase()] ?? null;
}

export function buildGuideLogoMap(guide: Record<string, unknown>): Record<string, string> {
  const map: Record<string, string> = {};
  const server = getServerUrl();
  for (const raw of Object.values(guide)) {
    const ch = raw as { Image?: string; Name?: string; ID?: string; ChannelID?: string; Number?: string };
    if (!ch?.Image) continue;
    const url = ch.Image.startsWith(TMS_CDN_PREFIX)
      ? `${server}${TMS_LOCAL_PATH}${ch.Image.slice(TMS_CDN_PREFIX.length)}`
      : ch.Image;
    const keys = [ch.ChannelID, ch.ID, ch.Name, ch.Number]
      .filter((k): k is string => Boolean(k && k.trim()))
      .map((k) => k.trim());
    for (const key of keys) {
      map[key] = url;
      map[key.toLowerCase()] = url;
    }
  }
  return map;
}
