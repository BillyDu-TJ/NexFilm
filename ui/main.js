const invoke = window.__TAURI__.core.invoke;
const { listen } = window.__TAURI__.event;
const { getContactSheetLayout, createContactSheetFilename } = window.NexFilmContactSheet;
const { getNeutralExposureOffsets } = window.NexFilmDensity;
const { getHistogramScale, getHistogramY } = window.NexFilmHistogram;
const { cloneSettingsValue, createCopyPayload, mergeCopyPayload } = window.NexFilmSettingsCopy;
const {
    createExportInvokeArgs,
    describeResize,
    formatExportTemplate,
    validateExportSettings,
} = window.NexFilmExport;

// DOM: Global
const btnImport = document.getElementById('btn-import');
const btnExportDialog = document.getElementById('btn-export-dialog');
const btnImportTriggers = document.querySelectorAll('.btn-import-trigger');
const toastContainer = document.getElementById('toast-container');

// DOM: Navigation & Views
const navHistory = document.getElementById('nav-history');
const navLibrary = document.getElementById('nav-library');
const navDevelop = document.getElementById('nav-develop');
const navSponsor = document.getElementById('nav-sponsor');
const viewHistory = document.getElementById('view-history');
const viewLibrary = document.getElementById('view-library');
const viewDevelop = document.getElementById('view-develop');

// DOM: Sponsor Modal
const sponsorModal = document.getElementById('sponsor-modal');
const sponsorModalContent = document.getElementById('sponsor-modal-content');
const btnCloseSponsor = document.getElementById('btn-close-sponsor');

// DOM: Library View
const libraryGrid = document.getElementById('library-grid');
const libraryEmpty = document.getElementById('library-empty');
const btnSelectAll = document.getElementById('btn-select-all');
const btnDeselectAll = document.getElementById('btn-deselect-all');
const librarySelectionCount = document.getElementById('library-selection-count');
const btnDeleteLibraryImages = document.getElementById('btn-delete-library-images');
const btnDeleteDevelopImage = document.getElementById('btn-delete-develop-image');
const btnDeleteRollImages = document.getElementById('btn-delete-roll-images');
const btnEditRoll = document.getElementById('btn-edit-roll');

// DOM: Develop View
const filmstripContainer = document.getElementById('filmstrip-container');
const canvasWrapper = document.getElementById('canvas-wrapper');
const previewCanvas = document.getElementById('preview-canvas');
const dummyPusher = document.getElementById('dummy-pusher');
const developInspector = document.getElementById('develop-inspector');
const rightPanelBlocker = document.getElementById('right-panel-blocker');

// DOM: Visualization
const histCanvas = document.getElementById('histogram-canvas');
const waveCanvas = document.getElementById('waveform-canvas');
const vizModeTabs = document.getElementById('viz-mode-tabs');
const btnVizHistogram = document.getElementById('btn-viz-histogram');
const btnVizWaveform = document.getElementById('btn-viz-waveform');

const histCtx = histCanvas.getContext('2d');
const waveCtx = waveCanvas.getContext('2d');

// DOM: Export Modal
const exportModal = document.getElementById('export-modal');
const exportModalContent = document.getElementById('export-modal-content');
const btnCloseExport = document.getElementById('btn-close-export');
const btnCancelExport = document.getElementById('btn-cancel-export');
const btnConfirmExport = document.getElementById('btn-confirm-export');
const btnChooseExportDir = document.getElementById('btn-choose-export-dir');
const exportSelectionCount = document.getElementById('export-selection-count');
const exportModalSubtitle = document.getElementById('export-modal-subtitle');
const exportOutputSummary = document.getElementById('export-output-summary');
const exportOutputDir = document.getElementById('export-output-dir');
const exportFormat = document.getElementById('export-format');
const exportColorSpace = document.getElementById('export-colorspace');
const exportQualityGroup = document.getElementById('export-quality-group');
const exportQuality = document.getElementById('export-quality');
const exportQualityValue = document.getElementById('export-quality-val');
const exportResizeMode = document.getElementById('export-resize-mode');
const exportLongEdgeGroup = document.getElementById('export-long-edge-group');
const exportLongEdge = document.getElementById('export-long-edge');
const exportUpscale = document.getElementById('export-upscale');
const exportSharpening = document.getElementById('export-sharpening');
const exportNaming = document.getElementById('export-naming');
const exportNamePreview = document.getElementById('export-name-preview');
const exportConflictPolicy = document.getElementById('export-conflict-policy');

const btnModeColor = document.getElementById('btn-mode-color');
const btnModeBw = document.getElementById('btn-mode-bw');

// DOM: Crop & Transform
const btnCropMode = document.getElementById('btn-crop-mode');
const btnPerspectiveMode = document.getElementById('btn-perspective-mode');
const btnRecalibrate = document.getElementById('btn-recalibrate');
const btnAutoArea = document.getElementById('btn-auto-area');
const btnBatchApply = document.getElementById('btn-batch-apply');
const btnResetCrop = document.getElementById('btn-reset-crop');
const btnAutoColor = document.getElementById('btn-auto-color');
const btnSprocketPicker = document.getElementById('btn-sprocket-picker');
const btnResetColor = document.getElementById('btn-reset-color');
const btnRotateLeft = document.getElementById('btn-rotate-left');
const btnRotateRight = document.getElementById('btn-rotate-right');
const btnFlipH = document.getElementById('btn-flip-h');
const btnFlipV = document.getElementById('btn-flip-v');

const cropOverlay = document.getElementById('crop-overlay');
const cropMask = document.getElementById('crop-mask');
const cropBox = document.getElementById('crop-box');
const cropGrid = document.getElementById('crop-grid');
const cropEdges = document.getElementById('crop-edges');
const cropHandles = document.getElementById('crop-handles');
const rotateHandleOuter = document.getElementById('rotate-handle-outer');
const perspectiveOverlay = document.getElementById('perspective-overlay');
const perspectivePanel = document.getElementById('perspective-panel');
const btnClosePerspective = document.getElementById('btn-close-perspective');
const btnResetPerspective = document.getElementById('btn-reset-perspective');
const perspectiveControls = {
    angle: { el: document.getElementById('perspective-rotate'), val: document.getElementById('val-perspective-rotate') },
    perspective_vertical: { el: document.getElementById('perspective-vertical'), val: document.getElementById('val-perspective-vertical') },
    perspective_horizontal: { el: document.getElementById('perspective-horizontal'), val: document.getElementById('val-perspective-horizontal') },
    perspective_aspect: { el: document.getElementById('perspective-aspect'), val: document.getElementById('val-perspective-aspect') },
    perspective_scale: { el: document.getElementById('perspective-scale'), val: document.getElementById('val-perspective-scale') },
};
const perspectiveConstrain = document.getElementById('perspective-constrain');

const batchApplyModal = document.getElementById('batch-apply-modal');
const batchApplyModalContent = document.getElementById('batch-apply-modal-content');
const btnCloseBatchApply = document.getElementById('btn-close-batch-apply');
const btnCancelBatchApply = document.getElementById('btn-cancel-batch-apply');
const btnConfirmBatchApply = document.getElementById('btn-confirm-batch-apply');

const copySettingsModal = document.getElementById('copy-settings-modal');
const copySettingsContent = document.getElementById('copy-settings-content');
const btnCloseCopySettings = document.getElementById('btn-close-copy-settings');
const btnCancelCopySettings = document.getElementById('btn-cancel-copy-settings');
const btnConfirmCopySettings = document.getElementById('btn-confirm-copy-settings');

let currentDMin = [0.1, 0.1, 0.1];
let currentDMax = [2.0, 2.0, 2.0];
const CHANNEL_CONTROL_SCALE = 0.5;
const LUT_CONTROL_SCALE = 0.5;

const sliders = {
    masterDmin: { el: document.getElementById('master-dmin'), val: document.getElementById('val-master-dmin') },
    masterDmax: { el: document.getElementById('master-dmax'), val: document.getElementById('val-master-dmax') },
    exposure: { el: document.getElementById('exposure'), val: document.getElementById('val-exposure') },
    gamma: { el: document.getElementById('gamma'), val: document.getElementById('val-gamma') },
    saturation: { el: document.getElementById('saturation'), val: document.getElementById('val-saturation') },
    temperature: { el: document.getElementById('temperature'), val: document.getElementById('val-temperature') },
    tint: { el: document.getElementById('tint'), val: document.getElementById('val-tint') },
    expr: { el: document.getElementById('expr'), val: document.getElementById('val-expr') },
    expg: { el: document.getElementById('expg'), val: document.getElementById('val-expg') },
    expb: { el: document.getElementById('expb'), val: document.getElementById('val-expb') },
    highlights: { el: document.getElementById('highlights'), val: document.getElementById('val-highlights') },
    shadows: { el: document.getElementById('shadows'), val: document.getElementById('val-shadows') },
    lutOpacity: { el: document.getElementById('lut-opacity'), val: document.getElementById('val-lut-opacity') },
    sprocketTolerance: { el: document.getElementById('sprocket-tolerance'), val: document.getElementById('val-sprocket-tolerance') },
    sprocketFeather: { el: document.getElementById('sprocket-feather'), val: document.getElementById('val-sprocket-feather') }
};

const imageStates = new Map();
let copiedSettings = null;
let isEyedropperActive = false;
let isSprocketPickerActive = false;
let activeId = null;
let proxyPixels = null;
let proxyWidth = 0;
let proxyHeight = 0;
let activeProxyIsFull = false;
let hasProcessedActiveImage = false;

let lastHistPixels = null;
let current_geom = NexFilmGeometry.normalizeGeometryState({});
let isCropMode = false;
let isPerspectiveMode = false;
let currentImageWidth = 1;
let currentImageHeight = 1;
let zoomLevel = 1.0;

let originalFilmOptions = null;
let missingFileId = null;
let totalImportCount = 0;
let currentImportCount = 0;
let importFailedCount = 0;
let importInProgress = false;

window.addEventListener('resize', () => {
    if (activeId) updateCanvasTransform();
});

canvasWrapper.parentElement.addEventListener('wheel', (e) => {
    if (!activeId) return;
    e.preventDefault();
    if (e.deltaY < 0) {
        zoomLevel *= 1.1;
    } else {
        zoomLevel /= 1.1;
    }
    zoomLevel = Math.max(0.1, Math.min(zoomLevel, 10.0));
    updateCanvasTransform();
}, { passive: false });

filmstripContainer.addEventListener('wheel', (e) => {
    e.preventDefault();
    filmstripContainer.scrollLeft += e.deltaY;
}, { passive: false });

let isWaveform = false;
let lastPixels = null;
const HIST_W = 256;
const HIST_H = 256;

// Library Multi-Selection State
let allLibraryItems = [];
const itemIndex = new Map();
let selectedLibraryIds = new Set();
let lastSelectedLibraryId = null;

function normalizePath(path) {
    return (path || '').replace(/\\/g, '/').toLowerCase();
}

function itemIdentity(item) {
    const rollId = item?.roll_id || 'LOOSE_DEFAULT';
    return `${rollId}::${normalizePath(item?.file_path)}`;
}

function rememberItem(item) {
    if (!item || !item.id) return item;
    const previous = itemIndex.get(item.id) || {};
    const merged = { ...previous, ...item };
    if (merged.rendered_thumbnail_base64) {
        merged.thumbnail_base64 = merged.rendered_thumbnail_base64;
        merged.thumbnail_kind = 'rendered';
    } else if (!merged.thumbnail_base64 && merged.embedded_thumbnail_base64) {
        merged.thumbnail_base64 = merged.embedded_thumbnail_base64;
        merged.thumbnail_kind = 'embedded';
    }
    itemIndex.set(merged.id, merged);
    return merged;
}

function rememberItems(items) {
    (items || []).forEach(rememberItem);
}

function findKnownItem(id) {
    return itemIndex.get(id) || allLibraryItems.find(i => i.id === id) || null;
}

function forgetRollItems(rollIds) {
    const deleted = new Set(rollIds || []);
    if (deleted.size === 0) return;
    const removedItemIds = new Set();
    for (const [id, item] of itemIndex.entries()) {
        if (deleted.has(item.roll_id)) removedItemIds.add(id);
    }
    allLibraryItems = allLibraryItems.filter(item => !deleted.has(item.roll_id));
    removedItemIds.forEach(id => itemIndex.delete(id));
    selectedLibraryIds = new Set([...selectedLibraryIds].filter(id => !removedItemIds.has(id)));
    if (activeId && removedItemIds.has(activeId)) activeId = null;
}

function getImageDeletionTargets(ids) {
    const targets = new Map();
    for (const id of ids || []) {
        const item = findKnownItem(id);
        if (!item || item.status === 'importing' || !item.file_path) continue;
        const target = {
            id: item.id,
            roll_id: item.roll_id || 'LOOSE_DEFAULT',
            file_path: item.file_path
        };
        targets.set(itemIdentity(target), target);
    }
    return [...targets.values()];
}

function forgetImageItems(targets) {
    const deletedIdentities = new Set((targets || []).map(itemIdentity));
    if (deletedIdentities.size === 0) return new Set();

    const removedIds = new Set();
    for (const [id, item] of itemIndex.entries()) {
        if (deletedIdentities.has(itemIdentity(item))) removedIds.add(id);
    }
    allLibraryItems = allLibraryItems.filter(item => !deletedIdentities.has(itemIdentity(item)));
    for (const id of removedIds) {
        itemIndex.delete(id);
        imageStates.delete(id);
        proxyCache.delete(id);
        readyProxyIds.delete(id);
        proxyAnalyzedBaseIds.delete(id);
        proxyPreparePromises.delete(id);
        proxyDisplayPromises.delete(id);
        delete undoStacks[id];
    }
    selectedLibraryIds = new Set([...selectedLibraryIds].filter(id => !removedIds.has(id)));

    const deletedPaths = new Set((targets || []).map(target => normalizePath(target.file_path)));
    if (currentImportSessionPaths) {
        currentImportSessionPaths = currentImportSessionPaths.filter(path => !deletedPaths.has(normalizePath(path)));
    }
    if (activeImportViewPaths) {
        activeImportViewPaths = activeImportViewPaths.filter(path => !deletedPaths.has(normalizePath(path)));
    }
    return removedIds;
}

function upsertLibraryItem(item) {
    if (!item || !item.id) return;
    const known = rememberItem(item);
    const identity = itemIdentity(known);
    const idx = allLibraryItems.findIndex(i => i.id === known.id || itemIdentity(i) === identity);
    if (idx >= 0) {
        if (allLibraryItems[idx].id && allLibraryItems[idx].id !== known.id) {
            itemIndex.delete(allLibraryItems[idx].id);
        }
        allLibraryItems[idx] = { ...allLibraryItems[idx], ...known };
    } else {
        allLibraryItems.push(known);
    }
}

function uniqueItemsByPath(items) {
    const byPath = new Map();
    const noPath = [];
    (items || []).forEach(item => {
        const key = itemIdentity(item);
        if (!normalizePath(item.file_path)) {
            noPath.push(item);
            return;
        }
        const existing = byPath.get(key);
        if (!existing) {
            byPath.set(key, item);
            return;
        }
        const preferNew =
            existing.status === 'importing' ||
            (!!existing.transient_edit && !item.transient_edit) ||
            (!!item.thumbnail_base64 && !existing.thumbnail_base64);
        byPath.set(key, preferNew ? { ...existing, ...item } : { ...item, ...existing });
    });
    return [...byPath.values(), ...noPath];
}

function appendMissingSourceBadge(container, item) {
    if (!item?.file_missing) return;
    const badge = document.createElement('div');
    badge.className = 'absolute right-1 top-1 bg-red-950/90 border border-red-700/70 px-1.5 py-0.5 text-[8px] font-bold tracking-wider text-red-200';
    badge.textContent = 'Source Missing';
    container.appendChild(badge);
}

function uniqueImportPaths(paths) {
    const unique = new Map();
    (paths || []).forEach(path => {
        const key = normalizePath(path);
        if (key && !unique.has(key)) unique.set(key, path);
    });
    return [...unique.values()];
}

const supportedImportExtensions = new Set([
    'dng', 'nef', 'nrw', 'cr2', 'cr3', 'arw', 'srf', 'sr2', 'raf', 'rw2',
    'orf', 'ori', 'srw', 'pef', '3fr', 'erf', 'kdc', 'dcr', 'iiq', 'mos',
    'mrw', 'x3f', 'rwl', 'raw', 'tif', 'tiff', 'jpg', 'jpeg', 'png'
]);

function isSupportedImportPath(path) {
    const filename = normalizePath(path).split('/').pop() || '';
    const extension = filename.includes('.') ? filename.split('.').pop() : '';
    return supportedImportExtensions.has(extension);
}

function removeTransientItems(ids) {
    const removed = new Set(ids || []);
    if (removed.size === 0) return;
    allLibraryItems = allLibraryItems.filter(item => !removed.has(item.id));
    removed.forEach(id => itemIndex.delete(id));
    selectedLibraryIds = new Set([...selectedLibraryIds].filter(id => !removed.has(id)));
}

// Delete Mode State
let isDeleteMode = false;
let selectedRollIds = new Set();

// Calibration State
let isCalibrationMode = false;
let calibrationDragState = null;
let calibrationPoints = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
const calibrationEdgeIndices = NexFilmGeometry.calibrationEdgeIndices;
let calibrationRevision = 0;
const filmAreaDetectionPromises = new Map();

function setDevelopInspectorCalibrationLocked(locked) {
    if (locked) developInspector.scrollTop = 0;
    developInspector.classList.toggle('calibration-locked', locked);
    rightPanelBlocker.classList.toggle('hidden', !locked);
}

function updateLibrarySelectionUI() {
    const selectedTargets = getImageDeletionTargets(selectedLibraryIds);
    librarySelectionCount.textContent = `${selectedLibraryIds.size} selected`;
    if (selectedLibraryIds.size > 0) {
        btnExportDialog.disabled = false;
        btnExportDialog.textContent = 'Export (' + selectedLibraryIds.size + ')';
        btnDeselectAll.classList.remove('hidden');
    } else {
        btnExportDialog.disabled = true;
        btnExportDialog.textContent = 'Export';
        btnDeselectAll.classList.add('hidden');
    }
    
    btnDeleteLibraryImages.disabled = importInProgress || selectedTargets.length === 0;
    btnDeleteRollImages.disabled = importInProgress || selectedTargets.length === 0;
    btnDeleteRollImages.textContent = selectedTargets.length > 0
        ? `Delete Selected (${selectedTargets.length})`
        : 'Delete Selected';

    // update visuals
    document.querySelectorAll('#library-grid .library-item[data-id], #history-internal-grid .library-item[data-id]').forEach(child => {
        const id = child.dataset.id;
        if (selectedLibraryIds.has(id)) {
            child.classList.add('selected');
        } else {
            child.classList.remove('selected');
        }
    });
    if (typeof updateExportDialogState === 'function') updateExportDialogState();
}

btnSelectAll.addEventListener('click', () => {
    allLibraryItems.forEach(item => selectedLibraryIds.add(item.id));
    updateLibrarySelectionUI();
});

btnDeselectAll.addEventListener('click', () => {
    selectedLibraryIds.clear();
    updateLibrarySelectionUI();
});


// Routing
let currentView = 'library'; // Tracks active view: 'library' | 'develop' | 'history'

function clearNativeSelection(event) {
    if (event) event.preventDefault();
    window.getSelection()?.removeAllRanges();
}

function createLooseImportRoll(paths) {
    const now = new Date();
    const localDate = new Date(now.getTime() - now.getTimezoneOffset() * 60_000)
        .toISOString()
        .slice(0, 10);
    return {
        roll_id: `loose_${Date.now()}_${Math.floor(Math.random() * 1000)}`,
        date: localDate,
        format: 'Loose',
        film_stock: 'Loose Import',
        camera: '',
        image_paths: paths
    };
}

async function flushActiveImageState() {
    if (activeId) {
        await flushPendingBackendSync();
        const outgoingId = activeId;
        const outgoingParams = saveCurrentState();
        await updateBackendParams(outgoingId, outgoingParams);
        scheduleInstantThumbnailUpdate();
        await flushPendingThumbnail();
    }
}

async function beginWorkingImport(rollId, paths) {
    await flushActiveImageState();
    resetWorkingLibrary();
    currentRollViewId = rollId;
    isRollEditing = true;
    currentImportSessionPaths = paths.map(normalizePath);
    activeImportViewPaths = currentImportSessionPaths.slice();
}

function getDevelopRollId() {
    return findKnownItem(activeId)?.roll_id || currentRollViewId || null;
}

function setImportManagementBusy(busy) {
    document.getElementById('btn-delete-rolls').disabled = busy;
    document.getElementById('btn-promote-roll').disabled = busy;
    btnEditRoll.disabled = busy;
    btnDeleteRollImages.disabled = busy || getImageDeletionTargets(selectedLibraryIds).length === 0;
    btnDeleteDevelopImage.disabled = busy || getImageDeletionTargets([activeId]).length === 0;
    btnDeleteLibraryImages.disabled = busy || getImageDeletionTargets(selectedLibraryIds).length === 0;
}

function resetWorkingLibrary() {
    currentImageRequestToken++;
    isCalibrationMode = false;
    document.getElementById('calibration-overlay').classList.add('hidden');
    setDevelopInspectorCalibrationLocked(false);
    activeId = null;
    activeProxyIsFull = false;
    hasProcessedActiveImage = false;
    proxyPixels = null;
    proxyWidth = 0;
    proxyHeight = 0;
    currentRollViewId = null;
    isRollEditing = false;
    currentImportSessionPaths = null;
    activeImportViewPaths = null;
    selectedLibraryIds.clear();
    allLibraryItems = [];
    imageStates.clear();
    filmstripContainer.innerHTML = '';
    hideThumbnailPlaceholder();
    previewCanvas.style.display = 'none';
    disableUI();
    updateLibrarySelectionUI();
}

function switchView(viewName) {
    currentView = viewName;
    const views = [
        { name: 'history', nav: navHistory, el: viewHistory },
        { name: 'library', nav: navLibrary, el: viewLibrary },
        { name: 'develop', nav: navDevelop, el: viewDevelop }
    ];

    views.forEach(v => {
        // 彻底清理旧的 Tailwind opacity 或 z-index 隐藏逻辑，只保留 display 切换
        v.el.classList.remove('opacity-0', 'pointer-events-none');
        if (v.name === viewName) {
            v.el.style.display = 'flex';
            v.nav.classList.add('text-zinc-100', 'border-zinc-100');
            v.nav.classList.remove('text-zinc-500', 'border-transparent');
        } else {
            v.el.style.display = 'none';
            v.nav.classList.remove('text-zinc-100', 'border-zinc-100');
            v.nav.classList.add('text-zinc-500', 'border-transparent'); 
        }
    });

    const isDevelop = viewName === 'develop';
    document.getElementById('btn-export-roll').classList.toggle('hidden', !isDevelop);
    btnDeleteDevelopImage.classList.toggle('hidden', !isDevelop);
    btnDeleteDevelopImage.disabled = importInProgress || getImageDeletionTargets([activeId]).length === 0;

    if (isDevelop) {
        // Re-enable UI if there's an active image (e.g., user switched away and came back)
        if (activeId) {
            enableUI();
        }
        requestRender();
    } else {
        // When leaving develop view, disable all tuning UI to prevent
        // orphaned slider event handlers from firing on stale state.
        disableUI();
    }
}

if (navHistory) navHistory.addEventListener('click', () => switchView('history'));
navLibrary.addEventListener('click', () => {
    activeImportViewPaths = null;
    switchView('library');
    renderLibraryAndFilmstrip();
});
navDevelop.addEventListener('click', () => switchView('develop'));

let sponsorLastFocusedElement = null;

function openSponsorModal() {
    sponsorLastFocusedElement = document.activeElement;
    sponsorModal.classList.add('is-open');
    sponsorModal.setAttribute('aria-hidden', 'false');
    requestAnimationFrame(() => btnCloseSponsor.focus());
}

function closeSponsorModal() {
    if (!sponsorModal.classList.contains('is-open')) return;
    sponsorModal.classList.remove('is-open');
    sponsorModal.setAttribute('aria-hidden', 'true');
    if (sponsorLastFocusedElement instanceof HTMLElement) sponsorLastFocusedElement.focus();
}

navSponsor.addEventListener('click', openSponsorModal);
btnCloseSponsor.addEventListener('click', closeSponsorModal);
sponsorModal.addEventListener('click', (event) => {
    if (event.target === sponsorModal) closeSponsorModal();
});
sponsorModalContent.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') {
        event.preventDefault();
        closeSponsorModal();
    } else if (event.key === 'Tab') {
        event.preventDefault();
        btnCloseSponsor.focus();
    }
});

// History Stack for Undo/Redo
const undoStacks = {};

function pushUndoState() {
    if (!activeId) return;
    if (!undoStacks[activeId]) undoStacks[activeId] = [];
    
    const mode = btnModeColor.classList.contains('bg-[#28282c]') ? 'Color' : 'BW';
    const params = {
        film_mode: mode,
        d_min: currentDMin.slice(),
        d_max: currentDMax.slice(),
        exposure: parseFloat(sliders.exposure.el.value),
        gamma: parseFloat(sliders.gamma.el.value),
        saturation: parseFloat(sliders.saturation.el.value),
        temperature: parseFloat(sliders.temperature.el.value),
        tint: parseFloat(sliders.tint.el.value),
        exp_r: parseFloat(sliders.expr.el.value),
        exp_g: parseFloat(sliders.expg.el.value),
        exp_b: parseFloat(sliders.expb.el.value),
        highlights: parseFloat(sliders.highlights.el.value),
        shadows: parseFloat(sliders.shadows.el.value),
        lut_path: currentLutPath,
        lut_opacity: parseFloat(sliders.lutOpacity.el.value),
        working_colorspace: currentWorkingColorspace,
        sprocket_uv: Array.from(currentSprocketUV),
        sprocket_tolerance: currentSprocketTolerance,
        sprocket_feather: currentSprocketFeather
    };
    
    const geom = JSON.parse(JSON.stringify(current_geom));
    
    const stack = undoStacks[activeId];
    if (stack.length > 0) {
        const last = stack[stack.length - 1];
        if (JSON.stringify(last.params) === JSON.stringify(params) && JSON.stringify(last.geom) === JSON.stringify(geom)) return;
    }
    
    stack.push({ params, geom });
    if (stack.length > 50) stack.shift();
}

window.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
        if (isCropMode) {
            e.preventDefault();
            btnCropMode.click();
        }
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault();
        if (!activeId || !undoStacks[activeId] || undoStacks[activeId].length === 0) return;
        
        const prevState = undoStacks[activeId].pop();
        updateUIFromParams(prevState.params, prevState.geom);
        current_geom = NexFilmGeometry.normalizeGeometryState(prevState.geom);
        updateCanvasTransform();
        requestRender();
        if (isCropMode) updateCropOverlay();

        const undoId = activeId;
        const undoGeom = JSON.parse(JSON.stringify(current_geom));
        try {
            await restoreLutForImage(prevState.params);
            await persistGeometryQueued(undoId, undoGeom);
            await updateBackendParams(undoId, prevState.params);

            // Undoing crop/rotate/flip only changes WebGL geometry; the RAW
            // proxy remains canonical and does not need another decode.
            requestRender();

            requestThumbnailSync();
        } catch (error) {
            console.error('Failed to persist undo state', error);
            showToast('Could not save the undo state: ' + error, 'error');
        }
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
        
        const container = document.getElementById('filmstrip-container');
        if (!container || container.offsetParent === null) return; // Only if filmstrip is visible
        
        const items = Array.from(container.querySelectorAll('.film-item'));
        if (items.length === 0) return;
        
        let currentIndex = items.findIndex(item => item.classList.contains('active'));
        if (currentIndex === -1) currentIndex = 0;
        
        let newIndex = currentIndex;
        if (e.key === 'ArrowLeft' && currentIndex > 0) newIndex--;
        else if (e.key === 'ArrowRight' && currentIndex < items.length - 1) newIndex++;
        
        if (newIndex !== currentIndex) {
            e.preventDefault();
            items[newIndex].click();
            items[newIndex].scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'center' });
        }
    }
});

document.getElementById('btn-delete-rolls').addEventListener('click', () => {
    if (navHistory.classList.contains('text-zinc-100') && !historyRollViewId) {
        isDeleteMode = !isDeleteMode;
        if (!isDeleteMode) selectedRollIds.clear();
        renderLibraryAndFilmstrip();
    }
});

document.getElementById('btn-confirm-delete').addEventListener('click', async () => {
    if (selectedRollIds.size === 0) return;
    await requestRollDeletion(Array.from(selectedRollIds));
});

document.getElementById('btn-cancel-delete').addEventListener('click', () => {
    isDeleteMode = false;
    selectedRollIds.clear();
    renderLibraryAndFilmstrip();
});

async function requestRollDeletion(rollIds) {
    if (importInProgress) {
        showToast('Wait for the current import to finish before removing a roll.', 'error');
        return;
    }
    const uniqueRollIds = [...new Set((rollIds || []).filter(Boolean))];
    if (uniqueRollIds.length === 0) return;
    const choice = await showDeleteRollDialog(uniqueRollIds.length);
    if (!choice) return;

    try {
        const currentWorkingRollId = getDevelopRollId();
        const result = await invoke('delete_rolls', {
            rollIds: uniqueRollIds,
            deleteSourceFiles: choice === 'files'
        });
        forgetRollItems(uniqueRollIds);
        for (const rollId of uniqueRollIds) {
            for (const key of rollPreviewCache.keys()) {
                if (key.startsWith(`${rollId}:`)) rollPreviewCache.delete(key);
            }
        }
        selectedRollIds.clear();
        isDeleteMode = false;
        const deletedHistoryRoll = uniqueRollIds.includes(historyRollViewId);
        if (uniqueRollIds.includes(currentWorkingRollId)) {
            resetWorkingLibrary();
        }
        if (deletedHistoryRoll) historyRollViewId = null;
        allRolls = await invoke('get_rolls');
        await updateFilterSidebar();
        await renderLibraryAndFilmstrip();

        let message = `${uniqueRollIds.length} roll(s) removed from NexFilm`;
        if (choice === 'files') {
            message += `; ${result.deleted_source_files || 0} source file(s) deleted`;
            if (result.protected_source_files) message += `, ${result.protected_source_files} shared file(s) kept`;
            if (result.failed_source_files?.length) {
                showToast(`${message}. ${result.failed_source_files.length} source file(s) could not be deleted.`, 'error');
                return;
            }
        }
        showToast(message, 'success');
    } catch (error) {
        showToast(`Could not remove the roll: ${error}`, 'error');
    }
}

