const invoke = window.__TAURI__.core.invoke;

// DOM
const btnImport = document.getElementById('btn-import');
const btnExportBatch = document.getElementById('btn-export-batch');
const filmstripContainer = document.getElementById('filmstrip-container');
const canvasWrapper = document.getElementById('canvas-wrapper');
const previewCanvas = document.getElementById('preview-canvas');
const dummyPusher = document.getElementById('dummy-pusher');
const ctx = previewCanvas.getContext('2d');
const placeholder = document.getElementById('placeholder');

const btnModeColor = document.getElementById('btn-mode-color');
const btnModeBw = document.getElementById('btn-mode-bw');

// Crop Elements
const btnCropMode = document.getElementById('btn-crop-mode');
const btnRotateMode = document.getElementById('btn-rotate-mode');
const btnRotateLeft = document.getElementById('btn-rotate-left');
const btnRotateRight = document.getElementById('btn-rotate-right');
const cropOverlay = document.getElementById('crop-overlay');
const cropMask = document.getElementById('crop-mask');
const cropBox = document.getElementById('crop-box');
const cropGrid = document.getElementById('crop-grid');
const cropHandles = document.getElementById('crop-handles');

// New Geometry Elements
const btnAutoCrop = document.getElementById('btn-auto-crop');
const btnFlipH = document.getElementById('btn-flip-h');
const btnFlipV = document.getElementById('btn-flip-v');
// Angle slider removed

const sliders = {
    dmin: { el: document.getElementById('dmin'), val: document.getElementById('val-dmin') },
    dmax: { el: document.getElementById('dmax'), val: document.getElementById('val-dmax') },
    exposure: { el: document.getElementById('exposure'), val: document.getElementById('val-exposure') },
    gamma: { el: document.getElementById('gamma'), val: document.getElementById('val-gamma') },
    expr: { el: document.getElementById('expr'), val: document.getElementById('val-expr') },
    expg: { el: document.getElementById('expg'), val: document.getElementById('val-expg') },
    expb: { el: document.getElementById('expb'), val: document.getElementById('val-expb') }
};

let activeId = null;
let current_geom = { crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0.0, flip_h: false, flip_v: false, rotate_90_count: 0 };
let isCropMode = false;

// History Stack for Undo/Redo
const undoStacks = {};

function pushUndoState() {
    if (!activeId) return;
    if (!undoStacks[activeId]) undoStacks[activeId] = [];
    
    const mode = btnModeColor.classList.contains('bg-zinc-700') ? 'Color' : 'BW';
    const params = {
        film_mode: mode,
        d_min: parseFloat(sliders.dmin.el.value),
        d_max: parseFloat(sliders.dmax.el.value),
        exposure: parseFloat(sliders.exposure.el.value),
        gamma: parseFloat(sliders.gamma.el.value),
        exp_r: parseFloat(sliders.expr.el.value),
        exp_g: parseFloat(sliders.expg.el.value),
        exp_b: parseFloat(sliders.expb.el.value)
    };
    
    const geom = JSON.parse(JSON.stringify(current_geom));
    
    const stack = undoStacks[activeId];
    if (stack.length > 0) {
        const last = stack[stack.length - 1];
        if (JSON.stringify(last.params) === JSON.stringify(params) && 
            JSON.stringify(last.geom) === JSON.stringify(geom)) {
            return;
        }
    }
    
    stack.push({ params, geom });
    if (stack.length > 50) stack.shift();
}

// Keyboard shortcuts
window.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
        if (isCropMode || isRotateMode) {
            e.preventDefault();
            if (isCropMode) btnCropMode.click();
            if (isRotateMode) btnRotateMode.click();
        }
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        if (!activeId || !undoStacks[activeId] || undoStacks[activeId].length === 0) return;
        
        const prevState = undoStacks[activeId].pop();
        updateUIFromParams(prevState.params);
        current_geom = JSON.parse(JSON.stringify(prevState.geom));
        
        await invoke('update_geometry', { id: activeId, geom: current_geom });
        applyTuning(); 
        requestThumbnailSync();
        if (isCropMode || isRotateMode) {
            updateCropOverlay();
        }
    }
});

