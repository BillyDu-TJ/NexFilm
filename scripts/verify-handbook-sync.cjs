#!/usr/bin/env node

/**
 * scripts/verify-handbook-sync.cjs
 *
 * Verifies 100% symmetry between docs/handbook/zh and docs/handbook/en:
 * - Exactly 18 canonical markdown files per language (4 + 7 + 4 + 3)
 * - Zero obsolete migration files
 * - Perfect symmetry in filename, id, track, trackOrder, chapterOrder, inApp, and section IDs
 * - Byte-for-byte synchronization with ui/documentation-data.js on disk
 */

const fs = require('node:fs');
const path = require('node:path');
const { execSync } = require('node:child_process');

const rootDir = path.resolve(__dirname, '..');
const docsRootDir = path.join(rootDir, 'docs', 'handbook');
const uiDataFile = path.join(rootDir, 'ui', 'documentation-data.js');

console.log('[verify-handbook-sync] Checking handbook dual-end synchronization...');

function collectMarkdownFiles(dir) {
  const results = [];
  if (!fs.existsSync(dir)) return results;
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...collectMarkdownFiles(fullPath));
    } else if (entry.isFile() && entry.name.endsWith('.md')) {
      results.push(fullPath);
    }
  }
  return results.sort();
}

const zhFiles = collectMarkdownFiles(path.join(docsRootDir, 'zh'));
const enFiles = collectMarkdownFiles(path.join(docsRootDir, 'en'));

console.log(`[verify-handbook-sync] Discovered ${zhFiles.length} zh files and ${enFiles.length} en files.`);

if (zhFiles.length !== 18) {
  console.error(`❌ Expected exactly 18 zh handbook files, found ${zhFiles.length}!`);
  process.exit(1);
}

if (enFiles.length !== 18) {
  console.error(`❌ Expected exactly 18 en handbook files, found ${enFiles.length}!`);
  process.exit(1);
}

// 1. Check for legacy migration files
const BANNED_LEGACY_FILENAMES = ['color-science.md', 'hardware-calibration.md'];
for (const f of [...zhFiles, ...enFiles]) {
  const base = path.basename(f);
  if (BANNED_LEGACY_FILENAMES.includes(base)) {
    console.error(`❌ Obsolete temporary migration file detected: ${f}`);
    process.exit(1);
  }
}

