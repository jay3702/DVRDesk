export interface UpdateInfo {
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

export function isVersionNewer(latest: string, current: string): boolean {
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

// Deliberately the list endpoint, not /releases/latest — that endpoint is
// repo-wide and doesn't distinguish between this app's own plain vX.Y.Z
// release train and DVRDesk Native's separate native-vX.Y.Z one living in
// the same repo. Confirmed the hard way: publishing the first native-v
// release made /releases/latest start returning it instead of this app's
// own latest v* release, and the version parser silently choked on the
// unexpected tag shape rather than erroring — so this filters explicitly
// instead of trusting "latest" to mean what it sounds like it means.
export async function fetchLatestRelease(): Promise<UpdateInfo | null> {
  try {
    const response = await fetch('https://api.github.com/repos/jay3702/dvrdesk/releases?per_page=10', {
      headers: {
        Accept: 'application/vnd.github+json',
      },
    });
    if (!response.ok) return null;
    const payload = (await response.json()) as Array<{
      tag_name?: string;
      html_url?: string;
      draft?: boolean;
      prerelease?: boolean;
    }>;
    if (!Array.isArray(payload)) return null;
    const match = payload.find(
      (release) =>
        !release.draft &&
        !release.prerelease &&
        typeof release.tag_name === 'string' &&
        /^v\d+\.\d+\.\d+$/.test(release.tag_name),
    );
    if (!match) return null;
    const latestVersion = String(match.tag_name ?? '').trim();
    const latestUrl = String(match.html_url ?? '').trim();
    if (!latestVersion || !latestUrl) return null;
    return { latestVersion, latestUrl };
  } catch {
    return null;
  }
}
