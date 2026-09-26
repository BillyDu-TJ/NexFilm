// Boots ui/index.html with a minimal Tauri stub in headless Chrome and checks
// the Develop Detail UI: the collapsible sections, the tone curve editor, the
// Detail controls and the WebGL pass chain behind them. The unit contracts live
// in ui/tone-curve.test.js and src/core_math.rs; this one catches wiring that
// only breaks in a browser.
const assert = require('node:assert/strict');
const childProcess = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const root = path.join(__dirname, '..', 'ui');

const stub = `<script>
window.__uiErrors = [];
window.addEventListener('error', event => window.__uiErrors.push(
    String(event.message || event.error) + '@' + (event.filename || '') + ':' + event.lineno
));
window.addEventListener('unhandledrejection', event => window.__uiErrors.push('rejection: ' + String(event.reason)));

// A synthetic preview proxy in the transport format the WebGL path expects:
// 28-byte header plus 16-bit RGBA pixels. The preview needs a real frame for the
// Detail chain, the curve table and the scope readback to do any work.
function buildProxyBuffer() {
    const width = 320;
    const height = 214;
    const header = 28;
    const buffer = new ArrayBuffer(header + width * height * 4 * 2);
    const view = new DataView(buffer);
    view.setUint32(0, width, true);
    view.setUint32(4, height, true);
    view.setFloat32(8, 0.35, true);
    view.setFloat32(12, 0.35, true);
    view.setFloat32(16, 0.35, true);
    view.setUint32(20, 1, true);
    view.setUint32(24, 3, true);
    const pixels = new Uint16Array(buffer, header, width * height * 4);
    for (let y = 0; y < height; y += 1) {
        for (let x = 0; x < width; x += 1) {
            const index = (y * width + x) * 4;
            const base = 0.35 + 0.45 * (x / width) + 0.1 * (y / height);
            // The Detail chain needs real detail: a flat gradient would leave
            // every sharpening signal, and every mask, flat by definition.
            const blocks = ((x % 48) < 24 ? 0.5 : 0) + ((y % 40) < 20 ? 0.5 : 0);
            const modulation = 1 + blocks;
            pixels[index] = Math.round((base * 0.14 + 0.01) * modulation * 65535);
            pixels[index + 1] = Math.round((base * 0.155 + 0.008) * modulation * 65535);
            pixels[index + 2] = Math.round((base * 0.13 + 0.012) * modulation * 65535);
            pixels[index + 3] = 65535;
        }
    }
    return buffer;
}

const proxyBuffer = buildProxyBuffer();
const realFetch = window.fetch ? window.fetch.bind(window) : null;
window.fetch = async (input, init) => {
    if (String(input).startsWith('nexfilm-proxy:')) return new Response(proxyBuffer, { status: 200 });
    if (realFetch) return realFetch(input, init);
    throw new Error('fetch unavailable');
};

const previewParams = {
    film_mode: 'Color', d_min: [0.1, 0.1, 0.1], d_max: [2, 2, 2],
    exposure: 0, gamma: 1, contrast: 0.1, saturation: 0, temperature: 0, tint: 0,
    exp_r: 0, exp_g: 0, exp_b: 0, highlights: 0, shadows: 0,
    lut_path: null, lut_opacity: 1, working_colorspace: 'linear-srgb',
    sprocker_uv: null, sprocket_uv: null, sprocket_tolerance: 0.1, sprocket_feather: 0.05
};
const previewGeom = {
    crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0,
    flip_h: false, flip_v: false, rotate_90_count: 0,
    calibration_points: null, calibration_confirmed: true
};
const previewPipeline = {
    contract: 'legacy_v1',
    processing_report: { render_route: 'LegacyDirect', base_source: 'detected_film_base' },
    density_anchors: {},
    render_mapping: { mode: 'preserve_tone', density_low: [0.1, 0.1, 0.1], density_high: [2, 2, 2] }
};
const previewThumb = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="214"><rect width="320" height="214" fill="#50666a"/></svg>'
);
const previewItems = [{
    id: 'a-1', roll_id: 'roll-a', file_path: 'mock/a-1.tif',
    thumbnail_base64: previewThumb, embedded_thumbnail_base64: previewThumb,
    rendered_thumbnail_base64: null, thumbnail_kind: 'embedded',
    state_available: true, file_missing: false,
    base_analyzed: true, developed: true, pipeline_state: previewPipeline
}];
const previewRolls = [{
    roll_id: 'roll-a', date: '2026-07-12', format: '135', film_stock: 'Kodak Gold 200',
    camera: 'Nikon F3', notes: '', image_paths: ['mock/a-1.tif'], density_anchors: {}
}];
const clonePreview = value => JSON.parse(JSON.stringify(value));
window.__TAURI__ = {
    core: {
        convertFileSrc: (filePath, protocol) => protocol + '://' + filePath,
        invoke: async (command) => {
            if (command === 'get_rolls') return clonePreview(previewRolls);
            if (command === 'get_filmstrip') return clonePreview(previewItems);
            if (command === 'get_roll_filmstrip') return clonePreview(previewItems);
            if (command === 'get_calibration_profiles') return [];
            if (command === 'get_scanner_profiles') return [];
            if (command === 'get_builtin_luts') return [];
            if (command === 'get_user_cameras' || command === 'get_user_films') return [];
            if (command === 'get_roll_previews') return [previewThumb];
            if (command === 'switch_active_image') {
                return {
                    params: clonePreview(previewParams), geom: clonePreview(previewGeom),
                    base_analyzed: true, developed: true,
                    pipeline_state: clonePreview(previewPipeline)
                };
            }
            if (command === 'prepare_proxy') return true;
            if (command === 'get_embedded_preview' || command === 'get_density_calibration_preview') {
                return previewThumb;
            }
            return null;
        }
    },
    event: { listen: async () => () => {} }
};
</script>`;

