#!/usr/bin/env node

/**
 * scripts/compile-handbook.mjs
 *
 * SSOT (Single Source of Truth) handbook compiler for NexFilm Engine.
 * Reads docs/handbook/{zh,en} recursive markdown files, applies strict anti-slacking quality gates,
 * and compiles in-app chapters into ui/documentation-data.js.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, '..');

const docsRootDir = path.resolve(rootDir, 'docs', 'handbook');
const uiDataFile = path.resolve(rootDir, 'ui', 'documentation-data.js');

// Dynamically import katex and marked from website/node_modules
const katexModule = await import(new URL('../website/node_modules/katex/dist/katex.mjs', import.meta.url).href);
const katex = katexModule.default || katexModule;

const markedModule = await import(new URL('../website/node_modules/marked/lib/marked.esm.js', import.meta.url).href);
const { Marked } = markedModule;

const marked = new Marked({
  gfm: true,
  breaks: false,
  renderer: {
    code(token) {
      const text = typeof token === 'string' ? token : (token && token.text) || '';
      return `<pre class="doc-code-block"><code>${escapeHtml(text)}</code></pre>\n`;
    },
    heading(token) {
      const match = token.text.match(/^(.*?)(?:\s+\{#([a-zA-Z0-9_-]+)\})?\s*$/);
      const cleanText = match ? match[1].trim() : token.text.trim();
      const id = match && match[2]
        ? match[2].trim()
        : cleanText.toLowerCase().replace(/[^\p{L}\p{N}_-]+/gu, '-').replace(/^-+|-+$/g, '') || 'heading';
      const renderedText = this.parser.parseInline(token.tokens).replace(/\s*\{#[a-zA-Z0-9_-]+\}\s*$/, '');
      return `<h${token.depth} id="${id}" class="scroll-mt-24">${renderedText}</h${token.depth}>\n`;
    }
  }
});

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

const BANNED_ZH = [
  '颠覆', '黑科技', '秒杀', '极致', '震撼', '完美无瑕',
  '轻松拿捏', '赋能', '众所周知', '总而言之', '让我们一起', '不仅如此'
];

const BANNED_EN = [
  'blazingly', 'revolutionary', 'game-changing', 'magic',
  'effortlessly', 'dive into', 'delve', 'in conclusion', 'unleash'
];

const TRACK_TITLES = {
  'zh-CN': {
    'getting-started': '快速上手',
    'user-manual': '工作台手册',
    'color-science': '色彩科学',
    'architecture': '源码与架构'
  },
  'en': {
    'getting-started': 'Getting Started',
    'user-manual': 'User Manual',
    'color-science': 'Color Science',
    'architecture': 'Architecture'
  }
};

/**
 * Simple YAML frontmatter parser
 */
function parseFrontmatter(fileContent, filePath) {
  const match = fileContent.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n([\s\S]*)$/);
  if (!match) {
    throw new Error(`[compile-handbook] Missing YAML frontmatter in: ${filePath}`);
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

    // Parse value types
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

  // Validate required frontmatter fields
  const required = ['id', 'track', 'trackOrder', 'chapterOrder', 'kicker', 'title', 'subtitle', 'readingTime', 'inApp'];
  for (const req of required) {
    if (meta[req] === undefined) {
      throw new Error(`[compile-handbook] Frontmatter missing required field '${req}' in ${filePath}`);
    }
  }

  return { meta, markdownBody };
}

/**
 * Split markdown body into sections: ## Heading {#section-id}
 */
