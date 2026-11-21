/**
 * Download mirror URL helpers.
 *
 * The mirror only changes the transport used for release assets. GitHub
 * Releases remain the canonical source, and every generated mirror URL keeps
 * a direct GitHub fallback so the website stays usable if the mirror is not
 * provisioned yet.
 */

export const DEFAULT_DOWNLOAD_MIRROR_BASE = 'https://dl.nexfilm.tech';

/**
 * @param {string | null | undefined} value
 * @returns {string}
 */
export function normalizeDownloadMirrorBase(value) {
  if (typeof value !== 'string') return '';
  return value.trim().replace(/\/+$/, '');
}

/**
 * @param {string} base
 * @param {string} tagName
 * @param {string} fileName
 * @returns {string}
 */
export function buildMirrorDownloadUrl(base, tagName, fileName) {
  const normalizedBase = normalizeDownloadMirrorBase(base);
  if (!normalizedBase || !tagName || !fileName) return '';
  return `${normalizedBase}/${encodeURIComponent(tagName)}/${encodeURIComponent(fileName)}`;
}

/**
 * @param {string} tagName
 * @param {string} fileName
 * @returns {string}
 */
export function githubAssetUrl(tagName, fileName) {
  if (!tagName || !fileName) return '';
  return `https://github.com/BillyDu-TJ/NexFilm/releases/download/${encodeURIComponent(tagName)}/${encodeURIComponent(fileName)}`;
}

/**
 * @param {string} base
 * @param {string} tagName
 * @param {string} fileName
 * @param {string} [githubDownloadUrl]
 * @returns {string}
 */
export function preferredDownloadUrl(base, tagName, fileName, githubDownloadUrl = '') {
  return (
    buildMirrorDownloadUrl(base, tagName, fileName) ||
    githubDownloadUrl ||
    githubAssetUrl(tagName, fileName)
  );
}
