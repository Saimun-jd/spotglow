/**
 * SpotGlow - LRCLIB Real-time Synchronized Lyrics Client & LRC Parser
 */

export interface LyricLine {
  id: number;
  timeMs: number;
  text: string;
}

export interface LrcLyrics {
  id: number;
  trackName: string;
  artistName: string;
  albumName?: string;
  duration: number;
  instrumental: boolean;
  synced: boolean;
  lines: LyricLine[];
  plainLyrics?: string;
}

interface LrclibRawResponse {
  id: number;
  name?: string;
  trackName: string;
  artistName: string;
  albumName?: string;
  duration: number;
  instrumental: boolean;
  plainLyrics?: string;
  syncedLyrics?: string;
}

// In-memory cache to prevent duplicate network calls
const lyricsCache = new Map<string, LrcLyrics | null>();

/**
 * Sanitizes song titles and artist names by stripping metadata noise like
 * "(feat. XYZ)", "[Remastered]", "- 2011 Remaster", etc.
 */
export function sanitizeSongQuery(str: string): string {
  if (!str) return "";
  return str
    .replace(/\s*\(feat\.[^)]*\)/gi, "")
    .replace(/\s*\[feat\.[^\]]*\]/gi, "")
    .replace(/\s*\(with[^)]*\)/gi, "")
    .replace(/\s*\(remaster(?:ed)?[^)]*\)/gi, "")
    .replace(/\s*\[remaster(?:ed)?[^\]]*\]/gi, "")
    .replace(/\s*-\s*\d{4}\s*remaster(?:ed)?/gi, "")
    .replace(/\s*-\s*remaster(?:ed)?/gi, "")
    .replace(/\s*\(deluxe(?: edition)?\)/gi, "")
    .replace(/\s*\[deluxe(?: edition)?\]/gi, "")
    .replace(/\s*\(bonus track\)/gi, "")
    .replace(/\s*-\s*radio edit/gi, "")
    .replace(/\s*-\s*single/gi, "")
    .replace(/\s*-\s*live/gi, "")
    .replace(/\s*\(live[^)]*\)/gi, "")
    .replace(/\s*\[live[^\]]*\]/gi, "")
    .replace(/["']/g, "")
    .replace(/\s{2,}/g, " ")
    .trim();
}

/**
 * Extracts candidate titles in prioritized order to maximize LRCLIB hit rate:
 * 1. Original title
 * 2. Cleaned title
 * 3. Base title before dash (e.g. 'Epitaph - Including...' -> 'Epitaph')
 * 4. Base title before parenthesis / quotes
 */
export function extractCandidateTitles(rawTitle: string): string[] {
  const candidates: string[] = [];
  const add = (t: string) => {
    const cleaned = t.trim();
    if (cleaned && !candidates.includes(cleaned)) {
      candidates.push(cleaned);
    }
  };

  add(rawTitle);

  const sanitized = sanitizeSongQuery(rawTitle);
  add(sanitized);

  // Strip anything following a dash: "Song Name - Subtitle/Remaster/Feature"
  const beforeDash = rawTitle.split(/\s*-\s*/)[0];
  if (beforeDash) {
    add(beforeDash);
    add(sanitizeSongQuery(beforeDash));
  }

  // Strip anything following parenthesis, bracket, colon, or quote
  const beforeParen = rawTitle.replace(/[\(\[\"\'\:\/].*$/g, "");
  if (beforeParen) {
    add(beforeParen);
    add(sanitizeSongQuery(beforeParen));
  }

  // Combined: before dash, then before parenthesis
  if (beforeDash) {
    const basePure = beforeDash.replace(/[\(\[\"\'\:\/].*$/g, "");
    if (basePure) {
      add(basePure);
      add(sanitizeSongQuery(basePure));
    }
  }

  return candidates;
}

/**
 * Extracts candidate artists (e.g. strips secondary featured artists)
 */
export function extractCandidateArtists(rawArtist: string): string[] {
  const candidates: string[] = [];
  const add = (a: string) => {
    const cleaned = a.trim();
    if (cleaned && !candidates.includes(cleaned)) {
      candidates.push(cleaned);
    }
  };

  add(rawArtist);

  const primary = rawArtist.split(/[,&]|(?:\s+feat\.?\s+)|(?:\s+with\s+)/i)[0];
  if (primary) {
    add(primary);
    add(sanitizeSongQuery(primary));
  }

  return candidates;
}

/**
 * Parses standard LRC format strings ([mm:ss.xx] or [mm:ss.xxx]) into sorted LyricLine array
 */
export function parseLrc(lrcText: string): LyricLine[] {
  if (!lrcText) return [];

  const lines = lrcText.split(/\r?\n/);
  const result: LyricLine[] = [];
  const timeRegex = /\[(\d{2,}):(\d{2})(?:\.(\d{2,3}))?\]/g;
  let counter = 0;

  for (const rawLine of lines) {
    const trimmed = rawLine.trim();
    if (!trimmed) continue;

    timeRegex.lastIndex = 0;
    const matches: number[] = [];
    let match: RegExpExecArray | null;
    let lastMatchEnd = 0;

    while ((match = timeRegex.exec(trimmed)) !== null) {
      const mins = parseInt(match[1], 10);
      const secs = parseInt(match[2], 10);
      let ms = 0;
      if (match[3]) {
        if (match[3].length === 2) {
          ms = parseInt(match[3], 10) * 10;
        } else {
          ms = parseInt(match[3].slice(0, 3), 10);
        }
      }
      matches.push(mins * 60000 + secs * 1000 + ms);
      lastMatchEnd = timeRegex.lastIndex;
    }

    if (matches.length > 0) {
      const lyricText = trimmed.slice(lastMatchEnd).trim();
      for (const timeMs of matches) {
        result.push({
          id: counter++,
          timeMs,
          text: lyricText,
        });
      }
    }
  }

  result.sort((a, b) => a.timeMs - b.timeMs);
  return result;
}

/**
 * Performs fast binary search to find the active lyric line index for the current playback time.
 * Includes an anticipation lead time (default 350ms) so line transitions and scrolling
 * complete seamlessly right as the vocal hits the ear.
 */
export function findActiveLyricIndex(
  lines: LyricLine[],
  currentTimeMs: number,
  leadOffsetMs: number = 350
): number {
  if (!lines || lines.length === 0) return -1;
  const effectiveTimeMs = currentTimeMs + leadOffsetMs;
  if (effectiveTimeMs < lines[0].timeMs) return -1;

  let low = 0;
  let high = lines.length - 1;
  let found = -1;

  while (low <= high) {
    const mid = (low + high) >> 1;
    if (lines[mid].timeMs <= effectiveTimeMs) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }

  return found;
}

/**
 * Fetches synced lyrics from LRCLIB using aggressive candidate fallback logic.
 * Note: albumName is deliberately omitted from exact matches as Spotify album titles
 * frequently include release suffixes that break LRCLIB strict matching.
 */
export async function fetchLyrics(
  title: string,
  artist: string,
  _album?: string,
  durationSec?: number
): Promise<LrcLyrics | null> {
  if (!title || !artist) return null;

  const cacheKey = `${title.toLowerCase().trim()}::${artist.toLowerCase().trim()}`;
  if (lyricsCache.has(cacheKey)) {
    return lyricsCache.get(cacheKey) || null;
  }

  const titleCandidates = extractCandidateTitles(title);
  const artistCandidates = extractCandidateArtists(artist);

  try {
    let data: LrclibRawResponse | null = null;

    // 1. Direct GET endpoint attempts
    for (const t of titleCandidates) {
      for (const a of artistCandidates) {
        // Try with duration first
        data = await queryLrclibGet(t, a, durationSec);
        if (data?.syncedLyrics) break;

        // Try without duration in case of release length variations
        data = await queryLrclibGet(t, a, undefined);
        if (data?.syncedLyrics) break;
      }
      if (data?.syncedLyrics) break;
    }

    // 2. Search fallback attempts
    if (!data?.syncedLyrics) {
      for (const t of titleCandidates) {
        for (const a of artistCandidates) {
          data = await queryLrclibSearch(`${t} ${a}`, durationSec);
          if (data?.syncedLyrics) break;
        }
        if (data?.syncedLyrics) break;
      }
    }

    if (!data) {
      lyricsCache.set(cacheKey, null);
      return null;
    }

    let parsedLines: LyricLine[] = [];
    const isSynced = Boolean(data.syncedLyrics && data.syncedLyrics.trim().length > 0);

    if (isSynced && data.syncedLyrics) {
      parsedLines = parseLrc(data.syncedLyrics);
    } else if (data.plainLyrics) {
      parsedLines = data.plainLyrics
        .split(/\r?\n/)
        .map((t, idx) => ({ id: idx, timeMs: 0, text: t.trim() }))
        .filter((l) => l.text.length > 0);
    }

    const lyricsObj: LrcLyrics = {
      id: data.id,
      trackName: data.trackName || title,
      artistName: data.artistName || artist,
      albumName: data.albumName,
      duration: data.duration,
      instrumental: Boolean(data.instrumental),
      synced: isSynced && parsedLines.length > 0,
      lines: parsedLines,
      plainLyrics: data.plainLyrics,
    };

    lyricsCache.set(cacheKey, lyricsObj);
    return lyricsObj;
  } catch (err) {
    console.warn("[SpotGlow Lyrics] Error fetching lyrics from LRCLIB:", err);
    lyricsCache.set(cacheKey, null);
    return null;
  }
}

async function queryLrclibGet(
  title: string,
  artist: string,
  durationSec?: number
): Promise<LrclibRawResponse | null> {
  try {
    const params = new URLSearchParams({
      track_name: title,
      artist_name: artist,
    });
    if (durationSec && durationSec > 0) {
      params.append("duration", Math.round(durationSec).toString());
    }

    const resp = await fetch(`https://lrclib.net/api/get?${params.toString()}`);
    if (resp.status === 404) return null;
    if (!resp.ok) return null;
    return (await resp.json()) as LrclibRawResponse;
  } catch {
    return null;
  }
}

async function queryLrclibSearch(
  query: string,
  durationSec?: number
): Promise<LrclibRawResponse | null> {
  try {
    const resp = await fetch(`https://lrclib.net/api/search?q=${encodeURIComponent(query)}`);
    if (!resp.ok) return null;

    const list = (await resp.json()) as LrclibRawResponse[];
    if (!Array.isArray(list) || list.length === 0) return null;

    const withSynced = list.filter((item) => Boolean(item.syncedLyrics && item.syncedLyrics.trim().length > 0));
    const candidates = withSynced.length > 0 ? withSynced : list;

    if (durationSec && durationSec > 0) {
      candidates.sort((a, b) => Math.abs((a.duration || 0) - durationSec) - Math.abs((b.duration || 0) - durationSec));
    }

    return candidates[0] || null;
  } catch {
    return null;
  }
}