async function requestImageDeletion(ids) {
    if (importInProgress) {
        showToast('Wait for the current import to finish before deleting photos.', 'error');
        return;
    }
    const targets = getImageDeletionTargets(ids);
    if (targets.length === 0) return;
    const choice = await showDeleteImagesDialog(targets.length);
    if (!choice) return;

    const removedIdentities = new Set(targets.map(itemIdentity));
    const activeItem = activeId ? findKnownItem(activeId) : null;
    const activeTargeted = !!activeItem && removedIdentities.has(itemIdentity(activeItem));
    const filmstripIds = Array.from(filmstripContainer.querySelectorAll('.film-item[data-id]'))
        .map(element => element.dataset.id);
    const activeIndex = filmstripIds.indexOf(activeId);
    const remainingFilmstripIds = filmstripIds.filter(id => {
        const item = findKnownItem(id);
        return item && !removedIdentities.has(itemIdentity(item));
    });
    const nextId = activeTargeted
        ? remainingFilmstripIds[Math.min(Math.max(activeIndex, 0), remainingFilmstripIds.length - 1)] || null
        : null;

    try {
        if (activeTargeted) {
            await flushPendingBackendSync();
            await flushPendingThumbnail();
            currentImageRequestToken++;
            if (thumbnailSyncTimeout) {
                clearTimeout(thumbnailSyncTimeout);
                thumbnailSyncTimeout = null;
            }
            if (thumbnailCaptureRAF) {
                cancelAnimationFrame(thumbnailCaptureRAF);
                thumbnailCaptureRAF = null;
            }
        }

        const result = await invoke('delete_images', {
            images: targets.map(({ roll_id, file_path }) => ({ roll_id, file_path })),
            deleteSourceFiles: choice === 'files'
        });
        const removedIds = forgetImageItems(targets);
        for (const target of targets) {
            for (const key of rollPreviewCache.keys()) {
                if (key.startsWith(`${target.roll_id}:`)) rollPreviewCache.delete(key);
            }
        }

        if (activeTargeted) {
            activeId = null;
            activeProxyIsFull = false;
            hasProcessedActiveImage = false;
            proxyPixels = null;
            proxyWidth = 0;
            proxyHeight = 0;
            if (removedIds.has(missingFileId)) missingFileId = null;
            document.getElementById('missing-file-ui').classList.add('hidden');
            document.getElementById('missing-file-ui').classList.remove('flex');
            hideThumbnailPlaceholder();
            previewCanvas.style.display = 'none';
            disableUI();
        }

        allRolls = await invoke('get_rolls');
        await updateFilterSidebar();
        await renderLibraryAndFilmstrip();
        if (activeTargeted && nextId && findKnownItem(nextId)) {
            selectedLibraryIds.clear();
            selectedLibraryIds.add(nextId);
            await selectImage(nextId);
        }
        updateLibrarySelectionUI();
        btnDeleteDevelopImage.disabled = importInProgress || getImageDeletionTargets([activeId]).length === 0;

        let message = `${result.removed_images || targets.length} photo(s) removed from NexFilm`;
        if (choice === 'files') {
            message += `; ${result.deleted_source_files || 0} source file(s) deleted`;
            if (result.protected_source_files) message += `, ${result.protected_source_files} shared file(s) kept`;
            if (result.failed_source_files?.length) {
                showToast(`${message}. ${result.failed_source_files.length} source file(s) could not be deleted.`, 'error');
                return;
            }
        }
        showToast(message, 'success');
    } catch (error) {
        btnDeleteDevelopImage.disabled = importInProgress || getImageDeletionTargets([activeId]).length === 0;
        showToast(`Could not delete the selected photo(s): ${error}`, 'error');
    }
}

btnDeleteLibraryImages.addEventListener('click', () => requestImageDeletion(selectedLibraryIds));
btnDeleteDevelopImage.addEventListener('click', () => requestImageDeletion([activeId]));
btnDeleteRollImages.addEventListener('click', () => requestImageDeletion(selectedLibraryIds));

function showToast(message, type = "error") {
    const toast = document.createElement('div');
    toast.className = `px-4 py-3 rounded shadow-lg flex items-center gap-3 transform transition-all duration-300 translate-x-full ${type === 'error' ? 'bg-red-900/90 text-red-100 border border-red-700/50' : 'bg-zinc-800/90 text-zinc-100 border border-zinc-700/50'}`;
    toast.innerHTML = `
        <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            ${type === 'error' ? '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />' : '<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />'}
        </svg>
        <span class="text-[13px] font-medium tracking-wide">${message}</span>
    `;
    toastContainer.appendChild(toast);
    requestAnimationFrame(() => toast.classList.remove('translate-x-full'));
    setTimeout(() => {
        toast.classList.add('translate-x-full', 'opacity-0');
        setTimeout(() => { if (toast.parentNode) toast.parentNode.removeChild(toast); }, 300);
    }, 5000);
}

function updateSliderTrack(el) {
    const min = parseFloat(el.min);
    const max = parseFloat(el.max);
    const val = parseFloat(el.value);
    const percent = ((val - min) / (max - min)) * 100;
    el.style.setProperty('--val', `${percent}%`);
}

let thumbnailSyncTimeout = null;
let thumbnailCaptureRAF = null;
let lastThumbnailCaptureAt = 0;
let thumbnailPersistTimeout = null;
let pendingThumbnailPersist = null;

function captureActiveCanvasThumbnail() {
    if (!activeId || !previewCanvas || !activeProxyIsFull) return;
    try {
        const maxEdge = 640;
        const scale = Math.min(1, maxEdge / Math.max(previewCanvas.width || 1, previewCanvas.height || 1));
        const thumbCanvas = document.createElement('canvas');
        thumbCanvas.width = Math.max(1, Math.round((previewCanvas.width || 1) * scale));
        thumbCanvas.height = Math.max(1, Math.round((previewCanvas.height || 1) * scale));
        const ctx = thumbCanvas.getContext('2d', { alpha: false });
        ctx.drawImage(previewCanvas, 0, 0, thumbCanvas.width, thumbCanvas.height);
        const dataUrl = thumbCanvas.toDataURL('image/jpeg', 0.72);
        const persistId = activeId;
        const item = findKnownItem(activeId);
        if (item) {
            item.thumbnail_base64 = dataUrl;
            item.rendered_thumbnail_base64 = dataUrl;
            item.thumbnail_kind = 'rendered';
            rememberItem(item);
        }
        document.querySelectorAll(`img[data-img-id="${activeId}"]`).forEach(img => {
            img.src = dataUrl;
            img.style.transform = '';
            img.style.objectFit = 'cover';
            img.classList.remove('opacity-50', 'object-contain', 'p-4', 'p-2', 'bg-[#1C1C1E]');
            img.classList.add('object-cover');
        });
        pendingThumbnailPersist = {
            id: persistId,
            thumbnail: dataUrl.startsWith('data:') ? dataUrl.split(',')[1] : dataUrl
        };
        if (thumbnailPersistTimeout) clearTimeout(thumbnailPersistTimeout);
        thumbnailPersistTimeout = setTimeout(async () => {
            await flushPendingThumbnail();
        }, 1000);
    } catch (e) {
        console.error('thumbnail canvas capture failed', e);
    }
}

async function flushPendingThumbnail() {
    if (thumbnailPersistTimeout) {
        clearTimeout(thumbnailPersistTimeout);
        thumbnailPersistTimeout = null;
    }
    const pending = pendingThumbnailPersist;
    pendingThumbnailPersist = null;
    if (pending) {
        try {
            await invoke('set_thumbnail_data', pending);
        } catch (error) {
            if (!pendingThumbnailPersist) pendingThumbnailPersist = pending;
            throw error;
        }
    }
}

function scheduleInstantThumbnailUpdate() {
    if (!activeId || !activeProxyIsFull || thumbnailCaptureRAF) return;
    const now = performance.now();
    if (now - lastThumbnailCaptureAt < 250) return;
    lastThumbnailCaptureAt = now;
    thumbnailCaptureRAF = requestAnimationFrame(() => {
        thumbnailCaptureRAF = null;
        captureActiveCanvasThumbnail();
    });
}

function requestThumbnailSync() {
    scheduleInstantThumbnailUpdate();
    if (activeProxyIsFull) return;
    if (thumbnailSyncTimeout) clearTimeout(thumbnailSyncTimeout);
    thumbnailSyncTimeout = setTimeout(async () => {
        if (!activeId) return;
        try {
            await invoke('sync_thumbnail_buffer', { id: activeId });
            allLibraryItems = await invoke('get_filmstrip');
            rememberItems(allLibraryItems);
            const item = findKnownItem(activeId);
            if (item && item.thumbnail_base64 !== "FILE_MISSING") {
                document.querySelectorAll(`img[data-img-id="${activeId}"]`).forEach(img => {
                    setImageElementThumbnail(img, item.thumbnail_base64);
                });
            }
        } catch(e) { console.error(e); }
    }, 300);
}

function saveCurrentState() {
    if (!activeId) return null;
    const mode = btnModeColor.classList.contains('bg-[#28282c]') ? 'Color' : 'BW';
    const params = {
        film_mode: mode,
        d_min: currentDMin.slice(),
        d_max: currentDMax.slice(),
        exposure: parseFloat(sliders.exposure.el.value),
        gamma: parseFloat(sliders.gamma.el.value),
        saturation: parseFloat(sliders.saturation.el.value),
        temperature: parseFloat(sliders.temperature.el.value),
        tint: parseFloat(sliders.tint.el.value),
        exp_r: parseFloat(sliders.expr.el.value),
        exp_g: parseFloat(sliders.expg.el.value),
        exp_b: parseFloat(sliders.expb.el.value),
        highlights: parseFloat(sliders.highlights.el.value),
        shadows: parseFloat(sliders.shadows.el.value),
        lut_path: currentLutPath,
        lut_opacity: parseFloat(sliders.lutOpacity.el.value),
        working_colorspace: currentWorkingColorspace,
        sprocket_uv: Array.from(currentSprocketUV),
        sprocket_tolerance: currentSprocketTolerance,
        sprocket_feather: currentSprocketFeather
    };
    imageStates.set(activeId, { params, geom: JSON.parse(JSON.stringify(current_geom)) });
    return params;
}

async function updateBackendParams(targetId = activeId, paramsSnapshot = null) {
    // DOM event listeners pass an Event as their first argument. Keep the IPC
    // contract strict even if this helper is accidentally used as a callback.
    if (typeof targetId !== 'string') targetId = activeId;
    const params = paramsSnapshot || saveCurrentState();
    if (params && targetId) {
        const item = findKnownItem(targetId);
        const rollId = item?.roll_id || 'LOOSE_DEFAULT';
        try {
            await invoke('update_tuning_parameters', { id: targetId, params, rollId });
        } catch (error) {
            imageStates.delete(targetId);
            throw error;
        }
        // Note: caller (scheduleBackendSync) handles requestThumbnailSync after this completes
    }
}

// ==========================================
// WebGL Render Pipeline & Visualization
// ==========================================

let gl;
let shaderProgram;
let tex;
let vao;
let fbo;
let fboTex;

let u_base_density_loc;
let u_dmin_loc;
let u_dmax_loc;
let u_exposure_loc;
let u_gamma_loc;
let u_saturation_loc;
let u_temperature_loc;
let u_tint_loc;
let u_mode_loc;
let u_invert_enabled_loc;
let u_geometry_uv_loc;
let u_highlights_loc;
let u_shadows_loc;
let u_lut3d_loc;
let u_lut_opacity_loc;
let u_has_lut_loc;
let u_lut1d_loc;
let u_image_loc;
let u_crop_loc;
let u_homography_loc;
let u_perspective_loc;
let u_sprocket_uv_loc;
let u_sprocket_tolerance_loc;
let u_sprocket_feather_loc;
let u_calib_bounds_loc;
let u_calib_pts_loc;
let u_border_exposure_loc;
let u_baseline_pass_loc;

let currentBaseDensity = [0, 0, 0];
let proxyHasAnalyzedBase = false;
// Base analysis belongs to an image, not to the currently displayed buffer.
// Keep it while a proxy is reloaded after Auto Invert.
const proxyAnalyzedBaseIds = new Set();
let webGLInitialized = false;
let renderRequested = false;

function initWebGL() {
    gl = previewCanvas.getContext('webgl2', { preserveDrawingBuffer: true });
    if (!gl) {
        showToast("WebGL2 is not supported by your browser.", "error");
        return;
    }

    const vsSource = `#version 300 es
    in vec4 a_position;
    in vec2 a_texcoord;
    out vec2 v_texcoord;
    uniform vec4 u_crop;
    void main() {
        gl_Position = a_position;
        vec2 base_uv = vec2(a_texcoord.x, 1.0 - a_texcoord.y);
        v_texcoord = vec2(
            u_crop.x + base_uv.x * u_crop.z,
            u_crop.y + base_uv.y * u_crop.w
        );
    }`;

    const fsSource = `#version 300 es
    precision highp float;
    in vec2 v_texcoord;
    out vec4 outColor;

    uniform mediump usampler2D u_image;
    uniform vec3 u_base_density;
    uniform vec3 u_dmin;
    uniform vec3 u_dmax;
    uniform vec3 u_exposure;
    uniform float u_gamma;
    uniform float u_saturation;
    uniform float u_temperature;
    uniform float u_tint;
    uniform int u_mode;
    uniform int u_invert_enabled;
    
    uniform float u_highlights;
    uniform float u_shadows;
    
    uniform mediump sampler3D u_lut3d;
    uniform mediump sampler2D u_lut1d;
    uniform float u_lut_opacity;
    uniform int u_has_lut;
    uniform int u_lut_is_1d;
    
    uniform mat3 u_homography;
    uniform vec4 u_perspective;
    uniform mat3 u_geometry_uv;
    uniform vec2 u_sprocket_uv;
    uniform float u_sprocket_tolerance;
    uniform float u_sprocket_feather;
    uniform vec4 u_calib_bounds;

    const mat3 STATUS_M = mat3(
        1.0197, -0.0052, 0.0131,
        0.0317, 0.8933, -0.0011,
        0.0091, 0.0521, 0.9712
    );

    float getLuma(vec3 c) {
        return dot(c, vec3(0.299, 0.587, 0.114));
    }

    float tonePostGamma(float value) {
        float clamped = clamp(value, 0.0, 1.0);
        return clamp(value
            + u_shadows * pow(1.0 - clamped, 2.0) * value
            + u_highlights * pow(clamped, 2.0) * (1.0 - value), 0.0, 1.0);
    }

    vec3 applyPostGammaAdjustments(vec3 color) {
        color = vec3(tonePostGamma(color.r), tonePostGamma(color.g), tonePostGamma(color.b));
        if (u_mode != 0) return color;

        float temperature = clamp(u_temperature, -1.0, 1.0);
        color.r *= 1.0 + temperature * 0.20;
        color.b *= 1.0 - temperature * 0.20;

        float tint = clamp(u_tint, -1.0, 1.0);
        color.r *= 1.0 + tint * 0.10;
        color.g *= 1.0 - tint * 0.20;
        color.b *= 1.0 + tint * 0.10;

        float luma = getLuma(color);
        float saturation = 1.0 + clamp(u_saturation, -1.0, 1.0);
        return clamp(vec3(
            luma + (color.r - luma) * saturation,
            luma + (color.g - luma) * saturation,
            luma + (color.b - luma) * saturation
        ), 0.0, 1.0);
    }

    vec2 applyHomography(vec2 uv, mat3 h) {
        vec3 p = h * vec3(uv, 1.0);
        return p.xy / p.z;
    }

    vec2 applyPerspective(vec2 uv) {
        float safeScale = clamp(u_perspective.w, 0.5, 3.0);
        float aspectScale = exp(clamp(u_perspective.z, -100.0, 100.0) * 0.0035);
        vec2 centered = vec2(
            (uv.x * 2.0 - 1.0) / (safeScale * aspectScale),
            (uv.y * 2.0 - 1.0) / safeScale
        );
        float divisor = 1.0
            + clamp(u_perspective.y, -100.0, 100.0) * 0.003 * centered.x
            + clamp(u_perspective.x, -100.0, 100.0) * 0.003 * centered.y;
        return (centered / divisor + 1.0) * 0.5;
    }

    vec2 mapOrientedToSource(vec2 uv) {
        vec3 p = u_geometry_uv * vec3(uv, 1.0);
        return p.xy / p.z;
    }

    float lutTextureCoord(float value, float size) {
        return (clamp(value, 0.0, 1.0) * (size - 1.0) + 0.5) / size;
    }

    vec3 lutTextureCoord(vec3 value, vec3 size) {
        return (clamp(value, 0.0, 1.0) * (size - 1.0) + 0.5) / size;
    }

    void main() {
        vec2 oriented_uv = applyHomography(applyPerspective(v_texcoord), u_homography);
        vec2 warped_uv = mapOrientedToSource(oriented_uv);
        if (warped_uv.x < 0.0 || warped_uv.x > 1.0 || warped_uv.y < 0.0 || warped_uv.y > 1.0) {
            outColor = vec4(0.0, 0.0, 0.0, 1.0);
            return;
        }

        float mask = 0.0;
        if (u_sprocket_uv.x >= 0.0) {
            vec2 sprocket_source_uv = mapOrientedToSource(u_sprocket_uv);
            vec3 raw_color = vec3(texture(u_image, warped_uv).rgb) / 65535.0;
            vec3 raw_target = vec3(texture(u_image, sprocket_source_uv).rgb) / 65535.0;
            float luma_diff = abs(getLuma(raw_color) - getLuma(raw_target));
            mask = pow(1.0 - smoothstep(u_sprocket_tolerance, u_sprocket_tolerance + u_sprocket_feather + 0.0001, luma_diff), 3.0);
        }

        uvec4 texel = texture(u_image, warped_uv);
        vec3 raw_rgb = vec3(texel.rgb) / 65535.0;

        // Develop staging view: keep the linear sensor output visibly
        // negative until the user explicitly confirms the film area and runs
        // Auto Invert. This prevents a loaded proxy from silently becoming a
        // positive image because the density shader ran with a zero base.
        if (u_invert_enabled == 0) {
            vec3 staged = clamp(raw_rgb * exp2(u_exposure), 0.0, 1.0);
            if (u_mode != 0) {
                staged = vec3(getLuma(staged));
            }
            float safe_gamma = max(u_gamma, 1e-6);
            outColor = vec4(pow(staged, vec3(1.0 / safe_gamma)), 1.0);
            return;
        }
        
        float epsilon = 1e-6;
        float t_r = max(float(texel.r) / 65535.0, epsilon);
        float t_g = max(float(texel.g) / 65535.0, epsilon);
        float t_b = max(float(texel.b) / 65535.0, epsilon);
        
        vec3 density = vec3(-log(t_r) / log(10.0), -log(t_g) / log(10.0), -log(t_b) / log(10.0));
        
        if (u_mode == 0) {
            density = STATUS_M * (density - u_base_density);
        } else {
            density = density - u_base_density;
            float gray = (density.r + density.g + density.b) / 3.0;
            density = vec3(gray);
        }
        
        if (u_mode == 0) {
            density += u_exposure;
        } else {
            density += vec3(u_exposure.r);
        }
        
        vec3 effective_dmin = u_dmin;
        vec3 effective_dmax = u_dmax;
        if (u_mode != 0) {
            float bw_dmin = (u_dmin.r + u_dmin.g + u_dmin.b) / 3.0;
            float bw_dmax = (u_dmax.r + u_dmax.g + u_dmax.b) / 3.0;
            effective_dmin = vec3(bw_dmin);
            effective_dmax = vec3(bw_dmax);
        }
        vec3 density_range = effective_dmax - effective_dmin;
        bvec3 valid_range = greaterThan(abs(density_range), vec3(1e-6));
        vec3 safe_range = mix(vec3(1.0), density_range, valid_range);
        vec3 norm = mix(vec3(0.0), (density - effective_dmin) / safe_range, valid_range);
        
        float safe_gamma = max(u_gamma, 1e-6);
        vec3 final_rgb = vec3(
            pow(clamp(norm.r, 0.0, 1.0), 1.0 / safe_gamma),
            pow(clamp(norm.g, 0.0, 1.0), 1.0 / safe_gamma),
            pow(clamp(norm.b, 0.0, 1.0), 1.0 / safe_gamma)
        );
        final_rgb = applyPostGammaAdjustments(final_rgb);
        if (u_has_lut == 1) {
            vec3 lut_in = clamp(final_rgb, 0.0, 1.0);
            vec3 lut_color;
            if (u_lut_is_1d == 1) {
                float lut_size = float(textureSize(u_lut1d, 0).x);
                lut_color.r = texture(u_lut1d, vec2(lutTextureCoord(lut_in.r, lut_size), 0.5)).r;
                lut_color.g = texture(u_lut1d, vec2(lutTextureCoord(lut_in.g, lut_size), 0.5)).g;
                lut_color.b = texture(u_lut1d, vec2(lutTextureCoord(lut_in.b, lut_size), 0.5)).b;
            } else {
                vec3 lut_size = vec3(textureSize(u_lut3d, 0));
                lut_color = texture(u_lut3d, lutTextureCoord(lut_in, lut_size)).rgb;
            }
            final_rgb = mix(final_rgb, lut_color, u_lut_opacity);
        }
        if (u_mode != 0) {
            final_rgb = vec3(getLuma(final_rgb));
        }

        if (u_sprocket_uv.x >= 0.0) {
            // Spatial Masking: skip if inside calibration quad
            if (!(v_texcoord.x >= u_calib_bounds.x && v_texcoord.x <= u_calib_bounds.z && 
                  v_texcoord.y >= u_calib_bounds.y && v_texcoord.y <= u_calib_bounds.w)) {
                final_rgb = mix(final_rgb, vec3(1.0), mask);
            }
        }
        
        outColor = vec4(final_rgb, 1.0);
    }`;

    function createShader(gl, type, source) {
        const shader = gl.createShader(type);
        gl.shaderSource(shader, source);
        gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
            const info = gl.getShaderInfoLog(shader);
            console.error("Shader compilation error:", info);
            showToast("Shader error: " + info.substring(0, 50), "error");
            return null;
        }
        return shader;
    }

    const vs = createShader(gl, gl.VERTEX_SHADER, vsSource);
    const fs = createShader(gl, gl.FRAGMENT_SHADER, fsSource);

    shaderProgram = gl.createProgram();
    gl.attachShader(shaderProgram, vs);
    gl.attachShader(shaderProgram, fs);
    gl.linkProgram(shaderProgram);

    const posLoc = gl.getAttribLocation(shaderProgram, "a_position");
    const texLoc = gl.getAttribLocation(shaderProgram, "a_texcoord");

    u_base_density_loc = gl.getUniformLocation(shaderProgram, "u_base_density");
    u_dmin_loc = gl.getUniformLocation(shaderProgram, "u_dmin");
    u_dmax_loc = gl.getUniformLocation(shaderProgram, "u_dmax");
    u_exposure_loc = gl.getUniformLocation(shaderProgram, "u_exposure");
    u_gamma_loc = gl.getUniformLocation(shaderProgram, "u_gamma");
    u_saturation_loc = gl.getUniformLocation(shaderProgram, "u_saturation");
    u_temperature_loc = gl.getUniformLocation(shaderProgram, "u_temperature");
    u_tint_loc = gl.getUniformLocation(shaderProgram, "u_tint");
    u_mode_loc = gl.getUniformLocation(shaderProgram, "u_mode");
    u_invert_enabled_loc = gl.getUniformLocation(shaderProgram, "u_invert_enabled");
    u_geometry_uv_loc = gl.getUniformLocation(shaderProgram, "u_geometry_uv");
    u_highlights_loc = gl.getUniformLocation(shaderProgram, "u_highlights");
    u_shadows_loc = gl.getUniformLocation(shaderProgram, "u_shadows");
    u_lut3d_loc = gl.getUniformLocation(shaderProgram, "u_lut3d");
    u_lut1d_loc = gl.getUniformLocation(shaderProgram, "u_lut1d");
    u_lut_opacity_loc = gl.getUniformLocation(shaderProgram, "u_lut_opacity");
    u_has_lut_loc = gl.getUniformLocation(shaderProgram, "u_has_lut");
    u_lut_is_1d_loc = gl.getUniformLocation(shaderProgram, "u_lut_is_1d");
    u_image_loc = gl.getUniformLocation(shaderProgram, "u_image");
    u_crop_loc = gl.getUniformLocation(shaderProgram, "u_crop");
    u_homography_loc = gl.getUniformLocation(shaderProgram, "u_homography");
    u_perspective_loc = gl.getUniformLocation(shaderProgram, "u_perspective");
    u_sprocket_uv_loc = gl.getUniformLocation(shaderProgram, "u_sprocket_uv");
    u_sprocket_tolerance_loc = gl.getUniformLocation(shaderProgram, "u_sprocket_tolerance");
    u_sprocket_feather_loc = gl.getUniformLocation(shaderProgram, "u_sprocket_feather");
    u_calib_bounds_loc = gl.getUniformLocation(shaderProgram, "u_calib_bounds");
    
    gl.getExtension("OES_texture_float_linear");

    vao = gl.createVertexArray();
    gl.bindVertexArray(vao);

    const posBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, posBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(posLoc);
    gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

    const texBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, texBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(texLoc);
    gl.vertexAttribPointer(texLoc, 2, gl.FLOAT, false, 0, 0);

    tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);

    // Setup FBO for Histogram
    fboTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, fboTex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, HIST_W, HIST_H, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);

    fbo = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, fboTex, 0);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);

    webGLInitialized = true;
}

initWebGL();

let hasLUT = false;
let is1DLUT = false;
let lutTex = null;
let currentLutPath = null;
let currentWorkingColorspace = 'linear-srgb';

const selectColorspace = document.getElementById('select-colorspace');
const btnLoadLUT = document.getElementById('btn-load-lut');

selectColorspace.addEventListener('change', async (e) => {
    const previousColorspace = currentWorkingColorspace;
    try {
        currentWorkingColorspace = e.target.value;
        await updateBackendParams();
        if (activeId) {
            proxyCache.delete(activeId);
            readyProxyIds.delete(activeId);
            activeProxyIsFull = false;
            if (hasProcessedActiveImage) {
                const reloaded = await reloadDevelopProxy(current_geom, { prepare: true });
                if (!reloaded) throw new Error("Could not rebuild the proxy in the selected working space.");
            }
        }
    } catch(e) {
        currentWorkingColorspace = previousColorspace;
        e.target.value = previousColorspace;
        try {
            await updateBackendParams();
            if (hasProcessedActiveImage) {
                await reloadDevelopProxy(current_geom, { prepare: true });
            }
        } catch (rollbackError) {
            console.error("Failed to restore RAW working space", rollbackError);
        }
        showToast("Failed to update RAW working space: " + e, "error");
        console.error(e);
    }
});

const selectBuiltinLut = document.getElementById('select-builtin-lut');

async function initBuiltins() {
    try {
        const luts = await invoke('get_builtin_luts');
        if (luts && luts.length > 0) {
            luts.forEach(p => {
                const name = p.split('\\').pop().split('/').pop();
                const opt = document.createElement('option');
                opt.value = p; opt.textContent = name;
                selectBuiltinLut.appendChild(opt);
            });
        }
    } catch (e) {
        console.error("Failed to load builtins", e);
    }
}
initBuiltins();

async function applyLUT(lutData, sourcePath = null, quiet = false) {
    const size = lutData.size;
    is1DLUT = lutData.is_1d;
    const data = new Float32Array(new Uint8Array(lutData.data).buffer);
    
    if (!lutTex) lutTex = gl.createTexture();
    
    if (is1DLUT) {
        gl.bindTexture(gl.TEXTURE_2D, lutTex);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, size, 1, 0, gl.RGBA, gl.FLOAT, data);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    } else {
        gl.bindTexture(gl.TEXTURE_3D, lutTex);
        gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGBA32F, size, size, size, 0, gl.RGBA, gl.FLOAT, data);
        gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_WRAP_R, gl.CLAMP_TO_EDGE);
    }
    
    hasLUT = true;
    currentLutPath = sourcePath;
    sliders.lutOpacity.el.disabled = false;
    if (!quiet) showToast(is1DLUT ? "1D LUT loaded." : "3D LUT loaded.", "success");
    requestRender();
    requestThumbnailSync();
    if (activeId && !quiet) await updateBackendParams();
}