// Global Toast logic
function showToast(message, type = "error") {
    const container = document.getElementById('toast-container');
    const toast = document.createElement('div');
    toast.className = `px-4 py-3 rounded shadow-lg flex items-center gap-3 transform transition-all duration-300 translate-x-full ${type === 'error' ? 'bg-red-900/90 text-red-100 border border-red-700/50' : 'bg-zinc-800/90 text-zinc-100 border border-zinc-700/50'}`;
    toast.innerHTML = `
        <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            ${type === 'error' ? '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />' : '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />'}
        </svg>
        <span class="text-[13px] font-medium tracking-wide">${message}</span>
    `;
    container.appendChild(toast);
    
    // Animate in
    requestAnimationFrame(() => {
        toast.classList.remove('translate-x-full');
    });

    // Remove after 5 seconds
    setTimeout(() => {
        toast.classList.add('translate-x-full', 'opacity-0');
        setTimeout(() => {
            if (toast.parentNode) toast.parentNode.removeChild(toast);
        }, 300);
    }, 5000);
}

// CSS Variable updater for Track
function updateSliderTrack(el) {
    const min = parseFloat(el.min);
    const max = parseFloat(el.max);
    const val = parseFloat(el.value);
    const percent = ((val - min) / (max - min)) * 100;
    el.style.setProperty('--val', `${percent}%`);
}

let thumbnailSyncTimeout = null;
function requestThumbnailSync() {
    if (thumbnailSyncTimeout) clearTimeout(thumbnailSyncTimeout);
    thumbnailSyncTimeout = setTimeout(async () => {
        if (!activeId) return;
        try {
            await invoke('sync_thumbnail_buffer', { id: activeId });
            renderFilmstrip();
        } catch(e) {
            console.error(e);
        }
    }, 250);
}

// Custom throttle to replace lodash
function throttleAsync(fn, wait) {
    let isRunning = false;
    let pending = false;
    let lastArgs = [];

    const run = async () => {
        if (isRunning) return;
        isRunning = true;
        try {
            await fn(...lastArgs);
        } finally {
            isRunning = false;
            if (pending) {
                pending = false;
                setTimeout(run, wait);
            }
        }
    };

    return (...args) => {
        lastArgs = args;
        if (!isRunning) {
            run();
        } else {
            pending = true;
        }
    };
}

// Throttled UI logic
const applyTuning = throttleAsync(async () => {
    if (!activeId) return;
    
    const mode = btnModeColor.classList.contains('bg-zinc-700') ? 'Color' : 'BW';
    const params = {
        film_mode: mode,
        d_min: parseFloat(sliders.dmin.el.value),
        d_max: parseFloat(sliders.dmax.el.value),
        exposure: parseFloat(sliders.exposure.el.value),
        gamma: parseFloat(sliders.gamma.el.value),
        exp_r: parseFloat(sliders.expr.el.value),
        exp_g: parseFloat(sliders.expg.el.value),
        exp_b: parseFloat(sliders.expb.el.value)
    };
    
    // Apply visual CSS transforms (handled differently now, or reset to 1)
    // We will do CSS zooming in updateCanvasTransform

    try {
        const result = await invoke('apply_tuning_parameters', { params });
        let arrayBuffer;
        let byteOffset = 0;
        if (result instanceof ArrayBuffer) {
            arrayBuffer = result;
        } else if (result.buffer instanceof ArrayBuffer) {
            arrayBuffer = result.buffer;
            byteOffset = result.byteOffset || 0;
        } else if (Array.isArray(result)) {
            arrayBuffer = new Uint8Array(result).buffer;
        } else {
            throw new Error("Unknown buffer type returned from backend: " + typeof result);
        }

        const dataView = new DataView(arrayBuffer, byteOffset);
        const width = dataView.getUint32(0, true);
        const height = dataView.getUint32(4, true);
        
        const pixels = new Uint8ClampedArray(arrayBuffer, byteOffset + 8, width * height * 4);
        const imageData = new ImageData(pixels, width, height);
        
        if (previewCanvas.width !== width || previewCanvas.height !== height) {
            previewCanvas.width = width;
            previewCanvas.height = height;
        }
        ctx.putImageData(imageData, 0, 0);
        updateCanvasTransform(width, height);
        requestThumbnailSync();
    } catch (e) {
        console.error("Error applying tuning:", e);
    }
}, 32);

