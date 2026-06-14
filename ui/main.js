const invoke = window.__TAURI__.core.invoke;

// DOM: Global
const btnImport = document.getElementById('btn-import');
const btnExportDialog = document.getElementById('btn-export-dialog');
const btnImportTriggers = document.querySelectorAll('.btn-import-trigger');
const toastContainer = document.getElementById('toast-container');

// DOM: Navigation & Views
const navHistory = document.getElementById('nav-history');
const navLibrary = document.getElementById('nav-library');
const navDevelop = document.getElementById('nav-develop');
const viewHistory = document.getElementById('view-history');
const viewLibrary = document.getElementById('view-library');
const viewDevelop = document.getElementById('view-develop');

// DOM: Library View
const libraryGrid = document.getElementById('library-grid');
const libraryEmpty = document.getElementById('library-empty');
const btnSelectAll = document.getElementById('btn-select-all');
const btnDeselectAll = document.getElementById('btn-deselect-all');
const librarySelectionCount = document.getElementById('library-selection-count');

// DOM: Develop View
const filmstripContainer = document.getElementById('filmstrip-container');
const canvasWrapper = document.getElementById('canvas-wrapper');
const previewCanvas = document.getElementById('preview-canvas');
const dummyPusher = document.getElementById('dummy-pusher');

// DOM: Visualization
const histCanvas = document.getElementById('histogram-canvas');
const waveCanvas = document.getElementById('waveform-canvas');
const btnToggleViz = document.getElementById('btn-toggle-viz');
const vizTitle = document.getElementById('viz-title');

const histCtx = histCanvas.getContext('2d');
const waveCtx = waveCanvas.getContext('2d');

// DOM: Export Modal
const exportModal = document.getElementById('export-modal');
const exportModalContent = document.getElementById('export-modal-content');
const btnCloseExport = document.getElementById('btn-close-export');
const btnCancelExport = document.getElementById('btn-cancel-export');
const btnConfirmExport = document.getElementById('btn-confirm-export');

const btnModeColor = document.getElementById('btn-mode-color');
const btnModeBw = document.getElementById('btn-mode-bw');

// DOM: Crop & Transform
const btnCropMode = document.getElementById('btn-crop-mode');
const btnRotateMode = document.getElementById('btn-rotate-mode');
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
const cropHandles = document.getElementById('crop-handles');
const rotateHandleOuter = document.getElementById('rotate-handle-outer');

let currentDMin = [0.1, 0.1, 0.1];
let currentDMax = [2.0, 2.0, 2.0];

const sliders = {
    masterDmin: { el: document.getElementById('master-dmin'), val: document.getElementById('val-master-dmin') },
    masterDmax: { el: document.getElementById('master-dmax'), val: document.getElementById('val-master-dmax') },
    exposure: { el: document.getElementById('exposure'), val: document.getElementById('val-exposure') },
    gamma: { el: document.getElementById('gamma'), val: document.getElementById('val-gamma') },
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

let lastHistPixels = null;
let current_geom = { crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0.0, flip_h: false, flip_v: false, rotate_90_count: 0 };
let isCropMode = false;
let isRotateMode = false;
let currentImageWidth = 1;
let currentImageHeight = 1;
let zoomLevel = 1.0;

let originalFilmOptions = null;
let missingFileId = null;

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
let selectedLibraryIds = new Set();
let lastSelectedLibraryId = null;

// Delete Mode State
let isDeleteMode = false;
let selectedRollIds = new Set();

// Calibration State
let isCalibrationMode = false;
let calibrationDragIdx = -1;
let calibrationPoints = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

function updateLibrarySelectionUI() {
    librarySelectionCount.textContent = `${selectedLibraryIds.size} selected`;
    if (selectedLibraryIds.size > 0) {
        btnExportDialog.disabled = false;
        btnExportDialog.textContent = 'EXPORT (' + selectedLibraryIds.size + ')';
        btnDeselectAll.classList.remove('hidden');
    } else {
        btnExportDialog.disabled = true;
        btnExportDialog.textContent = 'EXPORT';
        btnDeselectAll.classList.add('hidden');
    }
    
    // update visuals
    Array.from(libraryGrid.children).forEach(child => {
        const id = child.dataset.id;
        if (selectedLibraryIds.has(id)) {
            child.classList.add('selected');
        } else {
            child.classList.remove('selected');
        }
    });
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
function switchView(viewName) {
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
        if (viewName !== 'develop') { document.getElementById('btn-export-roll').classList.add('hidden'); }
    });

    if (viewName === 'develop') { document.getElementById('btn-export-roll').classList.remove('hidden');
        requestRender();
    }
}

if (navHistory) navHistory.addEventListener('click', () => switchView('history'));
navLibrary.addEventListener('click', () => switchView('library'));
navDevelop.addEventListener('click', () => switchView('develop'));

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
        exp_r: parseFloat(sliders.expr.el.value),
        exp_g: parseFloat(sliders.expg.el.value),
        exp_b: parseFloat(sliders.expb.el.value),
        highlights: parseFloat(sliders.highlights.el.value),
        shadows: parseFloat(sliders.shadows.el.value),
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
        const oldGeomAngle = current_geom.angle;
        current_geom = JSON.parse(JSON.stringify(prevState.geom));
        
        await invoke('update_geometry', { id: activeId, geom: current_geom });
        updateBackendParams();
        
        if (oldGeomAngle !== current_geom.angle || prevState.geom.flip_h !== current_geom.flip_h || prevState.geom.flip_v !== current_geom.flip_v || prevState.geom.rotate_90_count !== current_geom.rotate_90_count) {
            await loadProxyImage();
        } else {
            requestRender();
        }
        
        requestThumbnailSync();
        if (isCropMode) updateCropOverlay();
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
    if (navHistory.classList.contains('text-zinc-100') && !currentRollViewId) {
        isDeleteMode = !isDeleteMode;
        if (!isDeleteMode) selectedRollIds.clear();
        renderLibraryAndFilmstrip();
    }
});

document.getElementById('btn-confirm-delete').addEventListener('click', async () => {
    if (selectedRollIds.size === 0) return;
    const confirm = await showConfirm(`Are you sure you want to delete ${selectedRollIds.size} roll(s)?`);
    if (confirm) {
        await invoke('delete_rolls', { rollIds: Array.from(selectedRollIds) });
        selectedRollIds.clear();
        isDeleteMode = false;
        allRolls = await invoke('get_rolls');
        renderLibraryAndFilmstrip();
        showToast("Rolls deleted", "success");
    }
});

document.getElementById('btn-cancel-delete').addEventListener('click', () => {
    isDeleteMode = false;
    selectedRollIds.clear();
    renderLibraryAndFilmstrip();
});

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
function requestThumbnailSync() {
    if (thumbnailSyncTimeout) clearTimeout(thumbnailSyncTimeout);
    thumbnailSyncTimeout = setTimeout(async () => {
        if (!activeId) return;
        try {
            await invoke('sync_thumbnail_buffer', { id: activeId });
            allLibraryItems = await invoke('get_filmstrip');
            const item = allLibraryItems.find(i => i.id === activeId);
            if (item && item.thumbnail_base64 !== "FILE_MISSING") {
                document.querySelectorAll(`img[data-img-id="${activeId}"]`).forEach(img => {
                    img.src = `data:image/jpeg;base64,${item.thumbnail_base64}`;
                    // Reset to cover styling
                    img.classList.remove('opacity-50', 'object-contain', 'p-4', 'p-2', 'bg-[#1C1C1E]');
                    img.classList.add('object-cover');
                });
            }
        } catch(e) { console.error(e); }
    }, 250);
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
        exp_r: parseFloat(sliders.expr.el.value),
        exp_g: parseFloat(sliders.expg.el.value),
        exp_b: parseFloat(sliders.expb.el.value),
        highlights: parseFloat(sliders.highlights.el.value),
        shadows: parseFloat(sliders.shadows.el.value)
    };
    imageStates.set(activeId, { params, geom: JSON.parse(JSON.stringify(current_geom)) });
    return params;
}

