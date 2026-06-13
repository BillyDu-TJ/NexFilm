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
const btnAutoCrop = document.getElementById('btn-auto-crop');
const btnAutoColor = document.getElementById('btn-auto-color');
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
    lutOpacity: { el: document.getElementById('lut-opacity'), val: document.getElementById('val-lut-opacity') }
};

const imageStates = new Map();
let copiedSettings = null;
let isEyedropperActive = false;
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
const HIST_W = 768;
const HIST_H = 256;

// Library Multi-Selection State
let allLibraryItems = [];
let selectedLibraryIds = new Set();

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
        btnDeselectAll.classList.remove('hidden');
    } else {
        btnExportDialog.disabled = true;
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
        if (v.name === viewName) {
            v.el.classList.remove('opacity-0', 'pointer-events-none');
            v.nav.classList.add('text-zinc-100', 'border-zinc-100');
            v.nav.classList.remove('text-zinc-500', 'border-transparent');
        } else {
            v.el.classList.add('opacity-0', 'pointer-events-none');
            v.nav.classList.remove('text-zinc-100', 'border-zinc-100');
            v.nav.classList.add('text-zinc-500', 'border-transparent');
        }
    });

    if (viewName === 'develop') {
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
        shadows: parseFloat(sliders.shadows.el.value)
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
            renderLibraryAndFilmstrip();
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
        invoke('update_tuning_parameters', { id: activeId, params }).catch(console.error);
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

    const mat3 STATUS_M = mat3(
        1.0197, -0.0052, 0.0131,
        0.0317, 0.8933, -0.0011,
        0.0091, 0.0521, 0.9712
    );

    void main() {
        if (v_texcoord.x < 0.0 || v_texcoord.x > 1.0 || v_texcoord.y < 0.0 || v_texcoord.y > 1.0) {
            outColor = vec4(0.0, 0.0, 0.0, 1.0);
            return;
        }

        uvec4 texel = texture(u_image, v_texcoord);
        
        float epsilon = 1e-6;
        float t_r = max(float(texel.r) / 65535.0, epsilon);
        float t_g = max(float(texel.g) / 65535.0, epsilon);
        float t_b = max(float(texel.b) / 65535.0, epsilon);
        
        // 1. 获取 Log 数据
        vec3 density = vec3(-log(t_r) / log(10.0), -log(t_g) / log(10.0), -log(t_b) / log(10.0));
        
        // 2. 片基与串扰
        if (u_mode == 0) {
            density = STATUS_M * (density - u_base_density);
        } else {
            density = density - u_base_density;
            float gray = (density.r + density.g + density.b) / 3.0;
            density = vec3(gray);
        }
        
        // 3. 曝光与色彩对齐
        if (u_mode == 0) {
            density += u_exposure;
        } else {
            density += vec3(u_exposure.r);
        }
        
        // 4. 对数域高光/阴影 (Log Tone Control) & 5. 归一化与数学截断
        vec3 norm = (density - u_dmin) / (u_dmax - u_dmin);
        
        norm = norm + u_shadows * pow(1.0 - clamp(norm, 0.0, 1.0), vec3(2.0)) * norm + u_highlights * pow(clamp(norm, 0.0, 1.0), vec3(2.0)) * (1.0 - norm);
        
        // 6. 应用 LUT
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
            // 7. 终端显示映射
            final_rgb = vec3(pow(clamp(norm.r, 0.0, 1.0), 1.0 / u_gamma), pow(clamp(norm.g, 0.0, 1.0), 1.0 / u_gamma), pow(clamp(norm.b, 0.0, 1.0), 1.0 / u_gamma));
        }
        
        outColor = vec4(final_rgb, 1.0);
    }`;

    function createShader(gl, type, source) {
        const shader = gl.createShader(type);
        gl.shaderSource(shader, source);
        gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) return null;
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

function updateDataViz(pixels) {
    lastPixels = pixels;
    if (isWaveform) drawWaveform(pixels);
    else drawHistogram(pixels);
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

async function loadProxyImage() {
    if (!activeId || !webGLInitialized) return;
    try {
        const result = await invoke('get_proxy_image_data', { id: activeId });
        let arrayBuffer;
        let byteOffset = 0;
        if (result instanceof ArrayBuffer) {
            arrayBuffer = result;
        } else if (result.buffer instanceof ArrayBuffer) {
            arrayBuffer = result.buffer;
            byteOffset = result.byteOffset || 0;
        } else if (Array.isArray(result)) {
            arrayBuffer = new Uint8Array(result).buffer;
        }

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
    } catch(e) { console.error("Failed to load proxy", e); }
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
    btnAutoCrop.disabled = false;
    document.getElementById('btn-reset-crop').disabled = false;
    btnAutoColor.disabled = false;
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

async function renderLibraryAndFilmstrip() {
    try {
        await fetchRolls();
        const items = await invoke('get_filmstrip');
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
                historyTitle.textContent = "ROLL CONTENTS";
                
                const currentRoll = allRolls.find(r => r.roll_id === currentRollViewId);
                const rollPaths = currentRoll ? new Set(currentRoll.image_paths) : new Set();
                const rollItems = items.filter(item => rollPaths.has(item.file_path.replace(/\\\\/g, '/')) || rollPaths.has(item.file_path));
                
                rollItems.forEach(item => {
                    const libDiv = document.createElement('div');
                    libDiv.className = `library-item rounded overflow-hidden relative ${selectedLibraryIds.has(item.id) ? 'selected' : ''}`;
                    libDiv.dataset.id = item.id;
                    libDiv.ondblclick = () => {
                        selectedLibraryIds.clear();
                        selectedLibraryIds.add(item.id);
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
                    if (item.thumbnail_base64 === "FILE_MISSING") {
                        libImg.src = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" fill="none" stroke="#ff0000" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>`);
                        libImg.className = 'w-full h-full object-contain opacity-50 bg-[#1C1C1E] p-4 pointer-events-none';
                    } else {
                        libImg.src = `data:image/jpeg;base64,${item.thumbnail_base64}`;
                        libImg.className = 'w-full h-full object-cover pointer-events-none';
                    }
                    libDiv.appendChild(libImg);
                    historyInternalGrid.appendChild(libDiv);
                });
            } else {
                // Rolls Archive View
                libraryRollsGrid.classList.remove('hidden');
                historyInternalGrid.classList.add('hidden');
                btnHistoryBack.classList.add('hidden');
                btnExportContactSheet.classList.add('hidden');
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
                
                filteredRolls.forEach(roll => {
                    const rollPaths = new Set(roll.image_paths);
                    let thumbSrc = '';
                    const firstItem = items.find(item => rollPaths.has(item.file_path.replace(/\\\\/g, '/')) || rollPaths.has(item.file_path));
                    if (firstItem) {
                        if (firstItem.thumbnail_base64 === "FILE_MISSING") {
                            thumbSrc = "data:image/svg+xml;base64," + btoa(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" fill="none" stroke="#ff0000" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>`);
                        } else {
                            thumbSrc = `data:image/jpeg;base64,${firstItem.thumbnail_base64}`;
                        }
                    }
                    
                    const card = document.createElement('div');
                    card.className = "group relative bg-[#1C1C1E] rounded-lg overflow-hidden cursor-pointer hover:border-zinc-500 transition-all duration-300 flex h-[200px] shadow-lg w-full";
                    
                    if (isDeleteMode && selectedRollIds.has(roll.roll_id)) {
                        card.style.border = "2px solid #ef4444";
                    } else {
                        card.style.border = "1px solid #28282c";
                    }

                    card.onclick = () => {
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
                });
            }
        }
        
        // --- Populate DEVELOP Filmstrip ---
        items.forEach(item => {
            const stripDiv = document.createElement('div');
            stripDiv.className = `film-item shrink-0 ${item.id === activeId ? 'active' : ''}`;
            stripDiv.onclick = () => {
                selectImage(item.id);
                selectedLibraryIds.clear();
                selectedLibraryIds.add(item.id);
                updateLibrarySelectionUI();
            };
            const stripImg = document.createElement('img');
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

document.getElementById('btn-history-back').addEventListener('click', () => {
    currentRollViewId = null;
    selectedLibraryIds.clear();
    renderLibraryAndFilmstrip();
});

async function selectImage(id) {
    if (activeId === id) return;
    try {
        saveCurrentState(); // Save current state before switching

        let state;
        try {
            if (imageStates.has(id)) {
                state = imageStates.get(id);
                await invoke('switch_active_image', { id });
            } else {
                state = await invoke('switch_active_image', { id });
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
        renderLibraryAndFilmstrip();
        
        enableUI();
        current_geom = JSON.parse(JSON.stringify(state.geom));
        updateUIFromParams(state.params, current_geom);
        updateCropOverlay();

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

        await loadProxyImage();
        requestRender(); // Force uniform update
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
            await invoke('import_images', { paths });
            await fetchRolls();
            await renderLibraryAndFilmstrip();
            
            if (allLibraryItems && allLibraryItems.length > 0) {
                const newPath = paths[0];
                const newPhoto = allLibraryItems.find(i => i.file_path === newPath || i.file_path.replace(/\\\\/g, '/') === newPath);
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
    
    try {
        const paths = await invoke('open_file_dialog');
        if (paths && paths.length > 0) {
            await invoke('append_to_roll', { rollId, paths });
            currentRollViewId = rollId;
            await renderLibraryAndFilmstrip();
            
            if (allLibraryItems && allLibraryItems.length > 0) {
                const newPath = paths[0];
                const newPhoto = allLibraryItems.find(i => i.file_path === newPath || i.file_path.replace(/\\\\/g, '/') === newPath);
                if (newPhoto) {
                    await selectImage(newPhoto.id);
                    switchView('develop');
                }
            }
            showToast("Images appended to roll.", "success");
        }
    } catch(e) {
        showToast("Failed to continue roll: " + e, "error");
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
        
        const outputDir = await invoke('select_export_dir');
        if (!outputDir) {
            btnConfirmExport.textContent = "Select Output Folder";
            btnConfirmExport.disabled = false;
            return;
        }
        closeExportModal();
        const count = await invoke('batch_export_images', { outputDir, format, colorSpace });
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
    dummyPusher.style.display = 'none';

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
    if (!activeId || !lastHistPixels) return;

    let r_arr = []; let g_arr = []; let b_arr = [];
    for (let i = 0; i < lastHistPixels.length; i += 4) {
        r_arr.push(lastHistPixels[i]);
        g_arr.push(lastHistPixels[i+1]);
        b_arr.push(lastHistPixels[i+2]);
    }

    r_arr.sort((a,b)=>a-b);
    g_arr.sort((a,b)=>a-b);
    b_arr.sort((a,b)=>a-b);

    let start = Math.floor(r_arr.length * 0.01);
    let end = Math.floor(r_arr.length * 0.99);

    const gammaVal = parseFloat(sliders.gamma.el.value);

    function toDensity(val, channelDmin, channelDmax) {
        let norm = Math.pow(val / 255.0, gammaVal);
        return norm * (channelDmax - channelDmin) + channelDmin;
    }

    const batchState = {
        dmin: [
            toDensity(r_arr[start], currentDMin[0], currentDMax[0]),
            toDensity(g_arr[start], currentDMin[1], currentDMax[1]),
            toDensity(b_arr[start], currentDMin[2], currentDMax[2])
        ],
        dmax: [
            toDensity(r_arr[end], currentDMin[0], currentDMax[0]),
            toDensity(g_arr[end], currentDMin[1], currentDMax[1]),
            toDensity(b_arr[end], currentDMin[2], currentDMax[2])
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

btnAutoCrop.addEventListener('click', async () => {
    if (!activeId) return; pushUndoState();
    try {
        const result = await invoke('geometry_auto_align', { id: activeId });
        current_geom.crop_rect = result.crop_rect; current_geom.angle = result.angle;
        updateCropOverlay(); await loadProxyImage(); requestThumbnailSync();
    } catch (err) { showToast("Auto failed: " + err, "error"); }
});

btnAutoColor.addEventListener('click', async () => {
    pushUndoState();
    await doAutoColor();
    requestThumbnailSync();
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

        // 5. Draw images
        for (let i = 0; i < rollItems.length; i++) {
            const item = rollItems[i];
            const r = Math.floor(i / frames_per_row);
            const c = i % frames_per_row;
            
            const x = outerMargin + c * (colWidth + hGap);
            const y = outerMargin + r * (rowHeightTotal + vGap);
            
            // Draw pure black borders
            ctx.fillStyle = '#000000';
            ctx.fillRect(x, y, colWidth, borderH); // Top
            ctx.fillRect(x, y + borderH + colHeight, colWidth, borderH); // Bottom
            
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
            ctx.font = 'bold 12px "Helvetica Neue Extended", "Helvetica Neue", Arial, sans-serif';
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
                ctx.fillText(filmName, x + colWidth/2, y + borderH * 0.35);
                
                // Bottom text (frame num)
                ctx.fillText(`${i+1}`, x + colWidth*0.25, y + borderH + colHeight + borderH * 0.65);
                ctx.fillText(`${i+1}A`, x + colWidth*0.75, y + borderH + colHeight + borderH * 0.65);
                
            } else {
                // --- 120 Procedural ---
                ctx.textAlign = 'center';
                // Top text
                ctx.fillText(filmName, x + colWidth/2, y + borderH * 0.5);
                // Bottom text (bold with arrow)
                ctx.font = 'bold 20px "Helvetica Neue Extended", "Helvetica Neue", Arial, sans-serif';
                ctx.fillText(`◄ ${i+1}`, x + colWidth/2, y + borderH + colHeight + borderH * 0.5);
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
        ctx.fillText(`Order #${currentRoll.roll_id} | ${currentRoll.camera || 'Unknown Camera'} | ${totalImages} images (${emptyFrames} empty)`, canvasW - outerMargin, footerY + 10);

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
    current_geom.calibration_points = calibrationPoints;
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