function setMode(mode) {
    if (mode === 'Color') {
        btnModeColor.classList.add('bg-zinc-700', 'text-zinc-100', 'shadow-sm');
        btnModeColor.classList.remove('text-zinc-500', 'hover:text-zinc-300');
        btnModeBw.classList.add('text-zinc-500', 'hover:text-zinc-300');
        btnModeBw.classList.remove('bg-zinc-700', 'text-zinc-100', 'shadow-sm');
        sliders.expr.el.disabled = false;
        sliders.expg.el.disabled = false;
        sliders.expb.el.disabled = false;
    } else {
        btnModeBw.classList.add('bg-zinc-700', 'text-zinc-100', 'shadow-sm');
        btnModeBw.classList.remove('text-zinc-500', 'hover:text-zinc-300');
        btnModeColor.classList.add('text-zinc-500', 'hover:text-zinc-300');
        btnModeColor.classList.remove('bg-zinc-700', 'text-zinc-100', 'shadow-sm');
        sliders.expr.el.disabled = true;
        sliders.expg.el.disabled = true;
        sliders.expb.el.disabled = true;
    }
}

function updateUIFromParams(params) {
    sliders.dmin.el.value = params.d_min;
    sliders.dmax.el.value = params.d_max;
    sliders.exposure.el.value = params.exposure;
    sliders.gamma.el.value = params.gamma;
    sliders.expr.el.value = params.exp_r;
    sliders.expg.el.value = params.exp_g;
    sliders.expb.el.value = params.exp_b;
    
    for (const key in sliders) {
        const s = sliders[key];
        s.val.textContent = parseFloat(s.el.value).toFixed(2);
        updateSliderTrack(s.el);
    }

    let modeStr = typeof params.film_mode === 'string' ? params.film_mode : (params.film_mode === 'BW' ? 'B&W' : 'Color');
    setMode(modeStr);
}

for (const key in sliders) {
    const s = sliders[key];
    s.el.addEventListener('mousedown', () => pushUndoState());
    s.el.addEventListener('input', (e) => {
        s.val.textContent = parseFloat(e.target.value).toFixed(3);
        updateSliderTrack(e.target);
        applyTuning();
    });
}

function enableUI() {
    for (const key in sliders) {
        sliders[key].el.disabled = false;
        updateSliderTrack(sliders[key].el);
    }
    btnExportBatch.disabled = false;
    btnCropMode.disabled = false;
    btnRotateMode.disabled = false;
    btnAutoCrop.disabled = false;
    btnRotateLeft.disabled = false;
    btnRotateRight.disabled = false;
    btnFlipH.disabled = false;
    btnFlipV.disabled = false;
    placeholder.style.display = 'none';
    canvasWrapper.style.display = 'flex';
    canvasWrapper.classList.remove('hidden');
    previewCanvas.style.display = 'block';
}

async function renderFilmstrip() {
    try {
        const items = await invoke('get_filmstrip');
        filmstripContainer.innerHTML = '';
        if (items.length === 0) return;
        
        items.forEach(item => {
            const div = document.createElement('div');
            div.className = `film-item shrink-0 h-[100px] w-[150px] rounded bg-zinc-900 overflow-hidden ${item.id === activeId ? 'active' : ''}`;
            div.onclick = () => selectImage(item.id);
            
            const img = document.createElement('img');
            img.src = `data:image/jpeg;base64,${item.thumbnail_base64}`;
            img.className = 'w-full h-full object-cover';
            
            div.appendChild(img);
            filmstripContainer.appendChild(div);
        });
    } catch (e) {
        console.error("Filmstrip error:", e);
    }
}

