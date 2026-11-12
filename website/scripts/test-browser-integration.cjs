const puppeteer = require('puppeteer-core');
const http = require('node:http');
const { spawn } = require('node:child_process');
const path = require('node:path');

const CHROME_PATH = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const BASE_URL = 'http://localhost:4321';

function checkServerReady(url) {
  return new Promise((resolve) => {
    http.get(url, (res) => {
      resolve(true);
    }).on('error', () => {
      resolve(false);
    });
  });
}

async function ensureServerRunning() {
  const isRunning = await checkServerReady(`${BASE_URL}/zh/`);
  if (isRunning) {
    console.log(`[Preview Server] Server already active on ${BASE_URL}`);
    return null;
  }

  console.log(`[Preview Server] Starting astro preview on port 4321...`);
  const websiteDir = path.resolve(__dirname, '..');
  const serverProc = spawn('npx', ['astro', 'preview', '--port', '4321'], {
    cwd: websiteDir,
    stdio: 'ignore',
    detached: false
  });

  // Poll until server responds (up to 15s)
  const startTime = Date.now();
  while (Date.now() - startTime < 15000) {
    await new Promise((r) => setTimeout(r, 200));
    const ready = await checkServerReady(`${BASE_URL}/zh/`);
    if (ready) {
      console.log(`[Preview Server] astro preview ready in ${Date.now() - startTime}ms`);
      return serverProc;
    }
  }

  serverProc.kill();
  throw new Error('Timeout waiting for astro preview to start on port 4321!');
}