async function restoreLutForImage(params) {
    const path = params?.lut_path || null;
    sliders.lutOpacity.el.value = params?.lut_opacity ?? 1.0;
    sliders.lutOpacity.val.textContent = parseFloat(sliders.lutOpacity.el.value).toFixed(3);
    updateSliderTrack(sliders.lutOpacity.el);
    if (!path) {
        hasLUT = false;
        currentLutPath = null;
        selectBuiltinLut.value = "";
        sliders.lutOpacity.el.disabled = true;
        return;
    }
    const alreadyLoaded = hasLUT && currentLutPath === path;
    currentLutPath = path;
    selectBuiltinLut.value = Array.from(selectBuiltinLut.options).some(option => option.value === path)
        ? path
        : "";
    if (alreadyLoaded) {
        sliders.lutOpacity.el.disabled = false;
        return;
    }
    try {
        const lutData = await invoke('load_3d_lut', { path });
        await applyLUT(lutData, path, true);
    } catch (error) {
        hasLUT = false;
        sliders.lutOpacity.el.disabled = true;
        showToast(`Failed to restore LUT: ${error}`, "error");
    }
}

selectBuiltinLut.addEventListener('change', async (e) => {
    if (!e.target.value) {
        hasLUT = false;
        currentLutPath = null;
        sliders.lutOpacity.el.disabled = true;
        updateBackendParams();
        requestRender();
        requestThumbnailSync();
        return;
    }
    try {
        const lutData = await invoke('load_3d_lut', { path: e.target.value });
        await applyLUT(lutData, e.target.value);
    } catch(err) {
        showToast("Failed to load built-in LUT", "error");
    }
});

btnLoadLUT.addEventListener('click', async () => {
    try {
        const path = await invoke('open_lut_dialog');
        if (path) {
            selectBuiltinLut.value = "";
            const lutData = await invoke('load_3d_lut', { path });
            await applyLUT(lutData, path);
        }
    } catch(e) {
        console.error(e);
        showToast("Failed to load LUT", "error");
    }
});

function setVisualizationMode(mode, { focus = false } = {}) {
    isWaveform = mode === 'waveform';
    const selectedButton = isWaveform ? btnVizWaveform : btnVizHistogram;
    const unselectedButton = isWaveform ? btnVizHistogram : btnVizWaveform;
    selectedButton.classList.add('active');
    selectedButton.setAttribute('aria-selected', 'true');
    selectedButton.tabIndex = 0;
    unselectedButton.classList.remove('active');
    unselectedButton.setAttribute('aria-selected', 'false');
    unselectedButton.tabIndex = -1;
    histCanvas.classList.toggle('hidden', isWaveform);
    waveCanvas.classList.toggle('hidden', !isWaveform);
    if (lastPixels) updateDataViz(lastPixels);
    if (focus) selectedButton.focus();
}

btnVizHistogram.addEventListener('click', () => setVisualizationMode('histogram'));
btnVizWaveform.addEventListener('click', () => setVisualizationMode('waveform'));
vizModeTabs.addEventListener('keydown', event => {
    let mode = null;
    if (event.key === 'ArrowLeft' || event.key === 'Home') mode = 'histogram';
    if (event.key === 'ArrowRight' || event.key === 'End') mode = 'waveform';
    if (!mode) return;
    event.preventDefault();
    setVisualizationMode(mode, { focus: true });
});

function drawHistogram(pixels) {
    const rHist = new Uint32Array(256);
    const gHist = new Uint32Array(256);
    const bHist = new Uint32Array(256);
    const lHist = new Uint32Array(256);

    const len = pixels.length;
    for (let i = 0; i < len; i += 4) {
        const r = pixels[i], g = pixels[i+1], b = pixels[i+2];
        const l = Math.round(0.299*r + 0.587*g + 0.114*b);
        rHist[r]++; gHist[g]++; bHist[b]++; lHist[l]++;
    }

    // Use a robust interior-bin scale so isolated clipping spikes do not
    // flatten the useful distribution. Endpoint peaks remain visible, capped.
    const maxVal = getHistogramScale([rHist, gHist, bHist]);

    histCanvas.width = histCanvas.offsetWidth;
    histCanvas.height = histCanvas.offsetHeight;
    const w = histCanvas.width, h = histCanvas.height;
    
    histCtx.clearRect(0, 0, w, h);
    histCtx.globalCompositeOperation = 'screen';

    function drawChannel(hist, color) {
        histCtx.fillStyle = color;
        histCtx.beginPath();
        histCtx.moveTo(0, h);
        for (let i = 0; i < 256; i++) {
            const x = (i / 255) * w;
            const y = getHistogramY(hist[i], maxVal, h);
            histCtx.lineTo(x, y);
        }
        histCtx.lineTo(w, h);
        histCtx.fill();
    }

    drawChannel(rHist, 'rgba(255, 60, 60, 0.6)');
    drawChannel(gHist, 'rgba(60, 255, 60, 0.6)');
    drawChannel(bHist, 'rgba(60, 60, 255, 0.6)');
    
    histCtx.globalCompositeOperation = 'source-over';
    
    histCtx.strokeStyle = 'rgba(255, 255, 255, 0.4)';
    histCtx.lineWidth = 1;
    histCtx.beginPath();
    for (let i = 0; i < 256; i++) {
        const x = (i / 255) * w;
        const y = getHistogramY(lHist[i], maxVal, h);
        if (i === 0) histCtx.moveTo(x, y);
        else histCtx.lineTo(x, y);
    }
    histCtx.stroke();
}

function drawWaveform(pixels) {
    waveCanvas.width = waveCanvas.offsetWidth * 2;
    waveCanvas.height = waveCanvas.offsetHeight * 2;
    const w = waveCanvas.width, h = waveCanvas.height;
    
    waveCtx.clearRect(0, 0, w, h);
    waveCtx.globalCompositeOperation = 'screen';
    
    const colW = w / 3;
    
    // Draw Red
    waveCtx.fillStyle = 'rgba(255, 60, 60, 0.15)';
    for (let y = 0; y < HIST_H; y+=1) {
        for (let x = 0; x < HIST_W; x+=1) {
            const idx = (y * HIST_W + x) * 4;
            const r = pixels[idx];
            const plotX = (x / HIST_W) * colW;
            const plotY_R = h - (r / 255.0) * h;
            waveCtx.fillRect(plotX, plotY_R, 1.5, 1.5);
        }
    }
    
    // Draw Green
    waveCtx.fillStyle = 'rgba(60, 255, 60, 0.15)';
    for (let y = 0; y < HIST_H; y+=1) {
        for (let x = 0; x < HIST_W; x+=1) {
            const idx = (y * HIST_W + x) * 4;
            const g = pixels[idx+1];
            const plotX = colW + (x / HIST_W) * colW;
            const plotY_G = h - (g / 255.0) * h;
            waveCtx.fillRect(plotX, plotY_G, 1.5, 1.5);
        }
    }
    
    // Draw Blue
    waveCtx.fillStyle = 'rgba(60, 150, 255, 0.15)';
    for (let y = 0; y < HIST_H; y+=1) {
        for (let x = 0; x < HIST_W; x+=1) {
            const idx = (y * HIST_W + x) * 4;
            const b = pixels[idx+2];
            const plotX = colW * 2 + (x / HIST_W) * colW;
            const plotY_B = h - (b / 255.0) * h;
            waveCtx.fillRect(plotX, plotY_B, 1.5, 1.5);
        }
    }
}

let vizTimeout = null;
let lastVizTime = 0;
function updateDataViz(pixels) {
    lastPixels = pixels;
    const now = performance.now();
    if (now - lastVizTime < 33) {
        if (!vizTimeout) {
            vizTimeout = setTimeout(() => {
                vizTimeout = null;
                lastVizTime = performance.now();
                if (isWaveform) drawWaveform(lastPixels);
                else drawHistogram(lastPixels);
            }, 33 - (now - lastVizTime));
        }
        return;
    }
    lastVizTime = now;
    if (vizTimeout) { clearTimeout(vizTimeout); vizTimeout = null; }
    if (isWaveform) drawWaveform(pixels);
    else drawHistogram(pixels);
}

let currentSprocketUV = new Float32Array([-1.0, -1.0]);
let currentSprocketTolerance = 0.10;
let currentSprocketFeather = 0.05;

function getHomography(pts) {
    const x0 = pts[0][0], y0 = pts[0][1];
    const x1 = pts[1][0], y1 = pts[1][1];
    const x2 = pts[2][0], y2 = pts[2][1];
    const x3 = pts[3][0], y3 = pts[3][1];

    const dx1 = x1 - x2;
    const dx2 = x3 - x2;
    const dx3 = x0 - x1 + x2 - x3;

    const dy1 = y1 - y2;
    const dy2 = y3 - y2;
    const dy3 = y0 - y1 + y2 - y3;

    let a, b, c, d, e, f, g, h;
    c = x0;
    f = y0;

    const det = dx1 * dy2 - dy1 * dx2;
    if (Math.abs(det) < 1e-6) {
        a = x1 - x0; b = x3 - x0;
        d = y1 - y0; e = y3 - y0;
        g = 0.0; h = 0.0;
    } else {
        g = (dx3 * dy2 - dy3 * dx2) / det;
        h = (dx1 * dy3 - dy1 * dx3) / det;
        a = x1 - x0 + g * x1;
        b = x3 - x0 + h * x3;
        d = y1 - y0 + g * y1;
        e = y3 - y0 + h * y3;
    }

    let minX = Math.min(x0, x1, x2, x3);
    let maxX = Math.max(x0, x1, x2, x3);
    let minY = Math.min(y0, y1, y2, y3);
    let maxY = Math.max(y0, y1, y2, y3);

    let Sx = 1.0 / Math.max(0.001, maxX - minX);
    let Sy = 1.0 / Math.max(0.001, maxY - minY);
    let Tx = -minX * Sx;
    let Ty = -minY * Sy;

    return new Float32Array([
        a * Sx, d * Sx, g * Sx,
        b * Sy, e * Sy, h * Sy,
        a * Tx + b * Ty + c, d * Tx + e * Ty + f, g * Tx + h * Ty + 1.0
    ]);
}

function requestRender() {
    if (!webGLInitialized || renderRequested) return;
    renderRequested = true;
    requestAnimationFrame(renderWebGL);
}