async function selectImage(id) {
    try {
        const state = await invoke('switch_active_image', { id });
        activeId = id;
        
        // Quick UI update for filmstrip active state
        document.querySelectorAll('.film-item').forEach(el => el.classList.remove('active'));
        renderFilmstrip();
        
        enableUI();
        updateUIFromParams(state.params);
        
        // Restore crop overlay
        current_geom = state.geom || { crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0.0, flip_h: false, flip_v: false, rotate_90_count: 0 };
        updateCropOverlay();
        
        applyTuning(); // Trigger render for this image
    } catch(e) {
        console.error("Select image error:", e);
    }
}

btnModeColor.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    setMode('Color');
    await invoke('set_film_mode', { id: activeId, mode: 'Color' });
    applyTuning();
});

btnModeBw.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    setMode('BW');
    await invoke('set_film_mode', { id: activeId, mode: 'B&W' });
    applyTuning();
});



btnImport.addEventListener('click', async () => {
    try {
        btnImport.textContent = "Importing...";
        btnImport.disabled = true;
        
        const paths = await invoke('open_file_dialog');
        if (paths.length > 0) {
            await invoke('import_images', { paths });
            const items = await invoke('get_filmstrip');
            if (items.length > 0) {
                // If this is the first import, select the first image
                if (!activeId) {
                    await selectImage(items[0].id);
                } else {
                    await renderFilmstrip();
                }
            }
        }
    } catch (e) {
        showToast("Import failed: " + e, "error");
    } finally {
        btnImport.textContent = "Import Roll";
        btnImport.disabled = false;
    }
});

btnExportBatch.addEventListener('click', async () => {
    try {
        btnExportBatch.textContent = "Exporting...";
        btnExportBatch.disabled = true;
        
        const outputDir = await invoke('select_export_dir');
        if (!outputDir) {
            btnExportBatch.textContent = "Batch Export";
            btnExportBatch.disabled = false;
            return;
        }

        const count = await invoke('batch_export_images', { outputDir });
        showToast(`Successfully exported ${count} image(s) to:\n${outputDir}`, "success");
    } catch (e) {
        showToast("Batch export failed: " + e, "error");
    } finally {
        btnExportBatch.textContent = "Batch Export";
        btnExportBatch.disabled = false;
    }
});

// Init tracks
for (const key in sliders) {
    updateSliderTrack(sliders[key].el);
}

// ==========================================
// CROP MODE INTERACTION
// ==========================================
let isRotateMode = false;
let currentImageWidth = 1;
let currentImageHeight = 1;

function updateCanvasTransform(w, h) {
    if (w) currentImageWidth = w;
    if (h) currentImageHeight = h;
    
    const cw = currentImageWidth;
    const ch = currentImageHeight;
    const rect = current_geom.crop_rect;

    canvasWrapper.style.overflow = 'hidden';
    previewCanvas.style.position = 'absolute';
    // Use fill instead of contain because we manually maintain aspect ratio
    previewCanvas.style.objectFit = 'fill'; 

    if (isCropMode || isRotateMode) {
        // Show full image
        canvasWrapper.style.aspectRatio = `${cw} / ${ch}`;
        dummyPusher.width = cw;
        dummyPusher.height = ch;
        
        previewCanvas.style.width = '100%';
        previewCanvas.style.height = '100%';
        previewCanvas.style.left = '0';
        previewCanvas.style.top = '0';
        
        cropOverlay.classList.remove('hidden');
        updateCropOverlay();
    } else {
        // Zoom to cropped region
        const cropW = cw * rect.width;
        const cropH = ch * rect.height;
        canvasWrapper.style.aspectRatio = `${cropW} / ${cropH}`;
        dummyPusher.width = cropW;
        dummyPusher.height = cropH;
        
        const scaleX = 100 / rect.width;
        const scaleY = 100 / rect.height;
        const offsetX = - (rect.x / rect.width) * 100;
        const offsetY = - (rect.y / rect.height) * 100;
        
        previewCanvas.style.width = `${scaleX}%`;
        previewCanvas.style.height = `${scaleY}%`;
        previewCanvas.style.left = `${offsetX}%`;
        previewCanvas.style.top = `${offsetY}%`;
        
        cropOverlay.classList.add('hidden');
    }
}