const probe = `<script>
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
setTimeout(() => {
    const results = window.__probeResults || (window.__probeResults = {});
    results.errors = window.__uiErrors || [];
    if (window.__uiShotMode) {
        // Only for the optional screenshot: the inspector is inside the Develop
        // view, and errors here must not fail the contract check.
        try { switchView('develop'); } catch (error) { results.shotError = String(error); }
        try {
            const anchor = window.__uiShotAnchor;
            if (anchor) inspectorControls.scrollTop = document.getElementById(anchor).offsetTop - 16;
        } catch (error) { results.shotScrollError = String(error); }
    }
    try {
        results.localeLabels = (() => {
            const previous = window.NexFilmI18n.getLocale();
            window.NexFilmI18n.setLocale('zh-CN');
            const labels = [
                document.querySelector('#inspector-curve h3 span[data-i18n]')?.textContent,
                document.querySelector('#inspector-detail h3 span[data-i18n]')?.textContent,
            ];
            window.NexFilmI18n.setLocale(previous);
            return labels;
        })();
        const group = document.getElementById('inspector-curve');
        const toggle = group.querySelector('[data-collapse-toggle]');
        results.group = Boolean(group);
        results.expandedBefore = !group.classList.contains('is-collapsed');
        toggle.click();
        results.collapsedAfter = group.classList.contains('is-collapsed');
        results.ariaAfter = toggle.getAttribute('aria-expanded');
        toggle.click();
        results.expandedAgain = !group.classList.contains('is-collapsed');
        results.collapsedModules = Array.from(document.querySelectorAll('[data-collapsible-module]'))
            .map(element => element.dataset.collapsibleModule);
        results.headerBackgrounds = Array.from(document.querySelectorAll('[data-collapsible-module]'))
            .map(element => {
                const heading = element.querySelector('h3');
                return element.dataset.collapsibleModule + ':'
                    + getComputedStyle(heading).backgroundColor;
            });
        results.collapseStored = localStorage.getItem('nexfilm-collapsed-modules');
        results.navButtons = document.querySelectorAll('.module-nav-btn').length;
        results.curveChannels = document.querySelectorAll('.tone-curve-channel').length;
        results.detailSliders = ['sharpening-amount', 'sharpening-radius', 'sharpening-detail',
            'sharpening-masking', 'texture', 'clarity', 'dehaze']
            .filter(id => Boolean(document.getElementById(id))).length;
        results.copyOptions = Array.from(document.querySelectorAll('.copy-setting-option'))
            .map(input => input.value)
            .filter(value => ['curve', 'sharpen', 'texture', 'clarity', 'dehaze'].includes(value));
        results.curveLutMidpoint = NexFilmToneCurve.buildLut([[0, 0], [0.5, 0.4], [1, 1]])[256];
        results.docSections = ['zh-CN', 'en'].map(locale => window.NexFilmDocsData
            .getChapters(locale).flatMap(chapter => chapter.sections).map(section => section.id));
        results.docUnbalancedTags = (() => {
            const unbalanced = [];
            for (const locale of ['zh-CN', 'en']) {
                const sections = window.NexFilmDocsData.getChapters(locale)
                    .flatMap(chapter => chapter.sections);
                for (const section of sections) {
                    for (const tag of ['p', 'table', 'tr', 'td', 'ul', 'li', 'ol', 'strong', 'code']) {
                        const opened = (section.content.match(
                            new RegExp('<' + tag + '[ >]', 'g')
                        ) || []).length;
                        const closed = (section.content.match(
                            new RegExp('</' + tag + '>', 'g')
                        ) || []).length;
                        if (opened !== closed) unbalanced.push(locale + ':' + section.id + ':' + tag);
                    }
                }
            }
            return unbalanced;
        })();
        sliders.sharpeningAmount.el.value = '100';
        sliders.sharpeningDetail.el.value = '40';
        sliders.sharpeningMasking.el.value = '30';
        sliders.texture.el.value = '20';
        sliders.clarity.el.value = '25';
        sliders.dehaze.el.value = '30';
        const settings = currentDetailSettings(2560, 1707);
        results.detailSigmas = [
            settings.coarseSigma, settings.fineSigma, settings.textureSigma, settings.claritySigma
        ];
        results.dehazeWindow = [settings.dehazeRadius, settings.dehazeSoften];
        results.detailActive = settings.active;
        if (gl) {
            while (gl.getError() !== gl.NO_ERROR) { /* clear stale errors */ }
            results.floatTargets = Boolean(gl.getExtension('EXT_color_buffer_float'));
            const base = ensureDetailTarget('preview', 'base', 2560, 1707);
            renderDetailStage(
                { full: 'preview', reduced: 'preview-reduced' },
                base,
                null,
                2560,
                1707,
                settings
            );
            results.detailGlError = gl.getError();
            results.detailTargets = Array.from(detailTargets.preview.keys()).sort();
        }
    } catch (error) {
        results.probeError = String(error);
    }
}, 1200);

// The live preview pass: select the frame, drag a curve point, then report the
// GL state and the scope readback. This is the check that catches a pass chain
// that stops drawing.
setTimeout(async () => {
    const results = window.__probeResults || (window.__probeResults = {});
    try {
        switchView('develop');
        await selectImage('a-1');
        await wait(400);
        results.proxyFull = activeProxyIsFull;
        results.glFailures = [];
        if (gl) {
            for (const name of ['drawArrays', 'readPixels', 'texImage2D', 'texSubImage2D',
                'framebufferTexture2D', 'useProgram', 'bindVertexArray', 'bindTexture',
                'viewport', 'bindFramebuffer', 'uniform1i', 'clear']) {
                const original = gl[name].bind(gl);
                gl[name] = (...args) => {
                    const value = original(...args);
                    const error = gl.getError();
                    if (error !== gl.NO_ERROR) {
                        results.glFailures.push(name + ':' + error);
                    }
                    return value;
                };
            }
        }

        curvePoints.rgb = [[0, 0], [0.4, 0.45], [1, 1]];
        invalidateCurveLut();
        requestRender();
        await wait(250);

        // The Detail controls have to be on for the chain to run at all.
        sliders.sharpeningAmount.el.value = '60';
        sliders.sharpeningDetail.el.value = '30';
        sliders.texture.el.value = '20';
        sliders.clarity.el.value = '20';
        sliders.dehaze.el.value = '15';
        requestRender();
        await wait(250);
        results.histogramBefore = (() => {
            let nonZero = 0;
            for (let index = 0; index < lastPixels.length; index += 4) {
                if (lastPixels[index] || lastPixels[index + 1] || lastPixels[index + 2]) nonZero += 1;
            }
            return nonZero;
        })();

        const rect = toneCurveCanvas.getBoundingClientRect();
        const fire = (type, x, y) => toneCurveCanvas.dispatchEvent(new PointerEvent(type, {
            bubbles: true, cancelable: true, pointerId: 21, pointerType: 'mouse',
            clientX: rect.left + rect.width * x, clientY: rect.top + rect.height * y,
        }));
        const before = curvePoints.rgb.map(point => [...point]);
        fire('pointerdown', 0.5, 0.55);
        const downIndex = curveDragIndex;
        for (const [x, y] of [[0.6, 0.62], [0.7, 0.7]]) {
            fire('pointermove', x, y);
            await wait(120);
        }
        const dragged = curvePoints.rgb.map(point => point.map(value => Number(value.toFixed(3))));
        fire('pointerup', 0.7, 0.7);
        await wait(250);
        results.drag = { before, downIndex, dragged, movedX: dragged[downIndex][0] > before[0][0] + 0.05 };

        // Pulling the first endpoint sideways grows a control point instead of
        // being a dead end.
        const endpointBefore = curvePoints.rgb.map(point => [...point]);
        fire('pointerdown', 0.0, 0.0);
        for (const [x, y] of [[0.1, 0.05], [0.2, 0.1]]) {
            fire('pointermove', x, y);
            await wait(80);
        }
        fire('pointerup', 0.2, 0.1);
        await wait(200);
        results.endpointPull = {
            grew: curvePoints.rgb.length > endpointBefore.length,
            endpointKept: curvePoints.rgb[0][0] === 0,
        };

        // The bottom-left dot is the one the plot's rounded corner would clip,
        // so the drag has to start where the dot is actually drawn.
        const previousScroll = inspectorControls.scrollTop;
        inspectorControls.scrollTop = document.getElementById('inspector-curve').offsetTop - 8;
        await wait(120);
        const plot = curveCanvasProjection();
        const canvasRect = toneCurveCanvas.getBoundingClientRect();
        const unitX = canvasRect.width / plot.pixelWidth;
        const unitY = canvasRect.height / plot.pixelHeight;
        const dotX = canvasRect.left + (plot.originX + 0.5) * unitX;
        const dotY = canvasRect.top + (plot.originY + plot.size - 0.5) * unitY;
        const dotTarget = document.elementFromPoint(dotX, dotY);
        results.curvePlotInset = [plot.originX, plot.originY, plot.size];
        results.curveDotTarget = dotTarget ? (dotTarget.id || dotTarget.className || dotTarget.tagName) : null;
        curvePoints.rgb = [[0, 0], [0.5, 0.5], [1, 1]];
        const cornerTarget = dotTarget || toneCurveCanvas;
        const fireAt = (type, clientX, clientY) => cornerTarget.dispatchEvent(new PointerEvent(type, {
            bubbles: true, cancelable: true, pointerId: 51, pointerType: 'mouse',
            clientX, clientY,
        }));
        fireAt('pointerdown', dotX, dotY);
        results.cornerDragIndex = curveDragIndex;
        fireAt('pointermove', dotX, dotY - 26);
        fireAt('pointerup', dotX, dotY - 26);
        results.cornerBlackPoint = curvePoints.rgb.map(point => point.map(v => Number(v.toFixed(3))));
        inspectorControls.scrollTop = previousScroll;

        // Dragging a Detail slider runs the same base-pass/chain sequence, so
        // it has to stay clean too.
        const amountSlider = sliders.sharpeningAmount.el;
        amountSlider.dispatchEvent(new PointerEvent('pointerdown', {
            bubbles: true, pointerId: 31, pointerType: 'mouse',
        }));
        for (const value of ['45', '70', '90']) {
            amountSlider.value = value;
            amountSlider.dispatchEvent(new Event('input', { bubbles: true }));
            await wait(120);
        }
        amountSlider.dispatchEvent(new Event('change', { bubbles: true }));
        amountSlider.dispatchEvent(new PointerEvent('pointerup', {
            bubbles: true, pointerId: 31, pointerType: 'mouse',
        }));
        await wait(250);
        results.detailSliderDrag = { dragging: sliderDragActive };

        // A frame that only moves a Detail slider must leave the base render and
        // every blur in place: one draw call, and pixels identical to a frame
        // whose chain was rebuilt from scratch.
        const originalDrawArrays = gl.drawArrays.bind(gl);
        let drawCalls = 0;
        gl.drawArrays = (...args) => { drawCalls += 1; return originalDrawArrays(...args); };
        const readCanvas = () => {
            const pixels = new Uint8Array(gl.canvas.width * gl.canvas.height * 4);
            gl.bindFramebuffer(gl.FRAMEBUFFER, null);
            gl.readPixels(
                0, 0, gl.canvas.width, gl.canvas.height, gl.RGBA, gl.UNSIGNED_BYTE, pixels
            );
            return pixels;
        };
        const hashOf = (pixels) => {
            let value = 2166136261;
            for (let index = 0; index < pixels.length; index += 97) {
                value = Math.imul(value ^ pixels[index], 16777619) >>> 0;
            }
            return value;
        };
        // 'spread' is how far the channels are from each other (0 = grayscale),
        // 'range' is how much the picture varies at all.
        const statsOf = (pixels) => {
            let worst = 0;
            let low = 255;
            let high = 0;
            for (let index = 0; index < pixels.length; index += 388) {
                worst = Math.max(worst,
                    Math.abs(pixels[index] - pixels[index + 1]),
                    Math.abs(pixels[index + 1] - pixels[index + 2]));
                low = Math.min(low, pixels[index]);
                high = Math.max(high, pixels[index]);
            }
            return { spread: worst, range: high - low };
        };
        const renderFrame = () => {
            renderWebGL();
            gl.finish();
            return readCanvas();
        };
        const setSlider = (key, value) => {
            sliders[key].el.value = String(value);
            sliders[key].el.dispatchEvent(new Event('input', { bubbles: true }));
        };
        setSlider('sharpeningAmount', 40);
        setSlider('texture', 24);
        sliderDragActive = true;
        renderFrame();
        const dragCounts = [];
        let dragPixels = null;
        for (let index = 0; index < 5; index += 1) {
            setSlider('texture', 25 + index);
            drawCalls = 0;
            dragPixels = renderFrame();
            dragCounts.push(drawCalls);
        }
        results.detailDragDrawCalls = dragCounts;
        const dragHash = hashOf(dragPixels);
        results.finishedHash = dragHash;
        detailStageCache.clear();
        results.detailDragHashMatches = hashOf(renderFrame()) === dragHash;
        sliderDragActive = false;
        results.finishedFrame = statsOf(dragPixels);

        // Alt over a Sharpening control switches the canvas to the grayscale
        // views Lightroom shows; the stored thumbnail keeps the finished frame.
        const pressAlt = (key, pointerId) => {
            const slider = sliders[key].el;
            slider.dispatchEvent(new PointerEvent('pointerdown', {
                bubbles: true, pointerId, pointerType: 'mouse', altKey: true,
            }));
            slider.dispatchEvent(new Event('input', { bubbles: true }));
            return slider;
        };
        const amountDrag = pressAlt('sharpeningAmount', 71);
        const signalPixels = renderFrame();
        results.sharpeningSignalPreview = {
            mode: sharpeningPreviewMode(),
            ...statsOf(signalPixels),
            hash: hashOf(signalPixels),
        };
        // The stored thumbnail runs the same chain with previewMode 0, so it has
        // to stay a finished frame while the canvas shows the Alt view.
        renderDevelopFrame(
            'thumbnail', null, gl.canvas.width, gl.canvas.height, current_geom.crop_rect, 0
        );
        gl.finish();
        const thumbnailPixels = readCanvas();
        results.altPreviewThumbnail = {
            ...statsOf(thumbnailPixels),
            hash: hashOf(thumbnailPixels),
        };
        renderFrame();
        // Radius and Detail have to stay readable before an Amount is set.
        const radiusDrag = pressAlt('sharpeningRadius', 73);
        setSlider('sharpeningAmount', 0);
        results.radiusPreviewAtZeroAmount = {
            mode: sharpeningPreviewMode(),
            ...statsOf(renderFrame()),
        };
        radiusDrag.dispatchEvent(new PointerEvent('pointerup', {
            bubbles: true, pointerId: 73, pointerType: 'mouse',
        }));
        setSlider('sharpeningAmount', 40);
        amountDrag.dispatchEvent(new PointerEvent('pointerup', {
            bubbles: true, pointerId: 71, pointerType: 'mouse',
        }));

        const maskDrag = pressAlt('sharpeningMasking', 72);
        const slider2 = sliders.sharpeningMasking.el;
        slider2.value = '40';
        slider2.dispatchEvent(new Event('input', { bubbles: true }));
        const maskPixels = renderFrame();
        results.sharpeningMaskPreview = {
            mode: sharpeningPreviewMode(),
            ...statsOf(maskPixels),
            hash: hashOf(maskPixels),
        };
        window.dispatchEvent(new KeyboardEvent('keyup', { key: 'Alt' }));
        const finishedPixels = renderFrame();
        results.afterAltRelease = {
            mode: sharpeningPreviewMode(),
            ...statsOf(finishedPixels),
            hash: hashOf(finishedPixels),
        };
        maskDrag.dispatchEvent(new PointerEvent('pointerup', {
            bubbles: true, pointerId: 72, pointerType: 'mouse',
        }));
        results.sharpeningDragReleased = sharpeningDragKey === null;
        gl.drawArrays = originalDrawArrays;

        results.histogramAfter = (() => {
            let nonZero = 0;
            for (let index = 0; index < lastPixels.length; index += 4) {
                if (lastPixels[index] || lastPixels[index + 1] || lastPixels[index + 2]) nonZero += 1;
            }
            return nonZero;
        })();
        results.glErrorAfter = gl ? gl.getError() : null;
    } catch (error) {
        results.previewError = String(error) + ' :: ' + (error.stack || '').split('\\n')[1];
    }
    document.title = 'PREVIEWPROBE' + JSON.stringify(results) + 'END';
}, 2400);
</script>`;