function renderWebGL() {
    renderRequested = false;
    if (!gl || !activeId) return;
    if (!activeProxyIsFull || !proxyPixels || proxyWidth <= 0 || proxyHeight <= 0) return;

    gl.useProgram(shaderProgram);
    gl.bindVertexArray(vao);

    const mode = btnModeColor.classList.contains('bg-[#28282c]') ? 0 : 1;
    // dmin/dmax are tracked globally
    const expVal = parseFloat(sliders.exposure.el.value);
    const exprVal = parseFloat(sliders.expr.el.value);
    const expgVal = parseFloat(sliders.expg.el.value);
    const expbVal = parseFloat(sliders.expb.el.value);
    const gammaVal = parseFloat(sliders.gamma.el.value);

        gl.uniform3f(
            u_base_density_loc,
            proxyHasAnalyzedBase ? currentBaseDensity[0] : 0.0,
            proxyHasAnalyzedBase ? currentBaseDensity[1] : 0.0,
            proxyHasAnalyzedBase ? currentBaseDensity[2] : 0.0,
        );
    gl.uniform3f(u_dmin_loc, currentDMin[0], currentDMin[1], currentDMin[2]);
    gl.uniform3f(u_dmax_loc, currentDMax[0], currentDMax[1], currentDMax[2]);
    gl.uniform3f(
        u_exposure_loc,
        mode === 0 ? expVal + exprVal * CHANNEL_CONTROL_SCALE : expVal,
        mode === 0 ? expVal + expgVal * CHANNEL_CONTROL_SCALE : expVal,
        mode === 0 ? expVal + expbVal * CHANNEL_CONTROL_SCALE : expVal
    );
    gl.uniform1f(u_gamma_loc, gammaVal);
    gl.uniform1f(u_saturation_loc, parseFloat(sliders.saturation.el.value));
    gl.uniform1f(u_temperature_loc, parseFloat(sliders.temperature.el.value));
    gl.uniform1f(u_tint_loc, parseFloat(sliders.tint.el.value));
    gl.uniform1i(u_mode_loc, mode);
    gl.uniform1i(u_invert_enabled_loc, proxyHasAnalyzedBase ? 1 : 0);
    
    gl.uniform1f(u_highlights_loc, parseFloat(sliders.highlights.el.value));
    gl.uniform1f(u_shadows_loc, parseFloat(sliders.shadows.el.value));
    gl.uniform1f(u_lut_opacity_loc, parseFloat(sliders.lutOpacity.el.value) * LUT_CONTROL_SCALE);
    gl.uniform1i(u_has_lut_loc, hasLUT ? 1 : 0);
    gl.uniform1i(u_lut_is_1d_loc, is1DLUT ? 1 : 0);
    gl.uniform1i(u_lut3d_loc, 1);
    gl.uniform1i(u_lut1d_loc, 2);
    gl.uniform1i(u_image_loc, 0);
    let pts = NexFilmGeometry.resolveCalibrationRenderPoints(
        current_geom.calibration_points,
        isCalibrationMode
    );
    let minX = Math.min(pts[0][0], pts[1][0], pts[2][0], pts[3][0]);
    let maxX = Math.max(pts[0][0], pts[1][0], pts[2][0], pts[3][0]);
    let minY = Math.min(pts[0][1], pts[1][1], pts[2][1], pts[3][1]);
    let maxY = Math.max(pts[0][1], pts[1][1], pts[2][1], pts[3][1]);
    
    let homographyMat = getHomography(pts);
    gl.uniformMatrix3fv(u_homography_loc, false, homographyMat);
    gl.uniform4f(
        u_perspective_loc,
        current_geom.perspective_vertical,
        current_geom.perspective_horizontal,
        current_geom.perspective_aspect,
        current_geom.perspective_scale
    );
    gl.uniform2fv(u_sprocket_uv_loc, currentSprocketUV);
    gl.uniform1f(u_sprocket_tolerance_loc, currentSprocketTolerance);
    gl.uniform1f(u_sprocket_feather_loc, currentSprocketFeather);
    gl.uniform4f(u_calib_bounds_loc, minX, minY, maxX, maxY);
    
    const geometryUv = NexFilmGeometry.createInverseGeometryMatrix(
        proxyWidth,
        proxyHeight,
        current_geom
    );
    gl.uniformMatrix3fv(u_geometry_uv_loc, false, geometryUv);

    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    if (hasLUT) {
        if (is1DLUT) {
            gl.activeTexture(gl.TEXTURE2);
            gl.bindTexture(gl.TEXTURE_2D, lutTex);
        } else {
            gl.activeTexture(gl.TEXTURE1);
            gl.bindTexture(gl.TEXTURE_3D, lutTex);
        }
    }

    // Render to FBO for Histogram. Keep readback at the visualization cadence;
    // the main canvas itself remains fully GPU-driven for 60fps slider motion.
    gl.uniform4f(u_crop_loc, current_geom.crop_rect.x, current_geom.crop_rect.y, current_geom.crop_rect.width, current_geom.crop_rect.height);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.viewport(0, 0, HIST_W, HIST_H);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
    
    const pixels = new Uint8Array(HIST_W * HIST_H * 4);
    const now = performance.now();
    if (now - lastVizTime >= 33) {
        gl.readPixels(0, 0, HIST_W, HIST_H, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    } else if (lastHistPixels) {
        pixels.set(lastHistPixels);
    } else {
        gl.readPixels(0, 0, HIST_W, HIST_H, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    }
    lastHistPixels = pixels;

    // Render to Main Canvas
    const orientedSize = NexFilmGeometry.getOrientedDimensions(
        proxyWidth,
        proxyHeight,
        current_geom
    );
    if (!isCropMode) {
        gl.uniform4f(u_crop_loc, current_geom.crop_rect.x, current_geom.crop_rect.y, current_geom.crop_rect.width, current_geom.crop_rect.height);
        gl.canvas.width = Math.max(1, Math.round(orientedSize.width * current_geom.crop_rect.width));
        gl.canvas.height = Math.max(1, Math.round(orientedSize.height * current_geom.crop_rect.height));
    } else {
        gl.uniform4f(u_crop_loc, 0.0, 0.0, 1.0, 1.0);
        gl.canvas.width = Math.max(1, Math.round(orientedSize.width));
        gl.canvas.height = Math.max(1, Math.round(orientedSize.height));
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, gl.canvas.width, gl.canvas.height);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    requestAnimationFrame(() => updateDataViz(pixels));
    
    scheduleInstantThumbnailUpdate();
}

const PROXY_CACHE_LIMIT = 8; // Matches backend MAX_PROXY_CACHE
const proxyCache = new Map(); // key: id, value: { arrayBuffer, geomKey, lastUsed: Date.now() }
const readyProxyIds = new Set();

function getProxyGeomKey() {
    // The backend proxy is canonical RAW pixels. Crop/rotate/flip are applied
    // by WebGL, so they must not invalidate or re-label the pixel cache.
    return currentWorkingColorspace;
}

function getFromCache(id) {
    if (proxyCache.has(id)) {
        const item = proxyCache.get(id);
        if (item.geomKey !== getProxyGeomKey()) {
            proxyCache.delete(id);
            return null;
        }
        item.lastUsed = Date.now();
        return item.arrayBuffer;
    }
    return null;
}

function addToCache(id, arrayBuffer, geom = current_geom) {
    if (proxyCache.has(id)) {
        const item = proxyCache.get(id);
        item.geomKey = getProxyGeomKey(geom);
        item.lastUsed = Date.now();
        item.arrayBuffer = arrayBuffer;
        return;
    }
    if (proxyCache.size >= PROXY_CACHE_LIMIT) {
        let oldestId = null;
        let oldestTime = Infinity;
        for (const [key, val] of proxyCache.entries()) {
            if (val.lastUsed < oldestTime) {
                oldestTime = val.lastUsed;
                oldestId = key;
            }
        }
        if (oldestId) {
            proxyCache.delete(oldestId);
        }
    }
    proxyCache.set(id, { arrayBuffer, geomKey: getProxyGeomKey(geom), lastUsed: Date.now() });
}

async function loadProxyImage(token = null, loadedGeom = current_geom) {
    if (!activeId || !webGLInitialized) return;
    
    const requestedId = activeId;
    const requestedGeom = JSON.parse(JSON.stringify(loadedGeom));
    const requestedGeomKey = getProxyGeomKey(requestedGeom);
    let arrayBuffer = getFromCache(activeId);
    let byteOffset = 0;
    
    const loadingMask = document.getElementById('loading-proxy-ui');
    
    if (!arrayBuffer) {
        try {
            const result = await invoke('get_proxy_image_data', { id: activeId });
            if (token !== null && token !== currentImageRequestToken) {
                if (loadingMask) loadingMask.classList.add('hidden');
                return;
            }
            if (requestedGeomKey !== getProxyGeomKey(current_geom)) return false;
            
            if (result instanceof ArrayBuffer) {
                arrayBuffer = result;
            } else if (result.buffer instanceof ArrayBuffer) {
                const resultOffset = result.byteOffset || 0;
                const resultLength = result.byteLength || result.length || result.buffer.byteLength;
                arrayBuffer = result.buffer.slice(resultOffset, resultOffset + resultLength);
                byteOffset = 0;
            } else if (Array.isArray(result)) {
                arrayBuffer = new Uint8Array(result).buffer;
            }
        } catch(e) {
            if (e === "PROXY_NOT_READY") {
                activeProxyIsFull = false;
                readyProxyIds.delete(requestedId);
                if (loadingMask) loadingMask.classList.add('hidden');
                return false;
            }
            console.error("Failed to load proxy", e); 
            if (loadingMask) loadingMask.classList.add('hidden');
            return false;
        }
        if (loadingMask) loadingMask.classList.add('hidden');
    }

    if (!arrayBuffer) return;

    try {
        const dataView = new DataView(arrayBuffer, byteOffset);
        const width = dataView.getUint32(0, true);
        const height = dataView.getUint32(4, true);
        const isFullProxy = dataView.byteLength >= 24 ? dataView.getUint32(20, true) === 1 : true;

        if (!isFullProxy) {
            activeProxyIsFull = false;
            readyProxyIds.delete(requestedId);
            const loadingMask = document.getElementById('loading-proxy-ui');
            if (loadingMask) loadingMask.classList.add('hidden');
            return false;
        }
        
        currentBaseDensity[0] = dataView.getFloat32(8, true);
        currentBaseDensity[1] = dataView.getFloat32(12, true);
        currentBaseDensity[2] = dataView.getFloat32(16, true);
        
        const pixels = new Uint16Array(arrayBuffer, byteOffset + 24, width * height * 4);
        proxyPixels = pixels;
        proxyWidth = width;
        proxyHeight = height;
        activeProxyIsFull = true;
        // A proxy is a clear, linear negative until Auto Invert explicitly
        // computes and persists the film-base estimate. Do not clear the
        // per-image state when the same proxy is reloaded after analysis.
        proxyHasAnalyzedBase = proxyAnalyzedBaseIds.has(requestedId);
        readyProxyIds.add(requestedId);
        addToCache(requestedId, arrayBuffer, requestedGeom);
        
        if (previewCanvas.width !== width || previewCanvas.height !== height) {
            previewCanvas.width = width;
            previewCanvas.height = height;
        }
        
        gl.bindTexture(gl.TEXTURE_2D, tex);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16UI, width, height, 0, gl.RGBA_INTEGER, gl.UNSIGNED_SHORT, pixels);
        
        updateCanvasTransform(width, height);
        requestRender();

        return true;
    } catch(e) {
        console.error("Error parsing proxy buffer:", e);
        return false;
    }
}

function setMode(mode) {
    if (mode === 'Color') {
        btnModeColor.classList.add('bg-[#28282c]', 'text-zinc-100', 'shadow-sm');
        btnModeColor.classList.remove('text-zinc-500', 'hover:text-zinc-300');
        btnModeBw.classList.add('text-zinc-500', 'hover:text-zinc-300');
        btnModeBw.classList.remove('bg-[#28282c]', 'text-zinc-100', 'shadow-sm');
        sliders.expr.el.disabled = false;
        sliders.expg.el.disabled = false;
        sliders.expb.el.disabled = false;
        sliders.saturation.el.disabled = false;
        sliders.temperature.el.disabled = false;
        sliders.tint.el.disabled = false;
    } else {
        btnModeBw.classList.add('bg-[#28282c]', 'text-zinc-100', 'shadow-sm');
        btnModeBw.classList.remove('text-zinc-500', 'hover:text-zinc-300');
        btnModeColor.classList.add('text-zinc-500', 'hover:text-zinc-300');
        btnModeColor.classList.remove('bg-[#28282c]', 'text-zinc-100', 'shadow-sm');
        sliders.expr.el.disabled = true;
        sliders.expg.el.disabled = true;
        sliders.expb.el.disabled = true;
        sliders.saturation.el.disabled = true;
        sliders.temperature.el.disabled = true;
        sliders.tint.el.disabled = true;
    }
}

function updateDMinMaxDisplay() {
    document.getElementById('val-dmin').innerHTML = `<span class="text-red-400">${currentDMin[0].toFixed(3)}</span><span class="text-emerald-400">${currentDMin[1].toFixed(3)}</span><span class="text-blue-400">${currentDMin[2].toFixed(3)}</span>`;
    document.getElementById('val-dmax').innerHTML = `<span class="text-red-400">${currentDMax[0].toFixed(3)}</span><span class="text-emerald-400">${currentDMax[1].toFixed(3)}</span><span class="text-blue-400">${currentDMax[2].toFixed(3)}</span>`;
}

function updateUIFromParams(params, geom) {
    current_geom = NexFilmGeometry.normalizeGeometryState(geom || current_geom);
    currentDMin = params.d_min.slice();
    currentDMax = params.d_max.slice();
    updateDMinMaxDisplay();
    sliders.masterDmin.el.value = 0; sliders.masterDmin.val.textContent = "0.000"; lastMasterDmin = 0;
    sliders.masterDmax.el.value = 0; sliders.masterDmax.val.textContent = "0.000"; lastMasterDmax = 0;
    
    sliders.exposure.el.value = params.exposure;
    sliders.gamma.el.value = params.gamma;
    sliders.saturation.el.value = params.saturation ?? 0;
    sliders.temperature.el.value = params.temperature ?? 0;
    sliders.tint.el.value = params.tint ?? params.hue ?? 0;
    sliders.expr.el.value = params.exp_r;
    sliders.expg.el.value = params.exp_g;
    sliders.expb.el.value = params.exp_b;
    if (params.highlights !== undefined) sliders.highlights.el.value = params.highlights;
    if (params.shadows !== undefined) sliders.shadows.el.value = params.shadows;
    sliders.lutOpacity.el.value = params.lut_opacity ?? 1.0;
    currentWorkingColorspace = params.working_colorspace || 'linear-srgb';
    selectColorspace.value = currentWorkingColorspace;
    
    currentSprocketUV = params.sprocket_uv ? new Float32Array(params.sprocket_uv) : new Float32Array([-1.0, -1.0]);
    currentSprocketTolerance = (params.sprocket_tolerance !== undefined && params.sprocket_tolerance !== null) ? params.sprocket_tolerance : 0.10;
    currentSprocketFeather = (params.sprocket_feather !== undefined && params.sprocket_feather !== null) ? params.sprocket_feather : 0.05;
    
    sliders.sprocketTolerance.el.value = currentSprocketTolerance;
    sliders.sprocketFeather.el.value = currentSprocketFeather;
    
    for (const key in sliders) {
        const s = sliders[key];
        s.val.textContent = parseFloat(s.el.value).toFixed(2);
        updateSliderTrack(s.el);
    }
    updatePerspectiveUI();
    setMode(params.film_mode === 'BW' ? 'B&W' : 'Color');
}

let backendSyncTimeout = null;
let pendingBackendSync = null;

async function flushPendingBackendSync() {
    if (backendSyncTimeout) {
        clearTimeout(backendSyncTimeout);
        backendSyncTimeout = null;
    }
    const pending = pendingBackendSync;
    pendingBackendSync = null;
    if (!pending) return;
    try {
        if (pending.key === 'angle') {
            await persistGeometryQueued(pending.id, pending.geom);
        }
        await updateBackendParams(pending.id, pending.params);
    } catch (error) {
        if (!pendingBackendSync) pendingBackendSync = pending;
        throw error;
    }
}

function scheduleBackendSync(key) {
    if (backendSyncTimeout) clearTimeout(backendSyncTimeout);
    const id = activeId;
    const params = saveCurrentState();
    if (!id || !params) return;
    pendingBackendSync = {
        id,
        key,
        params,
        geom: JSON.parse(JSON.stringify(current_geom))
    };
    backendSyncTimeout = setTimeout(async () => {
        const shouldReprocessGeometry = pendingBackendSync?.key === 'angle'
            && pendingBackendSync?.id === activeId
            && hasProcessedActiveImage;
        try {
            await flushPendingBackendSync();
            if (shouldReprocessGeometry) {
                await reloadDevelopProxy(current_geom, { showLoading: false });
            }
            requestThumbnailSync();
        } catch (error) {
            console.error("Failed to persist edit state", error);
            showToast("Could not save the latest edit: " + error, "error");
        }
    }, 250);
}

for (const key in sliders) {
    const s = sliders[key];
    s.el.addEventListener('pointerdown', () => pushUndoState());
    s.el.addEventListener('input', (e) => {
        s.val.textContent = parseFloat(e.target.value).toFixed(3);
        if (key === 'angle') {
            current_geom.angle = parseFloat(e.target.value);
        } else if (key === 'sprocketTolerance') {
            currentSprocketTolerance = parseFloat(e.target.value);
        } else if (key === 'sprocketFeather') {
            currentSprocketFeather = parseFloat(e.target.value);
        }
        updateSliderTrack(e.target);
        requestRender();
    });
    s.el.addEventListener('change', () => {
        scheduleBackendSync(key);
    });
}

function setupEditableSliderValues() {
    Object.entries(sliders).forEach(([key, { el, val }]) => {
        if (!el || !val) return;
        val.classList.add('slider-value');
        val.contentEditable = 'plaintext-only';
        val.spellcheck = false;
        val.title = 'Click to enter a value';

        const restoreValue = () => {
            val.textContent = Number.parseFloat(el.value).toFixed(3);
        };
        const commitValue = () => {
            if (el.disabled) {
                restoreValue();
                return;
            }
            const parsed = Number.parseFloat(val.textContent);
            if (!Number.isFinite(parsed)) {
                restoreValue();
                return;
            }
            const min = Number.parseFloat(el.min);
            const max = Number.parseFloat(el.max);
            const next = Math.min(max, Math.max(min, parsed));
            pushUndoState();
            el.value = String(next);
            el.dispatchEvent(new Event('input', { bubbles: true }));
            el.dispatchEvent(new Event('change', { bubbles: true }));
        };

        val.addEventListener('focus', () => {
            if (el.disabled) {
                val.blur();
                return;
            }
            const range = document.createRange();
            range.selectNodeContents(val);
            const selection = window.getSelection();
            selection.removeAllRanges();
            selection.addRange(range);
        });
        val.addEventListener('blur', commitValue);
        val.addEventListener('keydown', (event) => {
            if (event.key === 'Enter') {
                event.preventDefault();
                val.blur();
            } else if (event.key === 'Escape') {
                event.preventDefault();
                restoreValue();
                val.blur();
            }
        });
    });
}

setupEditableSliderValues();

function enableUI() {
    for (const key in sliders) {
        sliders[key].el.disabled = false;
        updateSliderTrack(sliders[key].el);
    }
    sliders.lutOpacity.el.disabled = !hasLUT;
    btnCropMode.disabled = false;
    btnPerspectiveMode.disabled = false;
    btnResetPerspective.disabled = false;
    perspectiveConstrain.disabled = false;
    Object.values(perspectiveControls).forEach(control => { control.el.disabled = false; });
    btnRecalibrate.disabled = false;
    btnAutoArea.disabled = false;
    btnBatchApply.disabled = !current_geom?.calibration_points;
    btnResetCrop.disabled = false;
    btnAutoColor.disabled = !current_geom?.calibration_points || isCalibrationMode;
    btnSprocketPicker.disabled = false;
    btnResetColor.disabled = false;
    btnRotateLeft.disabled = false;
    btnRotateRight.disabled = false;
    btnFlipH.disabled = false;
    btnFlipV.disabled = false;

    document.getElementById('btn-copy-settings').disabled = false;
    if (copiedSettings) document.getElementById('btn-paste-settings').disabled = false;
    document.getElementById('btn-wb-eyedropper').disabled = false;

    canvasWrapper.style.display = 'block';
    // CSS containment: prevent layout reflow when WebGL canvas dimensions change
    canvasWrapper.style.contain = 'layout style paint';
    previewCanvas.style.willChange = 'contents';
}

function disableUI() {
    for (const key in sliders) {
        sliders[key].el.disabled = true;
    }
    sliders.lutOpacity.el.disabled = true;
    btnCropMode.disabled = true;
    btnPerspectiveMode.disabled = true;
    btnResetPerspective.disabled = true;
    perspectiveConstrain.disabled = true;
    Object.values(perspectiveControls).forEach(control => { control.el.disabled = true; });
    btnRecalibrate.disabled = true;
    btnAutoArea.disabled = true;
    btnBatchApply.disabled = true;
    btnResetCrop.disabled = true;
    btnAutoColor.disabled = true;
    btnSprocketPicker.disabled = true;
    btnResetColor.disabled = true;
    btnRotateLeft.disabled = true;
    btnRotateRight.disabled = true;
    btnFlipH.disabled = true;
    btnFlipV.disabled = true;

    document.getElementById('btn-copy-settings').disabled = true;
    document.getElementById('btn-paste-settings').disabled = true;
    document.getElementById('btn-wb-eyedropper').disabled = true;
}

let allRolls = [];
let currentRollViewId = null;
let historyRollViewId = null;
let isRollEditing = false; // true only when Continue Editing (explicitly imported for editing), false for History preview
let editingRollId = null;
let currentImportSessionPaths = null;
let activeImportViewPaths = null;
const rollPreviewCache = new Map();
const ROLL_PREVIEW_CACHE_MS = 15_000;

function getRollPreviewCacheKey(roll) {
    const paths = roll.image_paths || [];
    return `${roll.roll_id}:${paths.length}:${paths[0] || ''}`;
}

async function fetchRolls() {
    try {
        allRolls = await invoke('get_rolls');
        await updateFilterSidebar();
    } catch(e) {
        console.error("Fetch rolls error", e);
    }
}

async function updateFilterSidebar() {
    const activeCameras = new Set(Array.from(document.querySelectorAll('.filter-camera:checked')).map(i => i.value));
    const activeDates = new Set(Array.from(document.querySelectorAll('.filter-date:checked')).map(i => i.value));

    const cameraList = document.getElementById('filter-camera-list');
    const dateList = document.getElementById('filter-date-list');
    
    let cameras = new Set();
    let dates = new Set();
    let films = new Set();
    
    allRolls.forEach(r => {
        if(r.camera) cameras.add(r.camera);
        if(r.date) dates.add(r.date);
        if(r.film_stock) films.add(r.film_stock);
    });
    
    // Update Filter Sidebar
    if (cameraList) {
        cameraList.innerHTML = Array.from(cameras).map(c => `
            <label class="flex items-center gap-2 cursor-pointer text-zinc-300 text-[12px]">
                <input type="checkbox" value="${c}" class="filter-checkbox filter-camera rounded bg-zinc-800 border-zinc-700 text-zinc-300 focus:ring-0" ${activeCameras.has(c) ? 'checked' : ''}> ${c}
            </label>
        `).join('');
    }
    
    if (dateList) {
        dateList.innerHTML = Array.from(dates).map(d => `
            <label class="flex items-center gap-2 cursor-pointer text-zinc-300 text-[12px]">
                <input type="checkbox" value="${d}" class="filter-checkbox filter-date rounded bg-zinc-800 border-zinc-700 text-zinc-300 focus:ring-0" ${activeDates.has(d) ? 'checked' : ''}> ${d}
            </label>
        `).join('');
    }
    
    // Populate Selects for Metadata Modal
    const selCamera = document.getElementById('roll-camera-select');
    if (selCamera) {
        let userCameras = [];
        try { userCameras = await invoke('get_user_cameras'); } catch(e) {}
        const allCams = new Set([...cameras, ...userCameras]);
        selCamera.innerHTML = `
            <option value="">Select Camera...</option>
            ${Array.from(allCams).map(c => `<option value="${c}">${c}</option>`).join('')}
            <option value="__new__">+ Add New...</option>
        `;
    }

    const selFilm = document.getElementById('roll-film-select');
    if (selFilm) {
        if (!originalFilmOptions) originalFilmOptions = selFilm.innerHTML;
        let userFilms = [];
        try { userFilms = await invoke('get_user_films'); } catch(e) {}
        
        if (userFilms.length > 0) {
            const addIdx = originalFilmOptions.lastIndexOf('<option value="__new__">');
            const baseStr = addIdx !== -1 ? originalFilmOptions.substring(0, addIdx) : originalFilmOptions;
            
            selFilm.innerHTML = baseStr + 
                `<optgroup label="User Defined">` + 
                userFilms.map(f => `<option value="${f}">${f}</option>`).join('') +
                `</optgroup>` +
                `<option value="__new__">+ Add New...</option>`;
        } else {
            selFilm.innerHTML = originalFilmOptions;
        }
    }

    
    
    document.querySelectorAll('.filter-checkbox').forEach(cb => {
        cb.addEventListener('change', renderLibraryAndFilmstrip);
    });
}

function getActiveFilters() {
    const formats = Array.from(document.querySelectorAll('#filter-format-list input:checked')).map(i => i.value);
    const cameras = Array.from(document.querySelectorAll('.filter-camera:checked')).map(i => i.value);
    const dates = Array.from(document.querySelectorAll('.filter-date:checked')).map(i => i.value);
    return { formats, cameras, dates };
}

let cleanupLibraryVirtualizer = null;

function createLibraryItemElement(item) {
    const libDiv = document.createElement('div');
    libDiv.className = `library-item overflow-hidden relative ${selectedLibraryIds.has(item.id) ? 'selected' : ''}`;
    libDiv.dataset.id = item.id;
    libDiv.onmousedown = event => {
        if (event.detail > 1) clearNativeSelection(event);
    };
    libDiv.ondblclick = event => {
        clearNativeSelection(event);
        selectedLibraryIds.clear();
        selectedLibraryIds.add(item.id);
        currentImportSessionPaths = null;
        if (item.roll_id) {
            currentRollViewId = item.roll_id;
            isRollEditing = true;
        } else {
            currentRollViewId = null;
            isRollEditing = false;
        }
        updateLibrarySelectionUI();
        selectImage(item.id);
        switchView('develop');
    };
    libDiv.onclick = (event) => {
        if (event.ctrlKey || event.metaKey) {
            if (selectedLibraryIds.has(item.id)) selectedLibraryIds.delete(item.id);
            else selectedLibraryIds.add(item.id);
        } else {
            selectedLibraryIds.clear();
            selectedLibraryIds.add(item.id);
        }
        updateLibrarySelectionUI();
    };

    if (item.status === 'importing') {
        const skeleton = document.createElement('div');
        skeleton.className = 'w-full h-full bg-[#2C2C2E] flex items-center justify-center animate-pulse';
        skeleton.innerHTML = `<svg class="animate-spin h-6 w-6 text-[#8E8E93]" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path></svg>`;
        libDiv.appendChild(skeleton);
        return libDiv;
    }

    const libImg = document.createElement('img');
    libImg.dataset.imgId = item.id;
    libImg.loading = 'lazy';
    libImg.decoding = 'async';
    if (item.thumbnail_base64 === 'FILE_MISSING') {
        libImg.src = 'data:image/svg+xml;base64,' + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" fill="none" stroke="#ff0000" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>`);
        libImg.className = 'w-full h-full object-contain opacity-50 bg-[#1C1C1E] p-4 pointer-events-none';
    } else if (!item.thumbnail_base64 || item.thumbnail_base64 === 'CALCULATING') {
        libImg.src = 'data:image/svg+xml;base64,' + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100"><rect width="100" height="100" fill="#2C2C2E"/><circle cx="50" cy="50" r="15" fill="none" stroke="#8E8E93" stroke-width="3" stroke-dasharray="23.5 23.5"><animateTransform attributeName="transform" type="rotate" repeatCount="indefinite" dur="1s" values="0 50 50;360 50 50"/></circle></svg>`);
        libImg.className = 'w-full h-full object-cover pointer-events-none opacity-80';
    } else {
        libImg.src = getThumbnailSrc(item.id) || '';
        libImg.className = 'w-full h-full object-cover pointer-events-none';
    }
    libDiv.appendChild(libImg);
    appendMissingSourceBadge(libDiv, item);
    return libDiv;
}

function mountVirtualLibraryGrid(items) {
    if (cleanupLibraryVirtualizer) cleanupLibraryVirtualizer();

    const scrollRoot = libraryGrid.parentElement;
    const rendered = new Map();
    const gap = 12;
    const minItemWidth = 190;
    const overscanRows = 2;
    let frame = null;
    let disposed = false;

    libraryGrid.style.display = 'block';
    libraryGrid.style.position = 'relative';

    const renderVisible = () => {
        frame = null;
        if (disposed) return;
        const width = libraryGrid.clientWidth || scrollRoot.clientWidth;
        if (width <= 0) return;
        const columns = Math.max(1, Math.floor((width + gap) / (minItemWidth + gap)));
        const itemWidth = (width - gap * (columns - 1)) / columns;
        const itemHeight = itemWidth * .75;
        const rowStride = itemHeight + gap;
        const rowCount = Math.ceil(items.length / columns);
        libraryGrid.style.height = `${Math.max(0, rowCount * rowStride - gap)}px`;

        const gridTop = libraryGrid.getBoundingClientRect().top
            - scrollRoot.getBoundingClientRect().top
            + scrollRoot.scrollTop;
        const viewportTop = Math.max(0, scrollRoot.scrollTop - gridTop);
        const viewportBottom = viewportTop + scrollRoot.clientHeight;
        const firstRow = Math.max(0, Math.floor(viewportTop / rowStride) - overscanRows);
        const lastRow = Math.min(rowCount - 1, Math.ceil(viewportBottom / rowStride) + overscanRows);
        const firstIndex = firstRow * columns;
        const lastIndex = Math.min(items.length - 1, (lastRow + 1) * columns - 1);

        rendered.forEach((element, index) => {
            if (index < firstIndex || index > lastIndex) {
                element.remove();
                rendered.delete(index);
            }
        });

        for (let index = firstIndex; index <= lastIndex; index++) {
            const row = Math.floor(index / columns);
            const column = index % columns;
            let element = rendered.get(index);
            if (!element) {
                element = createLibraryItemElement(items[index]);
                rendered.set(index, element);
                libraryGrid.appendChild(element);
            }
            element.style.position = 'absolute';
            element.style.width = `${itemWidth}px`;
            element.style.height = `${itemHeight}px`;
            element.style.transform = `translate3d(${column * (itemWidth + gap)}px, ${row * rowStride}px, 0)`;
        }
    };

    const queueRender = () => {
        if (frame === null) frame = requestAnimationFrame(renderVisible);
    };
    const resizeObserver = new ResizeObserver(queueRender);
    resizeObserver.observe(scrollRoot);
    scrollRoot.addEventListener('scroll', queueRender, { passive: true });
    renderVisible();

    cleanupLibraryVirtualizer = () => {
        disposed = true;
        if (frame !== null) cancelAnimationFrame(frame);
        resizeObserver.disconnect();
        scrollRoot.removeEventListener('scroll', queueRender);
        rendered.clear();
        cleanupLibraryVirtualizer = null;
    };
}

let renderVersion = 0;
async function renderLibraryAndFilmstrip(skipFetch = false) {
    renderVersion++;
    const currentVersion = renderVersion;
    try {
        let items = [...allLibraryItems];
        if (!skipFetch) {
            await fetchRolls();
            if (renderVersion !== currentVersion) return;
            const transientItems = allLibraryItems.filter(item => item.transient_edit || item.status === 'importing' || item.in_working_set);
            items = await invoke('get_filmstrip');
            if (renderVersion !== currentVersion) return;
            const fetchedIds = new Set(items.map(item => item.id));
            const fetchedIdentities = new Set(items.map(itemIdentity));
            const stillTransient = transientItems.filter(item =>
                !fetchedIds.has(item.id) && !fetchedIdentities.has(itemIdentity(item))
            );
            allLibraryItems = uniqueItemsByPath([...items, ...stillTransient]);
            items = [...allLibraryItems];
        }
        rememberItems(items);
        
        const libraryRollsGrid = document.getElementById('library-rolls-grid');
        const historyInternalGrid = document.getElementById('history-internal-grid');
        const btnHistoryBack = document.getElementById('btn-history-back');
        const historyTitle = document.getElementById('history-view-title');
        const btnExportContactSheet = document.getElementById('btn-export-contact-sheet');
        const historyEmpty = document.getElementById('history-empty');
        
        if (cleanupLibraryVirtualizer) cleanupLibraryVirtualizer();
        libraryGrid.innerHTML = '';
        libraryGrid.removeAttribute('style');
        libraryRollsGrid.innerHTML = '';
        historyInternalGrid.innerHTML = '';
        filmstripContainer.innerHTML = '';
        
        // --- Populate LIBRARY View (All Images) ---
        const allVisibleLibraryItems = uniqueItemsByPath(allLibraryItems);
        const libraryItems = activeImportViewPaths
            ? allVisibleLibraryItems.filter(item => activeImportViewPaths.includes(normalizePath(item.file_path)))
            : allVisibleLibraryItems;
        if (libraryItems.length === 0) {
            libraryEmpty.classList.remove('hidden');
            libraryGrid.classList.add('hidden');
            btnSelectAll.classList.add('hidden');
        } else {
            libraryEmpty.classList.add('hidden');
            libraryGrid.classList.remove('hidden');
            btnSelectAll.classList.remove('hidden');
            mountVirtualLibraryGrid(libraryItems);
        }
        
        // --- Populate HISTORY FILMS View ---
        if (allRolls.length === 0) {
            historyEmpty.classList.remove('hidden');
            libraryRollsGrid.classList.add('hidden');
            historyInternalGrid.classList.add('hidden');
            btnHistoryBack.classList.add('hidden');
            btnExportContactSheet.classList.add('hidden');
            document.getElementById('btn-promote-roll').classList.add('hidden');
            document.getElementById('btn-delete-rolls').classList.remove('hidden');
            btnEditRoll.classList.add('hidden');
            btnDeleteRollImages.classList.add('hidden');
        } else {
            historyEmpty.classList.add('hidden');
            const filters = getActiveFilters();
            
            if (historyRollViewId) {
                // Inner Roll View
                libraryRollsGrid.classList.add('hidden');
                historyInternalGrid.classList.remove('hidden');
                btnHistoryBack.classList.remove('hidden');
                btnExportContactSheet.classList.remove('hidden');
                document.getElementById('btn-promote-roll').classList.remove('hidden');
                document.getElementById('btn-delete-rolls').classList.add('hidden');
                btnEditRoll.classList.remove('hidden');
                btnDeleteRollImages.classList.remove('hidden');
                historyTitle.textContent = "Roll Contents";
                
                const currentRoll = allRolls.find(r => r.roll_id === historyRollViewId);
                if (currentRoll) {
                    try {
                        let rollStrip = await invoke('get_roll_filmstrip', { rollId: historyRollViewId });

                        // History Films is preview-only — never trigger import.
                        // If items are missing from state, the grid simply shows what's available.
                        // The user must use "Continue Editing Roll" to explicitly import.

                        // Merge into local items list for the history grid rendering only
                        rollStrip.forEach(newItem => {
                            rememberItem(newItem);
                            if (!items.some(i => i.id === newItem.id || itemIdentity(i) === itemIdentity(newItem))) {
                                items.push(newItem);
                            }
                        });
                        items = uniqueItemsByPath(items);
                        allLibraryItems = uniqueItemsByPath(allLibraryItems);
                    } catch (e) {
                        console.error("Failed to load or import roll filmstrip", e);
                    }
                    
                    document.getElementById('history-view-title').textContent = "Roll Contents";

                    currentRoll.image_paths.forEach(path => {
                        const existingItem = items.find(i =>
                            i.roll_id === historyRollViewId &&
                            normalizePath(i.file_path) === normalizePath(path)
                        );
                        if (!existingItem) return;

                        const libDiv = document.createElement('div');
                        libDiv.className = `library-item rounded overflow-hidden relative`;
                        
                        if (selectedLibraryIds.has(existingItem.id)) libDiv.classList.add('selected');
                        libDiv.dataset.id = existingItem.id;
                        libDiv.onmousedown = event => {
                            if (event.detail > 1) clearNativeSelection(event);
                        };
                        libDiv.ondblclick = event => {
                            clearNativeSelection(event);
                            selectedLibraryIds.clear();
                            selectedLibraryIds.add(existingItem.id);
                            updateLibrarySelectionUI();
                            // State 3 (Continue Editing / Import by Roll): allow switching to develop
                            // State 4 (History preview): selection only, no develop switching
                            if (isRollEditing && historyRollViewId === currentRollViewId && existingItem.state_available !== false) {
                                selectImage(existingItem.id);
                                switchView('develop');
                            } else if (isRollEditing && historyRollViewId === currentRollViewId) {
                                showToast("This frame has no persisted edit state.", "error");
                            }
                        };
                        libDiv.onclick = (e) => {
                            if (e.shiftKey && lastSelectedLibraryId) {
                                selectedLibraryIds.add(existingItem.id);
                            } else if (e.ctrlKey || e.metaKey) {
                                if (selectedLibraryIds.has(existingItem.id)) selectedLibraryIds.delete(existingItem.id);
                                else selectedLibraryIds.add(existingItem.id);
                            } else {
                                selectedLibraryIds.clear();
                                selectedLibraryIds.add(existingItem.id);
                                lastSelectedLibraryId = existingItem.id;
                            }
                            updateLibrarySelectionUI();
                        };
                        if (existingItem.status === 'importing') {
                            const skeleton = document.createElement('div');
                            skeleton.className = 'w-full h-full bg-[#2C2C2E] flex items-center justify-center animate-pulse';
                            skeleton.innerHTML = `<svg class="animate-spin h-6 w-6 text-[#8E8E93]" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path></svg>`;
                            libDiv.appendChild(skeleton);
                        } else {
                            const libImg = document.createElement('img');
                            libImg.dataset.imgId = existingItem.id;
                            if (existingItem.thumbnail_base64 === "FILE_MISSING") {
                                libImg.src = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" fill="none" stroke="#ff0000" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>`);
                                libImg.className = 'w-full h-full object-contain opacity-50 bg-[#1C1C1E] p-4 pointer-events-none';
                            } else if (!existingItem.thumbnail_base64 || existingItem.thumbnail_base64 === "CALCULATING") {
                                libImg.src = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100"><rect width="100" height="100" fill="#2C2C2E"/><circle cx="50" cy="50" r="15" fill="none" stroke="#8E8E93" stroke-width="3" stroke-dasharray="23.5 23.5"><animateTransform attributeName="transform" type="rotate" repeatCount="indefinite" dur="1s" values="0 50 50;360 50 50"/></circle></svg>`);
                                libImg.className = 'w-full h-full object-cover pointer-events-none opacity-80';
                            } else {
                                libImg.src = existingItem.thumbnail_base64.startsWith('data:') ? existingItem.thumbnail_base64 : `data:image/jpeg;base64,${existingItem.thumbnail_base64}`;
                                libImg.className = 'w-full h-full object-cover pointer-events-none';
                            }
                            libDiv.appendChild(libImg);
                            appendMissingSourceBadge(libDiv, existingItem);
                            if (!existingItem.rendered_thumbnail_base64) {
                                const undeveloped = document.createElement('div');
                                undeveloped.className = 'absolute inset-x-0 bottom-0 bg-black/75 px-2 py-1 text-center text-[9px] font-bold tracking-widest text-zinc-400';
                                undeveloped.textContent = existingItem.state_available === false ? 'No Saved State' : 'Undeveloped';
                                libDiv.appendChild(undeveloped);
                            }
                        }
                        historyInternalGrid.appendChild(libDiv);
                    });
                }
            } else {
                // Rolls Archive View
                libraryRollsGrid.classList.remove('hidden');
                historyInternalGrid.classList.add('hidden');
                btnHistoryBack.classList.add('hidden');
                btnExportContactSheet.classList.add('hidden');
                document.getElementById('btn-promote-roll').classList.add('hidden');
                document.getElementById('btn-delete-rolls').classList.remove('hidden');
                btnEditRoll.classList.add('hidden');
                btnDeleteRollImages.classList.add('hidden');
                historyTitle.textContent = "Roll Archive";
                
                let filteredRolls = allRolls.filter(r => {
                    if (filters.formats.length > 0 && !filters.formats.includes(r.format)) return false;
                    if (filters.cameras.length > 0 && !filters.cameras.includes(r.camera)) return false;
                    if (filters.dates.length > 0 && !filters.dates.includes(r.date)) return false;
                    return true;
                });
                if (isDeleteMode) {
                    document.getElementById('delete-action-bar').classList.remove('hidden');
                    document.getElementById('delete-count').textContent = `${selectedRollIds.size} SELECTED FOR DELETION`;
                } else {
                    document.getElementById('delete-action-bar').classList.add('hidden');
                }
                
                for (const roll of filteredRolls) {
                    if (renderVersion !== currentVersion) return;
                    const previewCacheKey = getRollPreviewCacheKey(roll);
                    const cachedPreview = rollPreviewCache.get(previewCacheKey);
                    const hasFreshPreview = cachedPreview && Date.now() - cachedPreview.cachedAt < ROLL_PREVIEW_CACHE_MS;
                    let thumbSrc = hasFreshPreview ? cachedPreview.src : '';
                    if (!hasFreshPreview) {
                        try {
                            const previews = await invoke('get_roll_previews', { rollId: roll.roll_id });
                            if (renderVersion !== currentVersion) return;
                            if (previews && previews.length > 0) {
                                thumbSrc = `data:image/jpeg;base64,${previews[0]}`;
                            }
                            rollPreviewCache.set(previewCacheKey, { src: thumbSrc, cachedAt: Date.now() });
                        } catch (e) {
                            console.error(e);
                        }
                    }
                    
                    const card = document.createElement('div');
                    card.className = "roll-row group cursor-pointer flex w-full";
                    if (isDeleteMode && selectedRollIds.has(roll.roll_id)) {
                        card.classList.add('delete-selected');
                    }

                    card.onclick = async () => {
                        if (isDeleteMode) {
                            if (selectedRollIds.has(roll.roll_id)) selectedRollIds.delete(roll.roll_id);
                            else selectedRollIds.add(roll.roll_id);
                            renderLibraryAndFilmstrip();
                        } else {
                            historyRollViewId = roll.roll_id;
                            selectedLibraryIds.clear();
                            renderLibraryAndFilmstrip();
                        }
                    };
                    card.innerHTML = `
                        <div class="roll-summary">
                            <div class="roll-title"></div>
                            <div class="roll-format"></div>
                            <div class="roll-meta">
                                <span data-roll-camera></span>
                                <span data-roll-date></span>
                            </div>
                            <button type="button" class="roll-edit-action">Edit Info</button>
                        </div>
                        <div class="roll-preview">
                            ${thumbSrc ? `<img src="${thumbSrc}" alt="" class="w-full h-full object-cover">` : `<div class="roll-preview-empty"></div>`}
                        </div>
                    `;
                    card.querySelector('.roll-title').textContent = roll.film_stock || 'Unknown Film';
                    card.querySelector('.roll-format').textContent = `${roll.format || '135'} format`;
                    card.querySelector('[data-roll-camera]').textContent = roll.camera || 'Unknown camera';
                    card.querySelector('[data-roll-date]').textContent = roll.date || 'Unknown date';
                    card.querySelector('.roll-edit-action').addEventListener('click', event => {
                        event.stopPropagation();
                        openRollMetadataEditor(roll);
                    });
                    libraryRollsGrid.appendChild(card);
                }
            }
        }
        
        // --- Populate DEVELOP Filmstrip ---
        let filmstripItems = items;
        if (currentRollViewId && isRollEditing) {
            // Only show roll items in filmstrip when explicitly imported for editing (Continue Editing).
            // History Films preview mode does NOT populate the filmstrip with roll items.
            const currentRoll = allRolls.find(r => r.roll_id === currentRollViewId);
            if (currentRoll) {
                const rollPaths = new Set(currentRoll.image_paths.map(normalizePath));
                filmstripItems = uniqueItemsByPath(items.filter(item =>
                    item.roll_id === currentRollViewId && rollPaths.has(normalizePath(item.file_path))
                ));
            } else {
                filmstripItems = uniqueItemsByPath(items.filter(item => item.roll_id === currentRollViewId));
            }
        } else if (currentImportSessionPaths) {
            filmstripItems = uniqueItemsByPath(items.filter(item => currentImportSessionPaths.includes(normalizePath(item.file_path))));
        } else {
            filmstripItems = uniqueItemsByPath(items.filter(item => item.roll_id === null || item.roll_id === 'LOOSE_DEFAULT'));
        }
        filmstripItems.forEach(item => {
            const stripDiv = document.createElement('div');
            stripDiv.className = `film-item shrink-0 ${item.id === activeId ? 'active' : ''}`;
            stripDiv.onclick = () => {
                selectImage(item.id);
                selectedLibraryIds.clear();
                selectedLibraryIds.add(item.id);
                updateLibrarySelectionUI();
            };
            stripDiv.dataset.id = item.id;
            
            if (item.status === 'importing') {
                const skeleton = document.createElement('div');
                skeleton.className = 'w-full h-full bg-[#2C2C2E] flex items-center justify-center animate-pulse rounded-[2px]';
                skeleton.innerHTML = `<svg class="animate-spin h-4 w-4 text-[#8E8E93]" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path></svg>`;
                stripDiv.appendChild(skeleton);
            } else {
                const stripImg = document.createElement('img');
                stripImg.dataset.imgId = item.id;
                if (item.thumbnail_base64 === "FILE_MISSING") {
                    stripImg.src = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" fill="none" stroke="#ff0000" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>`);
                    stripImg.className = 'w-full h-full object-contain rounded-[2px] pointer-events-none opacity-50 bg-[#1C1C1E] p-2';
                } else if (!item.thumbnail_base64 || item.thumbnail_base64 === "CALCULATING") {
                    stripImg.src = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100"><rect width="100" height="100" fill="#2C2C2E"/><circle cx="50" cy="50" r="15" fill="none" stroke="#8E8E93" stroke-width="3" stroke-dasharray="23.5 23.5"><animateTransform attributeName="transform" type="rotate" repeatCount="indefinite" dur="1s" values="0 50 50;360 50 50"/></circle></svg>`);
                    stripImg.className = 'w-full h-full object-cover rounded-[2px] pointer-events-none opacity-80';
                } else {
                    stripImg.src = getThumbnailSrc(item.id) || '';
                    stripImg.className = 'w-full h-full object-cover rounded-[2px] pointer-events-none';
                }
                stripDiv.appendChild(stripImg);
                appendMissingSourceBadge(stripDiv, item);
            }
            filmstripContainer.appendChild(stripDiv);
        });
        updateLibrarySelectionUI();
        btnDeleteDevelopImage.disabled = importInProgress || getImageDeletionTargets([activeId]).length === 0;
        
    } catch (e) { console.error("Filmstrip error:", e); }
}

document.getElementById('btn-promote-roll').addEventListener('click', async () => {
    if (historyRollViewId) {
        if (importInProgress) {
            showToast('Wait for the current import to finish before promoting a roll.', 'error');
            return;
        }
        try {
            const promotedRollId = historyRollViewId;
            await flushActiveImageState();
            await invoke('promote_roll', { rollId: promotedRollId });
            const promotedItems = await invoke('get_filmstrip');
            resetWorkingLibrary();
            allLibraryItems = uniqueItemsByPath(promotedItems);
            rememberItems(allLibraryItems);
            currentRollViewId = promotedRollId;
            isRollEditing = true;
            currentImportSessionPaths = null;
            activeImportViewPaths = null;
            selectedLibraryIds.clear();
            showToast("Roll promoted to Library successfully", "success");
            await renderLibraryAndFilmstrip();
            switchView('library');
        } catch (e) {
            showToast("Failed to promote roll", "error");
            console.error(e);
        }
    }
});

document.getElementById('btn-history-back').addEventListener('click', () => {
    historyRollViewId = null;
    selectedLibraryIds.clear();
    renderLibraryAndFilmstrip();
});

let currentImageRequestToken = 0;

// ═══════════════════════════════════════════════════════════════════
//  Optimistic UI: thumbnail placeholder ("李代桃僵")
//  Shows instantly (<16ms) while the 16-bit proxy loads in background.
// ═══════════════════════════════════════════════════════════════════

function updateThumbnailPlaceholderLayout(placeholder) {
    if (!placeholder || !placeholder.naturalWidth || !placeholder.naturalHeight || activeProxyIsFull) return;
    const isRendered = placeholder.dataset.rendered === 'true';
    const crop = current_geom?.crop_rect || { width: 1, height: 1 };
    const width = isRendered ? placeholder.naturalWidth / Math.max(crop.width, 0.001) : placeholder.naturalWidth;
    const height = isRendered ? placeholder.naturalHeight / Math.max(crop.height, 0.001) : placeholder.naturalHeight;
    updateCanvasTransform(width, height);
    if (isCalibrationMode) requestAnimationFrame(updateCalibrationPolygon);
}

function showThumbnailPlaceholder(thumbBase64, options = {}) {
    if (!thumbBase64) {
        hideThumbnailPlaceholder();
        return;
    }
    let placeholder = document.getElementById('thumbnail-placeholder');
    if (!placeholder) {
        placeholder = document.createElement('img');
        placeholder.id = 'thumbnail-placeholder';
        placeholder.className = 'absolute inset-0 w-full h-full object-contain z-10';
        placeholder.style.filter = 'saturate(0.96) brightness(0.92)';
        placeholder.style.transition = 'opacity 0.15s ease';
        placeholder.style.willChange = 'opacity';
        placeholder.style.contain = 'layout style paint';
        placeholder.style.background = '#050506';
        const canvasWrapper = document.getElementById('canvas-wrapper');
        if (canvasWrapper) {
            canvasWrapper.style.position = 'relative';
            canvasWrapper.appendChild(placeholder);
        }
    }
    placeholder.dataset.rendered = options.rendered ? 'true' : 'false';
    placeholder.src = thumbBase64;
    placeholder.onload = () => {
        updateThumbnailPlaceholderLayout(placeholder);
    };
    placeholder.style.opacity = '1';
    placeholder.style.display = 'block';
}

function hideThumbnailPlaceholder() {
    const placeholder = document.getElementById('thumbnail-placeholder');
    if (placeholder) {
        // Instant hide — no setTimeout. The WebGL canvas is already rendered
        // by the time this is called, so there's no flicker.
        placeholder.style.opacity = '0';
        placeholder.style.display = 'none';
    }
}

function getThumbnailSrc(id) {
    const item = findKnownItem(id);
    if (item && item.thumbnail_base64 && item.thumbnail_base64 !== 'FILE_MISSING') {
        if (item.thumbnail_base64.startsWith('data:')) {
            return item.thumbnail_base64;
        }
        return 'data:image/jpeg;base64,' + item.thumbnail_base64;
    }
    const img = document.querySelector(`img[data-img-id="${id}"]`);
    if (img && img.src && img.src.startsWith('data:')) {
        return img.src;
    }
    return null;
}

async function showEmbeddedDevelopPreview(id, token) {
    const fallback = getThumbnailSrc(id);
    showThumbnailPlaceholder(fallback);

    try {
        const preview = await invoke('get_embedded_preview', { id });
        if (token !== currentImageRequestToken || id !== activeId || activeProxyIsFull) return;
        showThumbnailPlaceholder(preview.startsWith('data:') ? preview : `data:image/jpeg;base64,${preview}`);
    } catch (e) {
        if (e === "FILE_MISSING") throw e;
        console.error('Failed to load embedded preview', e);
    }
}

function setImageElementThumbnail(img, thumbnail) {
    if (!img || !thumbnail || thumbnail === 'FILE_MISSING') return;
    img.src = thumbnail.startsWith('data:') ? thumbnail : `data:image/jpeg;base64,${thumbnail}`;
    img.style.transform = '';
    img.style.objectFit = 'cover';
    img.classList.remove('opacity-50', 'object-contain', 'p-4', 'p-2', 'bg-[#1C1C1E]');
    img.classList.add('object-cover');
}

async function selectImage(id) {
    if (activeId === id && hasProcessedActiveImage && !isCalibrationMode) return;
    if (activeId) {
        try {
            await flushPendingBackendSync();
            const outgoingId = activeId;
            const outgoingParams = saveCurrentState();
            await updateBackendParams(outgoingId, outgoingParams);
            scheduleInstantThumbnailUpdate();
            await flushPendingThumbnail();
        } catch (error) {
            console.error("Failed to save the current image before switching", error);
            showToast("Could not save the current image. The image was not switched.", "error");
            return;
        }
    }
    const myToken = ++currentImageRequestToken;
    const selectedItem = findKnownItem(id);
    const hasRenderedPreview = !!selectedItem?.rendered_thumbnail_base64;

    // Phase 1: show the best cached preview immediately.
    const thumbSrc = getThumbnailSrc(id);
    showThumbnailPlaceholder(thumbSrc, { rendered: hasRenderedPreview });
    const loadingUI = document.getElementById('loading-proxy-ui');
    if (loadingUI) {
        loadingUI.classList.add('hidden');
        loadingUI.classList.remove('flex');
    }
    // Keep the previous image's calibration UI from leaking into the new
    // selection. The authoritative persisted state below decides whether the
    // area overlay is shown once the lightweight state switch completes.
    isCalibrationMode = false;
    document.getElementById('calibration-overlay').classList.add('hidden');
    setDevelopInspectorCalibrationLocked(false);
    btnAutoColor.disabled = true;

    try {
        saveCurrentState(); // Save current state before switching

        let state;
        try {
            const rollId = currentRollViewId || 'LOOSE_DEFAULT';
            // Backend state is authoritative even when the UI has a cached copy.
            // This lightweight call also validates roll identity and source availability.
            state = await invoke('switch_active_image', { id, rollId });
            if (myToken !== currentImageRequestToken) {
                hideThumbnailPlaceholder();
                if (loadingUI) { loadingUI.classList.add('hidden'); loadingUI.classList.remove('flex'); }
                return;
            }
            imageStates.set(id, {
                params: state.params,
                geom: state.geom || { crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0.0, flip_h: false, flip_v: false, rotate_90_count: 0, calibration_points: null, calibration_confirmed: false }
            });
            if (state.base_analyzed) {
                proxyAnalyzedBaseIds.add(id);
            } else {
                proxyAnalyzedBaseIds.delete(id);
            }
        } catch (err) {
            if (err === "FILE_MISSING") {
                hideThumbnailPlaceholder();
                if (loadingUI) { loadingUI.classList.add('hidden'); loadingUI.classList.remove('flex'); }
                missingFileId = id;
                disableUI(); // No image to edit — lock all tuning controls
                document.getElementById('missing-file-ui').classList.remove('hidden');
                document.getElementById('missing-file-ui').classList.add('flex');
                document.getElementById('preview-canvas').style.display = 'none';
                canvasWrapper.style.display = 'block';
                canvasWrapper.style.width = '100%';
                canvasWrapper.style.height = '100%';
                activeId = id;
                btnDeleteDevelopImage.disabled = getImageDeletionTargets([activeId]).length === 0;
                renderLibraryAndFilmstrip();
                return;
            }
            hideThumbnailPlaceholder();
            if (loadingUI) { loadingUI.classList.add('hidden'); loadingUI.classList.remove('flex'); }
            throw err;
        }

        document.getElementById('missing-file-ui').classList.add('hidden');
        document.getElementById('missing-file-ui').classList.remove('flex');
        document.getElementById('preview-canvas').style.display = 'block';

        activeId = id;
        btnDeleteDevelopImage.disabled = getImageDeletionTargets([activeId]).length === 0;
        activeProxyIsFull = false;
        hasProcessedActiveImage = false;
        proxyPixels = null;
        proxyWidth = 0;
        proxyHeight = 0;
        readyProxyIds.delete(id);
        enableUI();
        current_geom = NexFilmGeometry.normalizeGeometryState(state.geom);
        const selectionCalibrationRevision = ++calibrationRevision;
        btnBatchApply.disabled = !current_geom.calibration_points;
        updateUIFromParams(state.params, current_geom);
        updateThumbnailPlaceholderLayout(document.getElementById('thumbnail-placeholder'));
        await restoreLutForImage(state.params);
        if (myToken !== currentImageRequestToken) return;

        if (previewCanvas) {
            previewCanvas.style.display = 'none';
            previewCanvas.width = 1;
            previewCanvas.height = 1;
        }

        const filmItems = document.querySelectorAll('#filmstrip-container .film-item');
        filmItems.forEach(item => {
            if (item.dataset.id === activeId) {
                item.classList.add('active');
                item.classList.remove('settings-updated');
                item.scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'center' });
            } else {
                item.classList.remove('active');
            }
        });

        updateSliderTrack(sliders.exposure.el);
        updateSliderTrack(sliders.gamma.el);

        if (myToken !== currentImageRequestToken) {
            return;
        }

        // A developed frame must keep its positive thumbnail on screen until
        // the matching proxy is ready. Undeveloped frames may upgrade the
        // embedded negative while their proxy is prepared.
        // The import-stage 1024px embedded preview is already in memory. Do not
        // ask LibRaw for a second large JPEG while the linear proxy decodes.
        // Every Develop entry requests the same coalesced half-size linear
        // proxy. The embedded/rendered thumbnail remains visible while it is
        // decoding, so RAW work never blocks navigation or slider input.
        void ensureProxyDisplayed(id, { persistThumbnail: hasRenderedPreview })
            .then(loaded => {
                if (loaded && myToken === currentImageRequestToken && id === activeId) {
                    queueAdjacentProxyPrewarm(id);
                }
            })
            .catch(error => {
                if (myToken === currentImageRequestToken) {
                    console.error('Background proxy preparation failed', error);
                }
            });

        canvasWrapper.style.display = 'block';
        updateCanvasTransform();
        if (current_geom.calibration_points) {
            calibrationPoints = JSON.parse(JSON.stringify(current_geom.calibration_points));
        } else {
            calibrationPoints = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        }
        const hasFilmArea = !!current_geom.calibration_points;
        const hasConfirmedFilmArea = hasFilmArea && current_geom.calibration_confirmed === true;
        isCalibrationMode = !hasConfirmedFilmArea;
        btnBatchApply.disabled = !hasConfirmedFilmArea;
        document.getElementById('calibration-overlay').classList.toggle('hidden', hasConfirmedFilmArea);
        // A batch-applied area is persisted edit state even if the target has
        // never rendered. It must reach Auto Invert without another save.
        setDevelopInspectorCalibrationLocked(isCalibrationMode);
        btnAutoColor.disabled = !hasConfirmedFilmArea;
        if (isCalibrationMode) {
            requestAnimationFrame(updateCalibrationPolygon);
            if (!hasFilmArea) {
                void initializeDefaultFilmArea(id, myToken, selectionCalibrationRevision);
            }
        }


    } catch(e) {
        hideThumbnailPlaceholder();
        const lu = document.getElementById('loading-proxy-ui');
        if (lu) { lu.classList.add('hidden'); lu.classList.remove('flex'); }
        console.error(e);
    }
}