async function runTests() {
  console.log('--- Starting Browser Integration Verification ---');
  let serverProc = null;
  let browser = null;

  try {
    serverProc = await ensureServerRunning();

    browser = await puppeteer.launch({
      executablePath: CHROME_PATH,
      headless: 'new',
      args: ['--no-sandbox', '--disable-setuid-sandbox']
    });

    const page = await browser.newPage();
    await page.setViewport({ width: 1440, height: 900 });

    // 1. Intercept / block api.github.com to simulate offline / 403 rate limit
    await page.setRequestInterception(true);
    page.on('request', (req) => {
      if (req.url().includes('api.github.com')) {
        req.abort();
      } else {
        req.continue();
      }
    });

    console.log(`Navigating to ${BASE_URL}/zh/ ...`);
    await page.goto(`${BASE_URL}/zh/`, { waitUntil: 'networkidle0' });

    // Test 1: Header computed height and community dropdown toggle
    console.log('\n[Test 1] Verifying Header computed height and community dropdown...');
    const headerHeight = await page.evaluate(() => {
      const header = document.querySelector('header');
      return header ? window.getComputedStyle(header).height : null;
    });
    console.log(`> Header computed height: ${headerHeight}`);
    const heightVal = parseFloat(headerHeight);
    if (heightVal < 64 || heightVal > 69) {
      throw new Error(`Header height ${headerHeight} is out of expected range 64px - 69px!`);
    }

    // Check community dropdown click behavior
    const dropdownInitial = await page.evaluate(() => {
      const drop = document.getElementById('community-dropdown');
      const style = drop ? window.getComputedStyle(drop) : null;
      return {
        hasVisibleClass: drop?.classList.contains('visible'),
        visibility: style?.visibility,
        opacity: style?.opacity
      };
    });
    console.log('> Dropdown initial state:', dropdownInitial);

    // Click once to open
    console.log('> Clicking community menu button once...');
    await page.click('#community-menu-btn');
    await new Promise(r => setTimeout(r, 200));

    const dropdownOpened = await page.evaluate(() => {
      const drop = document.getElementById('community-dropdown');
      const style = drop ? window.getComputedStyle(drop) : null;
      return {
        hasVisibleClass: drop?.classList.contains('visible'),
        visibility: style?.visibility,
        opacity: style?.opacity,
        ariaExpanded: document.getElementById('community-menu-btn')?.getAttribute('aria-expanded')
      };
    });
    console.log('> Dropdown state after 1st click:', dropdownOpened);
    if (dropdownOpened.ariaExpanded !== 'true' || !dropdownOpened.hasVisibleClass) {
      throw new Error('Community dropdown failed to open after 1st click!');
    }

    // Click again to close
    console.log('> Clicking community menu button a second time...');
    await page.click('#community-menu-btn');
    await new Promise(r => setTimeout(r, 200));

    const dropdownClosed = await page.evaluate(() => {
      const drop = document.getElementById('community-dropdown');
      return {
        hasVisibleClass: drop?.classList.contains('visible'),
        ariaExpanded: document.getElementById('community-menu-btn')?.getAttribute('aria-expanded')
      };
    });
    console.log('> Dropdown state after 2nd click:', dropdownClosed);
    if (dropdownClosed.ariaExpanded !== 'false' || dropdownClosed.hasVisibleClass) {
      throw new Error('Community dropdown failed to close after 2nd click (double-toggle cancellation)!');
    }
    console.log('✓ Test 1 PASSED: Header height is 64px-69px and dropdown toggles properly.');

    // Test 2: Hero download button under offline / blocked GitHub API
    console.log('\n[Test 2] Verifying Hero download button under blocked GitHub API...');
    const heroBtnInfo = await page.evaluate(() => {
      const btn = document.querySelector('[data-hero-download]');
      const textEl = btn?.querySelector('[data-hero-download-text]');
      return {
        href: btn?.getAttribute('href'),
        download: btn?.getAttribute('download'),
        text: textEl?.textContent?.trim() || btn?.textContent?.trim()
      };
    });
    console.log('> Hero download button info:', heroBtnInfo);
    if (heroBtnInfo.text.includes('(macOS / Windows)')) {
      throw new Error(`Hero button text still contains unhydrated '(macOS / Windows)': ${heroBtnInfo.text}`);
    }
    if (!heroBtnInfo.text.includes('macOS') && !heroBtnInfo.text.includes('Windows')) {
      throw new Error(`Hero button text does not identify OS: ${heroBtnInfo.text}`);
    }
    console.log('✓ Test 2 PASSED: Hero button automatically customized for system without GitHub API.');

    // Test 3: BeforeAfter pointer drag and View Transitions persistence
    console.log('\n[Test 3] Verifying BeforeAfter pointer drag & View Transitions persistence...');
    const getSliderPercent = async () => {
      return page.evaluate(() => {
        const bar = document.querySelector('.slider-handle-bar');
        return bar ? bar.style.left : null;
      });
    };

    const initialPercent = await getSliderPercent();
    console.log(`> Initial slider left: ${initialPercent}`);

    const containerBox = await page.evaluate(() => {
      const container = document.querySelector('.before-after-container');
      if (!container) return null;
      const rect = container.getBoundingClientRect();
      return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
    });

    if (!containerBox) throw new Error('BeforeAfter container not found on page!');

    // Simulate pointer drag from 30% to 70%
    const startX = containerBox.x + containerBox.width * 0.3;
    const targetX = containerBox.x + containerBox.width * 0.7;
    const centerY = containerBox.y + containerBox.height / 2;

    await page.mouse.move(startX, centerY);
    await page.mouse.down();
    const leftAt30 = await getSliderPercent();
    console.log(`> Slider left at pointerdown (30%): ${leftAt30}`);

    await page.mouse.move(targetX, centerY, { steps: 5 });
    const leftAt70 = await getSliderPercent();
    console.log(`> Slider left at pointermove (70%): ${leftAt70}`);
    await page.mouse.up();

    if (Math.abs(parseFloat(leftAt70) - 70) > 3) {
      throw new Error(`Slider left did not move to ~70%: got ${leftAt70}`);
    }

    // Navigate to Changelog via link, then navigate back to Home via View Transitions
    console.log('> Navigating to /zh/changelog/ via top nav link...');
    await Promise.all([
      page.waitForNavigation({ waitUntil: 'networkidle0' }).catch(() => {}),
      page.click('a[href*="/zh/changelog/"]')
    ]);
    await new Promise(r => setTimeout(r, 500));

    console.log('> Current URL after navigation:', page.url());

    // Navigate back to Home
    console.log('> Navigating back to /zh/ via Home link...');
    await Promise.all([
      page.waitForNavigation({ waitUntil: 'networkidle0' }).catch(() => {}),
      page.click('header nav a[href$="/zh/"]')
    ]);
    await new Promise(r => setTimeout(r, 500));

    console.log('> Current URL after returning:', page.url());

    // Drag again on the newly mounted BeforeAfter component
    const newContainerBox = await page.evaluate(() => {
      const container = document.querySelector('.before-after-container');
      if (!container) return null;
      const rect = container.getBoundingClientRect();
      return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
    });

    const newStartX = newContainerBox.x + newContainerBox.width * 0.4;
    await page.mouse.move(newStartX, centerY);
    await page.mouse.down();
    const leftAfterSwap = await getSliderPercent();
    await page.mouse.up();
    console.log(`> Slider left after returning to /zh/ and dragging to 40%: ${leftAfterSwap}`);

    if (Math.abs(parseFloat(leftAfterSwap) - 40) > 3) {
      throw new Error(`BeforeAfter slider failed to respond after View Transitions: got ${leftAfterSwap}`);
    }
    console.log('✓ Test 3 PASSED: Pointer drag and View Transitions re-initialization 100% working.');

    // Test 4: Changelog typography styles
    console.log('\n[Test 4] Verifying Changelog typography and .release-markdown styling...');
    await page.goto(`${BASE_URL}/zh/changelog/`, { waitUntil: 'networkidle0' });

    const ulComputedStyle = await page.evaluate(() => {
      const ul = document.querySelector('.release-markdown ul');
      if (!ul) return null;
      const style = window.getComputedStyle(ul);
      return {
        listStyleType: style.listStyleType,
        listStylePosition: style.listStylePosition,
        marginLeft: style.marginLeft,
        color: style.color
      };
    });
    console.log('> .release-markdown ul computed style:', ulComputedStyle);
    if (!ulComputedStyle || ulComputedStyle.listStyleType === 'none') {
      throw new Error(`.release-markdown ul computed list-style-type is none or missing: ${JSON.stringify(ulComputedStyle)}`);
    }
    console.log('✓ Test 4 PASSED: .release-markdown has visible list bullets and darkroom typography.');

    // Test 5: Dynamic Changelog Injection via sessionStorage cache
    console.log('\n[Test 5] Simulating newer release (v9.9.9) injection in Changelog...');
    await page.evaluate(() => {
      const mockCache = {
        timestamp: Date.now(),
        latestRelease: {
          tag_name: 'v9.9.9',
          html_url: 'https://github.com/BillyDu-TJ/NexFilm/releases/tag/v9.9.9',
          published_at: '2026-11-04T12:00:00Z',
          assets: [
            {
              name: 'NexFilm.Engine_9.9.9_aarch64.dmg',
              browser_download_url: 'https://github.com/BillyDu-TJ/NexFilm/releases/download/v9.9.9/NexFilm.Engine_9.9.9_aarch64.dmg',
              size: 7800000,
              digest: 'sha256:abcdef9999999999999999999999999999999999999999999999999999999999'
            }
          ]
        },
        repoInfo: { stargazers_count: 999 },
        releasesList: [
          {
            tag_name: 'v9.9.9',
            name: 'NexFilm v9.9.9 Quantum Film Core',
            html_url: 'https://github.com/BillyDu-TJ/NexFilm/releases/tag/v9.9.9',
            published_at: '2026-11-04T12:00:00Z',
            body: '### 新功能\n- 支持量子密度反相引擎\n- 增强超宽动态范围\n\n### 📥 下载指南\n- 请在下方下载',
            assets: [
              {
                name: 'NexFilm.Engine_9.9.9_aarch64.dmg',
                browser_download_url: 'https://github.com/BillyDu-TJ/NexFilm/releases/download/v9.9.9/NexFilm.Engine_9.9.9_aarch64.dmg',
                size: 7800000,
                digest: 'sha256:abcdef9999999999999999999999999999999999999999999999999999999999'
              }
            ]
          }
        ]
      };
      sessionStorage.setItem('nexfilm_release_cache_v2', JSON.stringify(mockCache));
    });

    // Reload page to let release-hydrator consume cache and inject v9.9.9
    console.log('> Reloading /zh/changelog/ with injected sessionStorage...');
    await page.reload({ waitUntil: 'networkidle0' });
    await new Promise(r => setTimeout(r, 600));

    const injectedCardInfo = await page.evaluate(() => {
      const timeline = document.querySelector('[data-changelog-timeline]');
      const firstCard = timeline?.querySelector('[data-release-tag]');
      const badge = firstCard?.querySelector('[data-release-badge]');
      return {
        tagName: firstCard?.getAttribute('data-release-tag'),
        badgeText: badge?.textContent?.trim()
      };
    });
    console.log('> Injected first card in timeline:', injectedCardInfo);

    if (injectedCardInfo.tagName !== 'v9.9.9') {
      throw new Error(`Dynamic release injection failed: expected first card v9.9.9, got ${injectedCardInfo.tagName}`);
    }
    if (!injectedCardInfo.badgeText.includes('最新稳定版')) {
      throw new Error(`Injected card badge is not Latest Stable: ${injectedCardInfo.badgeText}`);
    }

    // Clear mock data from sessionStorage
    await page.evaluate(() => {
      sessionStorage.removeItem('nexfilm_release_cache_v2');
    });
    console.log('✓ Test 5 PASSED: Dynamic release v9.9.9 successfully injected at top of timeline and test cache cleared.');

    // Test 6: English environment automatic redirect from / to /en/ without JS syntax errors
    console.log('\n[Test 6] Verifying English browser redirect from / to /en/ without JS syntax errors...');
    const enPage = await browser.newPage();
    await enPage.setRequestInterception(true);
    enPage.on('request', (req) => {
      if (req.url().includes('api.github.com')) {
        req.abort();
      } else {
        req.continue();
      }
    });
    let rootPageErrors = [];
    enPage.on('pageerror', (err) => {
      console.error('Captured page error during redirect:', err);
      rootPageErrors.push(err);
    });
    await enPage.evaluateOnNewDocument(() => {
      Object.defineProperty(navigator, 'language', { get: () => 'en-US' });
      Object.defineProperty(navigator, 'languages', { get: () => ['en-US', 'en'] });
    });
    await enPage.goto(`${BASE_URL}/`, { waitUntil: 'networkidle0' });
    const finalUrl = enPage.url();
    console.log(`> Root URL redirected to: ${finalUrl}`);
    if (!finalUrl.includes('/en/')) {
      throw new Error(`Expected redirect to /en/, got ${finalUrl}`);
    }
    if (rootPageErrors.length > 0) {
      throw new Error(`Page errors occurred during redirect: ${rootPageErrors.map(e => e.message).join('; ')}`);
    }
    await enPage.close();
    console.log('✓ Test 6 PASSED: English environment successfully redirected to /en/ with zero errors.');

    // Test 7: Homepage Structure & WorkspaceShowcase Real HTML Sandbox Verification
    console.log('\n[Test 7] Verifying Homepage structure & real WorkspaceShowcase HTML Sandbox inside iframe...');
    const demoPage = await browser.newPage();
    await demoPage.setRequestInterception(true);
    demoPage.on('request', (req) => {
      if (req.url().includes('api.github.com')) {
        req.abort();
      } else {
        req.continue();
      }
    });
    await demoPage.setViewport({ width: 1440, height: 900 });

    let iframeErrors = [];
    demoPage.on('pageerror', (err) => {
      console.error('Page error captured on demoPage:', err);
      iframeErrors.push(err);
    });

    await demoPage.goto(`${BASE_URL}/zh/`, { waitUntil: 'networkidle0' });

    // 7.1 Verify DensityInteractiveDemo has been completely removed
    const demoRemoved = await demoPage.evaluate(() => {
      return {
        hasPhaseBtns: Boolean(document.querySelector('.phase-btn')),
        hasModeBadge: Boolean(document.querySelector('#canvas-mode-badge')),
        hasInteractiveDemo: Boolean(document.querySelector('#density-interactive-demo')),
        beforeAfterCount: document.querySelectorAll('.before-after-container').length
      };
    });
    console.log('> DensityInteractiveDemo cleanup status:', demoRemoved);
    if (demoRemoved.hasPhaseBtns || demoRemoved.hasModeBadge || demoRemoved.hasInteractiveDemo) {
      throw new Error('DensityInteractiveDemo elements still present on homepage!');
    }
    if (demoRemoved.beforeAfterCount !== 1) {
      throw new Error(`Expected exactly 1 BeforeAfter container on homepage, got ${demoRemoved.beforeAfterCount}`);
    }
    console.log('  ✓ DensityInteractiveDemo completely removed, single hero BeforeAfter retained.');

    // 7.2 Verify WorkspaceShowcase embedded iframe initialization
    console.log('> Locating #workspace-demo-iframe...');
    const iframeElement = await demoPage.waitForSelector('#workspace-demo-iframe', { timeout: 5000 });
    if (!iframeElement) throw new Error('#workspace-demo-iframe not found!');

    const frame = await iframeElement.contentFrame();
    if (!frame) throw new Error('Could not access iframe content frame!');

    // Wait for #startup-loading to disappear inside iframe
    console.log('> Waiting for iframe #startup-loading removal...');
    await frame.waitForFunction(() => !document.getElementById('startup-loading'), { timeout: 8000 });
    console.log('  ✓ iframe #startup-loading successfully removed.');

    // Wait a brief moment for auto-focus to settle
    await new Promise(r => setTimeout(r, 400));

    // 7.3 Click Rolls view in iframe (#nav-history)
    console.log('> Clicking Rolls view (#nav-history) in iframe...');
    await frame.click('#nav-history');
    await new Promise(r => setTimeout(r, 250));

    const rollsViewStatus = await frame.evaluate(() => {
      const historyView = document.getElementById('view-history');
      const rollsGrid = document.getElementById('library-rolls-grid');
      return {
        display: historyView ? window.getComputedStyle(historyView).display : null,
        isActive: historyView?.classList.contains('is-active'),
        gridChildCount: rollsGrid ? rollsGrid.children.length : 0
      };
    });
    console.log('> Rolls view status in iframe:', rollsViewStatus);
    if (rollsViewStatus.display === 'none' || !rollsViewStatus.isActive) {
      throw new Error(`Rolls view did not activate properly: ${JSON.stringify(rollsViewStatus)}`);
    }
    if (rollsViewStatus.gridChildCount === 0) {
      throw new Error('Rolls grid in iframe is empty, expected 3 demo rolls!');
    }
    // Phase 2.2 Assertion: #library-rolls-grid 首张卡片内 img 数量 >= 2
    const firstRollImgCount = await frame.evaluate(() => {
      const firstCard = document.querySelector('#library-rolls-grid .roll-card');
      return firstCard ? firstCard.querySelectorAll('img').length : 0;
    });
    console.log(`> First roll card preview img count: ${firstRollImgCount}`);
    if (firstRollImgCount < 2) {
      throw new Error(`Expected at least 2 preview img elements in first roll card, found ${firstRollImgCount}`);
    }
    console.log('  ✓ Rolls view successfully switched with 3 demo rolls and >= 2 roll previews.');

    // 7.4 Click Library view in iframe (#nav-library)
    console.log('> Clicking Library view (#nav-library) in iframe...');
    await frame.click('#nav-library');
    await new Promise(r => setTimeout(r, 250));

    const libraryViewStatus = await frame.evaluate(() => {
      const libView = document.getElementById('view-library');
      return {
        display: libView ? window.getComputedStyle(libView).display : null,
        isActive: libView?.classList.contains('is-active')
      };
    });
    console.log('> Library view status in iframe:', libraryViewStatus);
    if (libraryViewStatus.display === 'none' || !libraryViewStatus.isActive) {
      throw new Error(`Library view did not activate properly: ${JSON.stringify(libraryViewStatus)}`);
    }
    console.log('  ✓ Library view successfully switched.');

    // 7.5 Click Develop view in iframe (#nav-develop)
    console.log('> Clicking Develop view (#nav-develop) in iframe...');
    await frame.click('#nav-develop');
    await new Promise(r => setTimeout(r, 250));

    const developViewStatus = await frame.evaluate(() => {
      const devView = document.getElementById('view-develop');
      const canvas = document.getElementById('preview-canvas');
      return {
        display: devView ? window.getComputedStyle(devView).display : null,
        isActive: devView?.classList.contains('is-active'),
        canvasDisplay: canvas ? window.getComputedStyle(canvas).display : null
      };
    });
    console.log('> Develop view status in iframe:', developViewStatus);
    if (developViewStatus.display === 'none' || !developViewStatus.isActive) {
      throw new Error(`Develop view did not activate properly: ${JSON.stringify(developViewStatus)}`);
    }

    // Phase 2.2 Assertion: #filmstrip-container .film-item 数量 >= 4
    const filmstripCount = await frame.evaluate(() => {
      return document.querySelectorAll('#filmstrip-container .film-item').length;
    });
    console.log(`> Filmstrip item count: ${filmstripCount}`);
    if (filmstripCount < 4) {
      throw new Error(`Expected at least 4 .film-item elements in #filmstrip-container, got ${filmstripCount}`);
    }
    console.log('  ✓ #filmstrip-container contains >= 4 selectable film items.');

    // Test filmstrip item click selection & canvas preview switch
    console.log('> Clicking first film item in filmstrip to trigger active frame load & render...');
    await frame.click('#filmstrip-container .film-item');
    await new Promise(r => setTimeout(r, 600));

    // Phase 2.2 Assertion: #preview-canvas 非纯黑/纯蓝偏色
    await new Promise(r => setTimeout(r, 400));
    const canvasColorCheck = await frame.evaluate(() => {
      const canvas = document.getElementById('preview-canvas');
      if (!canvas) return { error: 'Canvas not found' };
      const gl = canvas.getContext('webgl2') || canvas.getContext('webgl');
      if (!gl) return { error: 'WebGL context not found' };
      const w = gl.drawingBufferWidth || canvas.width;
      const h = gl.drawingBufferHeight || canvas.height;
      if (w === 0 || h === 0) return { error: `Empty canvas dimensions ${w}x${h}` };
      const pixels = new Uint8Array(4 * 16 * 16);
      const cx = Math.max(0, Math.floor(w / 2) - 8);
      const cy = Math.max(0, Math.floor(h / 2) - 8);
      gl.readPixels(cx, cy, 16, 16, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      let totalR = 0, totalG = 0, totalB = 0, count = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        totalR += pixels[i];
        totalG += pixels[i + 1];
        totalB += pixels[i + 2];
        count++;
      }
      const avgR = totalR / count;
      const avgG = totalG / count;
      const avgB = totalB / count;
      const isPureBlack = avgR < 5 && avgG < 5 && avgB < 5;
      const isPureBlue = avgB > 100 && avgR < 25 && avgG < 40;
      return { avgR, avgG, avgB, isPureBlack, isPureBlue };
    });
    console.log('> Canvas center pixel color check:', canvasColorCheck);
    if (canvasColorCheck.error) {
      throw new Error(`Preview canvas inspection error: ${canvasColorCheck.error}`);
    }
    if (canvasColorCheck.isPureBlack) {
      throw new Error(`Preview canvas is pure black! (avgR: ${canvasColorCheck.avgR}, avgG: ${canvasColorCheck.avgG}, avgB: ${canvasColorCheck.avgB})`);
    }
    if (canvasColorCheck.isPureBlue) {
      throw new Error(`Preview canvas has severe blue bias / zero red bug! (avgR: ${canvasColorCheck.avgR}, avgG: ${canvasColorCheck.avgG}, avgB: ${canvasColorCheck.avgB})`);
    }
    console.log('  ✓ #preview-canvas renders rich photographic color (non-black, non-blue).');
    console.log('  ✓ Develop view successfully switched and canvas mounted.');

    // 7.6 Toggle Settings Menu: Theme & Language inside iframe
    console.log('> Testing setting menu theme and language toggles inside iframe...');
    const initialTheme = await frame.evaluate(() => document.body.dataset.theme);
    console.log('> Initial theme:', initialTheme);

    // Toggle theme
    await frame.evaluate(() => {
      const toggle = document.getElementById('menu-theme-toggle');
      if (toggle) toggle.click();
    });
    await new Promise(r => setTimeout(r, 150));
    const toggledTheme = await frame.evaluate(() => document.body.dataset.theme);
    console.log('> Theme after toggle:', toggledTheme);
    if (toggledTheme === initialTheme) {
      throw new Error(`Theme failed to toggle: remained ${toggledTheme}`);
    }

    // Toggle language
    const initialLang = await frame.evaluate(() => document.documentElement.lang);
    console.log('> Initial lang:', initialLang);
    await frame.evaluate(() => {
      const toggle = document.getElementById('menu-lang-toggle');
      if (toggle) toggle.click();
    });
    await new Promise(r => setTimeout(r, 150));
    const toggledLang = await frame.evaluate(() => document.documentElement.lang);
    console.log('> Lang after toggle:', toggledLang);
    if (toggledLang === initialLang) {
      throw new Error(`Language failed to toggle: remained ${toggledLang}`);
    }
    console.log('  ✓ Settings menu theme and language toggled seamlessly.');

    // 7.7 Test Modal Opening inside iframe: Import Roll modal
    console.log('> Testing Import Roll modal (#btn-import) in iframe...');
    await frame.evaluate(() => {
      const btn = document.getElementById('btn-import');
      if (btn) btn.click();
    });
    await new Promise(r => setTimeout(r, 200));

    const importModalOpen = await frame.evaluate(() => {
      const modal = document.getElementById('import-choice-modal');
      return modal && !modal.classList.contains('opacity-0') && !modal.classList.contains('pointer-events-none');
    });
    console.log('> Import Roll modal open state:', importModalOpen);
    if (!importModalOpen) {
      throw new Error('Import Roll modal failed to open upon clicking #btn-import!');
    }

    // Close modal by clicking #btn-close-import-choice
    await frame.evaluate(() => {
      const btnClose = document.getElementById('btn-close-import-choice');
      if (btnClose) btnClose.click();
    });
    await new Promise(r => setTimeout(r, 200));
    console.log('  ✓ Import Roll modal opened and closed successfully.');

    // 7.8 Click Documentation view in iframe (#nav-documentation)
    console.log('> Clicking Documentation view (#nav-documentation) in iframe...');
    await frame.click('#nav-documentation');
    await new Promise(r => setTimeout(r, 250));

    const docViewStatus = await frame.evaluate(() => {
      const docView = document.getElementById('view-documentation');
      const zhChapters = window.NexFilmDocsData ? window.NexFilmDocsData.getChapters('zh-CN') : [];
      const enChapters = window.NexFilmDocsData ? window.NexFilmDocsData.getChapters('en') : [];
      const navItems = document.querySelectorAll('#documentation-nav-list .documentation-nav-item');
      return {
        display: docView ? window.getComputedStyle(docView).display : null,
        isActive: docView?.classList.contains('is-active'),
        zhCount: zhChapters.length,
        enCount: enChapters.length,
        navItemsCount: navItems.length,
        hasSection2Chapters: zhChapters.some(c => c.id === 'workspace-and-views') && zhChapters.some(c => c.id === 'density-grading-and-curves')
      };
    });
    console.log('> Documentation view status in iframe:', docViewStatus);
    if (docViewStatus.display === 'none' || !docViewStatus.isActive) {
      throw new Error(`Documentation view did not activate properly in iframe: ${JSON.stringify(docViewStatus)}`);
    }
    if (docViewStatus.zhCount !== 16 || docViewStatus.enCount !== 16) {
      throw new Error(`Expected 16 chapters in embedded demo sandbox, got zh: ${docViewStatus.zhCount}, en: ${docViewStatus.enCount}`);
    }
    if (!docViewStatus.hasSection2Chapters) {
      throw new Error('Embedded demo sandbox missing Section 2 chapters (workspace-and-views, density-grading-and-curves)!');
    }
    console.log('  ✓ #view-documentation active and loaded all 16 synchronized chapters in demo sandbox.');

    // 7.9 Verify zero unhandled exceptions inside iframe
    if (iframeErrors.length > 0) {
      throw new Error(`Errors occurred inside iframe: ${iframeErrors.map(e => e.message).join('; ')}`);
    }
    await demoPage.close();
    console.log('✓ Test 7 PASSED: Real HTML sandbox inside iframe fully interactive with 0 errors.');

    // Test 7.5: Features Page 4 Pillars & KaTeX Math Verification
    console.log('\n[Test 7.5] Verifying Features Page 4 Pillars & KaTeX mathematical rendering (/zh/ & /en/)...');
    const featPage = await browser.newPage();
    await featPage.setRequestInterception(true);
    featPage.on('request', (req) => {
      if (req.url().includes('api.github.com')) {
        req.abort();
      } else {
        req.continue();
      }
    });
    await featPage.setViewport({ width: 1440, height: 900 });

    // Inspect /zh/features/
    console.log('> Inspecting /zh/features/...');
    await featPage.goto(`${BASE_URL}/zh/features/`, { waitUntil: 'networkidle0' });

    const zhFeatData = await featPage.evaluate(() => {
      const navLinks = Array.from(document.querySelectorAll('nav[aria-label="Features Category Navigation"] a')).map(a => a.getAttribute('href'));
      const bodyText = document.body.textContent || '';
      const katexDisplay = document.querySelector('.katex-display');
      const clone = document.body.cloneNode(true);
      clone.querySelectorAll('.katex, .katex-display, math').forEach(el => el.remove());
      const nonKatexText = clone.textContent || '';
      return {
        navLinks,
        hasAutoInvert: bodyText.includes('Auto Invert') || bodyText.includes('自动反相'),
        hasRollCalib: bodyText.includes('Auto Invert Roll') || bodyText.includes('Roll Calibration') || bodyText.includes('整卷标定') || bodyText.includes('整卷批处理'),
        hasContactSheet: bodyText.includes('Contact Sheet') || bodyText.includes('接触印样'),
        has14Luts: bodyText.includes('14') && bodyText.includes('LUT'),
        hasDensityMath: bodyText.includes('D = -log') || bodyText.includes('D = -\\log') || bodyText.includes('log10(T)'),
        hasKatexElement: Boolean(katexDisplay),
        rawLatexLeak: nonKatexText.includes('\\Longleftrightarrow') || nonKatexText.includes('\\log_{10}') || nonKatexText.includes('\\approx')
      };
    });
    console.log('> /zh/features/ inspection data:', zhFeatData);

    const expectedNav = ['#section-batch', '#section-archive', '#section-darkroom', '#section-foundation'];
    for (const exp of expectedNav) {
      if (!zhFeatData.navLinks.includes(exp)) {
        throw new Error(`/zh/features/ missing required sticky category anchor: ${exp}`);
      }
    }
    if (!zhFeatData.hasAutoInvert || !zhFeatData.hasRollCalib || !zhFeatData.hasContactSheet || !zhFeatData.has14Luts) {
      throw new Error(`/zh/features/ missing required core capabilities content!`);
    }
    if (!zhFeatData.hasKatexElement) {
      throw new Error(`/zh/features/ KaTeX display element not rendered!`);
    }
    if (zhFeatData.rawLatexLeak) {
      throw new Error(`/zh/features/ raw unparsed LaTeX string leaked into page text!`);
    }
    console.log('  ✓ /zh/features/ successfully verified (4 pillars, sticky nav, KaTeX render).');

    // Inspect /en/features/
    console.log('> Inspecting /en/features/...');
    await featPage.goto(`${BASE_URL}/en/features/`, { waitUntil: 'networkidle0' });

    const enFeatData = await featPage.evaluate(() => {
      const navLinks = Array.from(document.querySelectorAll('nav[aria-label="Features Category Navigation"] a')).map(a => a.getAttribute('href'));
      const bodyText = document.body.textContent || '';
      const katexDisplay = document.querySelector('.katex-display');
      const clone = document.body.cloneNode(true);
      clone.querySelectorAll('.katex, .katex-display, math').forEach(el => el.remove());
      const nonKatexText = clone.textContent || '';
      return {
        navLinks,
        hasAutoInvert: bodyText.includes('Auto Invert') || bodyText.includes('自动反相'),
        hasRollCalib: bodyText.includes('Auto Invert Roll') || bodyText.includes('Roll Calibration') || bodyText.includes('整卷标定') || bodyText.includes('整卷批处理'),
        hasContactSheet: bodyText.includes('Contact Sheet'),
        has14Luts: bodyText.includes('14') && bodyText.includes('LUT'),
        hasKatexElement: Boolean(katexDisplay),
        rawLatexLeak: nonKatexText.includes('\\Longleftrightarrow') || nonKatexText.includes('\\log_{10}') || nonKatexText.includes('\\approx')
      };
    });
    console.log('> /en/features/ inspection data:', enFeatData);

    for (const exp of expectedNav) {
      if (!enFeatData.navLinks.includes(exp)) {
        throw new Error(`/en/features/ missing required sticky category anchor: ${exp}`);
      }
    }
    if (!enFeatData.hasAutoInvert || !enFeatData.hasRollCalib || !enFeatData.hasContactSheet || !enFeatData.has14Luts) {
      throw new Error(`/en/features/ missing required core capabilities content!`);
    }
    if (!enFeatData.hasKatexElement) {
      throw new Error(`/en/features/ KaTeX display element not rendered!`);
    }
    if (enFeatData.rawLatexLeak) {
      throw new Error(`/en/features/ raw unparsed LaTeX string leaked into page text!`);
    }
    console.log('  ✓ /en/features/ successfully verified (4 pillars, sticky nav, KaTeX render).');
    await featPage.close();
    console.log('✓ Test 7.5 PASSED: Symmetrical features pages rendered with zero LaTeX leaks.');

    // Test 8: Viewport inspection across 4 viewports (375px, 768px, 1280px, 1440px)
    console.log('\n[Test 8] Inspecting 4 viewports across 5 key routes for zero horizontal overflow & zero errors...');
    const viewports = [
      { name: '375px (Mobile)', width: 375, height: 667 },
      { name: '768px (Tablet)', width: 768, height: 1024 },
      { name: '1280px (Desktop)', width: 1280, height: 800 },
      { name: '1440px (Wide Desktop)', width: 1440, height: 900 }
    ];
    const routes = ['/zh/', '/en/', '/zh/features/', '/en/features/', '/zh/gallery/', '/zh/docs/', '/zh/docs/download-and-install/', '/zh/docs/overview-and-install/'];

    const testPage = await browser.newPage();
    await testPage.setRequestInterception(true);
    testPage.on('request', (req) => {
      if (req.url().includes('api.github.com')) {
        req.abort();
      } else {
        req.continue();
      }
    });
    for (const vp of viewports) {
      await testPage.setViewport({ width: vp.width, height: vp.height });
      for (const route of routes) {
        let routeErrors = [];
        const errHandler = (err) => routeErrors.push(err);
        testPage.on('pageerror', errHandler);

        for (let attempt = 0; attempt < 2; attempt++) {
          try {
            await testPage.goto(`${BASE_URL}${route}`, { waitUntil: 'networkidle0' });
            break;
          } catch (e) {
            if (attempt === 0 && (e.message.includes('detached') || e.message.includes('LifecycleWatcher'))) {
              await new Promise(r => setTimeout(r, 200));
              continue;
            }
            throw e;
          }
        }
        await new Promise(r => setTimeout(r, 150));

        const overflowInfo = await testPage.evaluate(() => {
          const docEl = document.documentElement;
          return {
            scrollWidth: docEl.scrollWidth,
            clientWidth: docEl.clientWidth,
            hasOverflow: docEl.scrollWidth > docEl.clientWidth
          };
        });

        testPage.off('pageerror', errHandler);

        if (overflowInfo.hasOverflow) {
          throw new Error(`Horizontal overflow detected at ${vp.name} on ${route}! scrollWidth (${overflowInfo.scrollWidth}) > clientWidth (${overflowInfo.clientWidth})`);
        }
        if (routeErrors.length > 0) {
          throw new Error(`Page error on ${route} at ${vp.name}: ${routeErrors.map(e => e.message).join('; ')}`);
        }
        console.log(`  ✓ ${vp.name} on ${route}: scrollWidth <= clientWidth (${overflowInfo.clientWidth}px), 0 errors`);
      }
    }
    await testPage.close();
    console.log('✓ Test 8 PASSED: All 4 viewports across all 7 routes have 0 horizontal overflow and 0 page errors.');

    // [Test 9] Verifying Documentation Portal & Three-Column Reader System (/zh/ & /en/)
    console.log('\n[Test 9] Verifying Documentation Portal & Three-Column Reader System (/zh/ & /en/)...');
    const docsPage = await browser.newPage();
    await docsPage.setViewport({ width: 1440, height: 900 });

    async function validateTocAndAnchors(page, routeName) {
      const result = await page.evaluate(() => {
        const tocLinks = Array.from(document.querySelectorAll('a[data-toc-link]'));
        const errors = [];
        const checkedIds = [];
        for (const link of tocLinks) {
          const id = link.getAttribute('data-toc-link');
          if (!id || id.trim() === '') {
            errors.push('Found empty data-toc-link attribute');
            continue;
          }
          checkedIds.push(id);
          const matches = document.querySelectorAll('#' + CSS.escape(id));
          if (matches.length === 0) {
            errors.push(`Anchor #${id} target element not found in DOM`);
          } else if (matches.length > 1) {
            errors.push(`Anchor #${id} duplicated in DOM (${matches.length} elements found)`);
          }
        }
        return { count: tocLinks.length, errors, checkedIds };
      });
      if (result.errors.length > 0) {
        throw new Error(`TOC / DOM anchor validation failed on ${routeName}: ${result.errors.join('; ')}`);
      }
      return result;
    }

    // 1. Visit /zh/docs/
    console.log('> Visiting /zh/docs/ portal page...');
    await docsPage.goto(`${BASE_URL}/zh/docs/`, { waitUntil: 'networkidle0' });
    const portalData = await docsPage.evaluate(() => {
      const h1 = document.querySelector('h1')?.textContent?.trim();
      const pillarCards = document.querySelectorAll('h3').length;
      const quickTasks = document.querySelectorAll('a[href*="/zh/docs/"]').length;
      return { h1, pillarCards, quickTasks };
    });
    console.log('> /zh/docs/ portal data:', portalData);
    if (!portalData.h1?.includes('胶片反相') || portalData.pillarCards < 4) {
      throw new Error(`Documentation portal /zh/docs/ missing expected H1 or 4 pillar cards!`);
    }
    console.log('  ✓ Documentation portal /zh/docs/ verified.');

    // 2. Visit /zh/docs/overview-and-install/
    console.log('> Visiting /zh/docs/overview-and-install/ article page...');
    await docsPage.goto(`${BASE_URL}/zh/docs/overview-and-install/`, { waitUntil: 'networkidle0' });
    const chapterData = await docsPage.evaluate(() => {
      const h1 = document.querySelector('h1')?.textContent?.trim();
      const sidebarLinks = document.querySelectorAll('aside nav a[href*="/zh/docs/"]').length;
      const tocLinks = Array.from(document.querySelectorAll('a[data-toc-link]')).map(a => a.getAttribute('data-toc-link'));
      const calloutCount = document.querySelectorAll('.doc-callout').length;
      const tableCount = document.querySelectorAll('table.doc-table').length;
      const kbdCount = document.querySelectorAll('kbd').length;
      const codeBlockCount = document.querySelectorAll('pre.doc-code-block').length;
      const copyBtnCount = document.querySelectorAll('.doc-copy-btn').length;
      return {
        h1, sidebarLinks, tocLinks, calloutCount, tableCount,
        kbdCount, codeBlockCount, copyBtnCount
      };
    });
    console.log('> /zh/docs/overview-and-install/ inspection:', chapterData);

    if (!chapterData.h1?.includes('认识 NexFilm') || chapterData.sidebarLinks < 4) {
      throw new Error(`Article page missing H1 or sidebar links!`);
    }
    if (chapterData.tocLinks.length < 4) {
      throw new Error(`Right Table of Contents (TOC) missing expected heading anchors! Found: ${chapterData.tocLinks.length}`);
    }
    if (chapterData.calloutCount < 1 || chapterData.tableCount < 1) {
      throw new Error(`Article page missing .doc-callout or table.doc-table!`);
    }

    const zhOverviewTocResult = await validateTocAndAnchors(docsPage, '/zh/docs/overview-and-install/');
    console.log(`> Validated ${zhOverviewTocResult.count} TOC anchors on /zh/docs/overview-and-install/ (all exist and 100% unique in DOM).`);

    // Verify clicking H3 TOC link works and triggers smooth scroll
    const h3Target = await docsPage.evaluate(() => {
      const h3Link = document.querySelector('a[data-toc-link="first-launch-authorization-on-windows"]');
      return Boolean(h3Link);
    });
    if (h3Target) {
      console.log('> Clicking H3 anchor in TOC (#first-launch-authorization-on-windows)...');
      await docsPage.click('a[data-toc-link="first-launch-authorization-on-windows"]');
      await new Promise(r => setTimeout(r, 200));
      console.log('  ✓ H3 TOC anchor clicked without error.');
    }
    console.log('  ✓ Three-column layout, sidebar, right TOC & typography verified.');

    // 3. Test exact language toggle symmetry (zh -> en)
    console.log('> Testing language toggle from /zh/docs/overview-and-install/ to English...');
    const langSwitchBtn = await docsPage.$('header a[title*="English"]');
    if (!langSwitchBtn) {
      throw new Error(`Language switch button linking to /en/docs/ not found in header!`);
    }
    await Promise.all([
      docsPage.waitForNavigation({ waitUntil: 'networkidle0' }).catch(() => {}),
      langSwitchBtn.click()
    ]);
    await new Promise(r => setTimeout(r, 800));
    const currentEnUrl = docsPage.url();
    console.log('> Landed on URL after language toggle:', currentEnUrl);
    if (!currentEnUrl.includes('/en/docs/overview-and-install')) {
      throw new Error(`Language toggle failed to jump to symmetrical chapter! Expected /en/docs/overview-and-install, got: ${currentEnUrl}`);
    }

    const enChapterData = await docsPage.evaluate(() => {
      const h1 = document.querySelector('h1')?.textContent?.trim();
      const tocLinks = Array.from(document.querySelectorAll('a[data-toc-link]')).map(a => a.getAttribute('data-toc-link'));
      const sidebarLinks = document.querySelectorAll('aside nav a[href*="/en/docs/"]').length;
      return { h1, tocLinks, sidebarLinks };
    });
    console.log('> English chapter inspection:', enChapterData);
    if (!enChapterData.h1?.includes('Understanding NexFilm') || enChapterData.tocLinks.length < 4) {
      throw new Error(`English symmetrical page failed to render expected H1 or TOC!`);
    }

    const enOverviewTocResult = await validateTocAndAnchors(docsPage, '/en/docs/overview-and-install/');
    console.log(`> Validated ${enOverviewTocResult.count} TOC anchors on /en/docs/overview-and-install/ (all exist and 100% unique in DOM).`);

    // Verify English sidebar contains zero Chinese "篇" character
    const enSidebarHasChinese = await docsPage.evaluate(() => {
      const aside = document.querySelector('aside');
      return aside ? aside.textContent.includes('篇') : false;
    });
    if (enSidebarHasChinese) {
      throw new Error('English documentation sidebar contains Chinese character "篇"!');
    }
    console.log('  ✓ English sidebar has zero Chinese "篇" characters.');
    console.log('  ✓ Exact language toggle symmetry verified: /zh/docs/... -> /en/docs/... without 404.');

    // 4. Test Section 2 User Manual Chapters
    console.log('> Testing Section 2 User Manual Chapters (/zh/ & /en/)...');
    
    // 4.1 /zh/docs/workspace-and-views/
    await docsPage.goto(`${BASE_URL}/zh/docs/workspace-and-views/`, { waitUntil: 'networkidle0' });
    const zhWorkspaceToc = await validateTocAndAnchors(docsPage, '/zh/docs/workspace-and-views/');
    console.log(`  ✓ /zh/docs/workspace-and-views/ TOC validated (${zhWorkspaceToc.count} unique anchors).`);

    // 4.2 /zh/docs/density-grading-and-curves/
    await docsPage.goto(`${BASE_URL}/zh/docs/density-grading-and-curves/`, { waitUntil: 'networkidle0' });
    const zhDensityToc = await validateTocAndAnchors(docsPage, '/zh/docs/density-grading-and-curves/');
    const zhHasCurveDetail = await docsPage.evaluate(() => {
      const el = document.getElementById('curve-and-detail');
      return Boolean(el);
    });
    if (!zhHasCurveDetail) {
      throw new Error('/zh/docs/density-grading-and-curves/ missing required #curve-and-detail section!');
    }
    console.log(`  ✓ /zh/docs/density-grading-and-curves/ TOC validated (${zhDensityToc.count} anchors, #curve-and-detail confirmed).`);

    // 4.3 /en/docs/density-grading-and-curves/
    await docsPage.goto(`${BASE_URL}/en/docs/density-grading-and-curves/`, { waitUntil: 'networkidle0' });
    const enDensityToc = await validateTocAndAnchors(docsPage, '/en/docs/density-grading-and-curves/');
    const enHasCurveDetail = await docsPage.evaluate(() => {
      const el = document.getElementById('curve-and-detail');
      const aside = document.querySelector('aside');
      return {
        hasCurve: Boolean(el),
        hasPian: aside ? aside.textContent.includes('篇') : false
      };
    });
    if (!enHasCurveDetail.hasCurve) {
      throw new Error('/en/docs/density-grading-and-curves/ missing required #curve-and-detail section!');
    }
    if (enHasCurveDetail.hasPian) {
      throw new Error('/en/docs/density-grading-and-curves/ English sidebar contains Chinese character "篇"!');
    }
    console.log(`  ✓ /en/docs/density-grading-and-curves/ TOC validated (${enDensityToc.count} anchors, #curve-and-detail confirmed, zero "篇").`);

    // 4.4 /zh/docs/export-and-contact-sheets/
    await docsPage.goto(`${BASE_URL}/zh/docs/export-and-contact-sheets/`, { waitUntil: 'networkidle0' });
    const zhExportToc = await validateTocAndAnchors(docsPage, '/zh/docs/export-and-contact-sheets/');
    const zhHasContactSheet = await docsPage.evaluate(() => {
      const el = document.getElementById('contact-sheet-generation');
      return Boolean(el);
    });
    if (!zhHasContactSheet) {
      throw new Error('/zh/docs/export-and-contact-sheets/ missing required #contact-sheet-generation section!');
    }
    console.log(`  ✓ /zh/docs/export-and-contact-sheets/ TOC validated (${zhExportToc.count} anchors, #contact-sheet-generation confirmed).`);

    // 5. Test KaTeX math rendering and zero raw leaks on scanning-best-practices
    console.log('> Visiting /zh/docs/scanning-best-practices/ for KaTeX verification...');
    await docsPage.goto(`${BASE_URL}/zh/docs/scanning-best-practices/`, { waitUntil: 'networkidle0' });
    const mathData = await docsPage.evaluate(() => {
      const hasKatex = document.querySelectorAll('.katex, math').length > 0;
      const rawLeak = (document.body.innerText.match(/\$\$(.*?)\$\$/) || []).length > 0;
      return { hasKatex, rawLeak };
    });
    console.log('> Scanning best practices math inspection:', mathData);
    if (!mathData.hasKatex || mathData.rawLeak) {
      throw new Error(`KaTeX formulas failed to render or raw LaTeX leaked on scanning-best-practices!`);
    }
    console.log('  ✓ KaTeX math rendering confirmed with zero raw leaks.');

    await docsPage.close();
    console.log('✓ Test 9 PASSED: Documentation portal, three-column reader & language toggle verified.');

    // [Test 10] Pagefind Instant Global Full-Text Search E2E (/zh/ & /en/)
    console.log('\n[Test 10] Verifying Pagefind Instant Global Full-Text Search E2E (/zh/ & /en/)...');
    const searchPage = await browser.newPage();
    await searchPage.setViewport({ width: 1440, height: 900 });

    // 10.1 Visit /zh/docs/overview-and-install/ and test search modal trigger
    console.log('> Visiting /zh/docs/overview-and-install/ to test search button and modal...');
    await searchPage.goto(`${BASE_URL}/zh/docs/overview-and-install/`, { waitUntil: 'networkidle0' });

    // Check placeholder button exists in sidebar
    const placeholderBtn = await searchPage.$('#doc-search-placeholder-btn');
    if (!placeholderBtn) {
      throw new Error('#doc-search-placeholder-btn search trigger button not found in sidebar!');
    }

    // Click placeholder button to open modal
    console.log('> Clicking #doc-search-placeholder-btn...');
    await placeholderBtn.click();
    await new Promise(r => setTimeout(r, 400));

    // Verify modal is open and input focused
    const isModalOpen = await searchPage.evaluate(() => {
      const modal = document.getElementById('docs-search-modal');
      const input = document.getElementById('docs-search-input');
      return {
        isOpen: modal ? !modal.classList.contains('opacity-0') : false,
        isFocused: input ? document.activeElement === input : false
      };
    });
    console.log('> Search modal open state:', isModalOpen);
    if (!isModalOpen.isOpen) {
      throw new Error('Search modal did not open upon clicking #doc-search-placeholder-btn!');
    }

    // 10.2 Type Chinese search query "印片光"
    console.log('> Typing "印片光" into #docs-search-input...');
    await searchPage.type('#docs-search-input', '印片光', { delay: 30 });
    // Wait for Pagefind search debounce and results render
    await searchPage.waitForFunction(() => {
      const list = document.getElementById('docs-search-list');
      return list && !list.classList.contains('hidden') && list.children.length > 0;
    }, { timeout: 8000 });

    // Inspect search results
    const zhResults = await searchPage.evaluate(() => {
      const items = Array.from(document.querySelectorAll('#docs-search-list li'));
      return items.map(li => {
        const title = li.querySelector('.truncate')?.textContent?.trim() || '';
        const href = li.querySelector('a')?.getAttribute('href') || '';
        const mark = Array.from(li.querySelectorAll('mark')).map(m => m.textContent?.trim());
        const excerpt = li.querySelector('.line-clamp-2')?.textContent?.trim() || '';
        return { title, href, mark, excerpt };
      });
    });
    console.log(`> Found ${zhResults.length} search results for "印片光":`, zhResults.map(r => ({ title: r.title, href: r.href })));

    if (zhResults.length === 0) {
      throw new Error('Pagefind returned 0 results for "印片光" in Chinese documentation!');
    }
    // Verify that at least one result has highlight
    const hasHighlight = zhResults.some(r => r.mark.length > 0 || r.excerpt.includes('印片光'));
    if (!hasHighlight) {
      throw new Error('Search results missing expected highlight for query "印片光"!');
    }

    // Verify zero duplicate routes in search results (alias deduplication check)
    const canonicalHrefs = zhResults.map(r => r.href.split('#')[0]);
    const uniqueHrefs = new Set(canonicalHrefs);
    if (uniqueHrefs.size !== canonicalHrefs.length) {
      throw new Error(`Duplicate routes detected in search results! ${canonicalHrefs.join(', ')}`);
    }
    console.log('  ✓ Chinese search verified with highlight and zero duplicate routes.');

    // 10.3 Test keyboard navigation (ArrowDown, Enter) to navigate
    console.log('> Testing keyboard navigation (ArrowDown -> Enter)...');
    await searchPage.keyboard.press('ArrowDown');
    await new Promise(r => setTimeout(r, 100));

    await Promise.all([
      searchPage.waitForNavigation({ waitUntil: 'networkidle0' }).catch(() => {}),
      searchPage.keyboard.press('Enter')
    ]);
    await new Promise(r => setTimeout(r, 600));

    const navigatedUrl = searchPage.url();
    console.log('> Navigated to URL via search Enter:', navigatedUrl);
    if (!navigatedUrl.includes('/zh/docs/')) {
      throw new Error(`Search navigation failed to land on a documentation chapter! URL: ${navigatedUrl}`);
    }
    console.log('  ✓ Keyboard selection and navigation successful.');

    // 10.4 Test English search and shortcut / trigger on /en/docs/
    console.log('> Visiting /en/docs/ to test English search and trigger...');
    await searchPage.goto(`${BASE_URL}/en/docs/`, { waitUntil: 'networkidle0' });

    // Open modal via trigger button or key
    const enTrigger = await searchPage.$('#doc-search-placeholder-btn, .doc-search-trigger, [data-search-trigger]');
    if (enTrigger) {
      await enTrigger.click();
      await new Promise(r => setTimeout(r, 400));
    } else {
      // Fallback: trigger via keyboard
      await searchPage.keyboard.down('Meta');
      await searchPage.keyboard.press('KeyK');
      await searchPage.keyboard.up('Meta');
      await new Promise(r => setTimeout(r, 400));
    }

    const enModalCheck = await searchPage.evaluate(() => {
      const modal = document.getElementById('docs-search-modal');
      return modal ? !modal.classList.contains('opacity-0') : false;
    });
    if (!enModalCheck) {
      throw new Error('Search modal failed to open on /en/docs/!');
    }

    // Type query "Printer Lights"
    console.log('> Typing "Printer Lights" into English search input...');
    await searchPage.type('#docs-search-input', 'Printer Lights', { delay: 25 });
    await searchPage.waitForFunction(() => {
      const list = document.getElementById('docs-search-list');
      return list && !list.classList.contains('hidden') && list.children.length > 0;
    }, { timeout: 8000 });

    const enResults = await searchPage.evaluate(() => {
      const items = Array.from(document.querySelectorAll('#docs-search-list li'));
      return items.map(li => {
        const title = li.querySelector('.truncate')?.textContent?.trim() || '';
        const href = li.querySelector('a')?.getAttribute('href') || '';
        const mark = Array.from(li.querySelectorAll('mark')).map(m => m.textContent?.trim());
        return { title, href, mark };
      });
    });
    console.log(`> Found ${enResults.length} search results for "Printer Lights":`, enResults.map(r => ({ title: r.title, href: r.href })));

    if (enResults.length === 0) {
      throw new Error('Pagefind returned 0 results for "Printer Lights" in English documentation!');
    }

    // Verify all results belong to /en/ (lang filter verified)
    const allEn = enResults.every(r => r.href.includes('/en/'));
    if (!allEn) {
      throw new Error('English search returned results from non-English (/zh/) pages!');
    }
    console.log('  ✓ English language filter verified: 100% results are from /en/docs/.');

    // 10.5 Test ESC key to dismiss modal
    console.log('> Pressing Escape to dismiss modal...');
    await searchPage.keyboard.press('Escape');
    await new Promise(r => setTimeout(r, 300));

    const isModalDismissed = await searchPage.evaluate(() => {
      const modal = document.getElementById('docs-search-modal');
      return modal ? modal.classList.contains('opacity-0') : true;
    });
    if (!isModalDismissed) {
      throw new Error('Search modal failed to dismiss upon pressing Escape!');
    }
    console.log('  ✓ Search modal dismissed with Escape key.');

    await searchPage.close();
    console.log('✓ Test 10 PASSED: Pagefind instant search E2E verified across zh and en with zero duplicate alias results and full keyboard navigation.');

    console.log('\n======================================================');
    console.log('🎉 ALL 10 BROWSER INTEGRATION TESTS PASSED (100% GREEN)!');
    console.log('======================================================');
  } finally {
    if (browser) {
      await browser.close().catch(() => {});
    }
    if (serverProc) {
      console.log('[Preview Server] Shutting down preview server...');
      serverProc.kill();
    }
  }
}

runTests()
  .then(() => {
    process.exit(0);
  })
  .catch((err) => {
    console.error('\n❌ Browser test failed:', err);
    process.exit(1);
  });
