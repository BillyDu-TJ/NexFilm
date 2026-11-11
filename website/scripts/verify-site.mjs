#!/usr/bin/env node

/**
 * website/scripts/verify-site.mjs
 *
 * Full-scale build artifact audit for NexFilm Website (dist/):
 * 1. Zero broken internal links & anchor (#id) fragments
 * 2. 100% zh and en route symmetry
 * 3. Zero raw unrendered LaTeX leaks ($$, \frac, \log_{10})
 * 4. Zero legacy asset package names or mirrors (NexFilm_1.1.0_, ghproxy)
 * 5. Pagefind search index completeness and zero alias indexing
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const distDir = path.resolve(__dirname, '..', 'dist');

console.log('[verify-site] Starting full build artifact audit on dist/...');

if (!fs.existsSync(distDir)) {
  console.error('❌ website/dist directory not found! Run npm run build first.');
  process.exit(1);
}

function collectHtmlFiles(dir) {
  const results = [];
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...collectHtmlFiles(full));
    } else if (entry.isFile() && entry.name.endsWith('.html')) {
      results.push(full);
    }
  }
  return results;
}

const htmlFiles = collectHtmlFiles(distDir);
console.log(`[verify-site] Found ${htmlFiles.length} HTML pages in dist/.`);

// 1. Build Route & DOM ID Registry
const routeRegistry = new Map(); // normalizedPath -> { filePath, ids: Set<string>, content: string }

for (const file of htmlFiles) {
  const rel = path.relative(distDir, file);
  // Normalize route path:
  // index.html -> /
  // zh/index.html -> /zh/
  // zh/docs/index.html -> /zh/docs/
  // zh/docs/foo/index.html -> /zh/docs/foo/
  let route = '/' + rel.replace(/\\/g, '/');
  if (route.endsWith('/index.html')) {
    route = route.slice(0, -'index.html'.length);
  } else if (route === '/index.html') {
    route = '/';
  }

  const content = fs.readFileSync(file, 'utf8');
  const ids = new Set();
  const idMatches = content.matchAll(/\sid=["']([^"']+)["']/g);
  for (const m of idMatches) {
    ids.add(m[1]);
  }

  routeRegistry.set(route, { filePath: file, ids, content });
  if (route.endsWith('/') && route.length > 1) {
    routeRegistry.set(route.slice(0, -1), { filePath: file, ids, content });
  }
}

// ------------------------------------------------ Check 1: Links & Anchors
console.log('[verify-site] Checking 1: Verifying internal links & anchors...');
const linkViolations = [];

for (const [route, entry] of routeRegistry.entries()) {
  if (route.length > 1 && !route.endsWith('/')) continue; // Skip alias non-slash duplicates

  const linkMatches = entry.content.matchAll(/<a\s+[^>]*?href=["']([^"']+)["']/gi);
  for (const m of linkMatches) {
    const href = m[1].trim();
    if (!href || href.startsWith('http://') || href.startsWith('https://') || href.startsWith('mailto:') || href.startsWith('javascript:')) {
      continue;
    }

    let targetRoute = '';
    let targetHash = '';

    if (href.startsWith('#')) {
      // Local hash on the same page
      targetRoute = route;
      targetHash = href.slice(1);
    } else {
      const hashIdx = href.indexOf('#');
      if (hashIdx !== -1) {
        targetRoute = href.slice(0, hashIdx);
        targetHash = href.slice(hashIdx + 1);
      } else {
        targetRoute = href;
      }
    }

    // Resolve relative path if needed
    if (!targetRoute.startsWith('/')) {
      targetRoute = path.posix.resolve(route, targetRoute);
    }
    if (!targetRoute.endsWith('/') && !targetRoute.includes('.')) {
      targetRoute += '/';
    }

    // Ignore static asset links like favicon.png
    if (/\.(png|jpg|webp|svg|ico|css|js|json|xml|txt)$/i.test(targetRoute)) {
      const assetPath = path.join(distDir, targetRoute);
      if (!fs.existsSync(assetPath)) {
        linkViolations.push(`${route}: Static asset not found: ${href}`);
      }
      continue;
    }

    const targetEntry = routeRegistry.get(targetRoute);
    if (!targetEntry) {
      linkViolations.push(`${route}: Dead link: ${href} (resolved to ${targetRoute})`);
      continue;
    }

    if (targetHash && !targetEntry.ids.has(targetHash)) {
      linkViolations.push(`${route}: Target anchor #${targetHash} not found on ${targetRoute}`);
    }
  }
}

if (linkViolations.length > 0) {
  console.error(`❌ Found ${linkViolations.length} dead link / anchor violations:`);
  for (const v of linkViolations.slice(0, 15)) console.error(`  - ${v}`);
  process.exit(1);
}
console.log('✓ Check 1 PASSED: Zero dead internal links or missing #anchors.');

// --------------------------------------------- Check 2: Route Symmetry (zh / en)
console.log('[verify-site] Checking 2: Verifying zh / en route symmetry...');
const zhRoutes = Array.from(routeRegistry.keys()).filter(r => r.startsWith('/zh/') && r.endsWith('/'));
const symmetryViolations = [];

for (const zh of zhRoutes) {
  const enEquivalent = zh.replace(/^\/zh\//, '/en/');
  if (!routeRegistry.has(enEquivalent)) {
    symmetryViolations.push(`Missing English counterpart for ${zh} -> ${enEquivalent}`);
  }
}

const enRoutes = Array.from(routeRegistry.keys()).filter(r => r.startsWith('/en/') && r.endsWith('/'));
for (const en of enRoutes) {
  const zhEquivalent = en.replace(/^\/en\//, '/zh/');
  if (!routeRegistry.has(zhEquivalent)) {
    symmetryViolations.push(`Missing Chinese counterpart for ${en} -> ${zhEquivalent}`);
  }
}

if (symmetryViolations.length > 0) {
  console.error(`❌ Found ${symmetryViolations.length} route symmetry violations:`);
  for (const s of symmetryViolations) console.error(`  - ${s}`);
  process.exit(1);
}
console.log(`✓ Check 2 PASSED: All ${zhRoutes.length} zh routes match ${enRoutes.length} en routes symmetrically.`);

// ---------------------------------------- Check 3: Raw Unrendered LaTeX Leaks
console.log('[verify-site] Checking 3: Scanning for raw unrendered LaTeX leaks...');
const latexViolations = [];

for (const [route, entry] of routeRegistry.entries()) {
  if (route.length > 1 && !route.endsWith('/')) continue;

  // Strip <script>, <style>, <pre>, <code>, and KaTeX <annotation>/<math> accessibility tags before scanning
  const stripped = entry.content
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<style[\s\S]*?<\/style>/gi, '')
    .replace(/<pre[\s\S]*?<\/pre>/gi, '')
    .replace(/<code[\s\S]*?<\/code>/gi, '')
    .replace(/<annotation[\s\S]*?<\/annotation>/gi, '')
    .replace(/<!--[\s\S]*?-->/g, '');

  if (/\$\$[\s\S]+?\$\$/.test(stripped)) {
    latexViolations.push(`${route}: Raw $$ display math delimiter leak found`);
  }
  if (/MATH_PLACEHOLDER|PLACEHOLDER_\d+/.test(stripped)) {
    const match = stripped.match(/.{0,25}(MATH_PLACEHOLDER|PLACEHOLDER_\d+).{0,25}/);
    latexViolations.push(`${route}: Math placeholder leak found: "...${match ? match[0] : ''}..."`);
  }
  if (/\\(frac|log_\{10\}|bmatrix|sum_|lambda|epsilon|approx|cdot)/.test(stripped)) {
    const match = stripped.match(/.{0,20}\\(frac|log_\{10\}|bmatrix|sum_|lambda|epsilon|approx|cdot).{0,20}/);
    latexViolations.push(`${route}: Raw LaTeX macro leak: "...${match ? match[0] : ''}..."`);
  }
}

if (latexViolations.length > 0) {
  console.error(`❌ Found ${latexViolations.length} raw LaTeX leaks:`);
  for (const l of latexViolations) console.error(`  - ${l}`);
  process.exit(1);
}
console.log('✓ Check 3 PASSED: Zero raw unrendered LaTeX formula leaks in page text.');

// --------------------------------- Check 4: Zero Legacy Asset Names / Mirrors
console.log('[verify-site] Checking 4: Scanning for legacy asset names and proxy mirrors...');
const legacyAssetViolations = [];

for (const [route, entry] of routeRegistry.entries()) {
  if (route.length > 1 && !route.endsWith('/')) continue;

  if (entry.content.includes('NexFilm_1.1.0_')) {
    legacyAssetViolations.push(`${route}: Contains legacy package string "NexFilm_1.1.0_"`);
  }
  if (entry.content.includes('ghproxy')) {
    legacyAssetViolations.push(`${route}: Contains legacy proxy mirror "ghproxy"`);
  }
}

if (legacyAssetViolations.length > 0) {
  console.error(`❌ Found ${legacyAssetViolations.length} legacy asset string violations:`);
  for (const v of legacyAssetViolations) console.error(`  - ${v}`);
  process.exit(1);
}
console.log('✓ Check 4 PASSED: Zero legacy package names or mirrors found.');

// --------------------------------------------- Check 5: Pagefind Index Audit
console.log('[verify-site] Checking 5: Auditing Pagefind index artifacts & alias isolation...');
const pagefindJs = path.join(distDir, 'pagefind', 'pagefind.js');

if (!fs.existsSync(pagefindJs) || fs.statSync(pagefindJs).size === 0) {
  console.error('❌ Pagefind index not found or empty at dist/pagefind/pagefind.js!');
  process.exit(1);
}

// Verify that canonical chapters have data-pagefind-body, and alias routes DO NOT have data-pagefind-body
const canonicalDocRoutes = [
  '/zh/docs/overview-and-install/',
  '/zh/docs/scanning-best-practices/',
  '/zh/docs/first-roll-in-five-minutes/',
  '/zh/docs/roll-vs-loose-workflows/',
  '/zh/docs/workspace-and-views/',
  '/zh/docs/import-formats-and-library/',
  '/zh/docs/film-area-and-geometry/',
  '/zh/docs/density-grading-and-curves/',
  '/zh/docs/print-film-emulation/',
  '/zh/docs/export-and-contact-sheets/',
  '/zh/docs/troubleshooting-and-appendix/',
  '/zh/docs/three-mathematical-domains/',
  '/zh/docs/input-domain-and-unified-pipeline/',
  '/zh/docs/hardware-calibration-guide/',
  '/zh/docs/cinema-and-aces-workflow/',
  '/zh/docs/system-architecture/',
  '/zh/docs/core-modules-walkthrough/',
  '/zh/docs/building-and-contributing/'
];

const aliasDocRoutes = [
  '/zh/docs/quick-start/',
  '/zh/docs/interface-basics/',
  '/zh/docs/import-and-rolls/',
  '/zh/docs/develop-workspace/',
  '/zh/docs/troubleshooting/',
  '/zh/docs/appendix/',
  '/zh/docs/color-science/',
  '/zh/docs/hardware-calibration/'
];

for (const r of canonicalDocRoutes) {
  const entry = routeRegistry.get(r);
  if (!entry) {
    console.error(`❌ Missing canonical doc route: ${r}`);
    process.exit(1);
  }
  if (!entry.content.includes('data-pagefind-body')) {
    console.error(`❌ Canonical route ${r} is missing data-pagefind-body!`);
    process.exit(1);
  }
}

for (const r of aliasDocRoutes) {
  const entry = routeRegistry.get(r);
  if (!entry) {
    console.error(`❌ Missing alias doc route: ${r}`);
    process.exit(1);
  }
  if (entry.content.includes('data-pagefind-body')) {
    console.error(`❌ Alias route ${r} improperly contains data-pagefind-body (causes duplicate search results)!`);
    process.exit(1);
  }
  if (!entry.content.includes('<link rel="canonical"')) {
    console.error(`❌ Alias route ${r} missing <link rel="canonical"> tag!`);
    process.exit(1);
  }
}

console.log('✓ Check 5 PASSED: Pagefind index verified; canonical routes indexed and alias routes isolated with canonical links.');

console.log('\n======================================================');
console.log('🎉 ALL 5 SITE ARTIFACT VERIFICATIONS PASSED (100% GREEN)!');
console.log('======================================================');
