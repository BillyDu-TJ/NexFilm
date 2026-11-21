import {
  DEFAULT_DOWNLOAD_MIRROR_BASE,
  normalizeDownloadMirrorBase,
} from './download-links.js';

/**
 * Public R2 custom domain used for release assets.
 *
 * Set `PUBLIC_DOWNLOAD_MIRROR_BASE` to override it. An explicitly empty value
 * disables the mirror and makes the website use GitHub asset URLs directly.
 */
export const DOWNLOAD_MIRROR_BASE = normalizeDownloadMirrorBase(
  import.meta.env.PUBLIC_DOWNLOAD_MIRROR_BASE ?? DEFAULT_DOWNLOAD_MIRROR_BASE,
);