btnModeColor.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    setMode('Color');
    await updateBackendParams();
    if (hasProcessedActiveImage) requestRender();
    requestThumbnailSync();
});

btnModeBw.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    setMode('BW');
    await updateBackendParams();
    if (hasProcessedActiveImage) requestRender();
    requestThumbnailSync();
});

const doImportSingle = async () => {
    document.getElementById('import-choice-modal').classList.add('opacity-0', 'pointer-events-none');
    let importStarted = false;
    let transientIds = [];
    try {
        btnImport.textContent = "Importing...";
        btnImport.disabled = true;
        btnImportTriggers.forEach(btn => { 
            btn.dataset.originalText = btn.dataset.originalText || btn.textContent;
            btn.textContent = "Importing..."; 
            btn.disabled = true; 
        });
        
        const selectedPaths = uniqueImportPaths(await invoke('open_file_dialog'));
        if (selectedPaths.length > 0) {
            const paths = selectedPaths;
            const roll = createLooseImportRoll(paths);
            await beginWorkingImport(roll.roll_id, paths);
            
            initImportToast(paths.length);
            
            const tempItems = paths.map((p, idx) => ({
                id: 'temp_import_' + Date.now() + '_' + idx,
                roll_id: roll.roll_id,
                file_path: p,
                thumbnail_base64: '',
                status: 'importing'
            }));
            transientIds = tempItems.map(item => item.id);
            rememberItems(tempItems);
            allLibraryItems = tempItems;
            await renderLibraryAndFilmstrip(true);
            
            await invoke('import_roll', { roll, paths });
            importStarted = true;
        }
    } catch (e) {
        removeTransientItems(transientIds);
        resetWorkingLibrary();
        await renderLibraryAndFilmstrip(true);
        showToast("Import failed: " + e, "error");
    }
    finally {
        if (!importStarted) restoreImportButtons();
    }
};

const doImportRoll = async () => {
    let importStarted = false;
    let transientIds = [];
    try {
        const format = document.getElementById('roll-format').value;
        const date = document.getElementById('roll-date').value;
        
        let camera = document.getElementById('roll-camera-select').value;
        if (camera === "__new__") camera = document.getElementById('roll-camera-input').value;
        
        let film = document.getElementById('roll-film-select').value;
        if (film === "__new__") film = document.getElementById('roll-film-input').value;
        
        if(!film) {
            showToast("Film stock is required", "error");
            return;
        }

        document.getElementById('roll-metadata-modal').classList.add('opacity-0', 'pointer-events-none');
        btnImport.textContent = "Importing...";
        btnImport.disabled = true;
        btnImportTriggers.forEach(btn => { 
            btn.dataset.originalText = btn.dataset.originalText || btn.textContent;
            btn.textContent = "Importing..."; 
            btn.disabled = true; 
        });
        
        const paths = uniqueImportPaths(await invoke('open_file_dialog'));
        if (paths.length > 0) {
            initImportToast(paths.length);
            const roll_id = `roll_${Date.now()}_${Math.floor(Math.random()*1000)}`;
            const roll = { roll_id, date, format, film_stock: film, camera, image_paths: paths };
            await beginWorkingImport(roll_id, paths);
            const tempItems = paths.map((p, idx) => ({
                id: 'temp_import_' + Date.now() + '_' + idx,
                roll_id,
                file_path: p,
                thumbnail_base64: '',
                status: 'importing',
                in_working_set: true
            }));
            transientIds = tempItems.map(item => item.id);
            rememberItems(tempItems);
            allLibraryItems = tempItems;
            await renderLibraryAndFilmstrip(true);
            
            await invoke('import_roll', { roll, paths });
            importStarted = true;
            
            // Persist Camera and Film
            if (camera) {
                try { await invoke('add_user_camera', { camera }); } catch(e) {}
            }
            if (film) {
                try { await invoke('add_user_film', { film }); } catch(e) {}
            }
            
        }
    } catch (e) {
        removeTransientItems(transientIds);
        resetWorkingLibrary();
        await renderLibraryAndFilmstrip(true);
        showToast("Import failed: " + e, "error");
    }
    finally {
        if (!importStarted) restoreImportButtons();
        document.getElementById('roll-metadata-modal').classList.add('opacity-0', 'pointer-events-none');
    }
};

document.getElementById('btn-continue-roll').addEventListener('click', async () => {
    document.getElementById('import-choice-modal').classList.add('opacity-0', 'pointer-events-none');
    
    const select = document.getElementById('continue-roll-select');
    select.innerHTML = '';
    
    if (allRolls.length === 0) {
        showToast("No rolls in archive to continue.", "error");
        return;
    }
    
    allRolls.forEach(r => {
        const opt = document.createElement('option');
        opt.value = r.roll_id;
        opt.textContent = `${r.film_stock} (${r.date || 'Unknown Date'}) - ${r.camera || 'Unknown Camera'}`;
        select.appendChild(opt);
    });
    
    document.getElementById('continue-roll-modal').classList.remove('opacity-0', 'pointer-events-none');
});

document.getElementById('btn-close-continue-roll').addEventListener('click', () => {
    document.getElementById('continue-roll-modal').classList.add('opacity-0', 'pointer-events-none');
});

document.getElementById('btn-confirm-continue').addEventListener('click', async () => {
    if (importInProgress) {
        showToast('Wait for the current import to finish before opening another roll.', 'error');
        return;
    }
    const rollId = document.getElementById('continue-roll-select').value;
    if (!rollId) return;
    
    document.getElementById('continue-roll-modal').classList.add('opacity-0', 'pointer-events-none');
    
    const roll = allRolls.find(r => r.roll_id === rollId);
    if (!roll) {
        showToast("The selected roll no longer exists.", "error");
        return;
    }

    try {
        await flushActiveImageState();
        await invoke('promote_roll', { rollId });
        const persistedItems = await invoke('get_filmstrip');
        resetWorkingLibrary();
        allLibraryItems = uniqueItemsByPath(persistedItems);
        rememberItems(allLibraryItems);
        currentRollViewId = rollId;
        historyRollViewId = rollId;
        isRollEditing = true;
        currentImportSessionPaths = null;
        selectedLibraryIds.clear();
        const persistedPaths = new Set(
            persistedItems
                .filter(item => item.state_available !== false)
                .map(item => normalizePath(item.file_path))
        );
        const missingStateCount = roll.image_paths.filter(path => !persistedPaths.has(normalizePath(path))).length;
        if (missingStateCount > 0) {
            showToast(`${missingStateCount} frame(s) have no saved archive state and were not re-imported.`, "error");
        }
        await renderLibraryAndFilmstrip();
        switchView('history');
    } catch (e) {
        console.error("Failed to continue editing roll", e);
        showToast("Could not load the roll for editing: " + e, "error");
    }
});

function loadPersistedMetadata() {
    try {
        const cameras = JSON.parse(localStorage.getItem('user_cameras') || '[]');
        const films = JSON.parse(localStorage.getItem('user_films') || '[]');
        
        const selCamera = document.getElementById('roll-camera-select');
        cameras.forEach(c => {
            if (!Array.from(selCamera.options).some(o => o.value === c)) {
                const opt = document.createElement('option');
                opt.value = c; opt.textContent = c;
                selCamera.insertBefore(opt, selCamera.querySelector('option[value="__new__"]'));
            }
        });
        
        const selFilm = document.getElementById('roll-film-select');
        films.forEach(f => {
            if (!Array.from(selFilm.options).some(o => o.value === f)) {
                const opt = document.createElement('option');
                opt.value = f; opt.textContent = f;
                selFilm.insertBefore(opt, selFilm.querySelector('option[value="__new__"]'));
            }
        });
    } catch(e) {}
}
loadPersistedMetadata();

const handleInputEnter = (inputId, selectId) => {
    document.getElementById(inputId).addEventListener('keydown', (e) => {
        if (e.key === 'Enter') {
            e.preventDefault();
            const val = e.target.value.trim();
            if (val) {
                const select = document.getElementById(selectId);
                const option = document.createElement('option');
                option.value = val;
                option.textContent = val;
                select.insertBefore(option, select.querySelector('option[value="__new__"]'));
                select.value = val;
                e.target.classList.add('hidden');
                e.target.value = '';
            }
        }
    });
};

document.getElementById('roll-camera-select').addEventListener('change', (e) => {
    const input = document.getElementById('roll-camera-input');
    if (e.target.value === '__new__') {
        input.classList.remove('hidden');
        input.focus();
    } else input.classList.add('hidden');
});
handleInputEnter('roll-camera-input', 'roll-camera-select');

document.getElementById('roll-film-select').addEventListener('change', (e) => {
    const input = document.getElementById('roll-film-input');
    if (e.target.value === '__new__') {
        input.classList.remove('hidden');
        input.focus();
    } else input.classList.add('hidden');
});
handleInputEnter('roll-film-input', 'roll-film-select');

const showImportChoice = () => {
    editingRollId = null;
    document.getElementById('import-choice-modal').classList.remove('opacity-0', 'pointer-events-none');
    setTimeout(() => document.getElementById('import-choice-content').classList.remove('scale-95'), 10);
};

btnImport.addEventListener('click', showImportChoice);
btnImportTriggers.forEach(btn => btn.addEventListener('click', showImportChoice));

document.getElementById('btn-close-import-choice').addEventListener('click', () => {
    document.getElementById('import-choice-modal').classList.add('opacity-0', 'pointer-events-none');
});

document.getElementById('btn-import-single').addEventListener('click', doImportSingle);

document.getElementById('btn-import-by-roll').addEventListener('click', () => {
    document.getElementById('import-choice-modal').classList.add('opacity-0', 'pointer-events-none');
    editingRollId = null;
    document.getElementById('roll-metadata-title').textContent = 'Roll Metadata';
    document.getElementById('btn-confirm-roll-meta').textContent = 'Select Images';
    document.getElementById('roll-format').value = '135';
    if (!document.getElementById('roll-date').value) {
        const now = new Date();
        document.getElementById('roll-date').value = new Date(now.getTime() - now.getTimezoneOffset() * 60_000).toISOString().slice(0, 10);
    }
    document.getElementById('roll-metadata-modal').classList.remove('opacity-0', 'pointer-events-none');
    setTimeout(() => document.getElementById('roll-metadata-content').classList.remove('scale-95'), 10);
});

document.getElementById('btn-close-roll-meta').addEventListener('click', () => {
    document.getElementById('roll-metadata-modal').classList.add('opacity-0', 'pointer-events-none');
});
document.getElementById('btn-cancel-roll-meta').addEventListener('click', () => {
    document.getElementById('roll-metadata-modal').classList.add('opacity-0', 'pointer-events-none');
});
document.getElementById('btn-confirm-roll-meta').addEventListener('click', async () => {
    if (editingRollId) await updateCurrentRollMetadata();
    else await doImportRoll();
});

function setRollMetadataSelect(selectId, inputId, value) {
    const select = document.getElementById(selectId);
    const input = document.getElementById(inputId);
    const normalized = value || '';
    if (normalized && !Array.from(select.options).some(option => option.value === normalized)) {
        const option = document.createElement('option');
        option.value = normalized;
        option.textContent = normalized;
        select.insertBefore(option, select.querySelector('option[value="__new__"]'));
    }
    select.value = normalized;
    input.value = '';
    input.classList.add('hidden');
}

function openRollMetadataEditor(roll) {
    if (!roll) return;
    editingRollId = roll.roll_id;
    document.getElementById('roll-metadata-title').textContent = 'Edit Roll Info';
    document.getElementById('btn-confirm-roll-meta').textContent = 'Save Changes';
    document.getElementById('roll-format').value = roll.format || '135';
    document.getElementById('roll-date').value = roll.date || '';
    setRollMetadataSelect('roll-camera-select', 'roll-camera-input', roll.camera);
    setRollMetadataSelect('roll-film-select', 'roll-film-input', roll.film_stock);
    document.getElementById('roll-metadata-modal').classList.remove('opacity-0', 'pointer-events-none');
    requestAnimationFrame(() => document.getElementById('roll-metadata-content').classList.remove('scale-95'));
}

async function updateCurrentRollMetadata() {
    const rollId = editingRollId;
    if (!rollId) return;
    if (importInProgress) {
        showToast('Wait for the current import to finish before editing roll information.', 'error');
        return;
    }
    let camera = document.getElementById('roll-camera-select').value;
    if (camera === '__new__') camera = document.getElementById('roll-camera-input').value.trim();
    let filmStock = document.getElementById('roll-film-select').value;
    if (filmStock === '__new__') filmStock = document.getElementById('roll-film-input').value.trim();
    if (!filmStock) {
        showToast('Film stock is required', 'error');
        return;
    }

    try {
        const updated = await invoke('update_roll_metadata', {
            rollId,
            date: document.getElementById('roll-date').value,
            format: document.getElementById('roll-format').value,
            filmStock,
            camera
        });
        const index = allRolls.findIndex(roll => roll.roll_id === rollId);
        if (index >= 0) allRolls[index] = updated;
        rollPreviewCache.clear();
        if (camera) await invoke('add_user_camera', { camera }).catch(() => {});
        if (filmStock) await invoke('add_user_film', { film: filmStock }).catch(() => {});
        editingRollId = null;
        document.getElementById('roll-metadata-modal').classList.add('opacity-0', 'pointer-events-none');
        await updateFilterSidebar();
        await renderLibraryAndFilmstrip(true);
        showToast('Roll information updated', 'success');
    } catch (error) {
        showToast(`Could not update roll information: ${error}`, 'error');
    }
}

btnEditRoll.addEventListener('click', () => {
    openRollMetadataEditor(allRolls.find(roll => roll.roll_id === historyRollViewId));
});



// Export Modal Logic
let exportInProgress = false;
let exportProgressToast = null;
let exportOutputDirectory = '';
const EXPORT_SETTINGS_KEY = 'nexfilm-export-settings';
const EXPORT_FORMAT_LABELS = {
    jpeg: 'JPEG 8-bit',
    png: 'PNG 16-bit',
    tiff8: 'TIFF 8-bit',
    tiff16: 'TIFF 16-bit',
};

function readExportPreferences() {
    const defaults = {
        format: 'tiff16',
        colorSpace: 'srgb',
        quality: 92,
        resizeMode: 'original',
        longEdge: 2048,
        allowUpscale: false,
        sharpening: 'standard',
        namingTemplate: '{Roll}_{Seq}',
        conflictPolicy: 'unique',
    };
    try {
        const saved = JSON.parse(localStorage.getItem(EXPORT_SETTINGS_KEY) || '{}');
        return { ...defaults, ...saved };
    } catch (_) {
        return defaults;
    }
}

function persistExportPreferences(settings) {
    try {
        localStorage.setItem(EXPORT_SETTINGS_KEY, JSON.stringify(settings));
    } catch (_) {
        // Export remains usable when storage is unavailable.
    }
}

function applyExportPreferences() {
    const saved = readExportPreferences();
    if (saved.format === 'jpeg100') saved.format = 'jpeg';
    if (saved.format === 'tiff16_uncompressed') saved.format = 'tiff16';
    if (exportFormat.querySelector('option[value="' + saved.format + '"]')) exportFormat.value = saved.format;
    if (exportColorSpace.querySelector('option[value="' + saved.colorSpace + '"]')) exportColorSpace.value = saved.colorSpace;
    if (['none', 'low', 'standard', 'high'].includes(saved.sharpening)) exportSharpening.value = saved.sharpening;
    if (['original', 'long_edge'].includes(saved.resizeMode)) exportResizeMode.value = saved.resizeMode;
    if (Number.isInteger(Number(saved.longEdge))) exportLongEdge.value = String(saved.longEdge);
    exportQuality.value = String(Math.min(100, Math.max(40, Number(saved.quality) || 92)));
    exportUpscale.checked = Boolean(saved.allowUpscale);
    exportNaming.value = typeof saved.namingTemplate === 'string' ? saved.namingTemplate : '{Roll}_{Seq}';
    if (['unique', 'overwrite', 'skip'].includes(saved.conflictPolicy)) exportConflictPolicy.value = saved.conflictPolicy;
}

function currentExportIds() {
    const selected = new Set(selectedLibraryIds);
    const ordered = allLibraryItems.filter(item => selected.has(item.id)).map(item => item.id);
    selected.forEach(id => {
        if (!ordered.includes(id)) ordered.push(id);
    });
    return ordered;
}

function pathStem(path) {
    const name = String(path || '').replace(/\\/g, '/').split('/').pop() || 'Image';
    return name.replace(/\.[^.]*$/, '') || 'Image';
}

function currentExportPreviewMetadata(ids) {
    const item = findKnownItem(ids[0]);
    const roll = item && typeof allRolls !== 'undefined'
        ? allRolls.find(value => value.roll_id === item.roll_id)
        : null;
    return {
        roll: roll && roll.roll_id,
        camera: roll && roll.camera,
        film: roll && roll.film_stock,
        date: roll && roll.date,
        original: item && pathStem(item.file_path),
        seq: '001',
    };
}

function exportFileExtension(format) {
    return format === 'jpeg' ? 'jpg' : format === 'png' ? 'png' : 'tiff';
}

function collectExportSettings() {
    return {
        format: exportFormat.value,
        colorSpace: exportColorSpace.value,
        quality: Number(exportQuality.value) || 92,
        resizeMode: exportResizeMode.value,
        longEdge: Number(exportLongEdge.value) || 2048,
        allowUpscale: exportUpscale.checked,
        sharpening: exportSharpening.value,
        namingTemplate: exportNaming.value,
        conflictPolicy: exportConflictPolicy.value,
    };
}

function updateExportDialogState() {
    const ids = currentExportIds();
    const settings = collectExportSettings();
    const validationError = validateExportSettings(settings);
    const isJpeg = settings.format === 'jpeg';
    const isOriginal = settings.resizeMode === 'original';
    exportSelectionCount.textContent = ids.length + (ids.length === 1 ? ' frame' : ' frames');
    exportModalSubtitle.textContent = ids.length
        ? 'Review output settings before writing the finished frames.'
        : 'Select one or more frames in Library to begin.';
    exportQuality.disabled = !isJpeg;
    exportQualityGroup.classList.toggle('is-disabled', !isJpeg);
    exportLongEdge.disabled = isOriginal;
    exportLongEdgeGroup.classList.toggle('is-disabled', isOriginal);
    exportQualityValue.textContent = String(settings.quality);
    const metadata = currentExportPreviewMetadata(ids);
    exportNamePreview.textContent = formatExportTemplate(settings.namingTemplate, metadata) + '.' + exportFileExtension(settings.format);
    exportOutputDir.value = exportOutputDirectory;
    exportOutputSummary.textContent = (exportOutputDirectory || 'Choose a destination folder to continue.')
        + ' · ' + (EXPORT_FORMAT_LABELS[settings.format] || settings.format)
        + ' · ' + describeResize(settings);
    btnConfirmExport.disabled = exportInProgress || ids.length === 0 || !exportOutputDirectory || Boolean(validationError);
    btnConfirmExport.title = validationError || (!exportOutputDirectory ? 'Choose an output folder first.' : '');
    return validationError;
}

function saveCurrentExportPreferences() {
    persistExportPreferences(collectExportSettings());
    updateExportDialogState();
}

applyExportPreferences();
[
    exportFormat, exportColorSpace, exportResizeMode, exportLongEdge,
    exportUpscale, exportSharpening, exportNaming, exportConflictPolicy,
].forEach(element => element.addEventListener('change', saveCurrentExportPreferences));
exportNaming.addEventListener('input', saveCurrentExportPreferences);
exportQuality.addEventListener('input', () => {
    exportQualityValue.textContent = exportQuality.value;
    saveCurrentExportPreferences();
});