const chromeCandidates = [
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
];
const browser = chromeCandidates.find(fs.existsSync);
if (!browser) throw new Error('Chrome or Edge was not found');

const pagePath = path.join(root, 'develop-detail-verify.html');
const shotPath = process.env.NEXFILM_UI_SHOT;
const shotTheme = process.env.NEXFILM_UI_SHOT_THEME || 'dark';
const shotLanguage = process.env.NEXFILM_UI_SHOT_LANGUAGE || '';
fs.writeFileSync(
    pagePath,
    fs.readFileSync(path.join(root, 'index.html'), 'utf8')
        .replace(
            '<script src="geometry.js"></script>',
            `<script>window.__uiShotMode = ${Boolean(shotPath)};` +
                `window.__uiShotAnchor = ${JSON.stringify(process.env.NEXFILM_UI_SHOT_ANCHOR || '')};` +
                `${shotPath ? `localStorage.setItem('nexfilm-theme', ${JSON.stringify(shotTheme)});` : ''}` +
                `${shotPath && shotLanguage ? `localStorage.setItem('nexfilm-language', ${JSON.stringify(shotLanguage)});` : ''}</script>\n` +
                `${stub}\n    <script src="geometry.js"></script>`
        )
        .replace('</body>', `${probe}\n</body>`)
);

