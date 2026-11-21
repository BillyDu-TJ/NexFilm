#!/usr/bin/env node

/**
 * Copy one GitHub Release's installer assets to the public R2 mirror.
 *
 * GitHub stays the canonical source. This script only transports the exact
 * assets published on GitHub, verifies their SHA256 digest when GitHub provides
 * one, and writes a versioned latest.json manifest for the website.
 *
 * Usage:
 *   node scripts/sync-release-to-r2.mjs --tag v1.2.1
 *   node scripts/sync-release-to-r2.mjs --tag v1.2.1 --dry-run
 */

import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream } from 'node:fs';
import {
  mkdtemp,
  readFile,
  rm,
  stat,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { spawnSync } from 'node:child_process';

const REPO = 'BillyDu-TJ/NexFilm';
const DEFAULT_PUBLIC_BASE_URL = 'https://dl.nexfilm.tech';
const REQUIRED_ASSET_PATTERNS = [
  /aarch64\.dmg$/i,
  /x64-setup\.exe$/i,
  /x64\.dmg$/i,
];

function readArg(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function requireValue(name, value) {
  if (!value) {
    throw new Error(`Missing ${name}.`);
  }
  return value;
}

function contentTypeFor(name) {
  if (name.endsWith('.json')) return 'application/json';
  if (name.endsWith('.sig')) return 'text/plain';
  return 'application/octet-stream';
}

function cacheControlFor(name) {
  return name.endsWith('.json')
    ? 'no-cache, max-age=0, must-revalidate'
    : 'public, max-age=31536000, immutable';
}

function platformFor(name) {
  if (/aarch64\.dmg$/i.test(name)) return 'macos-arm64';
  if (/x64\.dmg$/i.test(name)) return 'macos-x64';
  if (/x64-setup\.exe$/i.test(name)) return 'windows-x64';
  return 'other';
}

function mirrorUrl(baseUrl, tag, name) {
  return `${baseUrl}/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;
}

async function downloadAsset(url, target) {
  const response = await fetch(url, {
    headers: {
      Accept: 'application/octet-stream',
      'User-Agent': 'NexFilm-Release-Mirror',
    },
  });
  if (!response.ok || !response.body) {
    throw new Error(`Cannot download ${url}: HTTP ${response.status}`);
  }
  await pipeline(
    Readable.fromWeb(response.body),
    createWriteStream(target, { flags: 'wx' }),
  );
}

async function sha256File(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) {
    hash.update(chunk);
  }
  return hash.digest('hex');
}

function expectedSha256(asset) {
  return typeof asset.digest === 'string' && asset.digest.startsWith('sha256:')
    ? asset.digest.slice('sha256:'.length)
    : '';
}

function uploadToR2({ bucket, accountId, file, key, name, dryRun }) {
  const args = [
    's3',
    'cp',
    file,
    `s3://${bucket}/${key}`,
    '--endpoint-url',
    `https://${accountId}.r2.cloudflarestorage.com`,
    '--content-type',
    contentTypeFor(name),
    '--cache-control',
    cacheControlFor(name),
    '--no-progress',
  ];
  if (dryRun) {
    console.log(`[dry-run] aws ${args.join(' ')}`);
    return;
  }
  const result = spawnSync('aws', args, {
    stdio: 'inherit',
    env: {
      ...process.env,
      AWS_ACCESS_KEY_ID: process.env.R2_ACCESS_KEY_ID,
      AWS_SECRET_ACCESS_KEY: process.env.R2_SECRET_ACCESS_KEY,
      AWS_DEFAULT_REGION: 'auto',
    },
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`aws s3 cp failed for ${key} with exit code ${result.status}.`);
  }
}

async function main() {
  const tag = requireValue('--tag', readArg('--tag'));
  const dryRun = process.argv.includes('--dry-run');
  const publicBaseUrl = (
    process.env.R2_PUBLIC_BASE_URL || DEFAULT_PUBLIC_BASE_URL
  ).replace(/\/+$/, '');
  const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;

  let bucket = '';
  let accountId = '';
  if (!dryRun) {
    bucket = requireValue('R2_BUCKET', process.env.R2_BUCKET);
    accountId = requireValue('R2_ACCOUNT_ID', process.env.R2_ACCOUNT_ID);
    requireValue('R2_ACCESS_KEY_ID', process.env.R2_ACCESS_KEY_ID);
    requireValue('R2_SECRET_ACCESS_KEY', process.env.R2_SECRET_ACCESS_KEY);
  }

  const headers = {
    Accept: 'application/vnd.github+json',
    'User-Agent': 'NexFilm-Release-Mirror',
  };
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }
  const releaseResponse = await fetch(
    `https://api.github.com/repos/${REPO}/releases/tags/${encodeURIComponent(tag)}`,
    { headers },
  );
  if (!releaseResponse.ok) {
    throw new Error(
      `Cannot read GitHub release ${tag}: HTTP ${releaseResponse.status}`,
    );
  }
  const release = await releaseResponse.json();
  const allAssets = Array.isArray(release.assets) ? release.assets : [];
  const assets = allAssets.filter((asset) =>
    REQUIRED_ASSET_PATTERNS.some((pattern) => pattern.test(asset.name)),
  );
  const missing = REQUIRED_ASSET_PATTERNS.filter(
    (pattern) => !assets.some((asset) => pattern.test(asset.name)),
  );
  if (missing.length > 0) {
    throw new Error(
      `Release ${tag} is missing required installer assets: ${missing
        .map(String)
        .join(', ')}`,
    );
  }

  const workDir = await mkdtemp(join(tmpdir(), `nexfilm-mirror-${tag}-`));
  const manifestAssets = [];
  console.log(
    `[mirror] ${dryRun ? 'Validating' : 'Copying'} ${assets.length} assets from ${tag}`,
  );

  try {
    for (const asset of assets) {
      const name = basename(asset.name);
      const target = join(workDir, name);
      console.log(`[mirror] Downloading ${name}`);
      await downloadAsset(asset.browser_download_url, target);
      const size = (await stat(target)).size;
      const sha256 = await sha256File(target);
      const expected = expectedSha256(asset);
      if (expected && expected.toLowerCase() !== sha256) {
        throw new Error(
          `SHA256 mismatch for ${name}: GitHub says ${expected}, downloaded ${sha256}.`,
        );
      }
      const mirror = mirrorUrl(publicBaseUrl, tag, name);
      manifestAssets.push({
        name,
        platform: platformFor(name),
        size,
        sha256,
        url: mirror,
        githubUrl: asset.browser_download_url,
      });
      uploadToR2({
        bucket,
        accountId,
        file: target,
        key: `${tag}/${name}`,
        name,
        dryRun,
      });
      console.log(
        `[mirror] ${dryRun ? 'Verified' : 'Uploaded'} ${name} (${size} bytes, sha256 ${sha256.slice(0, 12)}...)`,
      );
    }

    const manifest = {
      schemaVersion: 1,
      tag,
      publishedAt: release.published_at,
      generatedAt: new Date().toISOString(),
      source: release.html_url,
      mirrorBaseUrl: publicBaseUrl,
      assets: manifestAssets,
    };
    const manifestFile = join(workDir, 'latest.json');
    await writeFile(manifestFile, `${JSON.stringify(manifest, null, 2)}\n`);
    uploadToR2({
      bucket,
      accountId,
      file: manifestFile,
      key: 'latest.json',
      name: 'latest.json',
      dryRun,
    });
    uploadToR2({
      bucket,
      accountId,
      file: manifestFile,
      key: `${tag}/latest.json`,
      name: 'latest.json',
      dryRun,
    });

    const manifestOut = readArg('--manifest-out');
    if (manifestOut) {
      const outputPath = resolve(manifestOut);
      await writeFile(outputPath, `${JSON.stringify(manifest, null, 2)}\n`);
      console.log(`[mirror] Wrote manifest to ${outputPath}`);
    }
    console.log(
      `[mirror] ${dryRun ? 'Dry run complete' : 'Mirror complete'}: ${publicBaseUrl}/${tag}/`,
    );
  } finally {
    await rm(workDir, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(`[mirror] ${error.stack || error.message || error}`);
  process.exit(1);
});
