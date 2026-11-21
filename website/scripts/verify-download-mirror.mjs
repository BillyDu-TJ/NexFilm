#!/usr/bin/env node

/**
 * Verify that the built download pages use the configured mirror and still
 * expose a direct GitHub asset fallback.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const distDir = path.resolve(__dirname, '..', 'dist');
const mirrorBase = (
  process.env.PUBLIC_DOWNLOAD_MIRROR_BASE ?? 'https://dl.nexfilm.tech'
).replace(/\/+$/, '');

if (!fs.existsSync(distDir)) {
  console.error('❌ website/dist not found. Run the website build first.');
  process.exit(1);
}

const pages = [
  path.join(distDir, 'zh', 'download', 'index.html'),
  path.join(distDir, 'en', 'download', 'index.html'),
];

const mirrorPattern = new RegExp(
  `${mirrorBase.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}/v[^/"]+/NexFilm\\.Engine_[^"]+\\.(?:dmg|exe)`,
);
const githubPattern =
  /https:\/\/github\.com\/BillyDu-TJ\/NexFilm\/releases\/download\/v[^/"]+\/NexFilm\.Engine_[^"]+\.(?:dmg|exe)/;
const primaryHrefPattern =
  /<a\b[^>]*\bdata-asset-download\b[^>]*>/g;
const hrefPattern = /\bhref="([^"]+)"/;

for (const page of pages) {
  if (!fs.existsSync(page)) {
    console.error(`❌ Missing download page: ${page}`);
    process.exit(1);
  }
  const html = fs.readFileSync(page, 'utf8');
  const primaryHrefs = [...html.matchAll(primaryHrefPattern)]
    .map((match) => match[0].match(hrefPattern)?.[1])
    .filter(Boolean);
  if (primaryHrefs.length < 3) {
    console.error(`❌ ${page}: expected three primary download buttons, found ${primaryHrefs.length}.`);
    process.exit(1);
  }
  if (mirrorBase) {
    for (const href of primaryHrefs) {
      if (!href.startsWith(`${mirrorBase}/`)) {
        console.error(`❌ ${page}: primary download button still points outside the mirror: ${href}`);
        process.exit(1);
      }
    }
  }
  if (mirrorBase) {
    if (!mirrorPattern.test(html)) {
      console.error(`❌ ${page}: no mirrored installer URL was found.`);
      process.exit(1);
    }
  } else if (html.includes('dl.nexfilm.tech')) {
    console.error(`❌ ${page}: mirror was disabled but a mirror URL is still present.`);
    process.exit(1);
  }
  if (!githubPattern.test(html)) {
    console.error(`❌ ${page}: no direct GitHub installer fallback was found.`);
    process.exit(1);
  }
  if (html.includes('ghproxy')) {
    console.error(`❌ ${page}: third-party ghproxy links are not allowed.`);
    process.exit(1);
  }
}

console.log(
  mirrorBase
    ? `✓ Download pages use ${mirrorBase} and keep GitHub asset fallbacks.`
    : '✓ Mirror is disabled; download pages use GitHub asset fallbacks only.',
);
