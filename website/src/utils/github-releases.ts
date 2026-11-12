import { marked } from 'marked';
import fallbackReleases from '../data/releases-fallback.json';
import { RELEASE_EN_NOTES } from '../data/release-en-notes';

export interface ReleaseAssetInfo {
  name: string;
  downloadUrl: string;
  sizeBytes: number;
  sizeFormatted: string;
  sha256: string;
}

export interface PlatformAssets {
  macArm?: ReleaseAssetInfo;
  macIntel?: ReleaseAssetInfo;
  winX64?: ReleaseAssetInfo;
}

export interface CleanRelease {
  id: number;
  tagName: string;
  name: string;
  htmlUrl: string;
  publishedAt: string;
  publishedDate: string;
  isLatest: boolean;
  bodyZhHtml: string;
  bodyEnHtml: string;
  isEnFallback: boolean;
  assets: PlatformAssets;
  allAssets: ReleaseAssetInfo[];
}

export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0.00 MB';
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(2)} MB`;
}

export function cleanReleaseBody(body: string): string {
  if (!body) return '';
  const markerRegex = /###\s*📥\s*(下载指南|Download)[\s\S]*$/i;
  return body.replace(markerRegex, '').trim();
}

function extractSha256(digest?: string): string {
  if (!digest) return '';
  if (digest.startsWith('sha256:')) {
    return digest.slice(7);
  }
  return digest;
}

const MAC_ARM_REGEX = /(aarch64|arm64)\.dmg$/i;
const MAC_INTEL_REGEX = /(x64|x86_64)\.dmg$/i;
const WIN_X64_REGEX = /(x64-setup\.exe|x64_en-US\.msi|nsis.*\.exe)$/i;

function parseRawAssets(rawAssets: any[]): { platformAssets: PlatformAssets; allAssets: ReleaseAssetInfo[] } {
  const platformAssets: PlatformAssets = {};
  const allAssets: ReleaseAssetInfo[] = [];

  if (!Array.isArray(rawAssets)) {
    return { platformAssets, allAssets };
  }

  for (const asset of rawAssets) {
    const assetName = asset.name || '';
    const info: ReleaseAssetInfo = {
      name: assetName,
      downloadUrl: asset.browser_download_url || '',
      sizeBytes: asset.size || 0,
      sizeFormatted: formatBytes(asset.size || 0),
      sha256: extractSha256(asset.digest),
    };

    allAssets.push(info);

    if (MAC_ARM_REGEX.test(assetName) && !platformAssets.macArm) {
      platformAssets.macArm = info;
    } else if (MAC_INTEL_REGEX.test(assetName) && !platformAssets.macIntel) {
      platformAssets.macIntel = info;
    } else if (WIN_X64_REGEX.test(assetName) && !platformAssets.winX64) {
      platformAssets.winX64 = info;
    }
  }

  return { platformAssets, allAssets };
}

function processRelease(raw: any, index: number): CleanRelease {
  const tagName = raw.tag_name || 'v1.1.0';
  const cleanedBodyZh = cleanReleaseBody(raw.body || '');
  const bodyZhHtml = marked.parse(cleanedBodyZh, { async: false }) as string;

  let bodyEnHtml = '';
  let isEnFallback = false;

  if (RELEASE_EN_NOTES[tagName]) {
    bodyEnHtml = marked.parse(RELEASE_EN_NOTES[tagName], { async: false }) as string;
  } else if (raw.body && raw.body.includes('<!-- en -->')) {
    const enPart = cleanReleaseBody(raw.body.split('<!-- en -->')[1]);
    bodyEnHtml = marked.parse(enPart, { async: false }) as string;
  } else {
    isEnFallback = true;
    bodyEnHtml = marked.parse(cleanedBodyZh, { async: false }) as string;
  }

  const { platformAssets, allAssets } = parseRawAssets(raw.assets || []);

  const pubDate = raw.published_at ? raw.published_at.slice(0, 10) : '';

  return {
    id: raw.id || 0,
    tagName,
    name: raw.name || `NexFilm ${tagName}`,
    htmlUrl: raw.html_url || `https://github.com/BillyDu-TJ/NexFilm/releases/tag/${tagName}`,
    publishedAt: raw.published_at || '',
    publishedDate: pubDate,
    isLatest: index === 0,
    bodyZhHtml,
    bodyEnHtml,
    isEnFallback,
    assets: platformAssets,
    allAssets,
  };
}

let cachedReleases: CleanRelease[] | null = null;

export async function getReleases(): Promise<CleanRelease[]> {
  if (cachedReleases) return cachedReleases;

  const url = 'https://api.github.com/repos/BillyDu-TJ/NexFilm/releases?per_page=20';
  const headers: Record<string, string> = {
    'Accept': 'application/vnd.github.v3+json',
    'User-Agent': 'NexFilm-Website-Builder',
  };

  if (process.env.GITHUB_TOKEN) {
    headers['Authorization'] = `Bearer ${process.env.GITHUB_TOKEN}`;
  }

  let rawList: any[] = [];
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 4000); // 4s timeout for fast build fallback
    const res = await fetch(url, { headers, signal: controller.signal });
    clearTimeout(timer);

    if (res.ok) {
      const data = await res.json();
      if (Array.isArray(data) && data.length > 0) {
        rawList = data;
      }
    } else {
      console.warn(`[GitHub Releases] API returned status ${res.status}. Falling back to offline snapshot.`);
    }
  } catch (err) {
    console.warn('[GitHub Releases] Network fetch failed or timed out. Falling back to offline snapshot.', err);
  }

  if (rawList.length === 0) {
    rawList = fallbackReleases;
  }

  // Filter out drafts and sort descending by published_at
  const valid = rawList
    .filter((r) => !r.draft)
    .sort((a, b) => new Date(b.published_at || 0).getTime() - new Date(a.published_at || 0).getTime());

  cachedReleases = valid.map((r, idx) => processRelease(r, idx));
  return cachedReleases;
}

export async function getLatestRelease(): Promise<CleanRelease> {
  const list = await getReleases();
  return list[0];
}
