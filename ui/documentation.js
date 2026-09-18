// NexFilm documentation view.
//
// Responsibilities:
//   - render the chapter list, the on-this-page index, and the article body
//   - keep the URL hash in sync so a chapter or section can be linked to
//   - search across chapters and sections, jump to a hit, and highlight it
//
// The article is only re-rendered when the chapter or the language changes.
// Typing in the search box re-renders the navigation and the highlight layer,
// so the reader's scroll position and text selection survive.
(function (global) {
    'use strict';

    const DEFAULT_CHAPTER_ID = 'quick-start';

    let activeChapterId = DEFAULT_CHAPTER_ID;
    let activeSectionId = '';
    let searchQuery = '';
    let boundSearchInput = null;
    let boundScrollPanel = null;
    let scrollSpyFrame = 0;
    let highlightTimer = 0;

    function getLocale() {
        return (global.NexFilmI18n && typeof global.NexFilmI18n.getLocale === 'function')
            ? global.NexFilmI18n.getLocale()
            : 'zh-CN';
    }

    function t(key, params) {
        if (global.NexFilmI18n && typeof global.NexFilmI18n.t === 'function') {
            return global.NexFilmI18n.t(key, params);
        }
        return key;
    }

    function escapeHtml(value) {
        if (value == null) return '';
        return String(value)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#039;');
    }

    function escapeSelector(value) {
        if (global.CSS && typeof global.CSS.escape === 'function') return global.CSS.escape(value);
        return String(value).replace(/[^a-zA-Z0-9_-]/g, '\\\\$&');
    }

    function dataModule() {
        return global.NexFilmDocsData || null;
    }

    function chapters() {
        const module = dataModule();
        return module ? module.getChapters(getLocale()) : [];
    }

    function activeChapter() {
        const module = dataModule();
        return module ? module.getChapterById(activeChapterId, getLocale()) : null;
    }

    function chapterIndex(id) {
        return chapters().findIndex(chapter => chapter.id === id);
    }

    // Matching runs against visible text. Raw HTML would otherwise match tag
    // names and class attributes, which the reader cannot see.
    const plainTextCache = new Map();

    function plainText(html) {
        const cached = plainTextCache.get(html);
        if (cached !== undefined) return cached;
        const text = String(html)
            .replace(/<[^>]*>/g, ' ')
            .replace(/\s+/g, ' ')
            .trim()
            .toLowerCase();
        plainTextCache.set(html, text);
        return text;
    }

    function sectionMatches(section, query) {
        if (!query) return false;
        return (section.heading.toLowerCase() + ' ' + plainText(section.content)).includes(query);
    }

    // ---------------------------------------------------------------- routing

    function readHash() {
        const raw = String(global.location.hash || '');
        const match = raw.match(/^#doc\/([^/]+)(?:\/([^/]+))?$/);
        if (!match) return null;
        return {
            chapterId: decodeURIComponent(match[1]),
            sectionId: match[2] ? decodeURIComponent(match[2]) : ''
        };
    }

    function writeHash(replace) {
        const target = '#doc/' + encodeURIComponent(activeChapterId)
            + (activeSectionId ? '/' + encodeURIComponent(activeSectionId) : '');
        if (global.location.hash === target) return;
        if (replace && global.history && typeof global.history.replaceState === 'function') {
            global.history.replaceState(null, '', target);
        } else {
            global.location.hash = target;
        }
    }

    function syncFromHash() {
        const route = readHash();
        if (!route) return false;
        const module = dataModule();
        const chapter = module ? module.getChapterById(route.chapterId, getLocale()) : null;
        if (!chapter) return false;
        activeChapterId = chapter.id;
        activeSectionId = (route.sectionId
            && chapter.sections.some(section => section.id === route.sectionId))
            ? route.sectionId
            : '';
        return true;
    }

    // ------------------------------------------------------------- navigation

    function buildChapterNavItem(chapter, position) {
        const item = document.createElement('button');
        item.type = 'button';
        item.className = 'documentation-nav-item' + (chapter.id === activeChapterId ? ' is-active' : '');
        item.setAttribute('aria-current', chapter.id === activeChapterId ? 'true' : 'false');

        const index = document.createElement('span');
        index.className = 'documentation-nav-index';
        index.textContent = String(position).padStart(2, '0');

        const title = document.createElement('span');
        title.className = 'documentation-nav-title';
        title.textContent = chapter.title;

        const meta = document.createElement('span');
        meta.className = 'documentation-nav-meta';
        meta.textContent = t('documentation.readingTime', { count: chapter.readingTime });

        const row = document.createElement('span');
        row.className = 'documentation-nav-row';
        row.append(index, title);

        item.append(row, meta);
        item.addEventListener('click', () => selectChapter(chapter.id, ''));
        return item;
    }

    function buildResultNavItem(chapter, section) {
        const item = document.createElement('button');
        item.type = 'button';
        item.className = 'documentation-nav-item documentation-nav-result';

        const crumb = document.createElement('span');
        crumb.className = 'documentation-nav-crumb';
        crumb.textContent = chapter.title;

        const title = document.createElement('span');
        title.className = 'documentation-nav-title';
        title.textContent = section.heading;

        item.append(crumb, title);
        item.addEventListener('click', () => selectChapter(chapter.id, section.id));
        return item;
    }

    function renderNav() {
        const navList = document.getElementById('documentation-nav-list');
        const countBadge = document.getElementById('documentation-chapter-count');
        if (!navList) return;

        const list = chapters();
        navList.innerHTML = '';

        if (!searchQuery) {
            if (countBadge) countBadge.textContent = t('documentation.chapterCount', { count: list.length });
            list.forEach((chapter, index) => navList.appendChild(buildChapterNavItem(chapter, index + 1)));
            return;
        }

        const results = [];
        list.forEach(chapter => {
            chapter.sections.forEach(section => {
                if (sectionMatches(section, searchQuery)) results.push({ chapter, section });
            });
        });

        if (countBadge) countBadge.textContent = t('documentation.resultsCount', { count: results.length });

        if (results.length === 0) {
            const empty = document.createElement('div');
            empty.className = 'documentation-nav-empty';
            empty.textContent = t('documentation.searchEmpty');
            navList.appendChild(empty);
            return;
        }

        results.forEach(result => navList.appendChild(buildResultNavItem(result.chapter, result.section)));
    }

    // ------------------------------------------------------------------ article

    function buildTableOfContents(chapter) {
        const items = chapter.sections.map(section => `
            <button type="button" class="doc-toc-item" data-section-id="${escapeHtml(section.id)}">
                ${escapeHtml(section.heading)}
            </button>
        `).join('');
        return `
            <nav class="documentation-toc" aria-label="${escapeHtml(t('documentation.onThisPage'))}">
                <div class="documentation-toc-title">${escapeHtml(t('documentation.onThisPage'))}</div>
                ${items}
            </nav>
        `;
    }

    function buildSection(section) {
        return `
            <section class="documentation-article-section" id="doc-section-${escapeHtml(section.id)}">
                <h2 class="documentation-section-heading">
                    <a class="documentation-anchor" href="#doc/${encodeURIComponent(activeChapterId)}/${encodeURIComponent(section.id)}" aria-label="${escapeHtml(section.heading)}">#</a>
                    ${escapeHtml(section.heading)}
                </h2>
                <div class="documentation-section-body">${section.content}</div>
            </section>
        `;
    }

    function buildFooter(list, position) {
        const previous = position > 0 ? list[position - 1] : null;
        const next = position < list.length - 1 ? list[position + 1] : null;
        const previousButton = previous
            ? `<button type="button" class="doc-pager-button" data-chapter-id="${escapeHtml(previous.id)}">
                   <span class="doc-pager-label">${escapeHtml(t('documentation.previous'))}</span>
                   <span class="doc-pager-title">${escapeHtml(previous.title)}</span>
               </button>`
            : '<span></span>';
        const nextButton = next
            ? `<button type="button" class="doc-pager-button doc-pager-next" data-chapter-id="${escapeHtml(next.id)}">
                   <span class="doc-pager-label">${escapeHtml(t('documentation.next'))}</span>
                   <span class="doc-pager-title">${escapeHtml(next.title)}</span>
               </button>`
            : '<span></span>';
        return `
            <footer class="documentation-article-footer">
                ${previousButton}
                ${nextButton}
            </footer>
        `;
    }

    function renderArticle() {
        const container = document.getElementById('documentation-article-body');
        if (!container) return;

        const chapter = activeChapter();
        if (!chapter) {
            container.innerHTML = '';
            return;
        }

        const list = chapters();
        const position = Math.max(0, chapterIndex(chapter.id));
        const kicker = chapter.kicker || chapter.title;

        container.innerHTML = `
            <header class="documentation-article-header">
                <div class="documentation-article-kicker">${escapeHtml(kicker)}</div>
                <h1 class="documentation-article-title">${escapeHtml(chapter.title)}</h1>
                <p class="documentation-article-subtitle">${escapeHtml(chapter.subtitle || '')}</p>
                <div class="documentation-article-meta">${escapeHtml(t('documentation.readingTime', { count: chapter.readingTime }))}</div>
                ${buildTableOfContents(chapter)}
            </header>
            <div class="documentation-article-content">
                ${chapter.sections.map(buildSection).join('')}
            </div>
            ${buildFooter(list, position)}
        `;

        container.querySelectorAll('.doc-toc-item').forEach(button => {
            button.addEventListener('click', () => scrollToSection(button.getAttribute('data-section-id'), true));
        });
        container.querySelectorAll('.doc-pager-button').forEach(button => {
            button.addEventListener('click', () => selectChapter(button.getAttribute('data-chapter-id'), ''));
        });
        container.querySelectorAll('a.documentation-anchor').forEach(anchor => {
            anchor.addEventListener('click', event => {
                event.preventDefault();
                const section = anchor.closest('.documentation-article-section');
                if (section) scrollToSection(section.id.replace('doc-section-', ''), true);
            });
        });
        decorateCodeBlocks(container);
        applyHighlights();
        updateTocHighlight();
    }

    // ------------------------------------------------------------- copy buttons

    function decorateCodeBlocks(root) {
        root.querySelectorAll('.doc-code-block').forEach(block => {
            if (block.querySelector('.doc-copy-button')) return;
            const button = document.createElement('button');
            button.type = 'button';
            button.className = 'doc-copy-button';
            button.textContent = t('documentation.copy');
            button.addEventListener('click', async () => {
                const code = block.querySelector('code');
                const text = (code ? code.textContent : block.textContent) || '';
                const clipboard = global.navigator && global.navigator.clipboard;
                if (!clipboard || typeof clipboard.writeText !== 'function') return;
                try {
                    await clipboard.writeText(text.trim());
                    button.textContent = t('documentation.copied');
                    global.setTimeout(() => { button.textContent = t('documentation.copy'); }, 1500);
                } catch (error) {
                    console.error('Could not copy documentation text', error);
                }
            });
            block.appendChild(button);
        });
    }

    // ------------------------------------------------------------- highlighting

    function clearHighlights(root) {
        root.querySelectorAll('mark.doc-mark').forEach(mark => {
            const parent = mark.parentNode;
            if (!parent) return;
            parent.replaceChild(document.createTextNode(mark.textContent), mark);
            parent.normalize();
        });
    }

    function highlightTextNode(node, query) {
        const text = node.nodeValue || '';
        const lower = text.toLowerCase();
        if (!query || lower.indexOf(query) === -1) return;
        const fragment = document.createDocumentFragment();
        let cursor = 0;
        let index = lower.indexOf(query);
        while (index !== -1) {
            if (index > cursor) fragment.appendChild(document.createTextNode(text.slice(cursor, index)));
            const mark = document.createElement('mark');
            mark.className = 'doc-mark';
            mark.textContent = text.slice(index, index + query.length);
            fragment.appendChild(mark);
            cursor = index + query.length;
            index = lower.indexOf(query, cursor);
        }
        if (cursor < text.length) fragment.appendChild(document.createTextNode(text.slice(cursor)));
        node.parentNode.replaceChild(fragment, node);
    }

    function applyHighlights() {
        const root = document.getElementById('documentation-article-body');
        if (!root) return;
        clearHighlights(root);
        if (!searchQuery) return;

        const bodies = Array.from(root.querySelectorAll('.documentation-section-body'));
        if (bodies.length === 0) return;

        const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
            acceptNode: node => {
                if (!node.nodeValue || !node.nodeValue.trim()) return NodeFilter.FILTER_REJECT;
                if (node.parentElement && node.parentElement.closest('.doc-copy-button')) {
                    return NodeFilter.FILTER_REJECT;
                }
                return NodeFilter.FILTER_ACCEPT;
            }
        });

        const targets = [];
        while (walker.nextNode()) {
            const node = walker.currentNode;
            if (bodies.some(body => body.contains(node))) targets.push(node);
        }
        targets.forEach(node => highlightTextNode(node, searchQuery));
    }

    // ------------------------------------------------------------------- scroll

    function contentPanel() {
        return document.getElementById('documentation-content-scroll');
    }

    function scrollToSection(sectionId, updateHash) {
        const panel = contentPanel();
        if (!panel || !sectionId) return;
        const target = panel.querySelector('#doc-section-' + escapeSelector(sectionId));
        if (!target) return;
        activeSectionId = sectionId;
        const panelRect = panel.getBoundingClientRect();
        const targetRect = target.getBoundingClientRect();
        const top = panel.scrollTop + (targetRect.top - panelRect.top) - 12;
        panel.scrollTo({ top: Math.max(0, top), behavior: 'smooth' });
        updateTocHighlight();
        if (updateHash) writeHash(true);
    }

    function updateTocHighlight() {
        const container = document.getElementById('documentation-article-body');
        const panel = contentPanel();
        if (!container || !panel) return;

        const sectionElements = Array.from(container.querySelectorAll('.documentation-article-section'));
        if (sectionElements.length === 0) return;

        let currentId = sectionElements[0].id.replace('doc-section-', '');
        if (activeSectionId && sectionElements.some(el => el.id === 'doc-section-' + activeSectionId)) {
            currentId = activeSectionId;
        }
        container.querySelectorAll('.doc-toc-item').forEach(item => {
            item.classList.toggle('is-active', item.getAttribute('data-section-id') === currentId);
        });
    }

    function syncSectionFromScroll() {
        scrollSpyFrame = 0;
        const container = document.getElementById('documentation-article-body');
        const panel = contentPanel();
        if (!container || !panel) return;

        const panelTop = panel.getBoundingClientRect().top;
        let currentId = '';
        container.querySelectorAll('.documentation-article-section').forEach(section => {
            if (section.getBoundingClientRect().top - panelTop <= 24) {
                currentId = section.id.replace('doc-section-', '');
            }
        });
        if (!currentId || currentId === activeSectionId) return;
        activeSectionId = currentId;
        updateTocHighlight();
        writeHash(true);
    }

    function requestScrollSpy() {
        if (scrollSpyFrame) return;
        scrollSpyFrame = global.requestAnimationFrame(syncSectionFromScroll);
    }

    // -------------------------------------------------------------- interactions

    function selectChapter(chapterId, sectionId) {
        activeChapterId = chapterId;
        activeSectionId = sectionId || '';
        renderNav();
        renderArticle();
        writeHash(true);
        const panel = contentPanel();
        if (!panel) return;
        if (sectionId) {
            panel.scrollTop = 0;
            global.requestAnimationFrame(() => scrollToSection(sectionId, true));
        } else {
            panel.scrollTo({ top: 0, behavior: 'smooth' });
        }
    }

    function onSearchInput(value) {
        searchQuery = String(value || '').trim().toLowerCase();
        renderNav();
        if (highlightTimer) global.clearTimeout(highlightTimer);
        highlightTimer = global.setTimeout(() => {
            highlightTimer = 0;
            applyHighlights();
        }, 120);
    }

    function bindSearchInput() {
        const input = document.getElementById('documentation-search-input');
        if (!input || input === boundSearchInput) return;
        boundSearchInput = input;
        input.addEventListener('input', event => onSearchInput(event.target.value));
        input.addEventListener('keydown', event => {
            if (event.key === 'Escape') {
                input.value = '';
                onSearchInput('');
            } else if (event.key === 'Enter') {
                const first = document.querySelector('#documentation-nav-list .documentation-nav-item');
                if (first) first.click();
            }
        });
    }

    function bindScrollPanel() {
        const panel = contentPanel();
        if (!panel || panel === boundScrollPanel) return;
        boundScrollPanel = panel;
        panel.addEventListener('scroll', requestScrollSpy, { passive: true });
    }

    function render() {
        const module = dataModule();
        if (!module) return;

        if (!syncFromHash()) {
            if (!module.getChapterById(activeChapterId, getLocale())) {
                const fallback = module.getFallbackChapter(getLocale());
                activeChapterId = fallback ? fallback.id : DEFAULT_CHAPTER_ID;
                activeSectionId = '';
            }
        }

        const title = document.getElementById('documentation-view-title');
        if (title) title.textContent = t('documentation.title');
        const subtitle = document.getElementById('documentation-view-subtitle');
        if (subtitle) subtitle.textContent = t('documentation.subtitle');

        renderNav();
        renderArticle();
        bindSearchInput();
        bindScrollPanel();
    }

    global.addEventListener('hashchange', () => {
        if (!syncFromHash()) return;
        renderNav();
        renderArticle();
        if (!activeSectionId) return;
        const panel = contentPanel();
        if (panel) panel.scrollTop = 0;
        global.requestAnimationFrame(() => scrollToSection(activeSectionId, false));
    });

    global.NexFilmDocumentation = {
        render: render,
        setActiveChapter: function (id) {
            selectChapter(id, '');
        }
    };
})(window);
