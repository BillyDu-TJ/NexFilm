/**
 * NexFilm Client-side Release Hydrator & Real-time Auto-updater.
 * - Detects client OS synchronously on page-load (offline & rate-limit resilient).
 * - Hydrates hero CTA and download cards immediately from fallback assets without network latency.
 * - Caches GitHub API responses in sessionStorage (15 min TTL) to respect rate limits.
 * - Injects newer dynamic release cards onto Changelog timeline if published after static build.
 * - Updates GitHub Star count and latest version badges.
 */

interface CachedData {
  timestamp: number;
  latestRelease: any;
  repoInfo: { stargazers_count: number } | null;
  releasesList?: any[];
}

const CACHE_KEY = 'nexfilm_release_cache_v2';
const CACHE_TTL = 15 * 60 * 1000; // 15 minutes

const MAC_ARM_REGEX = /(aarch64|arm64)\.dmg$/i;
const MAC_INTEL_REGEX = /(x64|x86_64)\.dmg$/i;
const WIN_X64_REGEX = /(x64-setup\.exe|x64_en-US\.msi|nsis.*\.exe)$/i;

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0.00 MB';
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(2)} MB`;
}

function extractSha256(digest?: string): string {
  if (!digest) return '';
  if (digest.startsWith('sha256:')) return digest.slice(7);
  return digest;
}

function getStoredCache(): CachedData | null {
  try {
    const raw = sessionStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const data: CachedData = JSON.parse(raw);
    if (Date.now() - data.timestamp < CACHE_TTL) {
      return data;
    }
  } catch {
    // ignore parse or storage errors
  }
  return null;
}

function setStoredCache(data: Partial<CachedData>) {
  try {
    const existing = getStoredCache() || { timestamp: Date.now(), latestRelease: null, repoInfo: null };
    const merged: CachedData = {
      ...existing,
      ...data,
      timestamp: Date.now(),
    };
    sessionStorage.setItem(CACHE_KEY, JSON.stringify(merged));
  } catch {
    // ignore storage quota or private mode errors
  }
}

export function detectPlatform(): 'mac-arm' | 'mac-intel' | 'win-x64' | 'other' {
  if (typeof window === 'undefined') return 'mac-arm';

  const ua = navigator.userAgent.toLowerCase();
  const navPlatform = ((navigator as any).userAgentData?.platform || navigator.platform || '').toLowerCase();

  // 1. Windows detection
  if (navPlatform.includes('win') || ua.includes('windows')) {
    return 'win-x64';
  }

  // 2. Mac detection
  if (navPlatform.includes('mac') || ua.includes('macintosh') || ua.includes('mac os')) {
    try {
      const canvas = document.createElement('canvas');
      const gl = canvas.getContext('webgl') || canvas.getContext('experimental-webgl');
      if (gl) {
        const debugInfo = (gl as any).getExtension('WEBGL_debug_renderer_info');
        if (debugInfo) {
          const renderer = ((gl as any).getParameter(debugInfo.UNMASKED_RENDERER_WEBGL) || '').toLowerCase();
          if (renderer.includes('apple') || renderer.includes('apple m')) {
            return 'mac-arm';
          }
          if (renderer.includes('intel')) {
            return 'mac-intel';
          }
        }
      }
    } catch {
      // ignore webgl error
    }

    // Default recommendation on modern Mac is Apple Silicon
    return 'mac-arm';
  }

  return 'other';
}

function parseAssets(assets: any[]) {
  let macArm: any = null;
  let macIntel: any = null;
  let winX64: any = null;

  if (Array.isArray(assets)) {
    for (const a of assets) {
      const name = a.name || '';
      if (MAC_ARM_REGEX.test(name) && !macArm) macArm = a;
      else if (MAC_INTEL_REGEX.test(name) && !macIntel) macIntel = a;
      else if (WIN_X64_REGEX.test(name) && !winX64) winX64 = a;
    }
  }

  return { macArm, macIntel, winX64 };
}

function hydrateStars(count: number) {
  const elements = document.querySelectorAll<HTMLElement>('[data-github-stars]');
  const formatted = count >= 1000 ? `${(count / 1000).toFixed(1)}k` : `${count}`;
  elements.forEach((el) => {
    el.textContent = formatted;
  });
}

function hydrateHeaderVersion(release: any) {
  const tag = release.tag_name;
  if (!tag) return;

  const badgeEls = document.querySelectorAll<HTMLElement>('[data-header-version]');
  badgeEls.forEach((el) => {
    el.textContent = tag;
  });

  const linkEls = document.querySelectorAll<HTMLAnchorElement>('[data-header-version-link]');
  linkEls.forEach((link) => {
    link.href = release.html_url || `https://github.com/BillyDu-TJ/NexFilm/releases/tag/${tag}`;
  });
}

