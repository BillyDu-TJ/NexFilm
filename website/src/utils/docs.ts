import { getCollection, type CollectionEntry } from 'astro:content';
import { Marked } from 'marked';
import { renderBlockMath, renderInlineMath } from './math';

export interface DocSection {
  id: string;
  heading: string;
  content: string;
}

export interface TocHeading {
  id: string;
  text: string;
  depth: number;
}

export interface DocChapter {
  id: string;
  slug: string;
  canonicalPath: string;
  lang: 'zh' | 'en';
  track: string;
  trackTitle: string;
  trackOrder: number;
  chapterOrder: number;
  kicker: string;
  title: string;
  subtitle?: string;
  readingTime: number;
  sinceVersion: string;
  aliases?: string[];
  sections: DocSection[];
  toc: TocHeading[];
  githubSourceUrl: string;
}

export interface DocTrackGroup {
  track: string;
  title: string;
  order: number;
  chapters: DocChapter[];
}

const TRACK_CONFIG = {
  zh: {
    'getting-started': { title: '快速上手', order: 1, desc: '从下载安装、翻拍光源到第一次完整走通整卷流程' },
    'user-manual': { title: '工作台手册', order: 2, desc: '界面视图、底片范围、暗房印片光与母带级导出' },
    'color-science': { title: '色彩科学参考', order: 3, desc: '三大数学域推导、输入域解析与硬件校准白皮书' },
    'architecture': { title: '源码与架构', order: 4, desc: 'Tauri 2 进程模型、GPU/CPU 渲染等价与贡献指南' },
  },
  en: {
    'getting-started': { title: 'Getting Started', order: 1, desc: 'From installation and scanning setup to a complete first roll' },
    'user-manual': { title: 'User Manual', order: 2, desc: 'Interface, film area, printer lights & master exports' },
    'color-science': { title: 'Color Science', order: 3, desc: 'Three mathematical domains, unified pipeline & calibration' },
    'architecture': { title: 'Architecture', order: 4, desc: 'Tauri 2 runtime, GPU/CPU parity & contributing guidelines' },
  },
};