function updateBackendParams() {
    const params = saveCurrentState();
    if (params && activeId) {
        const rollId = currentRollViewId || 'LOOSE_DEFAULT';
        invoke('update_tuning_parameters', { id: activeId, params, rollId }).catch(console.error);
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
let u_mode_loc;
let u_transform_loc;
let u_highlights_loc;
let u_shadows_loc;
let u_lut3d_loc;
let u_lut_opacity_loc;
let u_has_lut_loc;
let u_lut1d_loc;
let u_image_loc;
let u_image_aspect_loc;
let u_crop_loc;
let u_calib_pts_loc;
let u_border_exposure_loc;
let u_baseline_pass_loc;

let currentBaseDensity = [0, 0, 0];
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
    uniform mat4 u_transform;
    uniform float u_aspect;
    uniform vec4 u_crop;
    uniform float u_image_aspect;
    void main() {
        vec4 pos = a_position;
        
        pos.x *= u_aspect;
        pos = u_transform * pos;
        pos.x /= u_aspect;
        gl_Position = pos;
        
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
    uniform int u_mode;
    
    uniform float u_highlights;
    uniform float u_shadows;
    
    uniform mediump sampler3D u_lut3d;
    uniform mediump sampler2D u_lut1d;
    uniform float u_lut_opacity;
    uniform int u_has_lut;
    uniform int u_lut_is_1d;
    
    uniform mat3 u_homography;
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

    vec2 applyHomography(vec2 uv, mat3 h) {
        vec3 p = h * vec3(uv, 1.0);
        return p.xy / p.z;
    }

    void main() {
        vec2 warped_uv = applyHomography(v_texcoord, u_homography);
        if (warped_uv.x < 0.0 || warped_uv.x > 1.0 || warped_uv.y < 0.0 || warped_uv.y > 1.0) {
            outColor = vec4(0.0, 0.0, 0.0, 1.0);
            return;
        }

        float mask = 0.0;
        if (u_sprocket_uv.x >= 0.0) {
            vec3 raw_color = vec3(texture(u_image, warped_uv).rgb) / 65535.0;
            vec3 raw_target = vec3(texture(u_image, u_sprocket_uv).rgb) / 65535.0;
            float luma_diff = abs(getLuma(raw_color) - getLuma(raw_target));
            mask = pow(1.0 - smoothstep(u_sprocket_tolerance, u_sprocket_tolerance + u_sprocket_feather + 0.0001, luma_diff), 3.0);
        }

        uvec4 texel = texture(u_image, warped_uv);
        
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
        
        vec3 norm = (density - u_dmin) / (u_dmax - u_dmin);
        
        norm = norm + u_shadows * pow(1.0 - clamp(norm, 0.0, 1.0), vec3(2.0)) * norm + u_highlights * pow(clamp(norm, 0.0, 1.0), vec3(2.0)) * (1.0 - norm);
        
        vec3 final_rgb;
        if (u_has_lut == 1) {
            vec3 lut_in = clamp(norm, 0.0, 1.0);
            vec3 lut_color;
            if (u_lut_is_1d == 1) {
                lut_color.r = texture(u_lut1d, vec2(lut_in.r, 0.5)).r;
                lut_color.g = texture(u_lut1d, vec2(lut_in.g, 0.5)).g;
                lut_color.b = texture(u_lut1d, vec2(lut_in.b, 0.5)).b;
            } else {
                lut_color = texture(u_lut3d, lut_in).rgb;
            }
            final_rgb = mix(vec3(pow(clamp(norm.r, 0.0, 1.0), 1.0 / u_gamma), pow(clamp(norm.g, 0.0, 1.0), 1.0 / u_gamma), pow(clamp(norm.b, 0.0, 1.0), 1.0 / u_gamma)), lut_color, u_lut_opacity);
        } else {
            final_rgb = vec3(pow(clamp(norm.r, 0.0, 1.0), 1.0 / u_gamma), pow(clamp(norm.g, 0.0, 1.0), 1.0 / u_gamma), pow(clamp(norm.b, 0.0, 1.0), 1.0 / u_gamma));
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
    u_mode_loc = gl.getUniformLocation(shaderProgram, "u_mode");
    u_transform_loc = gl.getUniformLocation(shaderProgram, "u_transform");
    u_highlights_loc = gl.getUniformLocation(shaderProgram, "u_highlights");
    u_shadows_loc = gl.getUniformLocation(shaderProgram, "u_shadows");
    u_lut3d_loc = gl.getUniformLocation(shaderProgram, "u_lut3d");
    u_lut1d_loc = gl.getUniformLocation(shaderProgram, "u_lut1d");
    u_lut_opacity_loc = gl.getUniformLocation(shaderProgram, "u_lut_opacity");
    u_has_lut_loc = gl.getUniformLocation(shaderProgram, "u_has_lut");
    u_lut_is_1d_loc = gl.getUniformLocation(shaderProgram, "u_lut_is_1d");
    u_image_loc = gl.getUniformLocation(shaderProgram, "u_image");
    u_aspect_loc = gl.getUniformLocation(shaderProgram, "u_aspect");
    u_image_aspect_loc = gl.getUniformLocation(shaderProgram, "u_image_aspect");
    u_crop_loc = gl.getUniformLocation(shaderProgram, "u_crop");
    u_homography_loc = gl.getUniformLocation(shaderProgram, "u_homography");
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

const btnLoadDCP = document.getElementById('btn-load-dcp');
const selectColorspace = document.getElementById('select-colorspace');
const btnLoadLUT = document.getElementById('btn-load-lut');

btnLoadDCP.addEventListener('click', async () => {
    try {
        const path = await invoke('open_dcp_dialog');
        if (path) {
            await invoke('load_dcp_profile', { path });
            showToast("DCP Profile loaded.", "success");
            if (activeId) await loadProxyImage();
        }
    } catch(e) {
        showToast("Failed to load DCP", "error");
    }
});

selectColorspace.addEventListener('change', async (e) => {
    try {
        await invoke('set_working_colorspace', { colorspace: e.target.value });
        if (activeId) await loadProxyImage();
    } catch(e) { console.error(e); }
});

const selectBuiltinDcp = document.getElementById('select-builtin-dcp');
const selectBuiltinLut = document.getElementById('select-builtin-lut');

async function initBuiltins() {
    try {
        const dcps = await invoke('get_builtin_dcps');
        if (dcps && dcps.length > 0) {
            dcps.forEach(p => {
                const name = p.split('\\').pop().split('/').pop();
                const opt = document.createElement('option');
                opt.value = p; opt.textContent = name;
                selectBuiltinDcp.appendChild(opt);
            });
        }
        
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

selectBuiltinDcp.addEventListener('change', async (e) => {
    if (!e.target.value) return;
    try {
        await invoke('load_dcp_profile', { path: e.target.value });
        showToast("Built-in DCP Profile loaded.", "success");
        if (activeId) await loadProxyImage();
    } catch(err) {
        showToast("Failed to load DCP", "error");
    }
});

async function applyLUT(lutData) {
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
    sliders.lutOpacity.el.disabled = false;
    showToast(is1DLUT ? "1D LUT loaded." : "3D LUT loaded.", "success");
    requestRender();
}

selectBuiltinLut.addEventListener('change', async (e) => {
    if (!e.target.value) {
        hasLUT = false;
        requestRender();
        return;
    }
    try {
        const lutData = await invoke('load_3d_lut', { path: e.target.value });
        await applyLUT(lutData);
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
            await applyLUT(lutData);
        }
    } catch(e) {
        console.error(e);
        showToast("Failed to load LUT", "error");
    }
});

btnToggleViz.addEventListener('click', () => {
    isWaveform = !isWaveform;
    vizTitle.textContent = isWaveform ? 'Waveform' : 'Histogram';
    btnToggleViz.textContent = isWaveform ? 'Histogram' : 'Waveform';
    histCanvas.classList.toggle('hidden', isWaveform);
    waveCanvas.classList.toggle('hidden', !isWaveform);
    if (lastPixels) updateDataViz(lastPixels);
});

function drawHistogram(pixels) {
    const rHist = new Uint32Array(256);
    const gHist = new Uint32Array(256);
    const bHist = new Uint32Array(256);
    const lHist = new Uint32Array(256);

    let maxVal = 0;
    const len = pixels.length;
    for (let i = 0; i < len; i += 4) {
        const r = pixels[i], g = pixels[i+1], b = pixels[i+2];
        const l = Math.round(0.299*r + 0.587*g + 0.114*b);
        rHist[r]++; gHist[g]++; bHist[b]++; lHist[l]++;
    }

    // Ignore extreme shadows (0) and highlights (255) for dynamic scaling
    for (let i = 1; i < 255; i++) {
        if (rHist[i] > maxVal) maxVal = rHist[i];
        if (gHist[i] > maxVal) maxVal = gHist[i];
        if (bHist[i] > maxVal) maxVal = bHist[i];
    }
    if (maxVal === 0) maxVal = 1;

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
            const y = h - (hist[i] / maxVal) * h * 0.9;
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
        const y = h - (lHist[i] / maxVal) * h * 0.9;
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

    gl.useProgram(shaderProgram);
    gl.bindVertexArray(vao);

    const mode = btnModeColor.classList.contains('bg-[#28282c]') ? 0 : 1;
    // dmin/dmax are tracked globally
    const expVal = parseFloat(sliders.exposure.el.value);
    const exprVal = parseFloat(sliders.expr.el.value);
    const expgVal = parseFloat(sliders.expg.el.value);
    const expbVal = parseFloat(sliders.expb.el.value);
    const gammaVal = parseFloat(sliders.gamma.el.value);

    gl.uniform3f(u_base_density_loc, currentBaseDensity[0], currentBaseDensity[1], currentBaseDensity[2]);
    gl.uniform3f(u_dmin_loc, currentDMin[0], currentDMin[1], currentDMin[2]);
    gl.uniform3f(u_dmax_loc, currentDMax[0], currentDMax[1], currentDMax[2]);
    gl.uniform3f(u_exposure_loc, expVal + exprVal, expVal + expgVal, expVal + expbVal);
    gl.uniform1f(u_gamma_loc, gammaVal);
    gl.uniform1i(u_mode_loc, mode);
    gl.uniform1i(u_mode_loc, mode);
    
    gl.uniform1f(u_highlights_loc, parseFloat(sliders.highlights.el.value));
    gl.uniform1f(u_shadows_loc, parseFloat(sliders.shadows.el.value));
    gl.uniform1f(u_lut_opacity_loc, parseFloat(sliders.lutOpacity.el.value));
    gl.uniform1i(u_has_lut_loc, hasLUT ? 1 : 0);
    gl.uniform1i(u_lut_is_1d_loc, is1DLUT ? 1 : 0);
    gl.uniform1i(u_lut3d_loc, 1);
    gl.uniform1i(u_lut1d_loc, 2);
    gl.uniform1i(u_image_loc, 0);
    gl.uniform1f(u_aspect_loc, gl.canvas.width / gl.canvas.height);
    gl.uniform1f(u_image_aspect_loc, proxyWidth / proxyHeight);
    
    let pts = current_geom.calibration_points || [[0, 0], [1, 0], [1, 1], [0, 1]];
    let minX = Math.min(pts[0][0], pts[1][0], pts[2][0], pts[3][0]);
    let maxX = Math.max(pts[0][0], pts[1][0], pts[2][0], pts[3][0]);
    let minY = Math.min(pts[0][1], pts[1][1], pts[2][1], pts[3][1]);
    let maxY = Math.max(pts[0][1], pts[1][1], pts[2][1], pts[3][1]);
    
    let homographyMat = getHomography(pts);
    gl.uniformMatrix3fv(u_homography_loc, false, homographyMat);
    gl.uniform2fv(u_sprocket_uv_loc, currentSprocketUV);
    gl.uniform1f(u_sprocket_tolerance_loc, currentSprocketTolerance);
    gl.uniform1f(u_sprocket_feather_loc, currentSprocketFeather);
    gl.uniform4f(u_calib_bounds_loc, minX, minY, maxX, maxY);
    
    let a = current_geom.angle * Math.PI / 180.0;
    if (!isCropMode && !isRotateMode) a = 0;
    let s = Math.sin(a), c = Math.cos(a);
    let transformMat = new Float32Array([
        c, s, 0, 0,
        -s, c, 0, 0,
        0, 0, 1, 0,
        0, 0, 0, 1
    ]);
    gl.uniformMatrix4fv(u_transform_loc, false, transformMat);

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

    // Render to FBO for Histogram
    gl.uniform4f(u_crop_loc, current_geom.crop_rect.x, current_geom.crop_rect.y, current_geom.crop_rect.width, current_geom.crop_rect.height);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.viewport(0, 0, HIST_W, HIST_H);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
    
    const pixels = new Uint8Array(HIST_W * HIST_H * 4);
    gl.readPixels(0, 0, HIST_W, HIST_H, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    lastHistPixels = pixels;

    // Render to Main Canvas
    if (!isCropMode) {
        gl.uniform4f(u_crop_loc, current_geom.crop_rect.x, current_geom.crop_rect.y, current_geom.crop_rect.width, current_geom.crop_rect.height);
        gl.canvas.width = Math.max(1, proxyWidth * current_geom.crop_rect.width);
        gl.canvas.height = Math.max(1, proxyHeight * current_geom.crop_rect.height);
    } else {
        gl.uniform4f(u_crop_loc, 0.0, 0.0, 1.0, 1.0);
        gl.canvas.width = proxyWidth;
        gl.canvas.height = proxyHeight;
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, gl.canvas.width, gl.canvas.height);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    requestAnimationFrame(() => updateDataViz(pixels));
}

const PROXY_CACHE_LIMIT = 5;
const proxyCache = new Map(); // key: id, value: { arrayBuffer, lastUsed: Date.now() }

function getFromCache(id) {
    if (proxyCache.has(id)) {
        const item = proxyCache.get(id);
        item.lastUsed = Date.now();
        return item.arrayBuffer;
    }
    return null;
}

function addToCache(id, arrayBuffer) {
    if (proxyCache.has(id)) {
        proxyCache.get(id).lastUsed = Date.now();
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
    proxyCache.set(id, { arrayBuffer, lastUsed: Date.now() });
}

async function executePreload(ids) {
    for (const id of ids) {
        if (proxyCache.has(id)) continue;
        try {
            const result = await invoke('get_proxy_image_data', { id: id });
            let arrayBuffer;
            if (result instanceof ArrayBuffer) {
                arrayBuffer = result;
            } else if (result.buffer instanceof ArrayBuffer) {
                arrayBuffer = result.buffer;
            } else if (Array.isArray(result)) {
                arrayBuffer = new Uint8Array(result).buffer;
            }
            if (arrayBuffer) {
                addToCache(id, arrayBuffer);
            }
        } catch (e) {
            console.error("Silent preload failed for " + id, e);
        }
    }
}

function scheduleSilentPreWarming() {
    const items = Array.from(document.querySelectorAll('#filmstrip-container .film-item'));
    if (items.length === 0) return;
    const currentIndex = items.findIndex(item => item.classList.contains('active'));
    if (currentIndex === -1) return;

    const idsToPreload = [];
    if (currentIndex > 0) idsToPreload.push(items[currentIndex - 1].dataset.id);
    if (currentIndex < items.length - 1) idsToPreload.push(items[currentIndex + 1].dataset.id);

    if (window.requestIdleCallback) {
        requestIdleCallback(() => executePreload(idsToPreload));
    } else {
        setTimeout(() => executePreload(idsToPreload), 500);
    }
}

async function loadProxyImage(token = null) {
    if (!activeId || !webGLInitialized) return;
    
    let arrayBuffer = getFromCache(activeId);
    let byteOffset = 0;
    
    const loadingMask = document.getElementById('loading-proxy-ui');
    
    if (!arrayBuffer) {
        if (loadingMask) loadingMask.classList.remove('hidden');
        try {
            const result = await invoke('get_proxy_image_data', { id: activeId });
            if (token !== null && token !== currentImageRequestToken) {
                if (loadingMask) loadingMask.classList.add('hidden');
                return;
            }
            
            if (result instanceof ArrayBuffer) {
                arrayBuffer = result;
            } else if (result.buffer instanceof ArrayBuffer) {
                arrayBuffer = result.buffer;
                byteOffset = result.byteOffset || 0;
            } else if (Array.isArray(result)) {
                arrayBuffer = new Uint8Array(result).buffer;
            }
            
            if (arrayBuffer) addToCache(activeId, arrayBuffer);
        } catch(e) { 
            console.error("Failed to load proxy", e); 
            if (loadingMask) loadingMask.classList.add('hidden');
            return;
        }
        if (loadingMask) loadingMask.classList.add('hidden');
    }

    if (!arrayBuffer) return;

    try {
        const dataView = new DataView(arrayBuffer, byteOffset);
        const width = dataView.getUint32(0, true);
        const height = dataView.getUint32(4, true);
        
        currentBaseDensity[0] = dataView.getFloat32(8, true);
        currentBaseDensity[1] = dataView.getFloat32(12, true);
        currentBaseDensity[2] = dataView.getFloat32(16, true);
        
        const pixels = new Uint16Array(arrayBuffer, byteOffset + 20, width * height * 4);
        proxyPixels = pixels;
        proxyWidth = width;
        proxyHeight = height;
        
        if (previewCanvas.width !== width || previewCanvas.height !== height) {
            previewCanvas.width = width;
            previewCanvas.height = height;
        }
        
        gl.bindTexture(gl.TEXTURE_2D, tex);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16UI, width, height, 0, gl.RGBA_INTEGER, gl.UNSIGNED_SHORT, pixels);
        
        updateCanvasTransform(width, height);
        requestRender();
        
        scheduleSilentPreWarming();
    } catch(e) {
        console.error("Error parsing proxy buffer:", e);
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
    } else {
        btnModeBw.classList.add('bg-[#28282c]', 'text-zinc-100', 'shadow-sm');
        btnModeBw.classList.remove('text-zinc-500', 'hover:text-zinc-300');
        btnModeColor.classList.add('text-zinc-500', 'hover:text-zinc-300');
        btnModeColor.classList.remove('bg-[#28282c]', 'text-zinc-100', 'shadow-sm');
        sliders.expr.el.disabled = true;
        sliders.expg.el.disabled = true;
        sliders.expb.el.disabled = true;
    }
}

function updateDMinMaxDisplay() {
    document.getElementById('val-dmin').innerHTML = `<span class="text-red-400">${currentDMin[0].toFixed(3)}</span><span class="text-emerald-400">${currentDMin[1].toFixed(3)}</span><span class="text-blue-400">${currentDMin[2].toFixed(3)}</span>`;
    document.getElementById('val-dmax').innerHTML = `<span class="text-red-400">${currentDMax[0].toFixed(3)}</span><span class="text-emerald-400">${currentDMax[1].toFixed(3)}</span><span class="text-blue-400">${currentDMax[2].toFixed(3)}</span>`;
}

function updateUIFromParams(params, geom) {
    currentDMin = params.d_min.slice();
    currentDMax = params.d_max.slice();
    updateDMinMaxDisplay();
    sliders.masterDmin.el.value = 0; sliders.masterDmin.val.textContent = "0.000"; lastMasterDmin = 0;
    sliders.masterDmax.el.value = 0; sliders.masterDmax.val.textContent = "0.000"; lastMasterDmax = 0;
    
    sliders.exposure.el.value = params.exposure;
    sliders.gamma.el.value = params.gamma;
    sliders.expr.el.value = params.exp_r;
    sliders.expg.el.value = params.exp_g;
    sliders.expb.el.value = params.exp_b;
    if (params.highlights !== undefined) sliders.highlights.el.value = params.highlights;
    if (params.shadows !== undefined) sliders.shadows.el.value = params.shadows;
    
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
    setMode(params.film_mode === 'BW' ? 'B&W' : 'Color');
}

let backendSyncTimeout = null;
function scheduleBackendSync(key) {
    if (backendSyncTimeout) clearTimeout(backendSyncTimeout);
    backendSyncTimeout = setTimeout(async () => {
        if (key === 'angle' && activeId) {
            await invoke('update_geometry', { id: activeId, geom: current_geom });
            await loadProxyImage();
        } else {
            updateBackendParams();
        }
        requestThumbnailSync();
    }, 100);
}

for (const key in sliders) {
    const s = sliders[key];
    s.el.addEventListener('mousedown', () => pushUndoState());
    s.el.addEventListener('input', (e) => {
        s.val.textContent = parseFloat(e.target.value).toFixed(key === 'angle' ? 1 : 3);
        if (key === 'angle') {
            current_geom.angle = parseFloat(e.target.value);
        } else if (key === 'sprocketTolerance') {
            currentSprocketTolerance = parseFloat(e.target.value);
        } else if (key === 'sprocketFeather') {
            currentSprocketFeather = parseFloat(e.target.value);
        }
        updateSliderTrack(e.target);
        requestRender(); // Zero latency UI!
        scheduleBackendSync(key);
    });
}

function enableUI() {
    for (const key in sliders) {
        sliders[key].el.disabled = false;
        updateSliderTrack(sliders[key].el);
    }
    sliders.lutOpacity.el.disabled = false;
    btnCropMode.disabled = false;
    btnRotateMode.disabled = false;
    document.getElementById('btn-recalibrate').disabled = false;
    btnResetCrop.disabled = false;
    btnAutoColor.disabled = false;
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
}

let allRolls = [];
let currentRollViewId = null;
let currentImportSessionPaths = null;

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

let renderVersion = 0;
async function renderLibraryAndFilmstrip() {
    renderVersion++;
    const currentVersion = renderVersion;
    try {
        await fetchRolls();
        if (renderVersion !== currentVersion) return;
        const items = await invoke('get_filmstrip');
        if (renderVersion !== currentVersion) return;
        allLibraryItems = items;
        
        const libraryRollsGrid = document.getElementById('library-rolls-grid');
        const historyInternalGrid = document.getElementById('history-internal-grid');
        const btnHistoryBack = document.getElementById('btn-history-back');
        const historyTitle = document.getElementById('history-view-title');
        const btnExportContactSheet = document.getElementById('btn-export-contact-sheet');
        const historyEmpty = document.getElementById('history-empty');
        
        libraryGrid.innerHTML = '';
        libraryRollsGrid.innerHTML = '';
        historyInternalGrid.innerHTML = '';
        filmstripContainer.innerHTML = '';
        
        // --- Populate LIBRARY View (All Images) ---
        if (items.length === 0) {
            libraryEmpty.classList.remove('hidden');
            libraryGrid.classList.add('hidden');
            btnSelectAll.classList.add('hidden');
        } else {
            libraryEmpty.classList.add('hidden');
            libraryGrid.classList.remove('hidden');
            btnSelectAll.classList.remove('hidden');
            
            items.forEach(item => {
                const libDiv = document.createElement('div');
                libDiv.className = `library-item rounded overflow-hidden relative ${selectedLibraryIds.has(item.id) ? 'selected' : ''}`;
                libDiv.dataset.id = item.id;
                libDiv.ondblclick = () => {
                    selectedLibraryIds.clear();
                    selectedLibraryIds.add(item.id);
                    currentImportSessionPaths = null;
                    if (item.roll_id && item.roll_id !== 'LOOSE_DEFAULT') {
                        currentRollViewId = item.roll_id;
                    } else {
                        currentRollViewId = null;
                    }
                    updateLibrarySelectionUI();
                    selectImage(item.id);
                    switchView('develop');
                };
                libDiv.onclick = (e) => {
                    if (e.ctrlKey || e.metaKey) {
                        if (selectedLibraryIds.has(item.id)) selectedLibraryIds.delete(item.id);
                        else selectedLibraryIds.add(item.id);
                    } else {
                        selectedLibraryIds.clear();
                        selectedLibraryIds.add(item.id);
                    }
                    updateLibrarySelectionUI();
                };
                const libImg = document.createElement('img');
                libImg.dataset.imgId = item.id;
                if (item.thumbnail_base64 === "FILE_MISSING") {
                    libImg.src = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" fill="none" stroke="#ff0000" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>`);
                    libImg.className = 'w-full h-full object-contain opacity-50 bg-[#1C1C1E] p-4 pointer-events-none';
                } else {
                    libImg.src = `data:image/jpeg;base64,${item.thumbnail_base64}`;
                    libImg.className = 'w-full h-full object-cover pointer-events-none';
                }
                libDiv.appendChild(libImg);
                libraryGrid.appendChild(libDiv);
            });
        }
        
        // --- Populate HISTORY FILMS View ---
        if (items.length === 0 && allRolls.length === 0) {
            historyEmpty.classList.remove('hidden');
            libraryRollsGrid.classList.add('hidden');
            historyInternalGrid.classList.add('hidden');
            btnHistoryBack.classList.add('hidden');
            btnExportContactSheet.classList.add('hidden');
        } else {
            historyEmpty.classList.add('hidden');
            const filters = getActiveFilters();
            
            if (currentRollViewId) {
                // Inner Roll View
                libraryRollsGrid.classList.add('hidden');
                historyInternalGrid.classList.remove('hidden');
                btnHistoryBack.classList.remove('hidden');
                btnExportContactSheet.classList.remove('hidden');
                document.getElementById('btn-promote-roll').classList.remove('hidden');
                document.getElementById('btn-delete-rolls').classList.add('hidden');
                historyTitle.textContent = "ROLL CONTENTS";
                
                const currentRoll = allRolls.find(r => r.roll_id === currentRollViewId);
                if (currentRoll) {
                    try {
                        let rollStrip = await invoke('get_roll_filmstrip', { rollId: currentRollViewId });
                        
                        // Check if any paths are missing from the DB (e.g. from dropped table or first load)
                        const unimportedPaths = [];
                        currentRoll.image_paths.forEach(path => {
                            const found = rollStrip.find(i => i.file_path.replace(/\\/g, '/').toLowerCase() === path.replace(/\\/g, '/').toLowerCase());
                            if (!found) unimportedPaths.push(path);
                        });
                        
                        // If missing, auto-import them on the fly
                        if (unimportedPaths.length > 0) {
                            document.getElementById('history-view-title').textContent = "IMPORTING MISSING...";
                            await invoke('import_images', { paths: unimportedPaths, isLoose: false, inLibrary: false, rollId: currentRollViewId, isHistorical: false });
                            // Re-fetch roll strip after importing
                            rollStrip = await invoke('get_roll_filmstrip', { rollId: currentRollViewId });
                        }
                        
                        // Merge into local items list for rendering
                        rollStrip.forEach(newItem => {
                            if (!items.some(i => i.id === newItem.id)) {
                                items.push(newItem);
                            }
                        });
                        
                        const newIds = rollStrip.filter(i => unimportedPaths.includes(i.file_path)).map(i => i.id);
                        if (newIds.length > 0) {
                            invoke('start_precache', { ids: newIds }).catch(e => console.error("History precache error", e));
                        }
                    } catch (e) {
                        console.error("Failed to load or import roll filmstrip", e);
                    }
                    
                    document.getElementById('history-view-title').textContent = "ROLL CONTENTS";

                    currentRoll.image_paths.forEach(path => {
                        const existingItem = items.find(i => i.file_path.replace(/\\/g, '/').toLowerCase() === path.replace(/\\/g, '/').toLowerCase());
                        if (!existingItem) return;

                        const libDiv = document.createElement('div');
                        libDiv.className = `library-item rounded overflow-hidden relative`;
                        
                        if (selectedLibraryIds.has(existingItem.id)) libDiv.classList.add('selected');
                        libDiv.dataset.id = existingItem.id;
                        libDiv.ondblclick = () => {
                            selectedLibraryIds.clear();
                            selectedLibraryIds.add(existingItem.id);
                            currentImportSessionPaths = null;
                            if (existingItem.roll_id && existingItem.roll_id !== 'LOOSE_DEFAULT') {
                                currentRollViewId = existingItem.roll_id;
                            } else {
                                currentRollViewId = null;
                            }
                            updateLibrarySelectionUI();
                            selectImage(existingItem.id);
                            switchView('develop');
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
                        const libImg = document.createElement('img');
                        libImg.dataset.imgId = existingItem.id;
                        if (existingItem.thumbnail_base64 === "FILE_MISSING") {
                            libImg.src = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" fill="none" stroke="#ff0000" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>`);
                            libImg.className = 'w-full h-full object-contain opacity-50 bg-[#1C1C1E] p-4 pointer-events-none';
                        } else {
                            libImg.src = `data:image/jpeg;base64,${existingItem.thumbnail_base64}`;
                            libImg.className = 'w-full h-full object-cover pointer-events-none';
                        }
                        libDiv.appendChild(libImg);
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
                historyTitle.textContent = "THE ROLL ARCHIVE";
                
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
                    let thumbSrc = '';
                    try {
                        const previews = await invoke('get_roll_previews', { rollId: roll.roll_id });
                        if (renderVersion !== currentVersion) return;
                        if (previews && previews.length > 0) {
                            thumbSrc = `data:image/jpeg;base64,${previews[0]}`;
                        }
                    } catch (e) {
                        console.error(e);
                    }
                    
                    const card = document.createElement('div');
                    card.className = "group relative bg-[#1C1C1E] rounded-lg overflow-hidden cursor-pointer hover:border-zinc-500 transition-all duration-300 flex h-[200px] shadow-lg w-full";
                    
                    if (isDeleteMode && selectedRollIds.has(roll.roll_id)) {
                        card.style.border = "2px solid #ef4444";
                    } else {
                        card.style.border = "1px solid #28282c";
                    }

                    card.onclick = async () => {
                        if (isDeleteMode) {
                            if (selectedRollIds.has(roll.roll_id)) selectedRollIds.delete(roll.roll_id);
                            else selectedRollIds.add(roll.roll_id);
                            renderLibraryAndFilmstrip();
                        } else {
                            currentRollViewId = roll.roll_id;
                            selectedLibraryIds.clear();
                            renderLibraryAndFilmstrip();
                        }
                    };
                    card.innerHTML = `
                        <div class="flex-1 p-6 flex flex-col justify-between z-10 bg-gradient-to-r from-[#1C1C1E] to-[#1C1C1E]/80">
                            <div>
                                <div class="text-[24px] font-black tracking-tighter text-zinc-100 uppercase leading-none mb-2">${roll.film_stock || 'Unknown Film'}</div>
                                <div class="text-[12px] font-bold text-zinc-500 uppercase tracking-widest">${roll.format || '135'} FORMAT</div>
                            </div>
                            <div>
                                <div class="text-[11px] text-zinc-400 font-medium mb-1"><span class="text-zinc-600">CAM</span> ${roll.camera || 'Unknown'}</div>
                                <div class="text-[11px] text-zinc-400 font-medium"><span class="text-zinc-600">DAT</span> ${roll.date || 'Unknown'}</div>
                            </div>
                        </div>
                        <div class="w-1/2 h-full relative overflow-hidden shrink-0">
                            <div class="absolute inset-0 bg-gradient-to-r from-[#1C1C1E]/80 to-transparent z-10"></div>
                            ${thumbSrc ? `<img src="${thumbSrc}" class="w-full h-full object-cover scale-100 group-hover:scale-105 transition-transform duration-500 opacity-80 group-hover:opacity-100">` : `<div class="w-full h-full bg-[#121214]"></div>`}
                        </div>
                    `;
                    libraryRollsGrid.appendChild(card);
                }
            }
        }
        
        // --- Populate DEVELOP Filmstrip ---
        let filmstripItems = items;
        if (currentRollViewId) {
            const currentRoll = allRolls.find(r => r.roll_id === currentRollViewId);
            if (currentRoll) {
                const rollPaths = new Set(currentRoll.image_paths.map(p => p.replace(/\\/g, '/').toLowerCase()));
                filmstripItems = items.filter(item => rollPaths.has(item.file_path.replace(/\\/g, '/').toLowerCase()));
                invoke('start_precache', { ids: filmstripItems.map(i => i.id) }).catch(e => console.error("Precache error", e));
            }
        } else if (currentImportSessionPaths) {
            filmstripItems = items.filter(item => currentImportSessionPaths.includes(item.file_path.replace(/\\/g, '/').toLowerCase()));
        } else {
            filmstripItems = items.filter(item => item.roll_id === null || item.roll_id === 'LOOSE_DEFAULT');
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
            const stripImg = document.createElement('img');
            stripImg.dataset.imgId = item.id;
            if (item.thumbnail_base64 === "FILE_MISSING") {
                stripImg.src = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" fill="none" stroke="#ff0000" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>`);
                stripImg.className = 'w-full h-full object-contain rounded-[2px] pointer-events-none opacity-50 bg-[#1C1C1E] p-2';
            } else {
                stripImg.src = `data:image/jpeg;base64,${item.thumbnail_base64}`;
                stripImg.className = 'w-full h-full object-cover rounded-[2px] pointer-events-none';
            }
            stripDiv.appendChild(stripImg);
            filmstripContainer.appendChild(stripDiv);
        });
        
    } catch (e) { console.error("Filmstrip error:", e); }
}

document.getElementById('btn-promote-roll').addEventListener('click', async () => {
    if (currentRollViewId) {
        try {
            await invoke('promote_roll', { rollId: currentRollViewId });
            showToast("Roll promoted to Library successfully", "success");
            await loadLibraryItems();
            renderLibraryAndFilmstrip();
        } catch (e) {
            showToast("Failed to promote roll", "error");
            console.error(e);
        }
    }
});

document.getElementById('btn-history-back').addEventListener('click', () => {
    currentRollViewId = null;
    selectedLibraryIds.clear();
    renderLibraryAndFilmstrip();
});

let currentImageRequestToken = 0;

async function selectImage(id) {
    if (activeId === id) return;
    const myToken = ++currentImageRequestToken;
    try {
        saveCurrentState(); // Save current state before switching

        let state;
        try {
            const rollId = currentRollViewId || 'LOOSE_DEFAULT';
            if (imageStates.has(id)) {
                state = imageStates.get(id);
                await invoke('switch_active_image', { id, rollId });
            } else {
                state = await invoke('switch_active_image', { id, rollId });
                imageStates.set(id, { params: state.params, geom: state.geom || { crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0.0, flip_h: false, flip_v: false, rotate_90_count: 0 } });
            }
        } catch (err) {
            if (err === "FILE_MISSING") {
                missingFileId = id;
                document.getElementById('missing-file-ui').classList.remove('hidden');
                document.getElementById('missing-file-ui').classList.add('flex');
                document.getElementById('preview-canvas').style.display = 'none';
                canvasWrapper.style.display = 'block';
                canvasWrapper.style.width = '100%';
                canvasWrapper.style.height = '100%';
                activeId = id;
                renderLibraryAndFilmstrip();
                return;
            }
            throw err;
        }

        document.getElementById('missing-file-ui').classList.add('hidden');
        document.getElementById('missing-file-ui').classList.remove('flex');
        document.getElementById('preview-canvas').style.display = 'block';

        activeId = id;
        enableUI();
        current_geom = JSON.parse(JSON.stringify(state.geom));
        updateUIFromParams(state.params, current_geom);
        updateCropOverlay();

        renderLibraryAndFilmstrip();

        updateSliderTrack(sliders.exposure.el);
        updateSliderTrack(sliders.gamma.el);

        const loadingUI = document.getElementById('loading-proxy-ui');
        const rightPanel = document.querySelector('.w-\\[340px\\]');
        const filmstripContainer = document.getElementById('filmstrip-container');
        
        // Immediate blocking, no setTimeout
        if(loadingUI) { loadingUI.classList.remove('hidden'); loadingUI.classList.add('flex'); }
        if(rightPanel) { rightPanel.style.pointerEvents = 'none'; rightPanel.style.opacity = '0.5'; }
        if(filmstripContainer) { filmstripContainer.style.pointerEvents = 'none'; filmstripContainer.style.opacity = '0.5'; }

        await loadProxyImage(myToken);

        // 卫语句：防止异步状态雪崩
        if (myToken !== currentImageRequestToken) {
            return;
        }

        updateBackendParams();
        requestRender(); // Force uniform update

        // Restore UI
        if(loadingUI) { loadingUI.classList.add('hidden'); loadingUI.classList.remove('flex'); }
        if(rightPanel) { rightPanel.style.pointerEvents = 'auto'; rightPanel.style.opacity = '1'; }
        if(filmstripContainer) { filmstripContainer.style.pointerEvents = 'auto'; filmstripContainer.style.opacity = '1'; }

        if (!current_geom.calibration_points) {
            isCalibrationMode = true;
            document.getElementById('calibration-overlay').classList.remove('hidden');
            document.getElementById('right-panel-blocker').classList.remove('hidden');
            
            calibrationPoints = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            requestAnimationFrame(updateCalibrationPolygon);
        } else {
            isCalibrationMode = false;
            document.getElementById('calibration-overlay').classList.add('hidden');
            document.getElementById('right-panel-blocker').classList.add('hidden');
        }

        
    } catch(e) { console.error(e); }
}

btnModeColor.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    setMode('Color');
    await invoke('set_film_mode', { id: activeId, mode: 'Color' });
    updateBackendParams();
    requestRender();
    requestThumbnailSync();
});

btnModeBw.addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    setMode('BW');
    await invoke('set_film_mode', { id: activeId, mode: 'B&W' });
    updateBackendParams();
    requestRender();
    requestThumbnailSync();
});

const doImportSingle = async () => {
    document.getElementById('import-choice-modal').classList.add('opacity-0', 'pointer-events-none');
    try {
        btnImport.textContent = "Importing...";
        btnImport.disabled = true;
        btnImportTriggers.forEach(btn => { 
            btn.dataset.originalText = btn.dataset.originalText || btn.textContent;
            btn.textContent = "Importing..."; 
            btn.disabled = true; 
        });
        
        const paths = await invoke('open_file_dialog');
        if (paths && paths.length > 0) {
            currentRollViewId = null;
            currentImportSessionPaths = paths.map(p => p.replace(/\\/g, '/').toLowerCase());
            await invoke('import_images', { paths, isLoose: true, rollId: 'LOOSE_DEFAULT', isHistorical: false });
            await fetchRolls();
            await renderLibraryAndFilmstrip();
            
            if (allLibraryItems && allLibraryItems.length > 0) {
                const newPath = paths[0];
                const newPhoto = allLibraryItems.find(i => i.file_path === newPath || i.file_path.replace(/\\/g, '/') === newPath);
                if (newPhoto) {
                    await selectImage(newPhoto.id);
                    switchView('develop');
                }
            }
        }
    } catch (e) { showToast("Import failed: " + e, "error"); }
    finally {
        btnImport.textContent = "Import Roll";
        btnImport.disabled = false;
        btnImportTriggers.forEach(btn => { 
            btn.textContent = btn.dataset.originalText || "Import Roll"; 
            btn.disabled = false; 
        });
    }
};

const doImportRoll = async () => {
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
        
        const paths = await invoke('open_file_dialog');
        if (paths && paths.length > 0) {
            currentImportSessionPaths = null;
            const newRollId = "roll_" + Date.now();
            const roll_id = `roll_${Date.now()}_${Math.floor(Math.random()*1000)}`;
            const roll = { roll_id, date, format, film_stock: film, camera, image_paths: paths };
            
            await invoke('import_roll', { roll, paths });
            
            // Persist Camera and Film
            if (camera) {
                try { await invoke('add_user_camera', { camera }); } catch(e) {}
            }
            if (film) {
                try { await invoke('add_user_film', { film }); } catch(e) {}
            }
            
            await fetchRolls(); // Fetch rolls so the newly imported roll is available for filtering
            await renderLibraryAndFilmstrip();
            
            if (allLibraryItems && allLibraryItems.length > 0) {
                const newPath = paths[0];
                const newPhoto = allLibraryItems.find(i => i.file_path === newPath || i.file_path.replace(/\\\\/g, '/') === newPath);
                if (newPhoto) {
                    currentRollViewId = roll_id;
                    await selectImage(newPhoto.id);
                    switchView('develop');
                }
            }
        }
    } catch (e) { showToast("Import failed: " + e, "error"); }
    finally {
        btnImport.textContent = "Import Roll";
        btnImport.disabled = false;
        btnImportTriggers.forEach(btn => { 
            btn.textContent = btn.dataset.originalText || "Import Roll"; 
            btn.disabled = false; 
        });
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
    const rollId = document.getElementById('continue-roll-select').value;
    if (!rollId) return;
    
    document.getElementById('continue-roll-modal').classList.add('opacity-0', 'pointer-events-none');
    
    currentRollViewId = rollId;
    selectedLibraryIds.clear();
    
    const roll = allRolls.find(r => r.roll_id === rollId);
    if (roll) {
        const pathsToImport = roll.image_paths.filter(p => !allLibraryItems.some(item => item.file_path.replace(/\\/g, '/').toLowerCase() === p.replace(/\\/g, '/').toLowerCase()));
        if (pathsToImport.length > 0) {
            document.getElementById('history-view-title').textContent = "LOADING ROLL...";
            try {
                await invoke('import_images', { paths: pathsToImport, isLoose: false, inLibrary: false, rollId: currentRollViewId, isHistorical: true });
                allLibraryItems = await invoke('get_filmstrip');
            } catch (e) { console.error(e); }
            document.getElementById('history-view-title').textContent = "ROLL CONTENTS";
        }
    }
    
    await renderLibraryAndFilmstrip();
    switchView('history');
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
    document.getElementById('roll-metadata-modal').classList.remove('opacity-0', 'pointer-events-none');
    setTimeout(() => document.getElementById('roll-metadata-content').classList.remove('scale-95'), 10);
});

document.getElementById('btn-close-roll-meta').addEventListener('click', () => {
    document.getElementById('roll-metadata-modal').classList.add('opacity-0', 'pointer-events-none');
});
document.getElementById('btn-cancel-roll-meta').addEventListener('click', () => {
    document.getElementById('roll-metadata-modal').classList.add('opacity-0', 'pointer-events-none');
});
document.getElementById('btn-confirm-roll-meta').addEventListener('click', doImportRoll);



// Export Modal Logic
btnExportDialog.addEventListener('click', () => {
    exportModal.classList.remove('opacity-0', 'pointer-events-none');
    setTimeout(() => exportModalContent.classList.remove('scale-95'), 10);
});

const closeExportModal = () => {
    exportModalContent.classList.add('scale-95');
    exportModal.classList.add('opacity-0', 'pointer-events-none');
};
btnCloseExport.addEventListener('click', closeExportModal);
btnCancelExport.addEventListener('click', closeExportModal);

btnConfirmExport.addEventListener('click', async () => {
    try {
        btnConfirmExport.textContent = "Exporting...";
        btnConfirmExport.disabled = true;
        const format = document.getElementById('export-format').value;
        const colorSpace = document.getElementById('export-colorspace').value;
        const resampleMode = document.getElementById('export-resample').value;
        const applyUsm = document.getElementById('export-usm').checked;
        const namingToken = document.getElementById('export-naming').value;
        
        const quality = parseInt(document.getElementById('export-quality').value) || 100;
    const export_ids = Array.from(selectedLibraryIds);
    const outputDir = await invoke('select_export_dir');
        if (!outputDir) {
            btnConfirmExport.textContent = "Select Output Folder";
            btnConfirmExport.disabled = false;
            return;
        }
        closeExportModal();
        const count = await invoke('batch_export_images', { export_ids, outputDir, format, colorSpace, resampleMode, applyUsmFlag: applyUsm, namingToken, quality });
        showToast(`Successfully exported ${count} image(s) to:\n${outputDir}`, "success");
    } catch (e) { showToast("Batch export failed: " + e, "error"); } 
    finally {
        btnConfirmExport.textContent = "Select Output Folder";
        btnConfirmExport.disabled = false;
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
sliders.masterDmin.el.addEventListener('change', updateBackendParams);

sliders.masterDmax.el.addEventListener('input', (e) => {
    let current = parseFloat(e.target.value);
    let delta = current - lastMasterDmax;
    lastMasterDmax = current;
    currentDMax[0] += delta; currentDMax[1] += delta; currentDMax[2] += delta;
    sliders.masterDmax.val.textContent = current.toFixed(3);
    updateDMinMaxDisplay(); requestRender();
});
sliders.masterDmax.el.addEventListener('change', updateBackendParams);

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

    canvasWrapper.style.overflow = 'hidden';
    previewCanvas.style.position = 'absolute';
    previewCanvas.style.objectFit = 'fill'; 

    let aspect;
    if (isCropMode || isRotateMode) {
        aspect = cw / ch;
    } else {
        aspect = (cw * rect.width) / (ch * rect.height);
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

    if (isCropMode || isRotateMode) {
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
        isRotateMode = false;
        btnRotateMode.classList.remove('active');
        btnCropMode.classList.add('active');
        cropOverlay.classList.remove('hidden');
        cropBox.classList.remove('hidden');
        cropMask.classList.remove('hidden');
        cropHandles.classList.remove('hidden');
        updateCropOverlay();
    } else {
        btnCropMode.classList.remove('active');
        cropOverlay.classList.add('hidden');
    }
    updateCanvasTransform();
    if (!isCropMode) {
        loadProxyImage();
    } else {
        requestRender();
    }
});

btnRotateMode.addEventListener('click', () => {
    isRotateMode = !isRotateMode;
    if (isRotateMode) {
        isCropMode = false;
        btnCropMode.classList.remove('active');
        btnRotateMode.classList.add('active');
        cropOverlay.classList.remove('hidden');
        cropBox.classList.add('hidden');
        cropMask.classList.add('hidden');
        cropHandles.classList.add('hidden');
        updateCropOverlay();
    } else {
        btnRotateMode.classList.remove('active');
        cropOverlay.classList.add('hidden');
    }
    updateCanvasTransform();
    if (!isRotateMode) {
        loadProxyImage();
    } else {
        requestRender();
    }
});


function updateCropRectForRotation(rect, isCW, flipH, flipV) {
    let p1 = { x: rect.x, y: rect.y };
    let p2 = { x: rect.x + rect.width, y: rect.y + rect.height };
    
    function transform(p) {
        let x = p.x, y = p.y;
        if (flipV) y = 1 - y;
        if (flipH) x = 1 - x;
        if (isCW) { let t = x; x = 1 - y; y = t; } else { let t = x; x = y; y = 1 - t; }
        if (flipH) x = 1 - x;
        if (flipV) y = 1 - y;
        return { x, y };
    }
    
    let tp1 = transform(p1);
    let tp2 = transform(p2);
    let nx = Math.min(tp1.x, tp2.x), ny = Math.min(tp1.y, tp2.y);
    let nw = Math.abs(tp2.x - tp1.x), nh = Math.abs(tp2.y - tp1.y);
    return { x: nx, y: ny, width: nw, height: nh };
}

let geomSyncId = 0;
function sendGeometrySync() {
    geomSyncId++;
    const currentSyncId = geomSyncId;
    
    invoke('update_geometry', { id: activeId, geom: current_geom }).then(() => {
        if (geomSyncId !== currentSyncId) return;
        loadProxyImage().then(() => {
            requestThumbnailSync();
        });
    });
}

btnRotateLeft.addEventListener('click', () => {
    if (!activeId) return; pushUndoState();
    current_geom.rotate_90_count += 1;
    current_geom.crop_rect = updateCropRectForRotation(current_geom.crop_rect, true, current_geom.flip_h, current_geom.flip_v);
    sendGeometrySync();
});

btnRotateRight.addEventListener('click', () => {
    if (!activeId) return; pushUndoState();
    current_geom.rotate_90_count -= 1;
    current_geom.crop_rect = updateCropRectForRotation(current_geom.crop_rect, false, current_geom.flip_h, current_geom.flip_v);
    sendGeometrySync();
});

btnFlipH.addEventListener('click', () => {
    if (!activeId) return; pushUndoState();
    current_geom.flip_h = !current_geom.flip_h;
    current_geom.crop_rect.x = 1.0 - current_geom.crop_rect.x - current_geom.crop_rect.width;
    sendGeometrySync();
});

btnFlipV.addEventListener('click', () => {
    if (!activeId) return; pushUndoState();
    current_geom.flip_v = !current_geom.flip_v;
    current_geom.crop_rect.y = 1.0 - current_geom.crop_rect.y - current_geom.crop_rect.height;
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
    gl.uniform1f(u_highlights_loc, 0.0);
    gl.uniform1f(u_shadows_loc, 0.0);
    gl.uniform1i(u_has_lut_loc, 0);
    let pts = current_geom.calibration_points || [[0, 0], [1, 0], [1, 1], [0, 1]];
    let homographyMat = getHomography(pts);
    gl.uniformMatrix3fv(u_homography_loc, false, homographyMat);
    gl.uniform2fv(u_sprocket_uv_loc, new Float32Array([-1.0, -1.0])); // Disable target during calibration
    gl.uniform1f(u_sprocket_tolerance_loc, 0.0);
    
    let a = current_geom.angle * Math.PI / 180.0;
    if (!isCropMode && !isRotateMode) a = 0;
    let s = Math.sin(a), c = Math.cos(a);
    let transformMat = new Float32Array([
        c, s, 0, 0,
        -s, c, 0, 0,
        0, 0, 1, 0,
        0, 0, 0, 1
    ]);
    gl.uniformMatrix4fv(u_transform_loc, false, transformMat);
    gl.uniform4f(u_crop_loc, current_geom.crop_rect.x, current_geom.crop_rect.y, current_geom.crop_rect.width, current_geom.crop_rect.height);
    
    gl.uniform1f(u_aspect_loc, gl.canvas.width / gl.canvas.height);
    gl.uniform1f(u_image_aspect_loc, proxyWidth / proxyHeight);

    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);

    // Render pure image to FBO
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.viewport(0, 0, HIST_W, HIST_H);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
    
    const purePixels = new Uint8Array(HIST_W * HIST_H * 4);
    gl.readPixels(0, 0, HIST_W, HIST_H, gl.RGBA, gl.UNSIGNED_BYTE, purePixels);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);

    // Min/Max points computed from pts
    let minX = Math.min(pts[0][0], pts[1][0], pts[2][0], pts[3][0]);
    let maxX = Math.max(pts[0][0], pts[1][0], pts[2][0], pts[3][0]);
    let minY = Math.min(pts[0][1], pts[1][1], pts[2][1], pts[3][1]);
    let maxY = Math.max(pts[0][1], pts[1][1], pts[2][1], pts[3][1]);

    // --- PHASE 2: Calculation ---
    let r_arr = []; let g_arr = []; let b_arr = [];
    for (let i = 0; i < purePixels.length; i += 4) {
        let px = (i / 4) % HIST_W;
        let py = Math.floor((i / 4) / HIST_W);
        let a_tex_x = px / (HIST_W - 1);
        let a_tex_y = py / (HIST_H - 1);
        let base_uv_x = a_tex_x;
        let base_uv_y = 1.0 - a_tex_y;
        let v_tex_x = current_geom.crop_rect.x + base_uv_x * current_geom.crop_rect.width;
        let v_tex_y = current_geom.crop_rect.y + base_uv_y * current_geom.crop_rect.height;

        if (v_tex_x >= minX && v_tex_x <= maxX && v_tex_y >= minY && v_tex_y <= maxY) {
            r_arr.push(purePixels[i]);
            g_arr.push(purePixels[i+1]);
            b_arr.push(purePixels[i+2]);
        }
    }

    r_arr.sort((a,b)=>a-b);
    g_arr.sort((a,b)=>a-b);
    b_arr.sort((a,b)=>a-b);

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

    updateBackendParams();
    renderWebGL();
}

document.getElementById('btn-reset-crop').addEventListener('click', async () => {
    if (!activeId) return; pushUndoState();
    current_geom.crop_rect = { x: 0, y: 0, width: 1, height: 1 };
    current_geom.angle = 0.0;
    current_geom.flip_h = false;
    current_geom.flip_v = false;
    current_geom.rotate_90_count = 0;
    if (isCropMode) updateCropOverlay();
    await invoke('update_geometry', { id: activeId, geom: current_geom });
    await loadProxyImage();
    requestThumbnailSync();
});



document.getElementById('btn-recalibrate').addEventListener('click', () => {
    isCalibrationMode = true;
    document.getElementById('calibration-overlay').classList.remove('hidden');
    document.getElementById('right-panel-blocker').classList.remove('hidden');
    if (current_geom.calibration_points) {
        calibrationPoints = JSON.parse(JSON.stringify(current_geom.calibration_points));
    } else {
        calibrationPoints = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    }
    requestAnimationFrame(updateCalibrationPolygon);
});

btnAutoColor.addEventListener('click', async () => {
    pushUndoState();
    await doAutoColor();
    requestThumbnailSync();
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
                    <button id="btn-confirm-cancel" class="px-4 py-2 text-xs font-bold tracking-wider uppercase bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded transition-colors">CANCEL</button>
                    <button id="btn-confirm-ok" class="px-4 py-2 text-xs font-bold tracking-wider uppercase bg-red-900/50 hover:bg-red-800/60 border border-red-700/50 text-white rounded transition-colors">RESET</button>
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
    const x = current_geom.crop_rect.x * 100, y = current_geom.crop_rect.y * 100;
    const w = current_geom.crop_rect.width * 100, h = current_geom.crop_rect.height * 100;

    cropBox.setAttribute('x', `${x}%`); cropBox.setAttribute('y', `${y}%`);
    cropBox.setAttribute('width', `${w}%`); cropBox.setAttribute('height', `${h}%`);

    const maskPath = `M0,0 H100% V100% H0 Z M${x}%,${y}% V${y + h}% H${x + w}% V${y}% Z`;
    cropMask.setAttribute('d', maskPath);

    document.getElementById('grid-v1').setAttribute('x1', `${x + w/3}%`); document.getElementById('grid-v1').setAttribute('x2', `${x + w/3}%`);
    document.getElementById('grid-v1').setAttribute('y1', `${y}%`); document.getElementById('grid-v1').setAttribute('y2', `${y + h}%`);
    document.getElementById('grid-v2').setAttribute('x1', `${x + w*2/3}%`); document.getElementById('grid-v2').setAttribute('x2', `${x + w*2/3}%`);
    document.getElementById('grid-v2').setAttribute('y1', `${y}%`); document.getElementById('grid-v2').setAttribute('y2', `${y + h}%`);
    document.getElementById('grid-h1').setAttribute('y1', `${y + h/3}%`); document.getElementById('grid-h1').setAttribute('y2', `${y + h/3}%`);
    document.getElementById('grid-h1').setAttribute('x1', `${x}%`); document.getElementById('grid-h1').setAttribute('x2', `${x + w}%`);
    document.getElementById('grid-h2').setAttribute('y1', `${y + h*2/3}%`); document.getElementById('grid-h2').setAttribute('y2', `${y + h*2/3}%`);
    document.getElementById('grid-h2').setAttribute('x1', `${x}%`); document.getElementById('grid-h2').setAttribute('x2', `${x + w}%`);

    const setHandle = (pos, hx, hy) => {
        const handle = cropHandles.querySelector(`[data-pos="${pos}"]`);
        if (handle) { handle.setAttribute('x', `${hx}%`); handle.setAttribute('y', `${hy}%`); }
    };
    setHandle('nw', x, y); setHandle('n', x + w/2, y); setHandle('ne', x + w, y);
    setHandle('w', x, y + h/2); setHandle('e', x + w, y + h/2);
    setHandle('sw', x, y + h); setHandle('s', x + w/2, y + h); setHandle('se', x + w, y + h);
}

let isDraggingCrop = false;
let dragType = null;
let dragStartPos = { x: 0, y: 0 };
let dragStartAngle = 0;
let dragCenter = { x: 0, y: 0 };

cropOverlay.addEventListener('mousedown', (e) => {
    if (!isCropMode && !isRotateMode) return;
    pushUndoState();
    const target = e.target;
    
    if (target === rotateHandleOuter || isRotateMode) dragType = 'rotate';
    else if (target === cropBox && isCropMode) dragType = 'box';
    else if (target.classList.contains('crop-handle') && isCropMode) dragType = target.getAttribute('data-pos');
    else return;
    
    isDraggingCrop = true; dragStartPos = { x: e.clientX, y: e.clientY };
    dragStartRect = { ...current_geom.crop_rect }; dragStartAngle = current_geom.angle;
    const rect = canvasWrapper.getBoundingClientRect();
    dragCenter = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
    cropGrid.style.opacity = '1';
});

window.addEventListener('mousemove', (e) => {
    if (!isDraggingCrop) return;
    const renderRect = getRenderRect();
    const dx = (e.clientX - dragStartPos.x) / renderRect.width;
    const dy = (e.clientY - dragStartPos.y) / renderRect.height;
    let newRect = { ...dragStartRect };

    if (dragType === 'box') {
        newRect.x = newRect.x + dx;
        newRect.y = newRect.y + dy;
    } else if (dragType === 'rotate') {
        const startRad = Math.atan2(dragStartPos.y - dragCenter.y, dragStartPos.x - dragCenter.x);
        const currentRad = Math.atan2(e.clientY - dragCenter.y, e.clientX - dragCenter.x);
        let deltaDeg = (currentRad - startRad) * (180 / Math.PI);
        if (e.shiftKey) deltaDeg *= 0.1;
        current_geom.angle = dragStartAngle - deltaDeg;
        requestRender();
        return;
    } else {
        if (dragType.includes('w')) {
            const maxW = newRect.x + newRect.width; newRect.x = maxW - newRect.width + dx; newRect.width = newRect.width - dx;
        }
        if (dragType.includes('e')) { newRect.width = newRect.width + dx; }
        if (dragType.includes('n')) {
            const maxH = newRect.y + newRect.height; newRect.y = maxH - newRect.height + dy; newRect.height = newRect.height - dy;
        }
        if (dragType.includes('s')) { newRect.height = newRect.height + dy; }
        newRect.width = Math.max(0.01, newRect.width);
        newRect.height = Math.max(0.01, newRect.height);
    }
    current_geom.crop_rect = newRect; updateCropOverlay();
});

window.addEventListener('mouseup', async () => {
    if (isDraggingCrop) {
        isDraggingCrop = false; cropGrid.style.opacity = '0';
        if (activeId) {
            try {
                await invoke('update_geometry', { id: activeId, geom: current_geom });
                requestThumbnailSync();
            } catch (err) { showToast("Crop failed: " + err, "error"); }
        }
    }
});

const btnCopySettings = document.getElementById('btn-copy-settings');
const btnPasteSettings = document.getElementById('btn-paste-settings');
const btnWbEyedropper = document.getElementById('btn-wb-eyedropper');

btnCopySettings.addEventListener('click', () => {
    if (!activeId) return;
    copiedSettings = saveCurrentState();
    if (copiedSettings) {
        btnPasteSettings.disabled = false;
        showToast("Settings copied.", "success");
    }
});

btnPasteSettings.addEventListener('click', () => {
    if (!activeId || !copiedSettings) return;
    pushUndoState();
    updateUIFromParams(copiedSettings, current_geom);
    updateBackendParams();
    requestRender();
    showToast("Settings pasted.", "success");
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
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    
    // map click coordinate to proxy image space
    const px = Math.floor((x / rect.width) * proxyWidth);
    const py = Math.floor((y / rect.height) * proxyHeight);
    
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
        
        let dR = -Math.log10(tR) - currentBaseDensity[0];
        let dG = -Math.log10(tG) - currentBaseDensity[1];
        let dB = -Math.log10(tB) - currentBaseDensity[2];
        
        const currentExpG = parseFloat(sliders.expg.el.value);
        
        const targetExpR = (dG + currentExpG) - dR;
        const targetExpB = (dG + currentExpG) - dB;
        
        pushUndoState();
        
        sliders.expr.el.value = targetExpR;
        sliders.expb.el.value = targetExpB;
        
        sliders.expr.val.textContent = targetExpR.toFixed(3);
        sliders.expb.val.textContent = targetExpB.toFixed(3);
        
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
    if (!currentRollViewId) return;
    const btnExportContactSheet = document.getElementById('btn-export-contact-sheet');
    
    try {
        btnExportContactSheet.textContent = "GENERATING...";
        btnExportContactSheet.disabled = true;

        const currentRoll = allRolls.find(r => r.roll_id === currentRollViewId);
        const rollPaths = new Set(currentRoll.image_paths);
        const rollItems = allLibraryItems.filter(item => rollPaths.has(item.file_path.replace(/\\\\/g, '/')) || rollPaths.has(item.file_path));

        if (rollItems.length === 0) throw new Error("No images in roll");

        // 1. Determine frames_per_row and format flag
        let formatStr = currentRoll.format ? currentRoll.format.toLowerCase() : "135";
        let is120 = formatStr.includes("120");
        let frames_per_row = 6;
        let aspect = 2/3; // fallback 3:2

        if (is120) {
            if (formatStr.includes("6x4.5") || formatStr.includes("645")) { frames_per_row = 4; aspect = 3/4; }
            else if (formatStr.includes("6x6")) { frames_per_row = 4; aspect = 1; }
            else if (formatStr.includes("6x7")) { frames_per_row = 3; aspect = 6/7; }
            else if (formatStr.includes("6x9")) { frames_per_row = 2; aspect = 2/3; }
            else { frames_per_row = 3; aspect = 1; }
        }

        // 2. Pad with empty frames
        const totalRows = Math.ceil(rollItems.length / frames_per_row);
        const emptyFrames = (totalRows * frames_per_row) - rollItems.length;
        const totalImages = rollItems.length; // store original
        
        for (let i = 0; i < emptyFrames; i++) {
            rollItems.push({ isEmpty: true });
        }

        const rows = totalRows;
        
        // 3. Math for grid layout
        const canvasW = 3000;
        const outerMargin = 100;
        const hGap = is120 ? (canvasW * 0.02) : 0;
        const totalHGap = hGap * (frames_per_row - 1);
        const colWidth = (canvasW - outerMargin * 2 - totalHGap) / frames_per_row;
        const colHeight = colWidth * aspect;
        
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
            } else {
                // Load & Draw real image
                const img = new Image();
                await new Promise(res => {
                    img.onload = res;
                    img.onerror = res;
                    img.src = `data:image/jpeg;base64,${item.thumbnail_base64}`;
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
        const savedPath = await invoke('save_contact_sheet', { dataUrl });
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

document.getElementById('calibration-svg').addEventListener('pointerdown', (e) => {
    if (!isCalibrationMode) return;
    if (e.target.classList.contains('calib-handle')) {
        calibrationDragIdx = parseInt(e.target.dataset.idx);
        e.target.setPointerCapture(e.pointerId);
    }
});

let calibrationRAF = null;
window.addEventListener('pointermove', (e) => {
    if (isCalibrationMode && calibrationDragIdx !== -1) {
        const svgRect = document.getElementById('calibration-svg').getBoundingClientRect();
        let nx = (e.clientX - svgRect.left) / svgRect.width;
        let ny = (e.clientY - svgRect.top) / svgRect.height;
        nx = Math.max(0, Math.min(1, nx));
        ny = Math.max(0, Math.min(1, ny));
        calibrationPoints[calibrationDragIdx] = [nx, ny];
        
        if (!calibrationRAF) {
            calibrationRAF = requestAnimationFrame(() => {
                updateCalibrationPolygon();
                calibrationRAF = null;
            });
        }
    }
});

window.addEventListener('pointerup', (e) => {
    if (calibrationDragIdx !== -1) {
        if (e.target.classList && e.target.classList.contains('calib-handle')) {
            e.target.releasePointerCapture(e.pointerId);
        }
        calibrationDragIdx = -1;
    }
});

window.addEventListener('resize', () => {
    if (isCalibrationMode) updateCalibrationPolygon();
});

document.getElementById('btn-confirm-calibration').addEventListener('click', async () => {
    if (!activeId) return;
    pushUndoState();
    current_geom.calibration_points = JSON.parse(JSON.stringify(calibrationPoints));
    requestRender();
    saveCurrentState();
    await invoke('update_geometry', { id: activeId, geom: current_geom });
    isCalibrationMode = false;
    document.getElementById('calibration-overlay').classList.add('hidden');
    document.getElementById('right-panel-blocker').classList.add('hidden');
    showToast("Calibration applied.", "success");
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
    loadingOverlay.innerHTML = '<svg class="animate-spin h-10 w-10 text-orange-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path></svg><div class="tracking-widest text-sm uppercase font-bold text-zinc-300">LOADING DATABASE...</div>';
    document.body.appendChild(loadingOverlay);

    try {
        allRolls = await invoke('get_rolls');
        allLibraryItems = await invoke('get_filmstrip');
        await updateFilterSidebar();
    } catch(e) { console.error("Init Error", e); }

    await renderLibraryAndFilmstrip();
    if (loadingOverlay) loadingOverlay.remove();
});

const { listen } = window.__TAURI__.event;

let precacheToast = null;
listen('precache_progress', (event) => {
    const { total, current } = event.payload;
    if (total === 0) return;
    
    if (!precacheToast) {
        precacheToast = document.createElement('div');
        precacheToast.className = 'fixed bottom-4 right-4 bg-[#1C1C1E] border border-[#28282c] shadow-lg rounded p-4 z-50 flex flex-col gap-2 w-64';
        precacheToast.innerHTML = `
            <div class="text-[11px] font-bold tracking-wider text-zinc-300">IMPORTING ASSETS</div>
            <div class="w-full h-1 bg-zinc-800 rounded overflow-hidden">
                <div class="h-full bg-blue-500 transition-all duration-300" id="precache-bar" style="width: 0%"></div>
            </div>
            <div class="text-[10px] text-zinc-500" id="precache-text">0 / ${total}</div>
        `;
        document.body.appendChild(precacheToast);
    }
    
    const pct = (current / total) * 100;
    document.getElementById('precache-bar').style.width = `${pct}%`;
    document.getElementById('precache-text').textContent = `${current} / ${total}`;
    
    if (current >= total) {
        setTimeout(() => {
            if (precacheToast) {
                precacheToast.remove();
                precacheToast = null;
            }
        }, 1000);
    }
});

listen('thumbnail_updated', (event) => {
    const { id, thumbnail } = event.payload;
    const item = allLibraryItems.find(i => i.id === id);
    if (item) {
        item.thumbnail_base64 = thumbnail;
    }
    // Update any img elements showing this thumbnail
    document.querySelectorAll(`img[data-img-id="${id}"]`).forEach(img => {
        img.src = "data:image/jpeg;base64," + thumbnail;
    });
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

const handleDrop = async (event) => {
    document.getElementById('drag-overlay').classList.add('hidden');
    const paths = event.payload.paths || event.payload; // Tauri v2 uses payload.paths
    if (paths && paths.length > 0) {
        btnImport.textContent = 'Importing...';
        await invoke('import_images', { paths, isLoose: true, rollId: 'LOOSE_DEFAULT', isHistorical: false });
        allLibraryItems = await invoke('get_filmstrip');
        renderLibraryAndFilmstrip();
        btnImport.textContent = 'Import Roll';
        showToast(`Dropped ${paths.length} file(s)`, 'success');
    }
};

listen('tauri://file-drop', handleDrop);
listen('tauri://drag-drop', handleDrop);

document.getElementById('btn-export-roll').addEventListener('click', () => {
    if (!currentRollViewId) { showToast('No active roll.', 'error'); return; }
    const currentRoll = allRolls.find(r => r.roll_id === currentRollViewId);
    if (!currentRoll) return;
    const rollPaths = new Set(currentRoll.image_paths);
    const rollItems = allLibraryItems.filter(item => rollPaths.has(item.file_path.replace(/\\/g, '/')) || rollPaths.has(item.file_path));
    selectedLibraryIds.clear();
    rollItems.forEach(i => selectedLibraryIds.add(i.id));
    updateLibrarySelectionUI();
    exportModal.classList.remove('opacity-0', 'pointer-events-none');
    setTimeout(() => exportModalContent.classList.remove('scale-95'), 10);
});

let panX = 0, panY = 0, isPanning = false, startPanX = 0, startPanY = 0, isSpacePressed = false;
window.addEventListener('keydown', e => { if(e.code==='Space') isSpacePressed=true; });
window.addEventListener('keyup', e => { if(e.code==='Space') isSpacePressed=false; });

canvasWrapper.parentElement.addEventListener('mousedown', e => {
    if (isSprocketPickerActive) {
        if (!gl || !activeId) return;
        const rect = previewCanvas.getBoundingClientRect();
        const scaleX = previewCanvas.width / rect.width;
        const scaleY = previewCanvas.height / rect.height;
        const x = Math.floor((e.clientX - rect.left) * scaleX);
        const y = Math.floor((rect.bottom - e.clientY) * scaleY);
        if (x >= 0 && x < previewCanvas.width && y >= 0 && y < previewCanvas.height) {
            let ndc_x = (x / previewCanvas.width) * 2.0 - 1.0;
            let ndc_y = (y / previewCanvas.height) * 2.0 - 1.0;
            
            let aspect = previewCanvas.width / previewCanvas.height;
            let px = ndc_x * aspect;
            let py = ndc_y;
            
            let a = current_geom.angle * Math.PI / 180.0;
            if (!isCropMode && !isRotateMode) a = 0;
            let s = Math.sin(-a), c = Math.cos(-a);
            let rx = px * c - py * s;
            let ry = px * s + py * c;
            rx /= aspect;
            
            let base_u = (rx + 1.0) / 2.0;
            let base_v = (ry + 1.0) / 2.0;
            
            let vs_base_u = base_u;
            let vs_base_v = 1.0 - base_v;
            
            let tex_u = current_geom.crop_rect.x + vs_base_u * current_geom.crop_rect.width;
            let tex_v = current_geom.crop_rect.y + vs_base_v * current_geom.crop_rect.height;
            
            let pts = current_geom.calibration_points || [[0, 0], [1, 0], [1, 1], [0, 1]];
            let hMat = getHomography(pts);
            let w_homo = hMat[2]*tex_u + hMat[5]*tex_v + hMat[8];
            let raw_u = (hMat[0]*tex_u + hMat[3]*tex_v + hMat[6]) / w_homo;
            let raw_v = (hMat[1]*tex_u + hMat[4]*tex_v + hMat[7]) / w_homo;
            
            pushUndoState();
            currentSprocketUV = new Float32Array([raw_u, raw_v]);
            isSprocketPickerActive = false;
            canvasWrapper.parentElement.style.cursor = '';
            btnSprocketPicker.classList.remove('bg-zinc-600');
            requestRender();
        }
        return;
    }
    if (!activeId || isCropMode || isRotateMode || isCalibrationMode) return;
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
    if (!activeId || isCropMode || isRotateMode || isCalibrationMode) return;
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
    } else {
        navLibrary.textContent = 'LIBRARY';
        navDevelop.textContent = 'DEVELOP';
        navHistory.textContent = 'HISTORY FILMS';
    }
});
let isDarkTheme = true;
document.getElementById('menu-theme-toggle').addEventListener('click', () => {
    isDarkTheme = !isDarkTheme;
    document.getElementById('menu-theme-toggle').querySelector('span').textContent = isDarkTheme ? 'Theme: Dark' : 'Theme: Light';
    if(isDarkTheme) {
        document.body.style.backgroundColor = '#121214';
    } else {
        document.body.style.backgroundColor = '#f4f4f5';
    }
});