btnCropMode.addEventListener('click', () => {
    isCropMode = !isCropMode;
    if (isCropMode) {
        btnCropMode.classList.add('bg-zinc-800', 'text-zinc-100');
        isRotateMode = false;
        btnRotateMode.classList.remove('bg-zinc-800', 'text-zinc-100');
        cropBox.style.cursor = 'move';
        cropMask.style.pointerEvents = 'none';
    } else {
        btnCropMode.classList.remove('bg-zinc-800', 'text-zinc-100');
    }
    updateCanvasTransform();
});

btnRotateMode.addEventListener('click', () => {
    isRotateMode = !isRotateMode;
    if (isRotateMode) {
        btnRotateMode.classList.add('bg-zinc-800', 'text-zinc-100');
        isCropMode = false;
        btnCropMode.classList.remove('bg-zinc-800', 'text-zinc-100');
        cropBox.style.cursor = 'crosshair';
        cropMask.style.pointerEvents = 'auto'; // allow dragging anywhere on mask
        cropMask.style.cursor = 'crosshair';
    } else {
        btnRotateMode.classList.remove('bg-zinc-800', 'text-zinc-100');
        cropMask.style.pointerEvents = 'none';
    }
    updateCanvasTransform();
});

function updateCropRectForRotation(rect, isCW, flipH, flipV) {
    let p1 = { x: rect.x, y: rect.y };
    let p2 = { x: rect.x + rect.width, y: rect.y + rect.height };
    
    function transform(p) {
        let x = p.x, y = p.y;
        if (flipV) y = 1 - y;
        if (flipH) x = 1 - x;
        
        if (isCW) {
            let t = x;
            x = 1 - y;
            y = t;
        } else {
            let t = x;
            x = y;
            y = 1 - t;
        }
        
        if (flipH) x = 1 - x;
        if (flipV) y = 1 - y;
        return { x, y };
    }
    
    let tp1 = transform(p1);
    let tp2 = transform(p2);
    
    let nx = Math.min(tp1.x, tp2.x);
    let ny = Math.min(tp1.y, tp2.y);
    let nw = Math.abs(tp2.x - tp1.x);
    let nh = Math.abs(tp2.y - tp1.y);
    
    return { x: nx, y: ny, width: nw, height: nh };
}

btnRotateLeft.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    current_geom.rotate_90_count -= 1;
    current_geom.crop_rect = updateCropRectForRotation(
        current_geom.crop_rect, 
        false, 
        current_geom.flip_h, 
        current_geom.flip_v
    );
    await invoke('update_geometry', { id: activeId, geom: current_geom });
    applyTuning();
    requestThumbnailSync();
});

btnRotateRight.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    current_geom.rotate_90_count += 1;
    current_geom.crop_rect = updateCropRectForRotation(
        current_geom.crop_rect, 
        true, 
        current_geom.flip_h, 
        current_geom.flip_v
    );
    await invoke('update_geometry', { id: activeId, geom: current_geom });
    applyTuning();
    requestThumbnailSync();
});

btnFlipH.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    current_geom.flip_h = !current_geom.flip_h;
    
    // Mirror crop_rect horizontally
    current_geom.crop_rect.x = 1.0 - current_geom.crop_rect.x - current_geom.crop_rect.width;
    
    await invoke('update_geometry', { id: activeId, geom: current_geom });
    applyTuning();
    requestThumbnailSync();
});