function parseSections(markdownBody, filePath) {
  const rawSections = markdownBody.split(/^##\s+/m);
  const sections = [];

  for (let i = 1; i < rawSections.length; i++) {
    const raw = rawSections[i];
    const firstNewline = raw.indexOf('\n');
    const headingLine = firstNewline === -1 ? raw : raw.slice(0, firstNewline);
    const body = firstNewline === -1 ? '' : raw.slice(firstNewline + 1);

    const match = headingLine.match(/^(.*?)(?:\s+\{#([a-zA-Z0-9_-]+)\})?\s*$/);
    if (!match) {
      throw new Error(`[compile-handbook] Invalid heading syntax: "${headingLine}" in ${filePath}`);
    }

    const heading = match[1].trim();
    const id = match[2] ? match[2].trim() : heading.toLowerCase().replace(/[^a-z0-9_-]+/g, '-');
    sections.push({ id, heading, rawBody: body.trim() });
  }

  if (sections.length === 0) {
    throw new Error(`[compile-handbook] Chapter has no '## ' sections in ${filePath}`);
  }

  return sections;
}

/**
 * Transform GitHub Alerts (> [!NOTE], > [!WARNING], etc.) to .doc-callout
 */
function transformGithubAlerts(text, locale) {
  const alertPattern = /^>[ \t]*\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*(.*?)\r?\n((?:>[ \t]*.*\r?\n?)*)/gim;

  return text.replace(alertPattern, (match, type, title, bodyLines) => {
    const isWarn = ['WARNING', 'IMPORTANT', 'CAUTION'].includes(type.toUpperCase());
    const defaultTitles = {
      'NOTE': locale === 'zh-CN' ? '注意' : 'Note',
      'TIP': locale === 'zh-CN' ? '提示' : 'Tip',
      'IMPORTANT': locale === 'zh-CN' ? '重要' : 'Important',
      'WARNING': locale === 'zh-CN' ? '警告' : 'Warning',
      'CAUTION': locale === 'zh-CN' ? '注意' : 'Caution'
    };
    const finalTitle = title.trim() || defaultTitles[type.toUpperCase()] || type;
    const cleanBody = bodyLines
      .split(/\r?\n/)
      .map(line => line.replace(/^>[ \t]?/, ''))
      .join('\n')
      .trim();

    const bodyHtml = marked.parse(cleanBody).trim();
    const warnClass = isWarn ? ' doc-callout-warn' : '';
    return `<div class="doc-callout${warnClass}">\n    <div class="doc-callout-title">${finalTitle}</div>\n    <div class="doc-callout-body">${bodyHtml}</div>\n</div>\n`;
  });
}

/**
 * Render LaTeX math formulas into native offline MathML using KaTeX
 */
function renderKatexMathML(text) {
  // Preserve code blocks and inline code from formula processing
  const placeholders = [];
  let safe = text.replace(/(<pre[\s\S]*?<\/pre>|<code[\s\S]*?<\/code>|`[^`]+`|```[\s\S]*?```)/gi, m => {
    placeholders.push(m);
    return `___CODE_PLACEHOLDER_${placeholders.length - 1}___`;
  });

  function cleanFormula(f) {
    return f.trim()
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&amp;/g, '&');
  }

  // Display math $$...$$
  safe = safe.replace(/\$\$([\s\S]+?)\$\$/g, (match, formula) => {
    try {
      return katex.renderToString(cleanFormula(formula), { output: 'mathml', displayMode: true, throwOnError: true });
    } catch (e) {
      throw new Error(`[compile-handbook] KaTeX error in display formula "$$${formula}$$": ${e.message}`);
    }
  });

  // Inline math $...$
  safe = safe.replace(/(?<!\\|\$)\$([^\$\n]+?)\$(?!\$)/g, (match, formula) => {
    try {
      return katex.renderToString(cleanFormula(formula), { output: 'mathml', displayMode: false, throwOnError: true });
    } catch (e) {
      throw new Error(`[compile-handbook] KaTeX error in inline formula "$${formula}$": ${e.message}`);
    }
  });

  // Restore code placeholders
  return safe.replace(/___CODE_PLACEHOLDER_(\d+)___/g, (_, idx) => placeholders[Number(idx)]);
}

/**
 * Process a section's markdown body into compiled desktop HTML
 */
function compileSectionBody(rawBody, locale) {
  // 1. Transform GitHub Alerts
  let step = transformGithubAlerts(rawBody, locale);

  // 2. Render LaTeX math formulas to native MathML
  step = renderKatexMathML(step);

  // 3. If body already contains raw block elements (e.g. existing migrated chapters),
  // marked passes HTML through while formatting lists, bold, inline code, and tables.
  const html = marked.parse(step).trim();

  // Normalize table classes
  let finalHtml = html.replace(/<table(?!\s+class=)/g, '<table class="doc-table"');
  // Normalize code block classes
  finalHtml = finalHtml.replace(/<pre(?!\s+class=)>/g, '<pre class="doc-code-block">');

  return finalHtml;
}

/**
 * Collect all .md files in directory recursively
 */
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

/**
 * Quality Gate 1 & 2: Character/Word counts and structure metrics
 */
function evaluateContentMetrics(chapter, langDir) {
  let fullRaw = '';
  for (const sec of chapter.sections) {
    fullRaw += '\n' + sec.rawBody;
  }

  // Strip code blocks and inline code
  const proseOnly = fullRaw
    .replace(/```[\s\S]*?```/g, '')
    .replace(/`[^`]*`/g, '')
    .replace(/<pre[\s\S]*?<\/pre>/g, '')
    .replace(/<code[\s\S]*?<\/code>/g, '')
    .replace(/<[^>]+>/g, '');

  if (langDir === 'zh') {
    const visibleChars = proseOnly.replace(/\s+/g, '');
    return { count: visibleChars.length, unit: 'chars' };
  } else {
    const words = proseOnly.trim().split(/\s+/).filter(Boolean);
    return { count: words.length, unit: 'words' };
  }
}

/**
 * Quality Gate 3: Scan for prohibited marketing words & prose exclamation marks
 */
function scanToneViolations(chapter, filePath, langDir) {
  const violations = [];

  for (const sec of chapter.sections) {
    const prose = sec.rawBody
      .replace(/```[\s\S]*?```/g, '')
      .replace(/`[^`]*`/g, '')
      .replace(/<pre[\s\S]*?<\/pre>/g, '')
      .replace(/<code[\s\S]*?<\/code>/g, '')
      .replace(/^>[ \t]*\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]/gim, '');

    // Check exclamation marks
    if (/[!！]/.test(prose)) {
      const match = prose.match(/.{0,25}[!！].{0,25}/);
      violations.push(`Prose exclamation mark found in [${sec.id}]: "...${match ? match[0] : ''}..."`);
    }

    // Check prohibited words
    if (langDir === 'zh') {
      for (const word of BANNED_ZH) {
        if (prose.includes(word)) {
          violations.push(`Prohibited marketing word "${word}" found in [${sec.id}]`);
        }
      }
    } else {
      for (const word of BANNED_EN) {
        const regex = new RegExp(`\\b${word}\\b`, 'i');
        if (regex.test(prose)) {
          violations.push(`Prohibited marketing word "${word}" found in [${sec.id}]`);
        }
      }
    }
  }

  if (violations.length > 0) {
    console.error(`\n[compile-handbook] Tone / style violations in ${filePath}:`);
    for (const v of violations) console.error(`  - ${v}`);
    process.exit(1);
  }
}

/**
 * Quality Gate 4: HTML balanced tags check (p, table, tr, td, ul, li, ol, strong, code)
 */
function verifyTagBalance(chapters, locale) {
  const unbalanced = [];
  const trackedTags = ['p', 'table', 'tr', 'td', 'ul', 'li', 'ol', 'strong', 'code'];

  for (const chapter of chapters) {
    for (const sec of chapter.sections) {
      for (const tag of trackedTags) {
        const opened = (sec.content.match(new RegExp('<' + tag + '[ >]', 'g')) || []).length;
        const closed = (sec.content.match(new RegExp('</' + tag + '>', 'g')) || []).length;
        if (opened !== closed) {
          unbalanced.push(`${locale}:${chapter.id}:${sec.id}:${tag} (opened: ${opened}, closed: ${closed})`);
        }
      }
    }
  }

  if (unbalanced.length > 0) {
    console.error(`\n[compile-handbook] HTML tag balance violations found in ${locale}:`);
    for (const u of unbalanced) console.error(`  - ${u}`);
    process.exit(1);
  }
}

// ================================================================= Main Execution

console.log('[compile-handbook] Starting SSOT handbook compilation...');

const zhFiles = collectMarkdownFiles(path.join(docsRootDir, 'zh'));
const enFiles = collectMarkdownFiles(path.join(docsRootDir, 'en'));

console.log(`[compile-handbook] Found ${zhFiles.length} zh files and ${enFiles.length} en files.`);

if (zhFiles.length === 0 || enFiles.length === 0) {
  console.error('[compile-handbook] No handbook files found in docs/handbook/{zh,en}!');
  process.exit(1);
}

// Parse all chapters
const parsedData = { 'zh-CN': [], 'en': [] };
const relativeFileMap = new Map();

for (const [langDir, locale, files] of [['zh', 'zh-CN', zhFiles], ['en', 'en', enFiles]]) {
  for (const filePath of files) {
    const relPath = path.relative(path.join(docsRootDir, langDir), filePath);
    const content = fs.readFileSync(filePath, 'utf8');
    const { meta, markdownBody } = parseFrontmatter(content, filePath);
    const sections = parseSections(markdownBody, filePath);

    const chapterObj = {
      filePath,
      relPath,
      id: meta.id,
      track: meta.track,
      trackTitle: TRACK_TITLES[locale][meta.track] || meta.track,
      trackOrder: meta.trackOrder,
      chapterOrder: meta.chapterOrder,
      kicker: meta.kicker,
      title: meta.title,
      subtitle: meta.subtitle,
      readingTime: meta.readingTime,
      inApp: Boolean(meta.inApp),
      sinceVersion: meta.sinceVersion || 'v1.1.0',
      aliases: meta.aliases || [],
      sections: sections.map(sec => ({
        id: sec.id,
        heading: sec.heading,
        rawBody: sec.rawBody,
        content: compileSectionBody(sec.rawBody, locale)
      }))
    };

    // Tone & style check
    scanToneViolations(chapterObj, filePath, langDir);

    parsedData[locale].push(chapterObj);

    if (langDir === 'zh') {
      relativeFileMap.set(relPath, { zh: chapterObj });
    } else {
      if (relativeFileMap.has(relPath)) {
        relativeFileMap.get(relPath).en = chapterObj;
      } else {
        relativeFileMap.set(relPath, { en: chapterObj });
      }
    }
  }
}

// ------------------------------------------------------------- Gate 1: Symmetry
console.log('[compile-handbook] Checking Gate 1: 100% zh/en symmetry...');

for (const [relPath, pair] of relativeFileMap.entries()) {
  if (!pair.zh || !pair.en) {
    console.error(`[compile-handbook] Asymmetric files: missing translation for ${relPath}`);
    process.exit(1);
  }

  if (pair.zh.id !== pair.en.id) {
    console.error(`[compile-handbook] ID mismatch in ${relPath}: zh "${pair.zh.id}" vs en "${pair.en.id}"`);
    process.exit(1);
  }
  if (pair.zh.track !== pair.en.track) {
    console.error(`[compile-handbook] Track mismatch in ${relPath}: zh "${pair.zh.track}" vs en "${pair.en.track}"`);
    process.exit(1);
  }
  if (pair.zh.inApp !== pair.en.inApp) {
    console.error(`[compile-handbook] inApp mismatch in ${relPath}: zh "${pair.zh.inApp}" vs en "${pair.en.inApp}"`);
    process.exit(1);
  }

  const zhSecIds = pair.zh.sections.map(s => s.id);
  const enSecIds = pair.en.sections.map(s => s.id);
  if (JSON.stringify(zhSecIds) !== JSON.stringify(enSecIds)) {
    console.error(`[compile-handbook] Section ID array mismatch in ${relPath}:`);
    console.error('  zh sections:', zhSecIds);
    console.error('  en sections:', enSecIds);
    process.exit(1);
  }
}

console.log('✓ Gate 1 PASSED: zh and en chapters and sections match 100% symmetrically.');

// ----------------------------- Gate 2: Quality & Volume for Refined Chapters
console.log('[compile-handbook] Checking Gate 2: Quality & Volume metrics across all 4 tracks...');

const HEAVY_USER_MANUAL_CHAPTERS = [
  'density-grading-and-curves',
  'export-and-contact-sheets',
  'troubleshooting-and-appendix'
];

for (const [langDir, locale] of [['zh', 'zh-CN'], ['en', 'en']]) {
  const gettingStartedChapters = parsedData[locale].filter(ch => ch.track === 'getting-started');
  if (gettingStartedChapters.length !== 5) {
    console.error(`[compile-handbook] Expected 5 getting-started chapters in ${locale}, found ${gettingStartedChapters.length}`);
    process.exit(1);
  }

  for (const ch of gettingStartedChapters) {
    const metrics = evaluateContentMetrics(ch, langDir);
    const minThreshold = langDir === 'zh' ? 1800 : 950;
    if (metrics.count < minThreshold) {
      console.error(`[compile-handbook] Volume gate failed in ${ch.filePath}:`);
      console.error(`  Found ${metrics.count} ${metrics.unit}, minimum required is ${minThreshold} ${metrics.unit}!`);
      process.exit(1);
    }

    if (ch.sections.length < 4 || ch.sections.length > 7) {
      console.error(`[compile-handbook] Section count gate failed in ${ch.filePath}: has ${ch.sections.length} sections (required: 4 to 7).`);
      process.exit(1);
    }

    const fullHtml = ch.sections.map(s => s.content).join('\n');
    if (!/<table[ >]/.test(fullHtml)) {
      console.error(`[compile-handbook] Structure gate failed in ${ch.filePath}: missing at least 1 <table>`);
      process.exit(1);
    }
    if (!/class="doc-callout/.test(fullHtml)) {
      console.error(`[compile-handbook] Structure gate failed in ${ch.filePath}: missing at least 1 .doc-callout`);
      process.exit(1);
    }

    console.log(`  ✓ ${locale} [${ch.id}]: ${metrics.count} ${metrics.unit}, ${ch.sections.length} sections, <table> & callouts verified.`);
  }

  const userManualChapters = parsedData[locale].filter(ch => ch.track === 'user-manual');
  if (userManualChapters.length !== 7) {
    console.error(`[compile-handbook] Expected 7 user-manual chapters in ${locale}, found ${userManualChapters.length}`);
    process.exit(1);
  }

  for (const ch of userManualChapters) {
    const metrics = evaluateContentMetrics(ch, langDir);
    const isHeavy = HEAVY_USER_MANUAL_CHAPTERS.includes(ch.id);
    const minThreshold = isHeavy
      ? (langDir === 'zh' ? 2400 : 1250)
      : (langDir === 'zh' ? 1800 : 950);

    if (metrics.count < minThreshold) {
      console.error(`[compile-handbook] Volume gate failed in ${ch.filePath}:`);
      console.error(`  Found ${metrics.count} ${metrics.unit}, minimum required is ${minThreshold} ${metrics.unit}${isHeavy ? ' (heavy chapter)' : ''}!`);
      process.exit(1);
    }

    if (ch.sections.length < 4 || ch.sections.length > 7) {
      console.error(`[compile-handbook] Section count gate failed in ${ch.filePath}: has ${ch.sections.length} sections (required: 4 to 7).`);
      process.exit(1);
    }

    const fullHtml = ch.sections.map(s => s.content).join('\n');
    if (!/<table[ >]/.test(fullHtml)) {
      console.error(`[compile-handbook] Structure gate failed in ${ch.filePath}: missing at least 1 <table>`);
      process.exit(1);
    }
    if (!/class="doc-callout/.test(fullHtml)) {
      console.error(`[compile-handbook] Structure gate failed in ${ch.filePath}: missing at least 1 .doc-callout`);
      process.exit(1);
    }

    console.log(`  ✓ ${locale} [${ch.id}]: ${metrics.count} ${metrics.unit}, ${ch.sections.length} sections, <table> & callouts verified.${isHeavy ? ' (heavy chapter)' : ''}`);
  }

  const colorScienceChapters = parsedData[locale].filter(ch => ch.track === 'color-science');
  if (colorScienceChapters.length !== 4) {
    console.error(`[compile-handbook] Expected 4 color-science chapters in ${locale}, found ${colorScienceChapters.length}`);
    process.exit(1);
  }

  for (const ch of colorScienceChapters) {
    const metrics = evaluateContentMetrics(ch, langDir);
    const minThreshold = langDir === 'zh' ? 2200 : 1100;
    if (metrics.count < minThreshold) {
      console.error(`[compile-handbook] Volume gate failed in ${ch.filePath}:`);
      console.error(`  Found ${metrics.count} ${metrics.unit}, minimum required is ${minThreshold} ${metrics.unit}!`);
      process.exit(1);
    }

    if (ch.sections.length < 4 || ch.sections.length > 7) {
      console.error(`[compile-handbook] Section count gate failed in ${ch.filePath}: has ${ch.sections.length} sections (required: 4 to 7).`);
      process.exit(1);
    }

    const fullHtml = ch.sections.map(s => s.content).join('\n');
    if (!/<table[ >]/.test(fullHtml)) {
      console.error(`[compile-handbook] Structure gate failed in ${ch.filePath}: missing at least 1 <table>`);
      process.exit(1);
    }
    if (!/class="doc-callout/.test(fullHtml)) {
      console.error(`[compile-handbook] Structure gate failed in ${ch.filePath}: missing at least 1 .doc-callout`);
      process.exit(1);
    }

    console.log(`  ✓ ${locale} [${ch.id}]: ${metrics.count} ${metrics.unit}, ${ch.sections.length} sections, <table> & callouts verified.`);
  }

  const architectureChapters = parsedData[locale].filter(ch => ch.track === 'architecture');
  if (architectureChapters.length !== 3) {
    console.error(`[compile-handbook] Expected 3 architecture chapters in ${locale}, found ${architectureChapters.length}`);
    process.exit(1);
  }

  for (const ch of architectureChapters) {
    const metrics = evaluateContentMetrics(ch, langDir);
    const minThreshold = langDir === 'zh' ? 1800 : 950;
    if (metrics.count < minThreshold) {
      console.error(`[compile-handbook] Volume gate failed in ${ch.filePath}:`);
      console.error(`  Found ${metrics.count} ${metrics.unit}, minimum required is ${minThreshold} ${metrics.unit}!`);
      process.exit(1);
    }

    if (ch.sections.length < 4 || ch.sections.length > 7) {
      console.error(`[compile-handbook] Section count gate failed in ${ch.filePath}: has ${ch.sections.length} sections (required: 4 to 7).`);
      process.exit(1);
    }

    const fullHtml = ch.sections.map(s => s.content).join('\n');
    if (!/<table[ >]/.test(fullHtml)) {
      console.error(`[compile-handbook] Structure gate failed in ${ch.filePath}: missing at least 1 <table>`);
      process.exit(1);
    }
    if (!/class="doc-callout/.test(fullHtml)) {
      console.error(`[compile-handbook] Structure gate failed in ${ch.filePath}: missing at least 1 .doc-callout`);
      process.exit(1);
    }

    console.log(`  ✓ ${locale} [${ch.id}]: ${metrics.count} ${metrics.unit}, ${ch.sections.length} sections, <table> & callouts verified.`);
  }
}

console.log('✓ Gate 2 PASSED: All refined chapters meet volume, section count, table & callout requirements.');

// ------------------------------------------------ Gate 4: HTML Tag Balance
console.log('[compile-handbook] Checking Gate 4: HTML balanced tags & redlines...');

for (const locale of ['zh-CN', 'en']) {
  verifyTagBalance(parsedData[locale], locale);
}

// Verify existence of curve-and-detail section
for (const locale of ['zh-CN', 'en']) {
  const allSectionIds = parsedData[locale].flatMap(ch => ch.sections).map(s => s.id);
  if (!allSectionIds.includes('curve-and-detail')) {
    console.error(`[compile-handbook] Critical regression redline: section 'curve-and-detail' is missing in ${locale}!`);
    process.exit(1);
  }
}

console.log('✓ Gate 4 PASSED: All HTML tags balanced and curve-and-detail confirmed.');

// ------------------------------------------------------------- Generate JS Data
console.log(`[compile-handbook] Generating ${uiDataFile}...`);

// Filter inApp chapters and sort by trackOrder then chapterOrder
const compiledOutput = { 'zh-CN': [], 'en': [] };

for (const locale of ['zh-CN', 'en']) {
  const inAppChapters = parsedData[locale]
    .filter(ch => ch.inApp)
    .sort((a, b) => a.trackOrder - b.trackOrder || a.chapterOrder - b.chapterOrder);

  compiledOutput[locale] = inAppChapters.map(ch => ({
    id: ch.id,
    track: ch.track,
    trackTitle: ch.trackTitle,
    trackOrder: ch.trackOrder,
    chapterOrder: ch.chapterOrder,
    kicker: ch.kicker,
    title: ch.title,
    subtitle: ch.subtitle,
    readingTime: ch.readingTime,
    sinceVersion: ch.sinceVersion,
    aliases: ch.aliases,
    sections: ch.sections.map(s => ({
      id: s.id,
      heading: s.heading,
      content: s.content
    }))
  }));
}

const fileHeader = `// AUTO-GENERATED FROM docs/handbook — DO NOT EDIT MANUALLY
(function (global) {
    'use strict';

    const CHAPTERS = ${JSON.stringify(compiledOutput, null, 4)};

    const LOCALES = ['zh-CN', 'en'];

    function normalizeLocale(locale) {
        const value = String(locale || '').toLowerCase();
        return value.startsWith('zh') ? 'zh-CN' : 'en';
    }

    function getChapters(locale) {
        return CHAPTERS[normalizeLocale(locale)] || CHAPTERS.en;
    }

    function getChapterById(id, locale) {
        return getChapters(locale).find(chapter => chapter.id === id || (chapter.aliases || []).includes(id)) || null;
    }

    function getFallbackChapter(locale) {
        const chapters = getChapters(locale);
        return chapters.length > 0 ? chapters[0] : null;
    }

    global.NexFilmDocsData = {
        locales: LOCALES,
        getChapters: getChapters,
        getChapterById: getChapterById,
        getFallbackChapter: getFallbackChapter
    };
})(typeof window !== 'undefined' ? window : globalThis);
`;

fs.writeFileSync(uiDataFile, fileHeader, 'utf8');
console.log(`✓ Successfully compiled handbook into ${uiDataFile} (${(fileHeader.length / 1024).toFixed(1)} KB)`);