function updateHeroDownloadButton(targetAsset: any, version: string, platformKey: string) {
  const heroBtn = document.querySelector<HTMLAnchorElement>('[data-hero-download]');
  if (!heroBtn || !targetAsset) return;

  const isZh = document.documentElement.lang !== 'en';
  const downloadUrl = targetAsset.downloadUrl || targetAsset.browser_download_url;
  const fileName = targetAsset.name;

  if (downloadUrl) heroBtn.href = downloadUrl;
  if (fileName) heroBtn.setAttribute('download', fileName);

  let textLabel = '';
  if (platformKey === 'win-x64') {
    textLabel = isZh ? `直接下载 Windows 版 (${version})` : `Download for Windows (${version})`;
  } else if (platformKey === 'mac-intel') {
    textLabel = isZh ? `直接下载 macOS Intel 版 (${version})` : `Download for macOS (Intel) (${version})`;
  } else if (platformKey === 'mac-arm') {
    textLabel = isZh ? `直接下载 macOS 版 (${version})` : `Download for macOS (${version})`;
  } else {
    textLabel = isZh ? `直接下载安装包 (${version})` : `Download Installer (${version})`;
  }

  const textEl = heroBtn.querySelector<HTMLElement>('[data-hero-download-text]');
  if (textEl) {
    textEl.textContent = textLabel;
  } else {
    // If specific child text node is missing, set text preserving inner SVG icon
    const existingSvg = heroBtn.querySelector('svg');
    if (existingSvg) {
      heroBtn.innerHTML = '';
      heroBtn.appendChild(existingSvg);
      const span = document.createElement('span');
      span.setAttribute('data-hero-download-text', '');
      span.textContent = textLabel;
      heroBtn.appendChild(span);
    }
  }

  const sizeEl = document.querySelector<HTMLElement>('[data-hero-size]');
  if (sizeEl) {
    const sizeStr = targetAsset.sizeFormatted || formatBytes(targetAsset.size);
    if (sizeStr) sizeEl.textContent = sizeStr;
  }

  const versionEl = document.querySelector<HTMLElement>('[data-hero-version]');
  if (versionEl) {
    versionEl.textContent = version;
  }
}

function highlightPlatformCard(platformKey: string) {
  // Highlight target card and show recommended badge
  const cards = ['card-mac-arm', 'card-mac-intel', 'card-win-x64'];
  const activeId = `card-${platformKey}`;

  cards.forEach((id) => {
    const card = document.getElementById(id);
    if (!card) return;
    const badge = card.querySelector<HTMLElement>('.recommended-badge');
    if (id === activeId) {
      card.classList.add('border-accent', 'ring-1', 'ring-accent/40');
      if (badge) badge.classList.remove('hidden');
    } else {
      card.classList.remove('border-accent', 'ring-1', 'ring-accent/40');
      if (badge) badge.classList.add('hidden');
    }
  });
}

function hydrateSyncLocalState() {
  const detected = detectPlatform();

  // 1. Immediately hydrate Hero Button using static fallback embedded data
  const heroBtn = document.querySelector<HTMLAnchorElement>('[data-hero-download]');
  if (heroBtn && heroBtn.dataset.fallbackAssets) {
    try {
      const fallback = JSON.parse(heroBtn.dataset.fallbackAssets);
      const version = fallback.version || 'v1.1.0';
      let targetAsset = fallback.macArm;
      if (detected === 'win-x64' && fallback.winX64) {
        targetAsset = fallback.winX64;
      } else if (detected === 'mac-intel' && fallback.macIntel) {
        targetAsset = fallback.macIntel;
      } else if (fallback.macArm) {
        targetAsset = fallback.macArm;
      }
      updateHeroDownloadButton(targetAsset, version, detected);
    } catch {
      // ignore JSON parse error
    }
  }

  // 2. Immediately highlight recommended download card
  highlightPlatformCard(detected);

  // 3. Setup copy SHA buttons with language check
  setupCopyButtons();
}