let results;
try {
    const args = [
        '--headless=new',
        '--no-sandbox',
        '--disable-gpu-sandbox',
        '--enable-webgl',
        '--allow-file-access-from-files',
        '--ignore-gpu-blocklist',
        '--use-angle=swiftshader',
        '--virtual-time-budget=40000',
        '--window-size=1600,1000',
        '--dump-dom',
        pathToFileURL(pagePath).href,
    ];
    // Set NEXFILM_UI_SHOT=<file> to also capture the rendered inspector.
    if (process.env.NEXFILM_UI_SHOT) {
        args.push(`--screenshot=${process.env.NEXFILM_UI_SHOT}`);
    }
    const run = childProcess.spawnSync(browser, args, { encoding: 'utf8', timeout: 120000 });
    if (run.error) throw run.error;
    const match = run.stdout.match(/<title[^>]*>PREVIEWPROBE([\s\S]*?)END<\/title>/);
    if (!match) {
        throw new Error(`The Develop view did not report its state: ${run.stderr.trim().slice(0, 500)}`);
    }
    const payload = match[1]
        .replaceAll('&quot;', '"')
        .replaceAll('&amp;', '&')
        .replaceAll('&lt;', '<')
        .replaceAll('&gt;', '>');
    results = JSON.parse(payload);
} finally {
    fs.rmSync(pagePath, { force: true });
}