// 2. Parse frontmatter & sections
function parseFrontmatter(fileContent, filePath) {
  const match = fileContent.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n([\s\S]*)$/);
  if (!match) {
    throw new Error(`Missing YAML frontmatter in: ${filePath}`);
  }
  const rawYml = match[1];
  const markdownBody = match[2];
  const meta = {};

  for (const line of rawYml.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const colonIdx = trimmed.indexOf(':');
    if (colonIdx === -1) continue;
    const key = trimmed.slice(0, colonIdx).trim();
    let val = trimmed.slice(colonIdx + 1).trim();

    if (val.startsWith('"') && val.endsWith('"')) {
      val = JSON.parse(val);
    } else if (val.startsWith("'") && val.endsWith("'")) {
      val = val.slice(1, -1);
    } else if (val === 'true') {
      val = true;
    } else if (val === 'false') {
      val = false;
    } else if (/^\d+$/.test(val)) {
      val = parseInt(val, 10);
    } else if (val.startsWith('[') && val.endsWith(']')) {
      try {
        val = JSON.parse(val);
      } catch {
        val = val.slice(1, -1).split(',').map(s => s.trim().replace(/^['"]|['"]$/g, ''));
      }
    }
    meta[key] = val;
  }
  return { meta, markdownBody };
}

function parseSectionIds(markdownBody, filePath) {
  const rawSections = markdownBody.split(/^##\s+/m);
  const ids = [];
  for (let i = 1; i < rawSections.length; i++) {
    const raw = rawSections[i];
    const firstNewline = raw.indexOf('\n');
    const headingLine = firstNewline === -1 ? raw : raw.slice(0, firstNewline);
    const match = headingLine.match(/^(.*?)(?:\s+\{#([a-zA-Z0-9_-]+)\})?\s*$/);
    if (!match) {
      throw new Error(`Invalid heading syntax: "${headingLine}" in ${filePath}`);
    }
    const heading = match[1].trim();
    const id = match[2] ? match[2].trim() : heading.toLowerCase().replace(/[^a-z0-9_-]+/g, '-');
    ids.push(id);
  }
  return ids;
}

// Map files by relative path
const relativeMap = new Map();
for (const zhPath of zhFiles) {
  const rel = path.relative(path.join(docsRootDir, 'zh'), zhPath);
  relativeMap.set(rel, { zh: zhPath });
}
for (const enPath of enFiles) {
  const rel = path.relative(path.join(docsRootDir, 'en'), enPath);
  if (!relativeMap.has(rel)) {
    console.error(`❌ English file ${rel} has no corresponding Chinese file!`);
    process.exit(1);
  }
  relativeMap.get(rel).en = enPath;
}

for (const [rel, pair] of relativeMap.entries()) {
  if (!pair.zh || !pair.en) {
    console.error(`❌ Asymmetric translation pair for: ${rel}`);
    process.exit(1);
  }

  const zhParsed = parseFrontmatter(fs.readFileSync(pair.zh, 'utf8'), pair.zh);
  const enParsed = parseFrontmatter(fs.readFileSync(pair.en, 'utf8'), pair.en);

  // Assert metadata symmetry
  const checkFields = ['id', 'track', 'trackOrder', 'chapterOrder', 'inApp'];
  for (const f of checkFields) {
    if (zhParsed.meta[f] !== enParsed.meta[f]) {
      console.error(`❌ Frontmatter mismatch for '${f}' in ${rel}: zh "${zhParsed.meta[f]}" vs en "${enParsed.meta[f]}"`);
      process.exit(1);
    }
  }

  // Assert section IDs symmetry
  const zhSecIds = parseSectionIds(zhParsed.markdownBody, pair.zh);
  const enSecIds = parseSectionIds(enParsed.markdownBody, pair.en);

  if (zhSecIds.length !== enSecIds.length) {
    console.error(`❌ Section count mismatch in ${rel}: zh has ${zhSecIds.length}, en has ${enSecIds.length}`);
    process.exit(1);
  }

  for (let i = 0; i < zhSecIds.length; i++) {
    if (zhSecIds[i] !== enSecIds[i]) {
      console.error(`❌ Section ID mismatch in ${rel} at index ${i}: zh "${zhSecIds[i]}" vs en "${enSecIds[i]}"`);
      process.exit(1);
    }
  }
}

console.log('✓ All 18 zh/en markdown pairs match symmetrically in metadata and section IDs.');

// 3. Byte-for-byte check against disk ui/documentation-data.js
if (!fs.existsSync(uiDataFile)) {
  console.error(`❌ ui/documentation-data.js does not exist! Run compile-handbook.mjs first.`);
  process.exit(1);
}

const diskContent = fs.readFileSync(uiDataFile, 'utf8');

// Run compilation
execSync(`node "${path.join(rootDir, 'scripts', 'compile-handbook.mjs')}"`, {
  stdio: 'inherit'
});

const recompiledContent = fs.readFileSync(uiDataFile, 'utf8');

if (diskContent !== recompiledContent) {
  console.error(`❌ ui/documentation-data.js on disk differed from compiler output!`);
  console.error(`   The checked-in ui/documentation-data.js was stale or manually modified.`);
  process.exit(1);
}

console.log('✓ Byte-for-byte verification passed: ui/documentation-data.js is 100% deterministic and synchronized with SSOT handbook.');
console.log('\n🎉 ALL HANDBOOK SYNC VERIFICATIONS PASSED (100% GREEN)!');