btnFlipV.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    current_geom.flip_v = !current_geom.flip_v;
    
    // Mirror crop_rect vertically
    current_geom.crop_rect.y = 1.0 - current_geom.crop_rect.y - current_geom.crop_rect.height;
    
    await invoke('update_geometry', { id: activeId, geom: current_geom });
    applyTuning();
    requestThumbnailSync();
});



btnAutoCrop.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    try {
        const result = await invoke('geometry_auto_align', { id: activeId });
        current_geom.crop_rect = result.crop_rect;
        current_geom.angle = result.angle;
        updateCropOverlay();
        applyTuning();
        requestThumbnailSync();
    } catch (err) {
        showToast("Auto align failed: " + err, "error");
    }
});

function getRenderRect() {
    // Because in Crop Mode canvasWrapper has the aspect ratio of the FULL image,
    // its bounding rect is exactly the render rectangle.
    return canvasWrapper.getBoundingClientRect();
}

function updateCropOverlay() {
    if (!isCropMode && !isRotateMode) return;

    // In crop mode, SVG width/height perfectly matches the full uncropped image.
    const x = current_geom.crop_rect.x * 100;
    const y = current_geom.crop_rect.y * 100;
    const w = current_geom.crop_rect.width * 100;
    const h = current_geom.crop_rect.height * 100;

    cropBox.setAttribute('x', `${x}%`);
    cropBox.setAttribute('y', `${y}%`);
    cropBox.setAttribute('width', `${w}%`);
    cropBox.setAttribute('height', `${h}%`);

    // Mask path: outer rect - inner rect
    const maskPath = `M0,0 H100% V100% H0 Z M${x}%,${y}% V${y + h}% H${x + w}% V${y}% Z`;
    cropMask.setAttribute('d', maskPath);

    // Grid lines
    document.getElementById('grid-v1').setAttribute('x1', `${x + w/3}%`);
    document.getElementById('grid-v1').setAttribute('x2', `${x + w/3}%`);
    document.getElementById('grid-v1').setAttribute('y1', `${y}%`);
    document.getElementById('grid-v1').setAttribute('y2', `${y + h}%`);
    
    document.getElementById('grid-v2').setAttribute('x1', `${x + w*2/3}%`);
    document.getElementById('grid-v2').setAttribute('x2', `${x + w*2/3}%`);
    document.getElementById('grid-v2').setAttribute('y1', `${y}%`);
    document.getElementById('grid-v2').setAttribute('y2', `${y + h}%`);

    document.getElementById('grid-h1').setAttribute('y1', `${y + h/3}%`);
    document.getElementById('grid-h1').setAttribute('y2', `${y + h/3}%`);
    document.getElementById('grid-h1').setAttribute('x1', `${x}%`);
    document.getElementById('grid-h1').setAttribute('x2', `${x + w}%`);

    document.getElementById('grid-h2').setAttribute('y1', `${y + h*2/3}%`);
    document.getElementById('grid-h2').setAttribute('y2', `${y + h*2/3}%`);
    document.getElementById('grid-h2').setAttribute('x1', `${x}%`);
    document.getElementById('grid-h2').setAttribute('x2', `${x + w}%`);

    // Position handles
    const setHandle = (pos, hx, hy) => {
        const handle = cropHandles.querySelector(`[data-pos="${pos}"]`);
        if (handle) {
            handle.setAttribute('x', `${hx}%`);
            handle.setAttribute('y', `${hy}%`);
        }
    };

    setHandle('nw', x, y);
    setHandle('n', x + w/2, y);
    setHandle('ne', x + w, y);
    setHandle('w', x, y + h/2);
    setHandle('e', x + w, y + h/2);
    setHandle('sw', x, y + h);
    setHandle('s', x + w/2, y + h);
    setHandle('se', x + w, y + h);
}

let isDraggingCrop = false;
let dragType = null; // 'box' or handle pos
let dragStartPos = { x: 0, y: 0 };
let dragStartAngle = 0;
let dragCenter = { x: 0, y: 0 };

