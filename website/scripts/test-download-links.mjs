import test from 'node:test';
import assert from 'node:assert/strict';
import {
  DEFAULT_DOWNLOAD_MIRROR_BASE,
  buildMirrorDownloadUrl,
  githubAssetUrl,
  normalizeDownloadMirrorBase,
  preferredDownloadUrl,
} from '../src/utils/download-links.js';

test('mirror base normalization removes whitespace and trailing slashes', () => {
  assert.equal(
    normalizeDownloadMirrorBase('  https://dl.nexfilm.tech///  '),
    'https://dl.nexfilm.tech',
  );
  assert.equal(normalizeDownloadMirrorBase(''), '');
  assert.equal(normalizeDownloadMirrorBase(undefined), '');
});

test('mirror URLs keep the release tag and asset name as separate segments', () => {
  assert.equal(
    buildMirrorDownloadUrl(
      DEFAULT_DOWNLOAD_MIRROR_BASE,
      'v1.2.1',
      'NexFilm.Engine_1.2.1_x64-setup.exe',
    ),
    'https://dl.nexfilm.tech/v1.2.1/NexFilm.Engine_1.2.1_x64-setup.exe',
  );
  assert.equal(
    buildMirrorDownloadUrl('https://dl.nexfilm.tech/', '', 'asset.dmg'),
    '',
  );
});

test('asset names with spaces are encoded without changing the release path', () => {
  assert.equal(
    buildMirrorDownloadUrl('https://dl.nexfilm.tech', 'v1.2.1', 'NexFilm Engine.dmg'),
    'https://dl.nexfilm.tech/v1.2.1/NexFilm%20Engine.dmg',
  );
});

test('GitHub asset URLs remain available as an explicit fallback', () => {
  const githubUrl = githubAssetUrl('v1.2.1', 'NexFilm.Engine_1.2.1_x64.dmg');
  assert.equal(
    githubUrl,
    'https://github.com/BillyDu-TJ/NexFilm/releases/download/v1.2.1/NexFilm.Engine_1.2.1_x64.dmg',
  );
  assert.equal(
    preferredDownloadUrl('', 'v1.2.1', 'NexFilm.Engine_1.2.1_x64.dmg', githubUrl),
    githubUrl,
  );
  assert.equal(
    preferredDownloadUrl(
      DEFAULT_DOWNLOAD_MIRROR_BASE,
      'v1.2.1',
      'NexFilm.Engine_1.2.1_x64.dmg',
      githubUrl,
    ),
    'https://dl.nexfilm.tech/v1.2.1/NexFilm.Engine_1.2.1_x64.dmg',
  );
});