assert.deepEqual(results.errors, [], 'the UI reported uncaught errors');
assert.equal(results.probeError, undefined, `the inspector probe failed: ${results.probeError}`);
assert.equal(results.group, true, 'the Tone Curve section is missing');
assert.equal(results.expandedBefore, true, 'sections must start expanded');
assert.equal(results.collapsedAfter, true, 'the toggle did not collapse the section');
assert.equal(results.ariaAfter, 'false', 'the toggle did not report its state');
assert.equal(results.expandedAgain, true, 'the toggle did not re-open the section');
assert.deepEqual(
    results.collapsedModules,
    ['density', 'printer', 'curve', 'aesthetics', 'detail', 'sprocket', 'input', 'print'],
    'the collapsible sections do not match the panel order'
);
assert.equal(results.navButtons, 10, 'the module navigation does not carry every section');
assert.equal(results.curveChannels, 4, 'the curve editor is missing a channel tab');
assert.equal(results.detailSliders, 7, 'the Detail panel is missing controls');
assert.deepEqual(
    results.copyOptions,
    ['curve', 'sharpen', 'texture', 'clarity', 'dehaze'],
    'Copy Settings does not carry the curve and detail controls'
);
assert.ok(
    results.curveLutMidpoint > 0.15 && results.curveLutMidpoint < 0.25,
    `the preview curve table is wrong: ${results.curveLutMidpoint}`
);
assert.equal(results.detailActive, true, 'the Detail stage did not activate');
assert.deepEqual(
    results.detailSigmas.map(value => Number(value.toFixed(3))),
    [1, 0.6, 1.6, 7.68],
    'the Detail radii do not follow the shared reference scale'
);
assert.deepEqual(results.dehazeWindow, [40, 10], 'the haze window is wrong');
assert.equal(results.detailGlError, 0, 'the Detail pass chain reported a GL error');
assert.deepEqual(
    results.detailTargets,
    ['base', 'coarse-h', 'coarse-v', 'fine-h', 'fine-v', 'texture-h', 'texture-v'],
    'the Detail chain did not allocate the expected render targets'
);
assert.deepEqual(
    results.docSections[0],
    results.docSections[1],
    'the two documentation locales no longer share the same sections'
);
assert.ok(
    results.docSections[0].includes('curve-and-detail'),
    'the Tone Curve and Detail documentation section is missing'
);
assert.deepEqual(
    results.docUnbalancedTags,
    [],
    'documentation HTML has unbalanced tags'
);