function getMarkedInstance() {
  return new Marked({
    gfm: true,
    breaks: false,
    renderer: {
      code(token) {
        const text = typeof token === 'string' ? token : (token && token.text) || '';
        const escaped = text
          .replace(/&/g, '&amp;')
          .replace(/</g, '&lt;')
          .replace(/>/g, '&gt;');
        return `<pre class="doc-code-block"><code>${escaped}</code></pre>\n`;
      },
      heading(token) {
        const match = token.text.match(/^(.*?)(?:\s+\{#([a-zA-Z0-9_-]+)\})?\s*$/);
        const cleanText = match ? match[1].trim() : token.text.trim();
        const id = match && match[2]
          ? match[2].trim()
          : cleanText.toLowerCase().replace(/[^\p{L}\p{N}_-]+/gu, '-').replace(/^-+|-+$/g, '') || 'heading';
        const renderedText = this.parser.parseInline(token.tokens).replace(/\s*\{#[a-zA-Z0-9_-]+\}\s*$/, '');
        return `<h${token.depth} id="${id}" class="scroll-mt-24">${renderedText}</h${token.depth}>\n`;
      },
    },
  });
}

function transformAlerts(text: string, lang: 'zh' | 'en'): string {
  const marked = getMarkedInstance();
  return text.replace(
    /^>[ \t]*\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*(.*?)\r?\n((?:>[ \t]*.*\r?\n?)*)/gim,
    (match, type, title, bodyLines) => {
      const isWarn = ['WARNING', 'IMPORTANT', 'CAUTION'].includes(type.toUpperCase());
      const defaultTitles = {
        NOTE: lang === 'zh' ? '注意' : 'Note',
        TIP: lang === 'zh' ? '提示' : 'Tip',
        IMPORTANT: lang === 'zh' ? '重要' : 'Important',
        WARNING: lang === 'zh' ? '警告' : 'Warning',
        CAUTION: lang === 'zh' ? '注意' : 'Caution',
      };
      const finalTitle = title.trim() || defaultTitles[type.toUpperCase() as keyof typeof defaultTitles] || type;
      const cleanBody = bodyLines
        .split(/\r?\n/)
        .map((line: string) => line.replace(/^>[ \t]?/, ''))
        .join('\n')
        .trim();

      const bodyHtml = (marked.parse(cleanBody) as string).trim();
      const warnClass = isWarn ? ' doc-callout-warn' : '';
      return `<div class="doc-callout${warnClass}">\n    <div class="doc-callout-title">${finalTitle}</div>\n    <div class="doc-callout-body">${bodyHtml}</div>\n</div>\n`;
    }
  );
}

function parseSectionsFromMarkdown(body: string, lang: 'zh' | 'en'): { sections: DocSection[]; toc: TocHeading[] } {
  const marked = getMarkedInstance();
  const rawSections = body.split(/^##\s+/m);
  const sections: DocSection[] = [];
  const toc: TocHeading[] = [];

  for (let i = 1; i < rawSections.length; i++) {
    const raw = rawSections[i];
    const firstNewline = raw.indexOf('\n');
    const headingLine = firstNewline === -1 ? raw : raw.slice(0, firstNewline);
    const sectionBody = firstNewline === -1 ? '' : raw.slice(firstNewline + 1);

    const match = headingLine.match(/^(.*?)(?:\s+\{#([a-zA-Z0-9_-]+)\})?\s*$/);
    const heading = match ? match[1].trim() : headingLine.trim();
    const id = match && match[2] ? match[2].trim() : heading.toLowerCase().replace(/[^a-z0-9_-]+/g, '-');

    toc.push({ id, text: heading, depth: 2 });

    // Look for H3 subsections for TOC
    const h3Matches = sectionBody.matchAll(/^###\s+(.*?)(?:\s+\{#([a-zA-Z0-9_-]+)\})?\s*$/gm);
    for (const h3 of h3Matches) {
      const h3Heading = h3[1].trim();
      const h3Id = h3[2]
        ? h3[2].trim()
        : h3Heading.toLowerCase().replace(/[^\p{L}\p{N}_-]+/gu, '-').replace(/^-+|-+$/g, '') || 'heading';
      toc.push({ id: h3Id, text: h3Heading, depth: 3 });
    }

    // Process alerts
    let processed = transformAlerts(sectionBody.trim(), lang);

    // Protect LaTeX formulas with placeholders before marked parsing
    const mathPlaceholders: string[] = [];
    processed = processed.replace(/\$\$([\s\S]+?)\$\$/g, (_, tex) => {
      mathPlaceholders.push(renderBlockMath(tex.trim()));
      return `%%MATH_PLACEHOLDER_${mathPlaceholders.length - 1}%%`;
    });
    processed = processed.replace(/(?<!\\|\$)\$([^\$\n]+?)\$(?!\$)/g, (_, tex) => {
      mathPlaceholders.push(renderInlineMath(tex.trim()));
      return `%%MATH_PLACEHOLDER_${mathPlaceholders.length - 1}%%`;
    });

    let html = (marked.parse(processed) as string).trim();

    // Restore rendered math placeholders
    html = html.replace(/%%MATH_PLACEHOLDER_(\d+)%%/g, (_, idx) => {
      return mathPlaceholders[Number(idx)];
    });

    // Ensure doc-table class
    html = html.replace(/<table(?!\s+class=)/g, '<table class="doc-table"');
    // Ensure doc-code-block class
    html = html.replace(/<pre(?!\s+class=)>/g, '<pre class="doc-code-block">');

    sections.push({ id, heading, content: html });
  }

  return { sections, toc };
}

export async function getAllChapters(lang: 'zh' | 'en'): Promise<DocChapter[]> {
  const allEntries = await getCollection('docs');
  const langPrefix = `${lang}/`;
  const filtered = allEntries.filter((entry) => entry.id.startsWith(langPrefix));

  const chapters: DocChapter[] = [];

  for (const entry of filtered) {
    const trackConfig = TRACK_CONFIG[lang][entry.data.track as keyof (typeof TRACK_CONFIG)['zh']];
    const trackTitle = trackConfig ? trackConfig.title : entry.data.track;

    const { sections, toc } = parseSectionsFromMarkdown(entry.body || '', lang);

    // Path inside docs/handbook/zh or docs/handbook/en
    const relativePath = entry.id.slice(langPrefix.length);
    const githubSourceUrl = `https://github.com/BillyDu-TJ/NexFilm/blob/main/docs/handbook/${lang}/${relativePath}.md`;

    chapters.push({
      id: entry.data.id,
      slug: entry.data.id,
      canonicalPath: `/${lang}/docs/${entry.data.id}/`,
      lang,
      track: entry.data.track,
      trackTitle,
      trackOrder: entry.data.trackOrder,
      chapterOrder: entry.data.chapterOrder,
      kicker: entry.data.kicker,
      title: entry.data.title,
      subtitle: entry.data.subtitle,
      readingTime: entry.data.readingTime,
      sinceVersion: entry.data.sinceVersion,
      aliases: entry.data.aliases,
      sections,
      toc,
      githubSourceUrl,
    });
  }

  return chapters.sort((a, b) => a.trackOrder - b.trackOrder || a.chapterOrder - b.chapterOrder);
}

export async function getTrackGroups(lang: 'zh' | 'en'): Promise<DocTrackGroup[]> {
  const chapters = await getAllChapters(lang);
  const groupsMap = new Map<string, DocTrackGroup>();

  for (const ch of chapters) {
    if (!groupsMap.has(ch.track)) {
      const config = TRACK_CONFIG[lang][ch.track as keyof (typeof TRACK_CONFIG)['zh']];
      groupsMap.set(ch.track, {
        track: ch.track,
        title: config ? config.title : ch.trackTitle,
        order: ch.trackOrder,
        chapters: [],
      });
    }
    groupsMap.get(ch.track)!.chapters.push(ch);
  }

  return Array.from(groupsMap.values()).sort((a, b) => a.order - b.order);
}

export async function getChapterBySlug(slug: string, lang: 'zh' | 'en'): Promise<DocChapter | null> {
  const chapters = await getAllChapters(lang);
  return chapters.find((ch) => ch.id === slug || (ch.aliases || []).includes(slug)) || null;
}

export function getPager(chapters: DocChapter[], currentId: string): { prev: DocChapter | null; next: DocChapter | null } {
  const index = chapters.findIndex((ch) => ch.id === currentId || (ch.aliases || []).includes(currentId));
  if (index === -1) return { prev: null, next: null };
  return {
    prev: index > 0 ? chapters[index - 1] : null,
    next: index < chapters.length - 1 ? chapters[index + 1] : null,
  };
}