cropOverlay.addEventListener('mousedown', (e) => {
    if (!isCropMode && !isRotateMode) return;
    pushUndoState();
    
    const target = e.target;
    if (isRotateMode) {
        dragType = 'rotate';
    } else {
        if (target === cropBox) {
            dragType = 'box';
        } else if (target.classList.contains('crop-handle')) {
            dragType = target.getAttribute('data-pos');
        } else {
            return;
        }
    }

    isDraggingCrop = true;
    dragStartPos = { x: e.clientX, y: e.clientY };
    dragStartRect = { ...current_geom.crop_rect };
    dragStartAngle = current_geom.angle;
    
    const rect = canvasWrapper.getBoundingClientRect();
    dragCenter = {
        x: rect.left + rect.width / 2,
        y: rect.top + rect.height / 2
    };

    cropGrid.style.opacity = '1'; // Show rule of thirds grid
});

window.addEventListener('mousemove', (e) => {
    if (!isDraggingCrop) return;
    
    const renderRect = getRenderRect();
    const dx = (e.clientX - dragStartPos.x) / renderRect.width;
    const dy = (e.clientY - dragStartPos.y) / renderRect.height;
    
    let newRect = { ...dragStartRect };

    if (dragType === 'box') {
        newRect.x = Math.max(0, Math.min(1 - newRect.width, newRect.x + dx));
        newRect.y = Math.max(0, Math.min(1 - newRect.height, newRect.y + dy));
    } else if (dragType === 'rotate') {
        const startRad = Math.atan2(dragStartPos.y - dragCenter.y, dragStartPos.x - dragCenter.x);
        const currentRad = Math.atan2(e.clientY - dragCenter.y, e.clientX - dragCenter.x);
        let deltaDeg = (currentRad - startRad) * (180 / Math.PI);
        
        let newAngle = dragStartAngle + deltaDeg;
        
        if (Math.abs(newAngle) < 1.0) newAngle = 0.0;
        else if (Math.abs(newAngle - 90) < 1.0) newAngle = 90.0;
        else if (Math.abs(newAngle + 90) < 1.0) newAngle = -90.0;
        else if (Math.abs(newAngle - 180) < 1.0) newAngle = 180.0;
        else if (Math.abs(newAngle + 180) < 1.0) newAngle = -180.0;
        
        current_geom.angle = newAngle;
        // Do not update DOM transform here to avoid desync with crop box.
        // We rely on backend reapply_geometry for rotation.
        return; // Skip crop overlay update
    } else {
        if (dragType.includes('w')) {
            const maxW = newRect.x + newRect.width;
            newRect.x = Math.max(0, Math.min(maxW - 0.05, newRect.x + dx));
            newRect.width = maxW - newRect.x;
        }
        if (dragType.includes('e')) {
            newRect.width = Math.max(0.05, Math.min(1 - newRect.x, newRect.width + dx));
        }
        if (dragType.includes('n')) {
            const maxH = newRect.y + newRect.height;
            newRect.y = Math.max(0, Math.min(maxH - 0.05, newRect.y + dy));
            newRect.height = maxH - newRect.y;
        }
        if (dragType.includes('s')) {
            newRect.height = Math.max(0.05, Math.min(1 - newRect.y, newRect.height + dy));
        }
    }

    current_geom.crop_rect = newRect;
    updateCropOverlay();
});

window.addEventListener('mouseup', async () => {
    if (isDraggingCrop) {
        isDraggingCrop = false;
        cropGrid.style.opacity = '0'; // Hide grid
        
        if (dragType === 'rotate') {
            previewCanvas.style.transform = 'rotate(0deg)';
        }

        if (activeId) {
            try {
                await invoke('update_geometry', { 
                    id: activeId, 
                    geom: current_geom 
                });
                applyTuning();
                requestThumbnailSync();
            } catch (err) {
                showToast("Crop failed: " + err, "error");
            }
        }
    }
});
