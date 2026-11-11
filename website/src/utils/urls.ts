/**
 * URL and routing utility functions for NexFilm website.
 * Supports arbitrary PUBLIC_BASE_PATH configurations.
 */

const rawBase = import.meta.env.BASE_URL || '/';
export const BASE_PATH = rawBase.endsWith('/') ? rawBase : `${rawBase}/`;

/**
 * Prefix an absolute path with the configured base path.
 * e.g. withBase('/zh/download/') -> '/zh/download/' or '/subpath/zh/download/'
 */
export function withBase(path: string): string {
  if (!path) return BASE_PATH;
  if (/^https?:\/\//i.test(path) || path.startsWith('//') || path.startsWith('#') || path.startsWith('mailto:')) {
    return path;
  }
  const cleanPath = path.startsWith('/') ? path.slice(1) : path;
  return `${BASE_PATH}${cleanPath}`;
}

/**
 * Safely switch language in the current URL path between /zh/ and /en/
 */
export function switchLocalePath(currentPath: string, targetLang: 'zh' | 'en'): string {
  // Strip BASE_PATH if present
  let relative = currentPath;
  if (BASE_PATH !== '/' && relative.startsWith(BASE_PATH)) {
    relative = '/' + relative.slice(BASE_PATH.length);
  }

  // Matches /zh/... or /en/...
  const langRegex = /^\/(zh|en)(\/.*)?$/;
  const match = relative.match(langRegex);

  if (match) {
    const subpath = match[2] || '/';
    return withBase(`/${targetLang}${subpath}`);
  }

  return withBase(`/${targetLang}/`);
}

/**
 * Check whether a target link matches the currently active route
 */
export function isRouteActive(currentPath: string, targetHref: string): boolean {
  const normCurrent = currentPath.endsWith('/') ? currentPath : `${currentPath}/`;
  const normTarget = targetHref.endsWith('/') ? targetHref : `${targetHref}/`;

  if (normCurrent === normTarget) return true;

  // Root language pages (/zh/ or /en/) should only match exactly
  if (normTarget.endsWith('/zh/') || normTarget.endsWith('/en/')) {
    return normCurrent === normTarget;
  }

  // Subpaths (e.g. /zh/docs/) match if current is inside it
  return normCurrent.startsWith(normTarget);
}