function showExportProgress(processed, total) {
    if (!exportProgressToast) {
        exportProgressToast = document.createElement('div');
        exportProgressToast.className = 'fixed bottom-6 right-6 z-[100] w-72 border border-[#3A3A3C] bg-[#1C1C1E] p-4 shadow-2xl';
        exportProgressToast.innerHTML = `
            <div class="mb-3 flex items-center justify-between text-[11px] font-bold tracking-widest text-zinc-200">
                <span>Exporting</span><span id="export-progress-text">0 / 0</span>
            </div>
            <div class="h-1.5 overflow-hidden bg-zinc-800"><div id="export-progress-bar" class="h-full bg-zinc-200" style="width:0%"></div></div>`;
        document.body.appendChild(exportProgressToast);
    }
    const safeTotal = Math.max(1, total || 0);
    const percent = Math.min(100, Math.max(0, (processed / safeTotal) * 100));
    exportProgressToast.querySelector('#export-progress-text').textContent = `${processed} / ${total}`;
    exportProgressToast.querySelector('#export-progress-bar').style.width = `${percent}%`;
}

function clearExportProgress() {
    if (exportProgressToast) exportProgressToast.remove();
    exportProgressToast = null;
}

listen('export_progress', (event) => {
    const { processed = 0, total = 0 } = event.payload || {};
    showExportProgress(processed, total);
});

function openExportModal() {
    if (exportInProgress) {
        showToast('An export is already running. You can continue editing while it finishes.', 'error');
        return;
    }
    updateExportDialogState();
    exportModal.classList.remove('opacity-0', 'pointer-events-none');
    exportModal.setAttribute('aria-hidden', 'false');
    setTimeout(() => exportModalContent.classList.remove('scale-95'), 10);
}

btnExportDialog.addEventListener('click', () => {
    openExportModal();
});

const closeExportModal = () => {
    exportModalContent.classList.add('scale-95');
    exportModal.classList.add('opacity-0', 'pointer-events-none');
    exportModal.setAttribute('aria-hidden', 'true');
};
btnCloseExport.addEventListener('click', closeExportModal);
btnCancelExport.addEventListener('click', closeExportModal);
btnChooseExportDir.addEventListener('click', async () => {
    const selected = await invoke('select_export_dir').catch(error => {
        showToast('Could not choose an output folder: ' + error, 'error');
        return null;
    });
    if (selected) {
        exportOutputDirectory = selected;
        updateExportDialogState();
    }
});

btnConfirmExport.addEventListener('click', async () => {
    const validationError = updateExportDialogState();
    const exportIds = currentExportIds();
    if (validationError || exportIds.length === 0 || !exportOutputDirectory) {
        showToast(validationError || 'Choose an output folder and at least one frame.', 'error');
        return;
    }
    const settings = collectExportSettings();
    const outputDirectory = exportOutputDirectory;
    const invokeArgs = createExportInvokeArgs(exportIds, outputDirectory, settings);
    persistExportPreferences(settings);
    exportInProgress = true;
    btnConfirmExport.textContent = 'Exporting...';
    btnConfirmExport.disabled = true;
    try {
        await flushPendingBackendSync();
        if (activeProxyIsFull) captureActiveCanvasThumbnail();
        await flushPendingThumbnail();
        closeExportModal();
        showExportProgress(0, exportIds.length);
        const result = await invoke('batch_export_images', invokeArgs);
        const exported = Number(result && result.exported) || 0;
        const skipped = Number(result && result.skipped) || 0;
        const failed = Number(result && result.failed) || 0;
        let message = 'Exported ' + exported + ' frame(s) to:\n'
            + (result && result.outputDir ? result.outputDir : outputDirectory);
        if (skipped) message += '\nSkipped ' + skipped + ' existing file(s).';
        if (failed) message += '\nFailed ' + failed + ' frame(s).';
        showToast(message, failed ? 'error' : 'success');
        if (failed && result.errors && result.errors[0]) console.error('Export failure:', result.errors[0]);
    } catch (error) {
        showToast('Batch export failed: ' + error, 'error');
    } finally {
        exportInProgress = false;
        clearExportProgress();
        btnConfirmExport.textContent = 'Export frames';
        updateExportDialogState();
        if (exportModal.getAttribute('aria-hidden') === 'false') btnConfirmExport.disabled = false;
    }
});

document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && exportModal.getAttribute('aria-hidden') === 'false' && !exportInProgress) {
        closeExportModal();
    }
});

// MASTER SLIDERS SETUP
let lastMasterDmin = 0;
let lastMasterDmax = 0;

sliders.masterDmin.el.addEventListener('input', (e) => {
    let current = parseFloat(e.target.value);
    let delta = current - lastMasterDmin;
    lastMasterDmin = current;
    currentDMin[0] += delta; currentDMin[1] += delta; currentDMin[2] += delta;
    sliders.masterDmin.val.textContent = current.toFixed(3);
    updateDMinMaxDisplay(); requestRender();
});
sliders.masterDmax.el.addEventListener('input', (e) => {
    let current = parseFloat(e.target.value);
    let delta = current - lastMasterDmax;
    lastMasterDmax = current;
    currentDMax[0] += delta; currentDMax[1] += delta; currentDMax[2] += delta;
    sliders.masterDmax.val.textContent = current.toFixed(3);
    updateDMinMaxDisplay(); requestRender();
});

for (const key in sliders) updateSliderTrack(sliders[key].el);

// ==========================================
// CROP MODE INTERACTION
// ==========================================
function updateCanvasTransform(w, h) {
    if (w) currentImageWidth = w;
    if (h) currentImageHeight = h;
    const cw = currentImageWidth;
    const ch = currentImageHeight;
    const rect = current_geom.crop_rect;
    const orientedSize = NexFilmGeometry.getOrientedDimensions(cw, ch, current_geom);

    canvasWrapper.style.overflow = 'hidden';
    previewCanvas.style.position = 'absolute';
    previewCanvas.style.objectFit = 'fill'; 

    let aspect;
    if (isCropMode) {
        aspect = orientedSize.width / orientedSize.height;
    } else {
        aspect = (orientedSize.width * rect.width) / (orientedSize.height * rect.height);
    }
    if (isNaN(aspect) || aspect === 0) aspect = 1;

    const parent = canvasWrapper.parentElement;
    const availableW = parent.clientWidth - 64; // p-8 padding
    const availableH = parent.clientHeight - 64;
    
    let targetW, targetH;
    if (availableW / availableH > aspect) {
        targetH = availableH;
        targetW = targetH * aspect;
    } else {
        targetW = availableW;
        targetH = targetW / aspect;
    }
    
    targetW *= zoomLevel;
    targetH *= zoomLevel;

    canvasWrapper.style.width = `${targetW}px`;
    canvasWrapper.style.height = `${targetH}px`;
    dummyPusher.style.display = 'none'; canvasWrapper.style.transform = `translate(${panX}px, ${panY}px)`;

    if (isCropMode) {
        previewCanvas.style.width = '100%';
        previewCanvas.style.height = '100%';
        previewCanvas.style.left = '0';
        previewCanvas.style.top = '0';
        
        cropOverlay.classList.remove('hidden');
        if (isCropMode) {
            updateCropOverlay();
        }
    } else {
        previewCanvas.style.width = '100%';
        previewCanvas.style.height = '100%';
        previewCanvas.style.left = '0';
        previewCanvas.style.top = '0';
        
        cropOverlay.classList.add('hidden');
    }
}

btnCropMode.addEventListener('click', () => {
    isCropMode = !isCropMode;
    if (isCropMode) {
        setPerspectiveMode(false);
        btnCropMode.classList.add('active');
        cropOverlay.classList.remove('hidden');
        cropBox.classList.remove('hidden');
        cropMask.classList.remove('hidden');
        cropEdges.classList.remove('hidden');
        cropHandles.classList.remove('hidden');
        updateCropOverlay();
    } else {
        btnCropMode.classList.remove('active');
        cropOverlay.classList.add('hidden');
    }
    updateCanvasTransform();
    requestRender();
});

function formatPerspectiveValue(key, value) {
    if (key === 'angle') return `${value.toFixed(2)}°`;
    if (key === 'perspective_scale') return `${Math.round(value * 100)}%`;
    return `${Math.round(value)}`;
}

function updatePerspectiveUI() {
    current_geom = NexFilmGeometry.normalizeGeometryState(current_geom);
    Object.entries(perspectiveControls).forEach(([key, control]) => {
        const value = current_geom[key];
        control.el.value = key === 'perspective_scale' ? String(value * 100) : String(value);
        control.val.textContent = formatPerspectiveValue(key, value);
        updateSliderTrack(control.el);
    });
    perspectiveConstrain.checked = current_geom.constrain_crop;
}

function setPerspectiveMode(enabled) {
    isPerspectiveMode = !!enabled;
    btnPerspectiveMode.classList.toggle('active', isPerspectiveMode);
    perspectivePanel.classList.toggle('hidden', !isPerspectiveMode);
    perspectivePanel.setAttribute('aria-hidden', String(!isPerspectiveMode));
    perspectiveOverlay.classList.toggle('hidden', !isPerspectiveMode);
    if (isPerspectiveMode) {
        isCropMode = false;
        btnCropMode.classList.remove('active');
        cropOverlay.classList.add('hidden');
        updatePerspectiveUI();
    }
    updateCanvasTransform();
    requestRender();
}

btnPerspectiveMode.addEventListener('click', () => setPerspectiveMode(!isPerspectiveMode));
btnClosePerspective.addEventListener('click', () => setPerspectiveMode(false));

function constrainPerspectiveScale() {
    if (!current_geom.constrain_crop) return;
    const minimumScale = NexFilmGeometry.getConstrainedPerspectiveScale({
        ...current_geom,
        perspective_scale: 1,
    });
    current_geom.perspective_scale = Math.max(current_geom.perspective_scale, minimumScale);
}

Object.entries(perspectiveControls).forEach(([key, control]) => {
    control.el.addEventListener('pointerdown', pushUndoState);
    control.el.addEventListener('input', event => {
        const rawValue = Number.parseFloat(event.target.value);
        current_geom[key] = key === 'perspective_scale' ? rawValue / 100 : rawValue;
        constrainPerspectiveScale();
        updatePerspectiveUI();
        updateCanvasTransform();
        requestRender();
    });
    control.el.addEventListener('change', sendGeometrySync);
});

perspectiveConstrain.addEventListener('change', () => {
    pushUndoState();
    current_geom.constrain_crop = perspectiveConstrain.checked;
    constrainPerspectiveScale();
    updatePerspectiveUI();
    sendGeometrySync();
});

btnResetPerspective.addEventListener('click', () => {
    if (!activeId) return;
    pushUndoState();
    Object.assign(current_geom, {
        angle: 0,
        perspective_vertical: 0,
        perspective_horizontal: 0,
        perspective_aspect: 0,
        perspective_scale: 1,
        constrain_crop: false,
    });
    updatePerspectiveUI();
    sendGeometrySync();
});


function updateSpatialSamples(transform) {
    if (current_geom.calibration_points) {
        current_geom.calibration_points = transform.calibrationPoints;
    }
    if (currentSprocketUV[0] >= 0 && currentSprocketUV[1] >= 0) {
        currentSprocketUV = new Float32Array(transform.transformPoint(currentSprocketUV));
    }
}

let geomSyncId = 0;
let geometrySyncChain = Promise.resolve();

function persistGeometryQueued(id, geom) {
    const snapshot = JSON.parse(JSON.stringify(geom));
    const write = geometrySyncChain
        .catch(() => {})
        .then(() => invoke('update_geometry', { id, geom: snapshot }));
    geometrySyncChain = write;
    return write;
}

function sendGeometrySync() {
    if (!activeId) return;
    const targetId = activeId;
    const geomSnapshot = JSON.parse(JSON.stringify(current_geom));
    geomSyncId++;
    const currentSyncId = geomSyncId;
    
    updateCanvasTransform();
    requestRender();
    
    // Geometry writes must be serialized. Parallel IPC calls can commit out
    // of order in SQLite, which makes a flip visibly snap back when an older
    // request finishes after the newer one.
    void persistGeometryQueued(targetId, geomSnapshot)
        .then(() => {
            if (geomSyncId !== currentSyncId || targetId !== activeId) return;
            // Geometry is a GPU display transform; the canonical proxy does
            // not need to be decoded or uploaded again for a flip/rotation.
            requestRender();
            requestThumbnailSync();
        })
        .catch(error => {
            console.error("Failed to save geometry", error);
            showToast("Could not save geometry: " + error, "error");
        });
}

btnRotateLeft.addEventListener('click', () => {
    if (!activeId) return; pushUndoState();
    const transformed = NexFilmGeometry.transformGeometryForQuarterTurn(current_geom, true);
    current_geom.rotate_90_count += 1;
    current_geom.crop_rect = transformed.cropRect;
    updateSpatialSamples(transformed);
    sendGeometrySync();
});

btnRotateRight.addEventListener('click', () => {
    if (!activeId) return; pushUndoState();
    const transformed = NexFilmGeometry.transformGeometryForQuarterTurn(current_geom, false);
    current_geom.rotate_90_count -= 1;
    current_geom.crop_rect = transformed.cropRect;
    updateSpatialSamples(transformed);
    sendGeometrySync();
});

btnFlipH.addEventListener('click', () => {
    if (!activeId) return; pushUndoState();
    const transformed = NexFilmGeometry.transformGeometryForFlip(current_geom, true, false);
    current_geom.flip_h = !current_geom.flip_h;
    current_geom.crop_rect = transformed.cropRect;
    updateSpatialSamples(transformed);
    sendGeometrySync();
});

btnFlipV.addEventListener('click', () => {
    if (!activeId) return; pushUndoState();
    const transformed = NexFilmGeometry.transformGeometryForFlip(current_geom, false, true);
    current_geom.flip_v = !current_geom.flip_v;
    current_geom.crop_rect = transformed.cropRect;
    updateSpatialSamples(transformed);
    sendGeometrySync();
});

async function doAutoColor() {
    if (!activeId || !gl) return;

    // --- PHASE 1: Baseline Isolation ---
    // Force WebGL to a neutral state to prevent color feedback loops.
    // D-min=0, D-max=3, Exposure=0, Gamma=1, no LUT/Shadows/Highlights.
    const mode = btnModeColor.classList.contains('bg-[#28282c]') ? 0 : 1;
    gl.useProgram(shaderProgram);
    gl.bindVertexArray(vao);
    
    gl.uniform3f(u_base_density_loc, currentBaseDensity[0], currentBaseDensity[1], currentBaseDensity[2]);
    gl.uniform3f(u_dmin_loc, -1.0, -1.0, -1.0);
    gl.uniform3f(u_dmax_loc, 3.0, 3.0, 3.0);
    gl.uniform3f(u_exposure_loc, 0.0, 0.0, 0.0);
    gl.uniform1f(u_gamma_loc, 1.0);
    gl.uniform1i(u_mode_loc, mode);
    gl.uniform1i(u_invert_enabled_loc, 1);
    gl.uniform1f(u_highlights_loc, 0.0);
    gl.uniform1f(u_shadows_loc, 0.0);
    gl.uniform1i(u_has_lut_loc, 0);
    let pts = current_geom.calibration_points || [[0, 0], [1, 0], [1, 1], [0, 1]];
    let homographyMat = getHomography(pts);
    gl.uniformMatrix3fv(u_homography_loc, false, homographyMat);
    gl.uniform4f(
        u_perspective_loc,
        current_geom.perspective_vertical,
        current_geom.perspective_horizontal,
        current_geom.perspective_aspect,
        current_geom.perspective_scale
    );
    gl.uniform2fv(u_sprocket_uv_loc, new Float32Array([-1.0, -1.0])); // Disable target during calibration
    gl.uniform1f(u_sprocket_tolerance_loc, 0.0);
    
    const geometryUv = NexFilmGeometry.createInverseGeometryMatrix(
        proxyWidth,
        proxyHeight,
        current_geom
    );
    gl.uniformMatrix3fv(u_geometry_uv_loc, false, geometryUv);
    gl.uniform4f(u_crop_loc, current_geom.crop_rect.x, current_geom.crop_rect.y, current_geom.crop_rect.width, current_geom.crop_rect.height);

    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);

    // Render pure image to FBO
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.viewport(0, 0, HIST_W, HIST_H);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
    
    const purePixels = new Uint8Array(HIST_W * HIST_H * 4);
    gl.readPixels(0, 0, HIST_W, HIST_H, gl.RGBA, gl.UNSIGNED_BYTE, purePixels);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);

    const minX = Math.min(...pts.map(point => point[0]));
    const maxX = Math.max(...pts.map(point => point[0]));
    const minY = Math.min(...pts.map(point => point[1]));
    const maxY = Math.max(...pts.map(point => point[1]));

    // --- PHASE 2: Calculation ---
    let r_arr = []; let g_arr = []; let b_arr = [];
    const appendSample = (i) => {
        r_arr.push(purePixels[i]);
        g_arr.push(purePixels[i + 1]);
        b_arr.push(purePixels[i + 2]);
    };
    for (let i = 0; i < purePixels.length; i += 4) {
        const pixelIndex = i / 4;
        const baseUvX = (pixelIndex % HIST_W) / (HIST_W - 1);
        const baseUvY = 1.0 - Math.floor(pixelIndex / HIST_W) / (HIST_H - 1);
        const sourceX = current_geom.crop_rect.x + baseUvX * current_geom.crop_rect.width;
        const sourceY = current_geom.crop_rect.y + baseUvY * current_geom.crop_rect.height;

        if (sourceX >= minX && sourceX <= maxX && sourceY >= minY && sourceY <= maxY) {
            appendSample(i);
        }
    }

    // Invalid or stale geometry must not produce an empty histogram. Fall back
    // to pixels that the homography actually rendered, excluding its black border.
    const minimumSamples = Math.max(64, Math.floor(HIST_W * HIST_H * 0.005));
    if (r_arr.length < minimumSamples) {
        r_arr = []; g_arr = []; b_arr = [];
        for (let i = 0; i < purePixels.length; i += 4) {
            if (purePixels[i] !== 0 || purePixels[i + 1] !== 0 || purePixels[i + 2] !== 0) {
                appendSample(i);
            }
        }
    }

    if (r_arr.length < minimumSamples) {
        throw new Error('The selected film area contains too little image data.');
    }

    r_arr.sort((a,b)=>a-b);
    g_arr.sort((a,b)=>a-b);
    b_arr.sort((a,b)=>a-b);
    if (r_arr[r_arr.length - 1] === 0 && g_arr[g_arr.length - 1] === 0 && b_arr[b_arr.length - 1] === 0) {
        throw new Error('Calibration preview is not ready.');
    }

    function computeExtremes(arr) {
        let hist = new Array(256).fill(0);
        for(let i=0; i<arr.length; i++) hist[arr[i]]++;
        let total = arr.length; let threshold = total * 0.10;
        let min_val = 0; let acc_low = 0;
        for(let i=0; i<256; i++) {
            if(hist[i] > threshold && acc_low < total * 0.2) continue;
            acc_low += hist[i];
            if(acc_low >= total * 0.01) { min_val = i; break; }
        }
        let max_val = 255; let acc_high = 0;
        for(let i=255; i>=0; i--) {
            if(hist[i] > threshold && acc_high < total * 0.2) continue;
            acc_high += hist[i];
            if(acc_high >= total * 0.01) { max_val = i; break; }
        }
        return [min_val, max_val];
    }
    let [rStart, rEnd] = computeExtremes(r_arr);
    let [gStart, gEnd] = computeExtremes(g_arr);
    let [bStart, bEnd] = computeExtremes(b_arr);

    // In Baseline Isolation (Dmin=-1.0, Dmax=3.0, Gamma=1.0),
    // norm = (density - (-1.0)) / (3.0 - (-1.0)) = (density + 1.0) / 4.0
    // screen_val = norm * 255.
    // So to retrieve true density from screen_val: density = (val / 255.0) * 4.0 - 1.0
    function toDensity(val) {
        return (val / 255.0) * 4.0 - 1.0;
    }

    const batchState = {
        dmin: [
            toDensity(rStart),
            toDensity(gStart),
            toDensity(bStart)
        ],
        dmax: [
            toDensity(rEnd),
            toDensity(gEnd),
            toDensity(bEnd)
        ]
    };

    currentDMin = batchState.dmin;
    currentDMax = batchState.dmax;
    updateDMinMaxDisplay();

    sliders.expr.el.value = 0; sliders.expr.val.textContent = "0.000";
    sliders.expg.el.value = 0; sliders.expg.val.textContent = "0.000";
    sliders.expb.el.value = 0; sliders.expb.val.textContent = "0.000";
    
    updateSliderTrack(sliders.expr.el);
    updateSliderTrack(sliders.expg.el);
    updateSliderTrack(sliders.expb.el);

    await updateBackendParams();
    renderWebGL();
}

const proxyPreparePromises = new Map();
const proxyDisplayPromises = new Map();
const proxyPrewarmQueue = [];
const proxyPrewarmQueued = new Set();
let proxyPrewarmRunning = false;

function ensureProxyPrepared(id) {
    const existing = proxyPreparePromises.get(id);
    if (existing) return existing;
    const promise = invoke('prepare_proxy', { id }).finally(() => {
        if (proxyPreparePromises.get(id) === promise) {
            proxyPreparePromises.delete(id);
        }
    });
    proxyPreparePromises.set(id, promise);
    return promise;
}

function queueProxyPrewarm(id) {
    if (!id || proxyPrewarmQueued.has(id)) return;
    proxyPrewarmQueued.add(id);
    proxyPrewarmQueue.push(id);
    void drainProxyPrewarmQueue();
}

function getProxyPrewarmOrder() {
    const importPaths = activeImportViewPaths || currentImportSessionPaths;
    if (importPaths?.length) {
        return importPaths
            .map(path => allLibraryItems.find(item => normalizePath(item.file_path) === path)?.id)
            .filter(Boolean);
    }

    if (currentRollViewId && isRollEditing) {
        const roll = allRolls.find(candidate => candidate.roll_id === currentRollViewId);
        if (roll?.image_paths?.length) {
            return roll.image_paths
                .map(path => allLibraryItems.find(item =>
                    item.roll_id === currentRollViewId
                    && normalizePath(item.file_path) === normalizePath(path)
                )?.id)
                .filter(Boolean);
        }
    }

    // The rendered filmstrip is already scoped by the existing state machine.
    // Using it as the final fallback covers loose edits without pulling frames
    // from History Films' preview-only mode into the RAW decode queue.
    return Array.from(document.querySelectorAll('#filmstrip-container .film-item'))
        .map(element => element.dataset.id)
        .filter(Boolean);
}

function queueAdjacentProxyPrewarm(id) {
    const orderedIds = getProxyPrewarmOrder();
    const index = orderedIds.indexOf(id);
    if (index < 0) return;
    for (let offset = 1; offset <= 4; offset++) {
        queueProxyPrewarm(orderedIds[index + offset]);
    }
}

async function drainProxyPrewarmQueue() {
    if (proxyPrewarmRunning) return;
    proxyPrewarmRunning = true;
    try {
        while (proxyPrewarmQueue.length) {
            const id = proxyPrewarmQueue.shift();
            proxyPrewarmQueued.delete(id);
            if (!id || id === activeId) continue;
            try {
                // Decode ahead of time, but keep this deliberately single-file
                // so background prewarming never competes with the active edit.
                await ensureProxyPrepared(id);
            } catch (error) {
                console.debug('Proxy prewarm skipped', id, error);
            }
        }
    } finally {
        proxyPrewarmRunning = false;
    }
}

function ensureProxyDisplayed(id, options = {}) {
    const existing = proxyDisplayPromises.get(id);
    if (existing) return existing;
    const promise = (async () => {
        await ensureProxyPrepared(id);
        if (id !== activeId) return false;
        return reloadDevelopProxy(current_geom, {
            showLoading: false,
            persistThumbnail: options.persistThumbnail !== false,
        });
    })().finally(() => {
        if (proxyDisplayPromises.get(id) === promise) {
            proxyDisplayPromises.delete(id);
        }
    });
    proxyDisplayPromises.set(id, promise);
    return promise;
}

async function reloadDevelopProxy(geomSnapshot = current_geom, options = {}) {
    if (!activeId) return;
    const targetId = activeId;
    const snapshot = JSON.parse(JSON.stringify(geomSnapshot));
    const token = currentImageRequestToken;
    const loadingUI = document.getElementById('loading-proxy-ui');
    const showLoading = options.showLoading !== false;
    if (loadingUI && showLoading) {
        loadingUI.classList.remove('hidden');
        loadingUI.classList.add('flex');
    }

    try {
        proxyCache.delete(targetId);
        readyProxyIds.delete(targetId);
        if (options.prepare) await ensureProxyPrepared(targetId);
        if (token !== currentImageRequestToken) return;
        const loadedFullProxy = await loadProxyImage(token, snapshot);
        if (token !== currentImageRequestToken || targetId !== activeId) return false;
        if (!loadedFullProxy) {
            throw new Error("RAW processing is not ready yet.");
        }

        if (options.reveal !== false) {
            previewCanvas.style.display = 'block';
            hideThumbnailPlaceholder();
        }
        hasProcessedActiveImage = true;
        if (options.persistThumbnail !== false) requestThumbnailSync();
        return true;
    } catch (e) {
        if (token !== currentImageRequestToken || targetId !== activeId) return false;
        console.error(e);
        showToast((options.errorPrefix || "Proxy refresh failed: ") + e, "error");
        return false;
    } finally {
        if (loadingUI && showLoading && token === currentImageRequestToken) {
            loadingUI.classList.add('hidden');
            loadingUI.classList.remove('flex');
        }
    }
}

async function runAutoInvert() {
    if (!activeId) return;
    if (isCalibrationMode || !current_geom.calibration_points) {
        showToast("Confirm the film area before Auto Invert.", "info");
        return;
    }
    const targetId = activeId;
    const token = currentImageRequestToken;
    const geomSnapshot = JSON.parse(JSON.stringify(current_geom));
    try {
        await persistGeometryQueued(targetId, geomSnapshot);
    } catch (error) {
        showToast("Auto invert failed: " + error, "error");
        return;
    }
    try {
        await ensureProxyPrepared(targetId);
        await invoke('analyze_proxy_base_color', { id: targetId });
        proxyAnalyzedBaseIds.add(targetId);
        proxyHasAnalyzedBase = true;
        const pendingDisplay = proxyDisplayPromises.get(targetId);
        if (pendingDisplay) await pendingDisplay;
        if (token !== currentImageRequestToken || targetId !== activeId) return;
        // Base-density metadata is part of the response header. Reload once
        // after analysis so an already displayed proxy cannot retain the
        // default half-white base used during the pre-invert staging view.
        const loaded = await reloadDevelopProxy(geomSnapshot, {
            showLoading: true,
            persistThumbnail: false,
            reveal: false,
            errorPrefix: "Auto invert failed: ",
        });
        if (!loaded) return;
        await new Promise(resolve => requestAnimationFrame(resolve));
        await doAutoColor();
        previewCanvas.style.display = 'block';
        hideThumbnailPlaceholder();
        requestThumbnailSync();
    } catch (error) {
        showToast("Auto invert failed: " + error, "error");
    }
}

document.getElementById('btn-reset-crop').addEventListener('click', async () => {
    if (!activeId) return; pushUndoState();
    calibrationRevision++;
    current_geom.crop_rect = { x: 0, y: 0, width: 1, height: 1 };
    current_geom.angle = 0.0;
    current_geom.perspective_vertical = 0.0;
    current_geom.perspective_horizontal = 0.0;
    current_geom.perspective_aspect = 0.0;
    current_geom.perspective_scale = 1.0;
    current_geom.constrain_crop = false;
    current_geom.flip_h = false;
    current_geom.flip_v = false;
    current_geom.rotate_90_count = 0;
    current_geom.calibration_points = null;
    current_geom.calibration_confirmed = false;
    btnBatchApply.disabled = true;
    currentSprocketUV = new Float32Array([-1.0, -1.0]);
    if (isCropMode) updateCropOverlay();
    updatePerspectiveUI();
    await persistGeometryQueued(activeId, current_geom);
    await updateBackendParams();
    if (hasProcessedActiveImage) requestRender();
    requestThumbnailSync();
});



function enterCalibrationMode() {
    calibrationRevision++;
    isCalibrationMode = true;
    document.getElementById('calibration-overlay').classList.remove('hidden');
    setDevelopInspectorCalibrationLocked(true);
    btnBatchApply.disabled = true;
    if (current_geom.calibration_points) {
        calibrationPoints = JSON.parse(JSON.stringify(current_geom.calibration_points));
    } else {
        calibrationPoints = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    }
    requestAnimationFrame(updateCalibrationPolygon);
    if (activeProxyIsFull) requestRender();
}

btnRecalibrate.addEventListener('click', enterCalibrationMode);

function validateFilmAreaDetection(result) {
    if (!Array.isArray(result?.points) || result.points.length !== 4) {
        throw new Error('The detector returned an invalid quadrilateral.');
    }
    const points = result.points.map(point => [Number(point[0]), Number(point[1])]);
    if (!NexFilmGeometry.isValidCalibrationQuad(points)) {
        throw new Error('The detector returned an invalid point order.');
    }
    return points;
}

function requestFilmAreaDetection(id) {
    const existing = filmAreaDetectionPromises.get(id);
    if (existing) return existing;
    const source = findKnownItem(id);
    if (!source?.file_path) {
        return Promise.reject(new Error('Cached image identity is unavailable.'));
    }
    const request = invoke('auto_detect_film_border', {
        roll_id: source.roll_id || 'LOOSE_DEFAULT',
        file_path: source.file_path
    }).finally(() => {
        if (filmAreaDetectionPromises.get(id) === request) {
            filmAreaDetectionPromises.delete(id);
        }
    });
    filmAreaDetectionPromises.set(id, request);
    return request;
}

function setAutoAreaBusy(id, busy) {
    if (id !== activeId) return;
    btnAutoArea.disabled = busy || !activeId;
    btnAutoArea.toggleAttribute('aria-busy', busy);
    btnAutoArea.textContent = busy ? 'Detecting...' : 'Auto Area';
}