function hydrateDownloadCards(release: any) {
  const { macArm, macIntel, winX64 } = parseAssets(release.assets || []);
  const tag = release.tag_name;
  const tagUrl = release.html_url || `https://github.com/BillyDu-TJ/NexFilm/releases/tag/${tag}`;

  const platformMap: Record<string, any> = {
    'card-win-x64': winX64,
    'card-mac-arm': macArm,
    'card-mac-intel': macIntel,
  };

  for (const [cardId, asset] of Object.entries(platformMap)) {
    const card = document.getElementById(cardId);
    if (!card) continue;

    if (asset) {
      const downloadBtn = card.querySelector<HTMLAnchorElement>('[data-asset-download]');
      if (downloadBtn) {
        downloadBtn.href = asset.browser_download_url;
        downloadBtn.setAttribute('download', asset.name);
      }

      const nameEl = card.querySelector<HTMLElement>('[data-asset-name]');
      if (nameEl) nameEl.textContent = asset.name;

      const sizeEl = card.querySelector<HTMLElement>('[data-asset-size]');
      if (sizeEl) sizeEl.textContent = formatBytes(asset.size);

      const shaEl = card.querySelector<HTMLElement>('[data-asset-sha]');
      const copyBtn = card.querySelector<HTMLButtonElement>('[data-copy-sha256]');
      const cleanSha = extractSha256(asset.digest);
      if (cleanSha) {
        if (shaEl) shaEl.textContent = `${cleanSha.slice(0, 10)}...`;
        if (copyBtn) {
          copyBtn.dataset.sha256 = cleanSha;
          copyBtn.style.display = 'inline-flex';
        }
      }
    }

    const versionTag = card.querySelector<HTMLElement>('[data-card-version]');
    if (versionTag) versionTag.textContent = tag;

    const releaseLink = card.querySelector<HTMLAnchorElement>('[data-card-release-link]');
    if (releaseLink) releaseLink.href = tagUrl;
  }

  const detected = detectPlatform();
  highlightPlatformCard(detected);
}

function hydrateHeroDownload(release: any) {
  const heroBtn = document.querySelector<HTMLAnchorElement>('[data-hero-download]');
  if (!heroBtn) return;

  const { macArm, macIntel, winX64 } = parseAssets(release.assets || []);
  const detected = detectPlatform();

  let targetAsset = macArm;
  if (detected === 'win-x64' && winX64) {
    targetAsset = winX64;
  } else if (detected === 'mac-intel' && macIntel) {
    targetAsset = macIntel;
  } else if (macArm) {
    targetAsset = macArm;
  }

  if (targetAsset) {
    updateHeroDownloadButton(targetAsset, release.tag_name, detected);
  }
}

function setupCopyButtons() {
  const isEn = document.documentElement.lang === 'en';
  const copiedText = isEn ? 'Copied!' : '已复制';

  document.querySelectorAll<HTMLButtonElement>('[data-copy-sha256]').forEach((btn) => {
    if (btn.dataset.hasListener === 'true') return;
    btn.dataset.hasListener = 'true';

    btn.addEventListener('click', async (e) => {
      e.preventDefault();
      const hash = btn.dataset.sha256;
      if (!hash) return;

      try {
        await navigator.clipboard.writeText(hash);
        const originalHtml = btn.innerHTML;
        btn.innerHTML = `<svg class="w-3.5 h-3.5 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M5 13l4 4L19 7"/></svg><span class="text-accent font-sans">${copiedText}</span>`;
        setTimeout(() => {
          btn.innerHTML = originalHtml;
        }, 2000);
      } catch {
        // clipboard permission denied
      }
    });
  });
}

function formatInline(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/\*([^*]+)\*/g, '<em>$1</em>');
}