assert.deepEqual(
    results.localeLabels,
    ['曲线', '细节'],
    'the Tone Curve and Detail headings must follow the interface language'
);
assert.deepEqual(
    results.headerBackgrounds.filter(entry => !entry.endsWith('rgba(0, 0, 0, 0)')),
    [],
    'section headings must not carry a fill'
);
assert.equal(results.previewError, undefined, `the preview probe failed: ${results.previewError}`);
assert.equal(results.proxyFull, true, 'the preview proxy never loaded');
assert.equal(results.drag?.movedX, true, 'a curve drag did not follow the pointer horizontally');
assert.equal(results.endpointPull?.grew, true, 'a sideways endpoint drag must grow a control point');
assert.equal(results.endpointPull?.endpointKept, true, 'the endpoint must keep its input value');
assert.ok(
    results.curvePlotInset[0] === results.curvePlotInset[1] && results.curvePlotInset[0] > 0,
    `the curve plot must be inset from the canvas edge: ${results.curvePlotInset}`
);
assert.equal(
    results.curveDotTarget,
    'tone-curve-canvas',
    'the bottom-left dot must be drawn where the canvas itself receives the pointer'
);
assert.equal(results.cornerDragIndex, 0, 'a drag from the bottom-left dot must grab the black point');
assert.ok(
    results.cornerBlackPoint[0][1] > 0.02,
    `the black point did not follow the drag: ${JSON.stringify(results.cornerBlackPoint)}`
);
assert.equal(results.cornerBlackPoint[0][0], 0, 'the black point keeps its input value');
assert.equal(results.detailSliderDrag?.dragging, false, 'a Detail slider drag must release its drag state');
assert.ok(
    results.detailDragDrawCalls.filter(count => count === 1).length >= 4,
    `a Detail drag re-ran passes it could reuse: ${results.detailDragDrawCalls}`
);
assert.equal(
    results.detailDragHashMatches,
    true,
    'a frame that reused the Detail chain must match a rebuilt frame'
);
assert.ok(results.finishedFrame?.spread > 0, 'the finished frame must not be grayscale');
assert.ok(results.finishedFrame?.range > 0, 'the finished frame must carry a picture');
assert.equal(results.sharpeningSignalPreview?.mode, 2, 'Alt over Amount must show the sharpening signal');
assert.equal(results.sharpeningSignalPreview?.spread, 0, 'the sharpening signal view must be grayscale');
assert.ok(
    results.sharpeningSignalPreview?.range > 6,
    `the sharpening signal view must show the edges: ${JSON.stringify(
        results.sharpeningSignalPreview)}`
);
assert.notEqual(
    results.sharpeningSignalPreview?.hash,
    results.finishedHash,
    'the sharpening preview must replace the finished frame'
);
assert.equal(results.sharpeningMaskPreview?.mode, 1, 'Alt over Masking must show the mask');
assert.equal(results.sharpeningMaskPreview?.spread, 0, 'the mask view must be grayscale');
assert.ok(
    results.sharpeningMaskPreview?.range > 40,
    `the mask view must show masked and unmasked areas: ${JSON.stringify(
        results.sharpeningMaskPreview)}`
);
assert.notEqual(
    results.sharpeningMaskPreview?.hash,
    results.sharpeningSignalPreview?.hash,
    'the mask view and the sharpening signal view are different pictures'
);
assert.equal(results.afterAltRelease?.mode, 0, 'releasing Alt must leave the preview');
assert.ok(results.afterAltRelease?.spread > 0, 'releasing Alt must bring the colour back');
assert.equal(results.sharpeningDragReleased, true, 'the Sharpening drag state must be released');
assert.ok(
    results.altPreviewThumbnail?.spread > 0,
    `the stored thumbnail must keep the finished frame: ${JSON.stringify(
        results.altPreviewThumbnail)}`
);
assert.notEqual(
    results.altPreviewThumbnail?.hash,
    results.sharpeningSignalPreview?.hash,
    'the Alt view must not reach the stored thumbnail'
);
assert.equal(
    results.radiusPreviewAtZeroAmount?.mode, 2, 'Alt over Radius must show the sharpening signal'
);
assert.ok(
    results.radiusPreviewAtZeroAmount?.range > 6,
    `the Radius view must show edges before an Amount is set: ${JSON.stringify(
        results.radiusPreviewAtZeroAmount)}`
);
assert.ok(
    results.histogramAfter > results.histogramBefore * 0.9,
    `the scope readback collapsed after the drag: ${results.histogramBefore} -> ${results.histogramAfter}`
);
assert.deepEqual(results.glFailures, [], 'the WebGL passes reported errors');
assert.equal(results.glErrorAfter, 0, 'the WebGL context reported an error after the drag');

console.log('Develop Detail UI, tone curve editor and WebGL pass chain verified.');