function applyFilmAreaDetection(result) {
    calibrationPoints = validateFilmAreaDetection(result);
    btnBatchApply.disabled = true;
    updateCalibrationPolygon();
}

function isSafeDefaultFilmAreaDetection(result) {
    return result?.confidence === 'high' && result?.status === 'detected_gradient';
}

async function initializeDefaultFilmArea(id, requestToken, revision) {
    setAutoAreaBusy(id, true);
    try {
        const result = await requestFilmAreaDetection(id);
        if (id !== activeId || requestToken !== currentImageRequestToken || revision !== calibrationRevision) {
            return;
        }
        applyFilmAreaDetection(result);
        if (!isSafeDefaultFilmAreaDetection(result)) {
            showToast("自动探测结果需要确认，请手动微调并保存。", "info");
            return;
        }

        const detectedGeom = {
            ...JSON.parse(JSON.stringify(current_geom)),
            calibration_points: cloneCalibrationPoints(calibrationPoints),
            calibration_confirmed: false
        };
        await persistGeometryQueued(id, detectedGeom);
        if (id !== activeId || requestToken !== currentImageRequestToken || revision !== calibrationRevision) {
            return;
        }
        current_geom = detectedGeom;
        saveCurrentState();
        showToast("Film area detected. Review the frame and save it.", "success");
    } catch (error) {
        console.error('Automatic film-area initialization failed', error);
        if (id === activeId && requestToken === currentImageRequestToken) {
            showToast("Automatic film-area detection failed. Set the area manually.", "error");
        }
    } finally {
        setAutoAreaBusy(id, false);
    }
}

btnAutoArea.addEventListener('click', async () => {
    if (!activeId || btnAutoArea.getAttribute('aria-busy') === 'true') return;

    const detectionId = activeId;
    if (!isCalibrationMode) enterCalibrationMode();
    const revision = ++calibrationRevision;
    setAutoAreaBusy(detectionId, true);
    try {
        const result = await requestFilmAreaDetection(detectionId);
        if (detectionId !== activeId || revision !== calibrationRevision) return;
        pushUndoState();
        applyFilmAreaDetection(result);
        if (result.confidence === 'low') {
            showToast("自动探测置信度较低，请手动微调。", "info");
        } else {
            showToast("Film area detected. Review the corners before saving.", "success");
        }
    } catch (error) {
        console.error('Auto Area failed', error);
        showToast("Auto Area failed: " + error, "error");
    } finally {
        setAutoAreaBusy(detectionId, false);
    }
});

btnAutoColor.addEventListener('click', async () => {
    pushUndoState();
    await runAutoInvert();
});

btnSprocketPicker.addEventListener('click', () => {
    isSprocketPickerActive = !isSprocketPickerActive;
    if (isSprocketPickerActive) {
        canvasWrapper.parentElement.style.cursor = 'crosshair';
        btnSprocketPicker.classList.add('bg-zinc-600');
    } else {
        canvasWrapper.parentElement.style.cursor = '';
        btnSprocketPicker.classList.remove('bg-zinc-600');
    }
});

async function showConfirm(message) {
    return new Promise(resolve => {
        const overlay = document.createElement('div');
        overlay.className = 'fixed inset-0 bg-black/80 flex items-center justify-center z-50 backdrop-blur-sm';
        overlay.innerHTML = `
            <div class="bg-[#1a1a1e] border border-[#28282c] rounded-lg p-6 max-w-sm w-full mx-4 shadow-2xl">
                <h3 class="text-zinc-100 font-bold mb-2">Confirm Action</h3>
                <p class="text-zinc-400 text-sm mb-6">${message}</p>
                <div class="flex justify-end gap-3">
                    <button id="btn-confirm-cancel" class="px-4 py-2 text-xs font-bold tracking-wider uppercase bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded transition-colors">Cancel</button>
                    <button id="btn-confirm-ok" class="px-4 py-2 text-xs font-bold tracking-wider uppercase bg-red-900/50 hover:bg-red-800/60 border border-red-700/50 text-white rounded transition-colors">Reset</button>
                </div>
            </div>
        `;
        document.body.appendChild(overlay);
        
        document.getElementById('btn-confirm-cancel').onclick = () => {
            document.body.removeChild(overlay);
            resolve(false);
        };
        document.getElementById('btn-confirm-ok').onclick = () => {
            document.body.removeChild(overlay);
            resolve(true);
        };
    });
}

async function showDeleteRollDialog(rollCount) {
    return new Promise(resolve => {
        const overlay = document.createElement('div');
        overlay.className = 'fixed inset-0 bg-black/80 flex items-center justify-center z-[110] backdrop-blur-sm';
        overlay.innerHTML = `
            <div class="bg-[#1a1a1e] border border-[#28282c] rounded-lg p-6 max-w-md w-full mx-4 shadow-2xl">
                <h3 class="text-zinc-100 font-bold mb-2">Remove ${rollCount} roll${rollCount === 1 ? '' : 's'}?</h3>
                <p class="text-zinc-400 text-sm mb-2">Choose whether to remove only the NexFilm catalog records or also delete the original source files.</p>
                <p class="text-red-300 text-xs mb-6">Deleting source files is permanent. Files still referenced by another roll will be kept.</p>
                <div class="flex flex-wrap justify-end gap-3">
                    <button data-delete-choice="cancel" class="px-4 py-2 text-xs font-bold bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded transition-colors">Cancel</button>
                    <button data-delete-choice="catalog" class="px-4 py-2 text-xs font-bold bg-zinc-700 hover:bg-zinc-600 text-white rounded transition-colors">Remove Records</button>
                    <button data-delete-choice="files" class="px-4 py-2 text-xs font-bold bg-red-900/70 hover:bg-red-800 border border-red-700/60 text-white rounded transition-colors">Delete Source Files</button>
                </div>
            </div>
        `;
        document.body.appendChild(overlay);
        overlay.querySelectorAll('[data-delete-choice]').forEach(button => {
            button.addEventListener('click', () => {
                overlay.remove();
                resolve(button.dataset.deleteChoice === 'cancel' ? null : button.dataset.deleteChoice);
            });
        });
    });
}

async function showDeleteImagesDialog(imageCount) {
    return new Promise(resolve => {
        const overlay = document.createElement('div');
        overlay.className = 'fixed inset-0 bg-black/80 flex items-center justify-center z-[110] backdrop-blur-sm';
        overlay.innerHTML = `
            <div class="bg-[#1a1a1e] border border-[#28282c] rounded-lg p-6 max-w-md w-full mx-4 shadow-2xl">
                <h3 class="text-zinc-100 font-bold mb-2">Delete ${imageCount} selected photo${imageCount === 1 ? '' : 's'}?</h3>
                <p class="text-zinc-400 text-sm mb-2">Choose whether to remove the selected photo records from NexFilm or also delete their original source files.</p>
                <p class="text-red-300 text-xs mb-6">Deleting source files is permanent. Files still referenced elsewhere will be kept.</p>
                <div class="flex flex-wrap justify-end gap-3">
                    <button data-delete-image-choice="cancel" class="px-4 py-2 text-xs font-bold bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded transition-colors">Cancel</button>
                    <button data-delete-image-choice="catalog" class="px-4 py-2 text-xs font-bold bg-zinc-700 hover:bg-zinc-600 text-white rounded transition-colors">Remove from NexFilm</button>
                    <button data-delete-image-choice="files" class="px-4 py-2 text-xs font-bold bg-red-900/70 hover:bg-red-800 border border-red-700/60 text-white rounded transition-colors">Delete Source Files</button>
                </div>
            </div>
        `;
        document.body.appendChild(overlay);
        overlay.querySelectorAll('[data-delete-image-choice]').forEach(button => {
            button.addEventListener('click', () => {
                overlay.remove();
                resolve(button.dataset.deleteImageChoice === 'cancel' ? null : button.dataset.deleteImageChoice);
            });
        });
    });
}

btnResetColor.addEventListener('click', async () => {
    if (!activeId) return;
    
    const isConfirmed = await showConfirm("Are you sure you want to reset all color adjustments?");
    if (!isConfirmed) return;

    
    pushUndoState();
    
    currentDMin = [0.1, 0.1, 0.1];
    currentDMax = [2.0, 2.0, 2.0];
    updateDMinMaxDisplay();
    
    sliders.masterDmin.el.value = 0; sliders.masterDmin.val.textContent = "0.000"; lastMasterDmin = 0;
    sliders.masterDmax.el.value = 0; sliders.masterDmax.val.textContent = "0.000"; lastMasterDmax = 0;
    
    sliders.exposure.el.value = 0;
    sliders.gamma.el.value = 1;
    sliders.saturation.el.value = 0;
    sliders.temperature.el.value = 0;
    sliders.tint.el.value = 0;
    sliders.expr.el.value = 0;
    sliders.expg.el.value = 0;
    sliders.expb.el.value = 0;
    sliders.highlights.el.value = 0;
    sliders.shadows.el.value = 0;
    sliders.lutOpacity.el.value = 1;
    
    for (const key in sliders) {
        const s = sliders[key];
        s.val.textContent = parseFloat(s.el.value).toFixed(3);
        updateSliderTrack(s.el);
    }
    
    updateBackendParams();
    renderWebGL();
    requestThumbnailSync();
});

function getRenderRect() { return canvasWrapper.getBoundingClientRect(); }

function updateCropOverlay() {
    if (!isCropMode) return;
    const overlayRect = cropOverlay.getBoundingClientRect();
    if (!overlayRect.width || !overlayRect.height) return;
    const x = current_geom.crop_rect.x * overlayRect.width;
    const y = current_geom.crop_rect.y * overlayRect.height;
    const w = current_geom.crop_rect.width * overlayRect.width;
    const h = current_geom.crop_rect.height * overlayRect.height;

    cropBox.setAttribute('x', x); cropBox.setAttribute('y', y);
    cropBox.setAttribute('width', w); cropBox.setAttribute('height', h);

    const maskPath = `M0,0 H${overlayRect.width} V${overlayRect.height} H0 Z M${x},${y} V${y + h} H${x + w} V${y} Z`;
    cropMask.setAttribute('d', maskPath);

    const gridCommands = [];
    for (let index = 1; index < 6; index++) {
        const gridX = x + w * index / 6;
        const gridY = y + h * index / 6;
        gridCommands.push(`M${gridX},${y} V${y + h}`);
        gridCommands.push(`M${x},${gridY} H${x + w}`);
    }
    document.getElementById('crop-grid-lines').setAttribute('d', gridCommands.join(' '));

    const setHandle = (pos, hx, hy) => {
        const handle = cropHandles.querySelector(`[data-pos="${pos}"]`);
        if (handle) {
            handle.removeAttribute('transform');
            handle.setAttribute('x', hx - 5);
            handle.setAttribute('y', hy - 5);
            handle.setAttribute('width', 10);
            handle.setAttribute('height', 10);
        }
    };
    setHandle('nw', x, y); setHandle('n', x + w/2, y); setHandle('ne', x + w, y);
    setHandle('w', x, y + h/2); setHandle('e', x + w, y + h/2);
    setHandle('sw', x, y + h); setHandle('s', x + w/2, y + h); setHandle('se', x + w, y + h);

    const setEdge = (pos, x1, y1, x2, y2) => {
        const edge = cropEdges.querySelector(`[data-pos="${pos}"]`);
        if (!edge) return;
        edge.setAttribute('x1', x1);
        edge.setAttribute('y1', y1);
        edge.setAttribute('x2', x2);
        edge.setAttribute('y2', y2);
    };
    setEdge('n', x, y, x + w, y);
    setEdge('e', x + w, y, x + w, y + h);
    setEdge('s', x, y + h, x + w, y + h);
    setEdge('w', x, y, x, y + h);
}

let isDraggingCrop = false;
let dragType = null;
let dragStartPos = { x: 0, y: 0 };
let dragStartRect = { x: 0, y: 0, width: 1, height: 1 };
let dragStartAngle = 0;
let dragCenter = { x: 0, y: 0 };
let activeCropPointerId = null;
const MIN_CROP_SIZE = 0.01;

function clampCropRect(rect) {
    const width = Math.min(1, Math.max(MIN_CROP_SIZE, rect.width));
    const height = Math.min(1, Math.max(MIN_CROP_SIZE, rect.height));
    return {
        x: Math.min(1 - width, Math.max(0, rect.x)),
        y: Math.min(1 - height, Math.max(0, rect.y)),
        width,
        height,
    };
}

cropOverlay.addEventListener('pointerdown', (e) => {
    if (!isCropMode || e.button !== 0) return;
    const target = e.target;

    if (target.classList.contains('crop-handle') && isCropMode) {
        dragType = target.getAttribute('data-pos');
    } else if (target.classList.contains('crop-edge') && isCropMode) {
        dragType = target.getAttribute('data-pos');
    } else if (target === cropBox && isCropMode) {
        dragType = 'box';
    } else if (target === rotateHandleOuter) {
        dragType = 'rotate';
    } else {
        return;
    }

    pushUndoState();
    isDraggingCrop = true; dragStartPos = { x: e.clientX, y: e.clientY };
    dragStartRect = clampCropRect(current_geom.crop_rect); dragStartAngle = current_geom.angle;
    activeCropPointerId = e.pointerId;
    cropOverlay.setPointerCapture(e.pointerId);
    const rect = canvasWrapper.getBoundingClientRect();
    dragCenter = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    cropGrid.style.opacity = '1';
    cropOverlay.classList.add('is-dragging');
    e.preventDefault();
});

window.addEventListener('pointermove', (e) => {
    if (!isDraggingCrop || e.pointerId !== activeCropPointerId) return;
    const renderRect = getRenderRect();
    const dx = (e.clientX - dragStartPos.x) / renderRect.width;
    const dy = (e.clientY - dragStartPos.y) / renderRect.height;
    let newRect = { ...dragStartRect };

    if (dragType === 'box') {
        newRect.x = Math.min(1 - newRect.width, Math.max(0, newRect.x + dx));
        newRect.y = Math.min(1 - newRect.height, Math.max(0, newRect.y + dy));
    } else if (dragType === 'rotate') {
        const startRad = Math.atan2(dragStartPos.y - dragCenter.y, dragStartPos.x - dragCenter.x);
        const currentRad = Math.atan2(e.clientY - dragCenter.y, e.clientX - dragCenter.x);
        let deltaDeg = (currentRad - startRad) * (180 / Math.PI);
        if (e.shiftKey) deltaDeg *= 0.1;
        current_geom.angle = Math.max(-45, Math.min(45, dragStartAngle - deltaDeg));
        updatePerspectiveUI();
        updateCanvasTransform();
        requestRender();
        return;
    } else {
        const left = dragStartRect.x;
        const top = dragStartRect.y;
        const right = left + dragStartRect.width;
        const bottom = top + dragStartRect.height;
        if (dragType.includes('w')) {
            newRect.x = Math.min(right - MIN_CROP_SIZE, Math.max(0, left + dx));
            newRect.width = right - newRect.x;
        }
        if (dragType.includes('e')) {
            newRect.width = Math.min(1, Math.max(left + MIN_CROP_SIZE, right + dx)) - left;
        }
        if (dragType.includes('n')) {
            newRect.y = Math.min(bottom - MIN_CROP_SIZE, Math.max(0, top + dy));
            newRect.height = bottom - newRect.y;
        }
        if (dragType.includes('s')) {
            newRect.height = Math.min(1, Math.max(top + MIN_CROP_SIZE, bottom + dy)) - top;
        }
    }
    current_geom.crop_rect = clampCropRect(newRect); updateCropOverlay();
});

function finishCropDrag(e, persist) {
    if (!isDraggingCrop || e.pointerId !== activeCropPointerId) return;
    if (cropOverlay.hasPointerCapture(e.pointerId)) {
        cropOverlay.releasePointerCapture(e.pointerId);
    }
    isDraggingCrop = false;
    activeCropPointerId = null;
    cropGrid.style.opacity = '';
    cropOverlay.classList.remove('is-dragging');
    if (!persist) {
        if (dragType === 'rotate') current_geom.angle = dragStartAngle;
        else current_geom.crop_rect = { ...dragStartRect };
        updateCropOverlay();
        requestRender();
        return;
    }
    if (persist && activeId) {
        try {
            sendGeometrySync();
        } catch (err) { showToast("Crop failed: " + err, "error"); }
    }
}

window.addEventListener('pointerup', e => finishCropDrag(e, true));
window.addEventListener('pointercancel', e => finishCropDrag(e, false));

function batchTargetItems(mode) {
    const source = findKnownItem(activeId);
    if (!source) return [];
    const candidates = uniqueItemsByPath(Array.from(itemIndex.values()));
    if (mode === 'selected') {
        return Array.from(selectedLibraryIds)
            .map(findKnownItem)
            .filter(item => item && item.id !== activeId && item.file_path);
    }
    return candidates.filter(item =>
        item.id !== activeId &&
        item.roll_id === source.roll_id &&
        item.file_path &&
        !item.rendered_thumbnail_base64
    );
}

function refreshBatchTargetCounts() {
    const rollTargets = batchTargetItems('roll-unprocessed');
    const selectedTargets = batchTargetItems('selected');
    document.getElementById('batch-roll-count').textContent = String(rollTargets.length);
    document.getElementById('batch-selected-count').textContent = String(selectedTargets.length);
    const selectedRadio = document.querySelector('input[name="batch-target"][value="selected"]');
    selectedRadio.disabled = selectedTargets.length === 0;
    if (selectedRadio.checked && selectedRadio.disabled) {
        document.querySelector('input[name="batch-target"][value="roll-unprocessed"]').checked = true;
    }
}

function closeBatchApplyModal() {
    batchApplyModalContent.classList.add('scale-95');
    batchApplyModal.classList.add('opacity-0', 'pointer-events-none');
}

btnBatchApply.addEventListener('click', () => {
    if (!activeId || !current_geom?.calibration_points) {
        showToast("Set a film area before applying it to other frames.", "info");
        return;
    }
    refreshBatchTargetCounts();
    batchApplyModal.classList.remove('opacity-0', 'pointer-events-none');
    setTimeout(() => batchApplyModalContent.classList.remove('scale-95'), 10);
});

btnCloseBatchApply.addEventListener('click', closeBatchApplyModal);
btnCancelBatchApply.addEventListener('click', closeBatchApplyModal);

btnConfirmBatchApply.addEventListener('click', async () => {
    const source = findKnownItem(activeId);
    const mode = document.querySelector('input[name="batch-target"]:checked')?.value || 'roll-unprocessed';
    const targets = batchTargetItems(mode);
    if (!source?.file_path || targets.length === 0) {
        showToast("No eligible target frames.", "info");
        return;
    }

    const previousLabel = btnConfirmBatchApply.textContent;
    btnConfirmBatchApply.disabled = true;
    btnConfirmBatchApply.textContent = 'Applying...';
    try {
        // Batch Apply is an explicit commit point: persist the current draft
        // before the backend transaction reads its geometry as the source.
        await persistGeometryQueued(activeId, current_geom);
        const result = await invoke('batch_copy_settings', {
            source: {
                roll_id: source.roll_id || 'LOOSE_DEFAULT',
                file_path: source.file_path
            },
            targets: targets.map(item => ({
                roll_id: item.roll_id || 'LOOSE_DEFAULT',
                file_path: item.file_path
            })),
            modules: ['geometry']
        });
        closeBatchApplyModal();
        showToast(`Film area applied to ${result.updated} frame(s).`, "success");
    } catch (error) {
        console.error('Batch Apply failed', error);
        showToast("Batch Apply failed: " + error, "error");
    } finally {
        btnConfirmBatchApply.disabled = false;
        btnConfirmBatchApply.textContent = previousLabel;
    }
});

listen('settings_updated', (event) => {
    const targets = event.payload?.targets || [];
    for (const target of targets) {
        const identity = `${target.roll_id || 'LOOSE_DEFAULT'}::${normalizePath(target.file_path)}`;
        const item = Array.from(itemIndex.values()).find(candidate => itemIdentity(candidate) === identity);
        if (!item) continue;
        imageStates.delete(item.id);
        document.querySelectorAll(`.film-item[data-id="${CSS.escape(item.id)}"]`).forEach(element => {
            const elementRect = element.getBoundingClientRect();
            const stripRect = filmstripContainer.getBoundingClientRect();
            const visible = elementRect.right > stripRect.left && elementRect.left < stripRect.right;
            if (visible) element.classList.add('settings-updated');
        });
    }
});

const btnCopySettings = document.getElementById('btn-copy-settings');
const btnPasteSettings = document.getElementById('btn-paste-settings');
const btnWbEyedropper = document.getElementById('btn-wb-eyedropper');

function closeCopySettingsModal() {
    copySettingsContent.classList.add('scale-95');
    copySettingsModal.classList.add('opacity-0', 'pointer-events-none');
    copySettingsModal.setAttribute('aria-hidden', 'true');
    btnCopySettings.focus();
}

function openCopySettingsModal() {
    copySettingsModal.classList.remove('opacity-0', 'pointer-events-none');
    copySettingsModal.setAttribute('aria-hidden', 'false');
    setTimeout(() => {
        copySettingsContent.classList.remove('scale-95');
        btnConfirmCopySettings.focus();
    }, 10);
}

btnCopySettings.addEventListener('click', () => {
    if (!activeId) return;
    openCopySettingsModal();
});

btnCloseCopySettings.addEventListener('click', closeCopySettingsModal);
btnCancelCopySettings.addEventListener('click', closeCopySettingsModal);
copySettingsModal.addEventListener('click', event => {
    if (event.target === copySettingsModal) closeCopySettingsModal();
});
copySettingsModal.addEventListener('keydown', event => {
    if (event.key === 'Escape') closeCopySettingsModal();
});

btnConfirmCopySettings.addEventListener('click', () => {
    if (!activeId) return;
    const modules = Array.from(document.querySelectorAll('.copy-settings-module:checked'))
        .map(input => input.value);
    if (modules.length === 0) {
        showToast("Select at least one settings group.", "error");
        return;
    }
    const params = saveCurrentState();
    if (!params) return;
    copiedSettings = createCopyPayload(params, current_geom, modules);
    btnPasteSettings.disabled = false;
    closeCopySettingsModal();
    showToast(`${modules.length} settings group(s) copied.`, "success");
});

btnPasteSettings.addEventListener('click', async () => {
    if (!activeId || !copiedSettings) return;
    const targetId = activeId;
    const previousParams = cloneSettingsValue(saveCurrentState());
    const previousGeom = cloneSettingsValue(current_geom);
    const { params: nextParams, geom: nextGeom } = mergeCopyPayload(
        previousParams,
        previousGeom,
        copiedSettings
    );
    const modules = new Set(copiedSettings.modules);

    const geometryChanged = modules.has('crop') || modules.has('transform');
    const workingColorspaceChanged = previousParams.working_colorspace !== nextParams.working_colorspace;
    pushUndoState();
    try {
        current_geom = nextGeom;
        updateUIFromParams(nextParams, nextGeom);
        if (modules.has('edit')) await restoreLutForImage(nextParams);
        await updateBackendParams(targetId, nextParams);
        if (geometryChanged) await persistGeometryQueued(targetId, nextGeom);

        if (workingColorspaceChanged && hasProcessedActiveImage) {
            proxyCache.delete(targetId);
            readyProxyIds.delete(targetId);
            activeProxyIsFull = false;
            await reloadDevelopProxy(nextGeom, { showLoading: true });
        } else {
            updateCanvasTransform();
            requestRender();
        }
        btnBatchApply.disabled = !current_geom?.calibration_points;
        if (isCropMode) updateCropOverlay();
        requestThumbnailSync();
        showToast("Settings pasted.", "success");
    } catch (error) {
        current_geom = previousGeom;
        updateUIFromParams(previousParams, previousGeom);
        await restoreLutForImage(previousParams);
        try {
            await updateBackendParams(targetId, previousParams);
            if (geometryChanged) await persistGeometryQueued(targetId, previousGeom);
        } catch (rollbackError) {
            console.error("Failed to roll back pasted settings", rollbackError);
        }
        updateCanvasTransform();
        requestRender();
        showToast(`Could not paste settings: ${error}`, "error");
    }
});

btnWbEyedropper.addEventListener('click', () => {
    isEyedropperActive = !isEyedropperActive;
    if (isEyedropperActive) {
        btnWbEyedropper.classList.add('text-white');
        previewCanvas.style.cursor = 'crosshair';
        showToast("White Balance Eyedropper activated. Click on a neutral gray area.", "success");
    } else {
        btnWbEyedropper.classList.remove('text-white');
        previewCanvas.style.cursor = 'default';
    }
});

previewCanvas.addEventListener('click', (e) => {
    if (!isEyedropperActive || !proxyPixels || !activeId) return;
    
    const rect = previewCanvas.getBoundingClientRect();
    const displayU = (e.clientX - rect.left) / rect.width;
    const displayV = (e.clientY - rect.top) / rect.height;
    if (displayU < 0 || displayU > 1 || displayV < 0 || displayV > 1) return;

    const points = NexFilmGeometry.resolveCalibrationRenderPoints(
        current_geom.calibration_points,
        isCalibrationMode
    );
    const sourceUv = NexFilmGeometry.mapDisplayPointToSource(
        [displayU, displayV],
        current_geom.crop_rect,
        getHomography(points),
        proxyWidth,
        proxyHeight,
        current_geom
    );
    if (!sourceUv || sourceUv.some(value => !Number.isFinite(value) || value < 0 || value > 1)) return;
    const px = Math.min(proxyWidth - 1, Math.floor(sourceUv[0] * proxyWidth));
    const py = Math.min(proxyHeight - 1, Math.floor(sourceUv[1] * proxyHeight));
    
    let sumR = 0, sumG = 0, sumB = 0;
    let count = 0;
    const radius = 2;
    for (let dy = -radius; dy <= radius; dy++) {
        for (let dx = -radius; dx <= radius; dx++) {
            const nx = px + dx;
            const ny = py + dy;
            if (nx >= 0 && nx < proxyWidth && ny >= 0 && ny < proxyHeight) {
                const idx = (ny * proxyWidth + nx) * 4;
                sumR += proxyPixels[idx];
                sumG += proxyPixels[idx + 1];
                sumB += proxyPixels[idx + 2];
                count++;
            }
        }
    }
    
    if (count > 0) {
        const avgR = sumR / count;
        const avgG = sumG / count;
        const avgB = sumB / count;
        
        const epsilon = 1e-6;
        const tR = Math.max(avgR / 65535.0, epsilon);
        const tG = Math.max(avgG / 65535.0, epsilon);
        const tB = Math.max(avgB / 65535.0, epsilon);
        
        const rawDensity = [
            -Math.log10(tR) - currentBaseDensity[0],
            -Math.log10(tG) - currentBaseDensity[1],
            -Math.log10(tB) - currentBaseDensity[2],
        ];
        const currentExpG = parseFloat(sliders.expg.el.value) * CHANNEL_CONTROL_SCALE;
        const [targetExpR, , targetExpB] = getNeutralExposureOffsets(rawDensity, currentExpG);
        const sliderExpR = Math.max(-1, Math.min(1, targetExpR / CHANNEL_CONTROL_SCALE));
        const sliderExpB = Math.max(-1, Math.min(1, targetExpB / CHANNEL_CONTROL_SCALE));
        
        pushUndoState();
        
        sliders.expr.el.value = sliderExpR;
        sliders.expb.el.value = sliderExpB;
        
        sliders.expr.val.textContent = sliderExpR.toFixed(3);
        sliders.expb.val.textContent = sliderExpB.toFixed(3);
        
        updateSliderTrack(sliders.expr.el);
        updateSliderTrack(sliders.expb.el);
        
        updateBackendParams();
        requestRender();
        
        isEyedropperActive = false;
        btnWbEyedropper.classList.remove('text-white');
        previewCanvas.style.cursor = 'default';
        showToast("White Balance updated.", "success");
    }
});