function cleanAndRenderMarkdown(markdown: string): string {
  if (!markdown) return '';
  // Strip duplicate download instructions section
  const stripped = markdown.split(/###\s*📥?\s*下载指南/i)[0].trim();

  const lines = stripped.split('\n');
  const htmlParts: string[] = [];
  let inList = false;

  for (let rawLine of lines) {
    const line = rawLine.trim();
    if (!line) {
      if (inList) {
        htmlParts.push('</ul>');
        inList = false;
      }
      continue;
    }

    if (line.startsWith('### ')) {
      if (inList) {
        htmlParts.push('</ul>');
        inList = false;
      }
      const title = line.replace(/^###\s+/, '');
      htmlParts.push(`<h3>${formatInline(title)}</h3>`);
    } else if (line.startsWith('## ')) {
      if (inList) {
        htmlParts.push('</ul>');
        inList = false;
      }
      const title = line.replace(/^##\s+/, '');
      htmlParts.push(`<h3>${formatInline(title)}</h3>`);
    } else if (line.startsWith('- ') || line.startsWith('* ')) {
      if (!inList) {
        htmlParts.push('<ul>');
        inList = true;
      }
      const item = line.replace(/^[-*]\s+/, '');
      htmlParts.push(`<li>${formatInline(item)}</li>`);
    } else {
      if (inList) {
        htmlParts.push('</ul>');
        inList = false;
      }
      htmlParts.push(`<p>${formatInline(line)}</p>`);
    }
  }

  if (inList) {
    htmlParts.push('</ul>');
  }

  return htmlParts.join('\n');
}

function hydrateChangelogTimeline(releasesList: any[]) {
  const timeline = document.querySelector<HTMLElement>('[data-changelog-timeline]');
  if (!timeline || !Array.isArray(releasesList) || releasesList.length === 0) return;

  const isEn = document.documentElement.lang === 'en';
  const existingTags = new Set(
    Array.from(timeline.querySelectorAll<HTMLElement>('[data-release-tag]'))
      .map((el) => el.getAttribute('data-release-tag'))
      .filter(Boolean)
  );

  const validReleases = releasesList.filter((r) => !r.draft && r.tag_name);
  const newReleases = validReleases.filter((r) => !existingTags.has(r.tag_name));
  if (newReleases.length === 0) return;

  // Demote previous first card
  const prevFirstCard = timeline.querySelector<HTMLElement>('[data-release-tag]');
  if (prevFirstCard) {
    const pin = prevFirstCard.querySelector<HTMLElement>('[data-timeline-pin]');
    if (pin) {
      pin.classList.remove('bg-accent', 'shadow-sm');
      pin.classList.add('bg-darkroom-line');
    }
    const badge = prevFirstCard.querySelector<HTMLElement>('[data-release-badge]');
    if (badge) {
      badge.textContent = isEn ? 'Release' : '历史发布';
      badge.className = 'text-xs px-2.5 py-0.5 rounded-full font-medium font-mono bg-darkroom-canvas text-text-muted border border-darkroom-line';
    }
  }

  // Prepend new releases (sorted newest first)
  for (const rel of newReleases) {
    const tagName = rel.tag_name;
    const htmlUrl = rel.html_url || `https://github.com/BillyDu-TJ/NexFilm/releases/tag/${tagName}`;
    const pubDate = (rel.published_at || '').split('T')[0] || '';
    const bodyHtml = cleanAndRenderMarkdown(rel.body || '');

    const cardWrapper = document.createElement('div');
    cardWrapper.className = 'relative pl-10 space-y-4';
    cardWrapper.setAttribute('data-release-tag', tagName);

    let assetsHtml = '';
    const assets = Array.isArray(rel.assets) ? rel.assets : [];
    if (assets.length > 0) {
      const assetItems = assets
        .map((a: any) => {
          const sha = extractSha256(a.digest);
          return `
            <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-1.5 p-2 rounded bg-darkroom-canvas/80 border border-darkroom-line">
              <div class="flex items-center gap-2 truncate">
                <svg class="w-3.5 h-3.5 text-text-muted flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
                </svg>
                <a href="${a.browser_download_url}" download="${a.name}" class="text-accent hover:underline truncate font-medium" title="Direct download this asset">
                  ${a.name}
                </a>
                <span class="text-text-muted text-[10px]">(${formatBytes(a.size)})</span>
              </div>
              ${
                sha
                  ? `
                <div class="flex items-center gap-2 text-text-muted">
                  <span class="truncate">SHA: ${sha.slice(0, 10)}...</span>
                  <button type="button" data-copy-sha256 data-sha256="${sha}" class="text-accent hover:text-accent-hover text-[10px] font-sans transition-colors inline-flex items-center gap-1">
                    ${isEn ? 'Copy' : '复制'}
                  </button>
                </div>
              `
                  : ''
              }
            </div>
          `;
        })
        .join('');

      assetsHtml = `
        <details class="pt-4 border-t border-darkroom-line group" open>
          <summary class="cursor-pointer text-xs font-mono text-text-muted hover:text-accent flex items-center gap-2 select-none">
            <svg class="w-3.5 h-3.5 transition-transform group-open:rotate-90 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
            </svg>
            <span>${isEn ? `View release packages & SHA-256 hashes (${assets.length} assets)` : `查看该版本安装包与 SHA-256 校验和 (${assets.length} 个产物)`}</span>
          </summary>
          <div class="mt-3 space-y-2 pl-5 font-mono text-[11px]">
            ${assetItems}
          </div>
        </details>
      `;
    }

    cardWrapper.innerHTML = `
      <div data-timeline-pin class="absolute left-1.5 top-2 w-4 h-4 rounded-full border-2 border-darkroom-bg flex items-center justify-center bg-accent shadow-sm"></div>
      <div class="p-6 sm:p-7 rounded-xl border border-darkroom-line bg-darkroom-panel shadow-darkroom-card space-y-5">
        <div class="flex flex-wrap items-center justify-between gap-3 border-b border-darkroom-line pb-4">
          <div class="flex items-center gap-3">
            <a href="${htmlUrl}" target="_blank" rel="noopener noreferrer" class="text-2xl font-bold text-text-primary font-mono hover:text-accent transition-colors flex items-center gap-1.5">
              <span>${tagName}</span>
              <svg class="w-4 h-4 opacity-50 hover:opacity-100" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
              </svg>
            </a>
            <span data-release-badge class="text-xs px-2.5 py-0.5 rounded-full font-medium font-mono bg-accent-soft text-accent border border-accent/40">
              ${isEn ? 'Current Stable' : '最新稳定版'}
            </span>
          </div>
          <span class="text-xs text-text-muted font-mono">${pubDate}</span>
        </div>
        <div class="release-markdown space-y-3">
          ${bodyHtml}
        </div>
        ${assetsHtml}
      </div>
    `;

    timeline.insertBefore(cardWrapper, timeline.firstChild);
    existingTags.add(tagName);
  }

  setupCopyButtons();
}

async function fetchLatestData(isChangelog: boolean) {
  const cached = getStoredCache();
  if (cached && cached.latestRelease && (!isChangelog || cached.releasesList)) {
    return cached;
  }

  const headers = { Accept: 'application/vnd.github.v3+json' };
  const result: Partial<CachedData> = {
    latestRelease: cached?.latestRelease || null,
    repoInfo: cached?.repoInfo || null,
    releasesList: cached?.releasesList || undefined,
  };

  try {
    const [latestRes, repoRes] = await Promise.allSettled([
      fetch('https://api.github.com/repos/BillyDu-TJ/NexFilm/releases/latest', { headers }),
      fetch('https://api.github.com/repos/BillyDu-TJ/NexFilm', { headers }),
    ]);

    if (latestRes.status === 'fulfilled' && latestRes.value.ok) {
      result.latestRelease = await latestRes.value.json();
    }

    if (repoRes.status === 'fulfilled' && repoRes.value.ok) {
      const repo = await repoRes.value.json();
      result.repoInfo = { stargazers_count: repo.stargazers_count || 110 };
    }

    if (isChangelog) {
      const listRes = await fetch('https://api.github.com/repos/BillyDu-TJ/NexFilm/releases?per_page=15', { headers });
      if (listRes.ok) {
        result.releasesList = await listRes.json();
      }
    }

    setStoredCache(result);
  } catch {
    // silent failure: keep static HTML / fallback data
  }

  return getStoredCache() || result;
}

export async function initReleaseHydrator() {
  const isChangelog = !!document.querySelector('[data-changelog-timeline]');

  // Step 1: Synchronous zero-network local hydration (immediate, offline/403 safe)
  hydrateSyncLocalState();

  // Step 2: Immediate fast hydration with stored cache if available
  const fastCache = getStoredCache();
  if (fastCache) {
    if (fastCache.repoInfo) hydrateStars(fastCache.repoInfo.stargazers_count);
    if (fastCache.latestRelease) {
      hydrateHeaderVersion(fastCache.latestRelease);
      hydrateHeroDownload(fastCache.latestRelease);
      hydrateDownloadCards(fastCache.latestRelease);
    }
    if (isChangelog && fastCache.releasesList) {
      hydrateChangelogTimeline(fastCache.releasesList);
    }
  }

  // Step 3: Background fetch for newer updates
  const data = await fetchLatestData(isChangelog);
  if (data?.repoInfo) {
    hydrateStars(data.repoInfo.stargazers_count);
  }
  if (data?.latestRelease) {
    hydrateHeaderVersion(data.latestRelease);
    hydrateHeroDownload(data.latestRelease);
    hydrateDownloadCards(data.latestRelease);
  }
  if (isChangelog && data?.releasesList) {
    hydrateChangelogTimeline(data.releasesList);
  }
}

// Driven exclusively via Astro View Transitions astro:page-load
if (typeof document !== 'undefined') {
  document.addEventListener('astro:page-load', () => initReleaseHydrator());
}