// Contact Sheet Generator Logic
document.getElementById('btn-export-contact-sheet').addEventListener('click', async () => {
    if (!historyRollViewId) return;
    const btnExportContactSheet = document.getElementById('btn-export-contact-sheet');
    
    try {
        btnExportContactSheet.textContent = "Generating...";
        btnExportContactSheet.disabled = true;

        const currentRoll = allRolls.find(r => r.roll_id === historyRollViewId);
        const rollItems = await invoke('get_roll_filmstrip', { rollId: historyRollViewId });

        if (rollItems.length === 0) throw new Error("No images in roll");

        // 1. Determine the format-specific row density and image aspect ratio.
        const canvasW = 3000;
        const outerMargin = 100;
        const layout = getContactSheetLayout(currentRoll.format, canvasW, outerMargin);
        const is120 = layout.is120;
        const frames_per_row = layout.framesPerRow;

        // 2. Pad with empty frames
        const totalRows = Math.ceil(rollItems.length / frames_per_row);
        const emptyFrames = (totalRows * frames_per_row) - rollItems.length;
        const totalImages = rollItems.length; // store original
        
        for (let i = 0; i < emptyFrames; i++) {
            rollItems.push({ isEmpty: true });
        }

        const rows = totalRows;
        
        // 3. Math for grid layout
        const hGap = layout.horizontalGap;
        const colWidth = layout.imageWidth;
        const colHeight = layout.imageHeight;
        
        const borderH = colHeight * 0.18; // 18% for top and bottom borders
        const rowHeightTotal = colHeight + borderH * 2;
        const vGap = is120 ? (rowHeightTotal * 0.15) : (rowHeightTotal * 0.08);
        const footerHeight = 250;
        
        const canvasH = outerMargin + rows * rowHeightTotal + (rows > 1 ? (rows - 1) * vGap : 0) + footerHeight;

        const canvas = document.createElement('canvas');
        canvas.width = canvasW;
        canvas.height = canvasH;
        const ctx = canvas.getContext('2d');

        // 4. Background (Dark Paper Color)
        ctx.fillStyle = '#121214';
        ctx.fillRect(0, 0, canvasW, canvasH);
        
        const filmName = (currentRoll.film_stock || 'UNKNOWN FILM').toUpperCase();

        for (let r = 0; r < rows; r++) {
            ctx.fillStyle = '#000000';
            const rowY = outerMargin + r * (rowHeightTotal + vGap);
            const rowW = canvasW - outerMargin * 2;
            ctx.fillRect(outerMargin, rowY, rowW, rowHeightTotal);
        }

        // 5. Draw images
        for (let i = 0; i < rollItems.length; i++) {
            const item = rollItems[i];
            const r = Math.floor(i / frames_per_row);
            const c = i % frames_per_row;
            
            const x = outerMargin + c * (colWidth + hGap);
            const y = outerMargin + r * (rowHeightTotal + vGap);
            
            if (item.isEmpty) {
                // Empty frame body
                ctx.fillStyle = '#000000';
                ctx.fillRect(x, y + borderH, colWidth, colHeight);
            } else if (!item.rendered_thumbnail_base64) {
                // Archive sheets must never silently use the orange import thumbnail.
                ctx.fillStyle = '#1C1C1E';
                ctx.fillRect(x, y + borderH, colWidth, colHeight);
                ctx.fillStyle = '#8E8E93';
                ctx.font = '600 18px Inter, Helvetica, sans-serif';
                ctx.textAlign = 'center';
                ctx.textBaseline = 'middle';
                ctx.fillText('NOT DEVELOPED', x + colWidth / 2, y + borderH + colHeight / 2);
            } else {
                // Load & Draw real image
                const img = new Image();
                await new Promise(res => {
                    img.onload = res;
                    img.onerror = res;
                    img.src = item.rendered_thumbnail_base64.startsWith('data:')
                        ? item.rendered_thumbnail_base64
                        : `data:image/jpeg;base64,${item.rendered_thumbnail_base64}`;
                });
                
                ctx.save();
                ctx.beginPath();
                ctx.rect(x, y + borderH, colWidth, colHeight);
                ctx.clip();
                
                let isVertical = img.height > img.width;
                if (isVertical) {
                    ctx.translate(x + colWidth/2, y + borderH + colHeight/2);
                    ctx.rotate(Math.PI / 2);
                    ctx.drawImage(img, -colHeight/2, -colWidth/2, colHeight, colWidth);
                } else {
                    ctx.drawImage(img, x, y + borderH, colWidth, colHeight);
                }
                ctx.restore();
            }
            
            // 6. Draw Procedural Edge Codes & Sprockets
            ctx.fillStyle = '#D97736'; // Orange brand color
            ctx.font = '900 16px "Helvetica Neue Extended", "Helvetica Neue", Arial, sans-serif';
            ctx.textBaseline = 'middle';
            
            if (!is120) {
                // --- 135 Procedural ---
                const numHoles = 8;
                const holeW = colWidth * 0.05;
                const holeH = borderH * 0.45;
                const holeSpacing = colWidth / numHoles;
                const holeYTop = y + borderH - holeH - borderH * 0.1;
                const holeYBottom = y + borderH + colHeight + borderH * 0.1;
                
                ctx.fillStyle = '#FFFFFF';
                for (let h = 0; h < numHoles; h++) {
                    const hx = x + h * holeSpacing + (holeSpacing - holeW)/2;
                    // top hole
                    ctx.beginPath();
                    ctx.roundRect(hx, holeYTop, holeW, holeH, holeW * 0.2);
                    ctx.fill();
                    // bottom hole
                    ctx.beginPath();
                    ctx.roundRect(hx, holeYBottom, holeW, holeH, holeW * 0.2);
                    ctx.fill();
                }
                
                // Orange Texts
                ctx.fillStyle = '#D97736';
                ctx.textAlign = 'center';
                // Top text (film name)
                if (c === 1 || c === 4) { ctx.fillText("NEXFILM", x + colWidth/2, y + borderH * 0.25); }
                
                // Bottom text (frame num)
                ctx.fillText(`${i+1}`, x + colWidth*0.25, y + borderH + colHeight + borderH * 0.75);
                ctx.fillText(`${i+1}A`, x + colWidth*0.75, y + borderH + colHeight + borderH * 0.75);
                
            } else {
                // --- 120 Procedural ---
                ctx.textAlign = 'center';
                // Top text
                ctx.fillText("NEXFILM", x + colWidth/2, y + borderH * 0.35);
                // Bottom text (bold with arrow)
                ctx.font = '900 24px "Helvetica Neue Extended", "Helvetica Neue", Arial, sans-serif';
                ctx.fillText(`\u25C4 ${i+1}`, x + colWidth/2, y + borderH + colHeight + borderH * 0.65);
            }
        }

        // 7. High-Res Footer Typography
        const footerY = canvasH - 80;
        ctx.fillStyle = '#FFFFFF';
        ctx.font = 'bold 36px "Helvetica Neue Extended", "Helvetica Neue", Inter, sans-serif';
        ctx.textAlign = 'left';
        ctx.fillText('NEXFILM ENGINE', outerMargin, footerY);
        
        ctx.textAlign = 'right';
        ctx.font = 'bold 36px "Helvetica Neue Extended", "Helvetica Neue", Inter, sans-serif';
        ctx.fillText(filmName, canvasW - outerMargin, footerY - 40);
        
        ctx.fillStyle = '#888888';
        ctx.font = '24px Inter, Helvetica, sans-serif';
        ctx.fillText(`${currentRoll.date || 'Unknown Date'} | ${currentRoll.camera || 'Unknown Camera'} | ${totalImages} images (${emptyFrames} empty)`, canvasW - outerMargin, footerY + 10);

        // 8. Convert to high quality JPEG
        const dataUrl = canvas.toDataURL('image/jpeg', 0.95);
        const filename = createContactSheetFilename(currentRoll);
        const savedPath = await invoke('save_contact_sheet', { dataUrl, filename });
        showToast("Contact sheet saved: " + savedPath, "success");

    } catch (e) {
        console.error(e);
        showToast("Failed to generate contact sheet: " + e, "error");
    } finally {
        btnExportContactSheet.textContent = "Export Contact Sheet";
        btnExportContactSheet.disabled = false;
    }
});

function updateCalibrationPolygon() {
    if (!isCalibrationMode) return;
    const polygon = document.getElementById('calibration-polygon');
    const grid = document.getElementById('calibration-grid');
    const handles = document.querySelectorAll('.calib-handle');
    const dots = document.querySelectorAll('.calib-dot');
    const edgeHandles = document.querySelectorAll('.calib-edge-handle');
    const edgeDots = document.querySelectorAll('.calib-edge-dot');
    const svgRect = document.getElementById('calibration-svg').getBoundingClientRect();
    if (!svgRect.width || !svgRect.height) {
        requestAnimationFrame(updateCalibrationPolygon);
        return;
    }
    
    let pointsStr = '';
    const pts = [];
    calibrationPoints.forEach((p, i) => {
        const cx = p[0] * svgRect.width;
        const cy = p[1] * svgRect.height;
        pts.push({x: cx, y: cy});
        pointsStr += `${cx},${cy} `;
        if (handles[i]) {
            handles[i].setAttribute('cx', cx);
            handles[i].setAttribute('cy', cy);
        }
        if (dots[i]) {
            dots[i].setAttribute('cx', cx);
            dots[i].setAttribute('cy', cy);
        }
    });
    polygon.setAttribute('points', pointsStr.trim());

    if (pts.length === 4) {
        const lerp = (p1, p2, t) => ({x: p1.x + (p2.x - p1.x) * t, y: p1.y + (p2.y - p1.y) * t});

        calibrationEdgeIndices.forEach(([startIndex, endIndex], edgeIndex) => {
            const start = pts[startIndex];
            const end = pts[endIndex];
            const edgeHandle = edgeHandles[edgeIndex];
            const edgeDot = edgeDots[edgeIndex];
            if (edgeHandle) {
                edgeHandle.setAttribute('x1', start.x);
                edgeHandle.setAttribute('y1', start.y);
                edgeHandle.setAttribute('x2', end.x);
                edgeHandle.setAttribute('y2', end.y);
            }
            if (edgeDot) {
                edgeDot.setAttribute('cx', (start.x + end.x) / 2);
                edgeDot.setAttribute('cy', (start.y + end.y) / 2);
            }
        });
        
        const top1 = lerp(pts[0], pts[1], 1/3);
        const top2 = lerp(pts[0], pts[1], 2/3);
        const bot1 = lerp(pts[3], pts[2], 1/3);
        const bot2 = lerp(pts[3], pts[2], 2/3);
        
        const left1 = lerp(pts[0], pts[3], 1/3);
        const left2 = lerp(pts[0], pts[3], 2/3);
        const right1 = lerp(pts[1], pts[2], 1/3);
        const right2 = lerp(pts[1], pts[2], 2/3);
        
        grid.setAttribute('d', `
            M ${top1.x} ${top1.y} L ${bot1.x} ${bot1.y}
            M ${top2.x} ${top2.y} L ${bot2.x} ${bot2.y}
            M ${left1.x} ${left1.y} L ${right1.x} ${right1.y}
            M ${left2.x} ${left2.y} L ${right2.x} ${right2.y}
        `);
    }
}

function cloneCalibrationPoints(points = calibrationPoints) {
    return points.map(point => [Number(point[0]), Number(point[1])]);
}

function setCalibrationDraft(points) {
    if (!NexFilmGeometry.isValidCalibrationQuad(points)) return false;
    calibrationPoints = cloneCalibrationPoints(points);
    btnBatchApply.disabled = true;

    if (!calibrationRAF) {
        calibrationRAF = requestAnimationFrame(() => {
            updateCalibrationPolygon();
            calibrationRAF = null;
        });
    }
    return true;
}

document.getElementById('calibration-svg').addEventListener('pointerdown', (e) => {
    if (!isCalibrationMode) return;
    const target = e.target;
    if (!target.classList.contains('calib-handle') && !target.classList.contains('calib-edge-handle')) {
        return;
    }

    calibrationRevision++;
    e.preventDefault();
    e.stopPropagation();
    if (target.setPointerCapture) target.setPointerCapture(e.pointerId);
    calibrationDragState = {
        type: target.classList.contains('calib-handle') ? 'corner' : 'edge',
        index: target.classList.contains('calib-handle')
            ? Number(target.dataset.idx)
            : Number(target.dataset.edge),
        pointerId: e.pointerId,
        target,
        startClient: [e.clientX, e.clientY],
        startPoints: cloneCalibrationPoints()
    };
});

let calibrationRAF = null;
window.addEventListener('pointermove', (e) => {
    const drag = calibrationDragState;
    if (!isCalibrationMode || !drag || drag.pointerId !== e.pointerId) return;
    e.preventDefault();

    const svgRect = document.getElementById('calibration-svg').getBoundingClientRect();
    if (!svgRect.width || !svgRect.height) return;
    const candidate = cloneCalibrationPoints(drag.startPoints);

    if (drag.type === 'corner') {
        candidate[drag.index] = [
            Math.max(0, Math.min(1, (e.clientX - svgRect.left) / svgRect.width)),
            Math.max(0, Math.min(1, (e.clientY - svgRect.top) / svgRect.height))
        ];
        setCalibrationDraft(candidate);
        return;
    }

    const pointerDeltaX = e.clientX - drag.startClient[0];
    const pointerDeltaY = e.clientY - drag.startClient[1];
    const translated = NexFilmGeometry.translateCalibrationEdge(
        drag.startPoints,
        drag.index,
        [pointerDeltaX, pointerDeltaY],
        [svgRect.width, svgRect.height]
    );
    if (translated) setCalibrationDraft(translated);
});

function finishCalibrationDrag(e, cancelled) {
    const drag = calibrationDragState;
    if (!drag || drag.pointerId !== e.pointerId) return;
    if (cancelled) setCalibrationDraft(drag.startPoints);
    if (drag.target.hasPointerCapture?.(e.pointerId)) {
        drag.target.releasePointerCapture(e.pointerId);
    }
    calibrationDragState = null;
}

window.addEventListener('pointerup', e => finishCalibrationDrag(e, false));
window.addEventListener('pointercancel', e => finishCalibrationDrag(e, true));

window.addEventListener('resize', () => {
    if (isCalibrationMode) updateCalibrationPolygon();
});

document.getElementById('btn-confirm-calibration').addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    const previousGeom = JSON.parse(JSON.stringify(current_geom));
    current_geom.calibration_points = JSON.parse(JSON.stringify(calibrationPoints));
    current_geom.calibration_confirmed = true;
    saveCurrentState();
    try {
        await persistGeometryQueued(activeId, current_geom);
        isCalibrationMode = false;
        document.getElementById('calibration-overlay').classList.add('hidden');
        setDevelopInspectorCalibrationLocked(false);
        btnAutoColor.disabled = false;
        btnBatchApply.disabled = false;
        if (activeProxyIsFull) requestRender();
        showToast("Film area saved. Run Auto Invert when ready.", "success");
    } catch (e) {
        current_geom = previousGeom;
        saveCurrentState();
        console.error("Calibration failed", e);
        showToast("Failed to save film area.", "error");
    }
});

document.getElementById('btn-locate-file').addEventListener('click', async () => {
    if (!missingFileId) return;
    try {
        const newPath = await invoke('locate_missing_file', { id: missingFileId });
        if (newPath) {
            document.getElementById('missing-file-ui').classList.add('hidden');
            document.getElementById('missing-file-ui').classList.remove('flex');
            document.getElementById('preview-canvas').style.display = 'block';
            let id = missingFileId;
            const relocatedItem = findKnownItem(id);
            if (relocatedItem) {
                relocatedItem.file_path = newPath;
                relocatedItem.file_missing = false;
                upsertLibraryItem(relocatedItem);
            }
            missingFileId = null;
            activeId = null; // force re-select
            await selectImage(id);
        }
    } catch(e) {
        if (e !== "Cancelled") showToast("Failed to locate file", "error");
    }
});

// Initialize on load
window.addEventListener('DOMContentLoaded', async () => {
    const loadingOverlay = document.createElement('div');
    loadingOverlay.id = 'startup-loading';
    loadingOverlay.className = 'absolute inset-0 bg-[#121214] z-50 flex flex-col items-center justify-center text-zinc-400 gap-4';
    loadingOverlay.innerHTML = '<svg class="animate-spin h-10 w-10 text-orange-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path></svg><div class="tracking-widest text-sm uppercase font-bold text-zinc-300">Loading Library...</div>';
    document.body.appendChild(loadingOverlay);

    try {
        allRolls = await invoke('get_rolls');
        allLibraryItems = await invoke('get_filmstrip');
        rememberItems(allLibraryItems);
        await updateFilterSidebar();
    } catch(e) { console.error("Init Error", e); }

    await renderLibraryAndFilmstrip();
    if (loadingOverlay) loadingOverlay.remove();
});

let precacheToast = null;

listen('import_progress', (event) => {
    const payload = event.payload;

    // Handle initial "start" phase event from the backend
    if (payload.phase === 'start') {
        if (payload.total > 0) {
            totalImportCount = payload.total;
            currentImportCount = 0;
            if (!precacheToast) {
                initImportToast(payload.total);
            }
            const bar = document.getElementById('precache-bar');
            const txt = document.getElementById('precache-text');
            if (bar) bar.style.width = '0%';
            if (txt) txt.textContent = `0 / ${payload.total}`;
        }
        return;
    }

    // Find skeleton by matching file_path, or find real item by id
    const payloadIdentity = itemIdentity(payload);
    let item = allLibraryItems.find(i => i.status === 'importing' && itemIdentity(i) === payloadIdentity);
    let searchId = payload.id;
    
    if (item) {
        searchId = item.id; // Keep track of the old skeleton id to update the DOM
        if (searchId !== payload.id) {
            itemIndex.delete(searchId);
        }
        item.id = payload.id;
        item.roll_id = payload.roll_id;
        item.thumbnail_base64 = payload.thumbnail_base64;
        item.embedded_thumbnail_base64 = payload.embedded_thumbnail_base64 || payload.thumbnail_base64;
        item.rendered_thumbnail_base64 = payload.rendered_thumbnail_base64 || null;
        item.thumbnail_kind = payload.thumbnail_kind || (item.rendered_thumbnail_base64 ? 'rendered' : 'embedded');
        item.file_path = payload.file_path;
        item.status = 'done';
        if (payload.roll_id && payload.roll_id === currentRollViewId && isRollEditing) {
            item.in_working_set = true;
        }
    } else {
        item = allLibraryItems.find(i => i.id === payload.id);
        if (item) {
            item.thumbnail_base64 = payload.thumbnail_base64;
            item.embedded_thumbnail_base64 = payload.embedded_thumbnail_base64 || payload.thumbnail_base64;
            item.rendered_thumbnail_base64 = payload.rendered_thumbnail_base64 || null;
            item.thumbnail_kind = payload.thumbnail_kind || (item.rendered_thumbnail_base64 ? 'rendered' : 'embedded');
            item.roll_id = payload.roll_id;
            item.file_path = payload.file_path;
            if (payload.roll_id && payload.roll_id === currentRollViewId && isRollEditing) {
                item.in_working_set = true;
            }
        } else {
            if (payload.roll_id && payload.roll_id === currentRollViewId && isRollEditing) {
                payload.in_working_set = true;
            }
            allLibraryItems.push(payload);
            item = payload;
        }
    }
    rememberItem(item);
    allLibraryItems = uniqueItemsByPath(allLibraryItems);

    // Update DOM matching the skeleton id (or real id)
    document.querySelectorAll(`.film-item[data-id="${searchId}"], .library-item[data-id="${searchId}"]`).forEach(el => {
        el.dataset.id = payload.id; // Correct the DOM id to real UUID
        el.classList.remove('importing');
        
        const skeleton = el.querySelector('.animate-pulse');
        if (skeleton) skeleton.remove();
        
        let img = el.querySelector('img');
        if (!img) {
            img = document.createElement('img');
            el.appendChild(img);
        }
        
        img.dataset.imgId = payload.id; // Correct img data-id
        setImageElementThumbnail(img, payload.thumbnail_base64);
        
        // Ensure onclick uses the real ID now
        if (el.classList.contains('film-item')) {
            el.onclick = () => {
                selectImage(payload.id);
                selectedLibraryIds.clear();
                selectedLibraryIds.add(payload.id);
                updateLibrarySelectionUI();
            };
        }
    });
    
    if (Number.isFinite(payload.total) && payload.total > 0) {
        totalImportCount = payload.total;
    }
    if (Number.isFinite(payload.processed)) {
        currentImportCount = Math.max(currentImportCount, payload.processed);
        const bar = document.getElementById('precache-bar');
        const txt = document.getElementById('precache-text');
        const pct = totalImportCount > 0 ? (currentImportCount / totalImportCount) * 100 : 0;
        if (bar) bar.style.width = `${pct}%`;
        if (txt) txt.textContent = `${currentImportCount} / ${totalImportCount}`;
    }
});

listen('import_error', (event) => {
    const payload = event.payload || {};
    const failedPaths = new Set((payload.file_paths || []).map(normalizePath));
    const failedIds = new Set();
    allLibraryItems = allLibraryItems.filter(item => {
        const sameRoll = !payload.roll_id || item.roll_id === payload.roll_id;
        const failed = item.status === 'importing'
            && sameRoll
            && failedPaths.has(normalizePath(item.file_path));
        if (failed && item.id) failedIds.add(item.id);
        return !failed;
    });
    failedIds.forEach(id => itemIndex.delete(id));

    importFailedCount += failedPaths.size;
    if (Number.isFinite(payload.processed)) {
        currentImportCount = Math.max(currentImportCount, payload.processed);
    }
    if (Number.isFinite(payload.total) && payload.total > 0) {
        totalImportCount = payload.total;
    }
    const bar = document.getElementById('precache-bar');
    const txt = document.getElementById('precache-text');
    const pct = totalImportCount > 0 ? (currentImportCount / totalImportCount) * 100 : 0;
    if (bar) bar.style.width = `${pct}%`;
    if (txt) txt.textContent = `${currentImportCount} / ${totalImportCount} (${importFailedCount} failed)`;
    showToast(`Import failed: ${payload.message || 'database persistence failed'}`, 'error');
    renderLibraryAndFilmstrip(true);
});

listen('import_complete', async (event) => {
    const failedCount = Number(event.payload?.failed) || importFailedCount;
    try {
        await fetchRolls();
        if (currentRollViewId && !allRolls.some(roll => roll.roll_id === currentRollViewId)) {
            currentRollViewId = null;
            isRollEditing = false;
            currentImportSessionPaths = null;
        }
    } catch (e) {
        console.error(e);
    }
    if (precacheToast) {
        const bar = document.getElementById('precache-bar');
        const txt = document.getElementById('precache-text');
        if (bar) bar.style.width = '100%';
        if (txt) {
            txt.textContent = failedCount > 0
                ? `${Math.max(0, totalImportCount - failedCount)} imported, ${failedCount} failed`
                : `${totalImportCount} / ${totalImportCount}`;
        }
        setTimeout(() => {
            if (precacheToast) {
                precacheToast.remove();
                precacheToast = null;
            }
            restoreImportButtons();
        }, 800);
    } else {
        restoreImportButtons();
    }
    if (currentView !== 'develop') {
        renderLibraryAndFilmstrip(false);
    }
});

listen('tauri://file-drop-hover', (event) => {
    document.getElementById('drag-overlay').classList.remove('hidden');
});
listen('tauri://drag-enter', (event) => {
    document.getElementById('drag-overlay').classList.remove('hidden');
});

listen('tauri://file-drop-cancelled', (event) => {
    document.getElementById('drag-overlay').classList.add('hidden');
});
listen('tauri://drag-leave', (event) => {
    document.getElementById('drag-overlay').classList.add('hidden');
});

function restoreImportButtons() {
    importInProgress = false;
    setImportManagementBusy(false);
    btnImport.textContent = "Import Roll";
    btnImport.disabled = false;
    btnImportTriggers.forEach(btn => {
        btn.textContent = btn.dataset.originalText || "Import Roll";
        btn.disabled = false;
    });
}

function initImportToast(count) {
    importInProgress = true;
    setImportManagementBusy(true);
    totalImportCount = count;
    currentImportCount = 0;
    importFailedCount = 0;
    if (!precacheToast) {
        precacheToast = document.createElement('div');
        precacheToast.className = 'fixed bottom-4 right-4 bg-[#1C1C1E] border border-[#28282c] shadow-lg rounded p-4 z-50 flex flex-col gap-2 w-64';
        precacheToast.innerHTML = `
            <div class="text-[11px] font-bold tracking-wider text-zinc-300">Importing Frames</div>
            <div class="w-full h-1 bg-zinc-800 rounded overflow-hidden">
                <div class="h-full bg-blue-500 transition-all duration-300" id="precache-bar" style="width: 0%"></div>
            </div>
            <div class="text-[10px] text-zinc-500" id="precache-text">Scanning files...</div>
        `;
        document.body.appendChild(precacheToast);
    }
}

const handleDrop = async (event) => {
    document.getElementById('drag-overlay').classList.add('hidden');
    const droppedPaths = uniqueImportPaths(event.payload.paths || event.payload)
        .filter(isSupportedImportPath);
    const paths = droppedPaths;
    if (paths.length > 0) {
        let transientIds = [];
        btnImport.textContent = 'Importing...';
        btnImport.disabled = true;
        initImportToast(paths.length);
        try {
            const roll = createLooseImportRoll(paths);
            await beginWorkingImport(roll.roll_id, paths);

            const tempItems = paths.map((p, idx) => ({
                id: 'temp_import_' + Date.now() + '_' + idx,
                roll_id: roll.roll_id,
                file_path: p,
                thumbnail_base64: '',
                status: 'importing'
            }));
            transientIds = tempItems.map(item => item.id);
            rememberItems(tempItems);
            allLibraryItems = tempItems;
            await renderLibraryAndFilmstrip(true);

            await invoke('import_roll', { roll, paths });
            showToast(`Queued ${paths.length} file(s) for import`, 'success');
        } catch (error) {
            removeTransientItems(transientIds);
            resetWorkingLibrary();
            restoreImportButtons();
            await renderLibraryAndFilmstrip(true);
            showToast("Import failed: " + error, "error");
        }
    } else if (droppedPaths.length > 0) {
        showToast("The dropped files are already imported.", "error");
    } else {
        showToast("No supported image files were dropped.", "error");
    }
};

listen('tauri://file-drop', handleDrop);
listen('tauri://drag-drop', handleDrop);

document.getElementById('btn-export-roll').addEventListener('click', async () => {
    if (!currentRollViewId) { showToast('No active roll.', 'error'); return; }
    const currentRoll = allRolls.find(r => r.roll_id === currentRollViewId);
    if (!currentRoll) { showToast('The active roll no longer exists.', 'error'); return; }
    try {
        const rollItems = await invoke('get_roll_filmstrip', { rollId: currentRollViewId });
        const missingStateCount = rollItems.filter(item => item.state_available === false).length;
        const missingFileCount = rollItems.filter(item => item.file_missing).length;
        if (missingStateCount > 0) {
            showToast(`Cannot export this roll: ${missingStateCount} frame(s) have no persisted state.`, 'error');
            return;
        }
        if (missingFileCount > 0) {
            showToast(`Cannot export this roll: ${missingFileCount} source file(s) are missing.`, 'error');
            return;
        }
        if (rollItems.length === 0) {
            showToast('This roll has no images to export.', 'error');
            return;
        }
        selectedLibraryIds.clear();
        rollItems.forEach(item => selectedLibraryIds.add(item.id));
        updateLibrarySelectionUI();
        openExportModal();
    } catch (error) {
        showToast('Could not load the roll for export: ' + error, 'error');
    }
});

let panX = 0, panY = 0, isPanning = false, startPanX = 0, startPanY = 0, isSpacePressed = false;
window.addEventListener('keydown', e => { if(e.code==='Space') isSpacePressed=true; });
window.addEventListener('keyup', e => { if(e.code==='Space') isSpacePressed=false; });

canvasWrapper.parentElement.addEventListener('mousedown', e => {
    if (isSprocketPickerActive) {
        if (!gl || !activeId) return;
        const rect = previewCanvas.getBoundingClientRect();
        const displayU = (e.clientX - rect.left) / rect.width;
        const displayV = (e.clientY - rect.top) / rect.height;
        if (displayU >= 0 && displayU <= 1 && displayV >= 0 && displayV <= 1) {
            const tex_u = current_geom.crop_rect.x + displayU * current_geom.crop_rect.width;
            const tex_v = current_geom.crop_rect.y + displayV * current_geom.crop_rect.height;
            const perspectiveUv = NexFilmGeometry.mapPerspectivePoint([tex_u, tex_v], current_geom);
            if (!perspectiveUv) return;
            let pts = current_geom.calibration_points || [[0, 0], [1, 0], [1, 1], [0, 1]];
            let hMat = getHomography(pts);
            let w_homo = hMat[2]*perspectiveUv[0] + hMat[5]*perspectiveUv[1] + hMat[8];
            const raw_u = (hMat[0]*perspectiveUv[0] + hMat[3]*perspectiveUv[1] + hMat[6]) / w_homo;
            const raw_v = (hMat[1]*perspectiveUv[0] + hMat[4]*perspectiveUv[1] + hMat[7]) / w_homo;
            
            pushUndoState();
            currentSprocketUV = new Float32Array([raw_u, raw_v]);
            isSprocketPickerActive = false;
            canvasWrapper.parentElement.style.cursor = '';
            btnSprocketPicker.classList.remove('bg-zinc-600');
            requestRender();
        }
        return;
    }
    if (!activeId || isCropMode || isPerspectiveMode || isCalibrationMode) return;
    if (e.button === 0 || e.button === 1) {
        isPanning = true;
        startPanX = e.clientX - panX;
        startPanY = e.clientY - panY;
        e.preventDefault();
    }
});
window.addEventListener('mousemove', e => {
    if (!isPanning) return;
    panX = e.clientX - startPanX;
    panY = e.clientY - startPanY;
    updateCanvasTransform();
});
window.addEventListener('mouseup', () => isPanning = false);
canvasWrapper.parentElement.addEventListener('dblclick', e => {
    if (!activeId || isCropMode || isPerspectiveMode || isCalibrationMode) return;
    zoomLevel = 1.0; panX = 0; panY = 0;
    updateCanvasTransform();
});

let isChinese = false;
document.getElementById('menu-lang-toggle').addEventListener('click', () => {
    isChinese = !isChinese;
    document.getElementById('menu-lang-toggle').querySelector('span').textContent = isChinese ? 'Language: ZH-CN' : 'Language: EN';
    if(isChinese) {
        navLibrary.textContent = '图库';
        navDevelop.textContent = '冲洗';
        navHistory.textContent = '历史卷';
        navSponsor.textContent = '赞赏';
    } else {
        navLibrary.textContent = 'Library';
        navDevelop.textContent = 'Develop';
        navHistory.textContent = 'Rolls';
        navSponsor.textContent = 'Sponsor';
    }
});
let isDarkTheme = localStorage.getItem('nexfilm-theme') !== 'light';

function applyTheme(theme) {
    isDarkTheme = theme !== 'light';
    document.body.dataset.theme = isDarkTheme ? 'dark' : 'light';
    const label = document.getElementById('theme-value') || document.getElementById('menu-theme-toggle')?.querySelector('span');
    if (label) label.textContent = isDarkTheme ? 'Dark' : 'Light';
    localStorage.setItem('nexfilm-theme', isDarkTheme ? 'dark' : 'light');
}

applyTheme(isDarkTheme ? 'dark' : 'light');
document.getElementById('menu-theme-toggle').addEventListener('click', () => {
    applyTheme(isDarkTheme ? 'light' : 'dark');
});
