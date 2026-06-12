const invoke = window.__TAURI__.core.invoke;

// DOM: Global
const btnImport = document.getElementById('btn-import');
const btnExportDialog = document.getElementById('btn-export-dialog');
const btnImportTrigger = document.querySelector('.btn-import-trigger');
const toastContainer = document.getElementById('toast-container');

// DOM: Navigation & Views
const navLibrary = document.getElementById('nav-library');
const navDevelop = document.getElementById('nav-develop');
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

const sliders = {
    dmin: { el: document.getElementById('dmin'), val: document.getElementById('val-dmin') },
    dmax: { el: document.getElementById('dmax'), val: document.getElementById('val-dmax') },
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

let isWaveform = false;
let lastPixels = null;
const HIST_SIZE = 256;

// Library Multi-Selection State
let allLibraryItems = [];
let selectedLibraryIds = new Set();

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
    if (viewName === 'library') {
        viewDevelop.classList.add('opacity-0', 'pointer-events-none');
        viewLibrary.classList.remove('opacity-0', 'pointer-events-none');
        
        navLibrary.classList.add('text-zinc-100', 'border-zinc-100');
        navLibrary.classList.remove('text-zinc-500', 'border-transparent');
        
        navDevelop.classList.add('text-zinc-500', 'border-transparent');
        navDevelop.classList.remove('text-zinc-100', 'border-zinc-100');
    } else {
        viewLibrary.classList.add('opacity-0', 'pointer-events-none');
        viewDevelop.classList.remove('opacity-0', 'pointer-events-none');
        
        navDevelop.classList.add('text-zinc-100', 'border-zinc-100');
        navDevelop.classList.remove('text-zinc-500', 'border-transparent');
        
        navLibrary.classList.add('text-zinc-500', 'border-transparent');
        navLibrary.classList.remove('text-zinc-100', 'border-zinc-100');
        
        requestRender();
    }
}

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
        d_min: parseFloat(sliders.dmin.el.value),
        d_max: parseFloat(sliders.dmax.el.value),
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
        d_min: parseFloat(sliders.dmin.el.value),
        d_max: parseFloat(sliders.dmax.el.value),
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
let u_lut_is_1d_loc;
let u_image_loc;
let u_aspect_loc;
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
    uniform float u_dmin;
    uniform float u_dmax;
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
        
        vec3 clamped_norm = clamp(norm, 0.0, 1.0);
        norm = norm + u_shadows * pow(1.0 - clamped_norm, vec3(2.0)) * norm + u_highlights * pow(clamped_norm, vec3(2.0)) * (1.0 - norm);
        
        // 进入 LUT 前，必须强制绝杀溢出
        norm = clamp(norm, 0.0, 1.0);
        
        // 6. 应用 LUT
        vec3 final_rgb;
        if (u_has_lut == 1) {
            vec3 lut_color;
            if (u_lut_is_1d == 1) {
                lut_color.r = texture(u_lut1d, vec2(clamp(norm.r, 0.0, 1.0), 0.5)).r;
                lut_color.g = texture(u_lut1d, vec2(clamp(norm.g, 0.0, 1.0), 0.5)).g;
                lut_color.b = texture(u_lut1d, vec2(clamp(norm.b, 0.0, 1.0), 0.5)).b;
            } else {
                lut_color = texture(u_lut3d, clamp(norm, 0.0, 1.0)).rgb;
            }
            final_rgb = mix(vec3(pow(norm.r, 1.0 / u_gamma), pow(norm.g, 1.0 / u_gamma), pow(norm.b, 1.0 / u_gamma)), lut_color, u_lut_opacity);
        } else {
            // 7. 终端显示映射
            final_rgb = vec3(pow(norm.r, 1.0 / u_gamma), pow(norm.g, 1.0 / u_gamma), pow(norm.b, 1.0 / u_gamma));
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
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, HIST_SIZE, HIST_SIZE, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
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
    waveCanvas.width = waveCanvas.offsetWidth;
    waveCanvas.height = waveCanvas.offsetHeight;
    const w = waveCanvas.width, h = waveCanvas.height;
    
    waveCtx.clearRect(0, 0, w, h);
    waveCtx.globalCompositeOperation = 'screen';
    
    // Draw Red
    waveCtx.fillStyle = 'rgba(255, 0, 0, 0.1)';
    for (let y = 0; y < HIST_SIZE; y+=2) {
        for (let x = 0; x < HIST_SIZE; x+=2) {
            const idx = (y * HIST_SIZE + x) * 4;
            const r = pixels[idx];
            const plotX = (x / HIST_SIZE) * w;
            const plotY_R = h - (r / 255.0) * h;
            waveCtx.fillRect(plotX, plotY_R, 2, 2);
        }
    }
    
    // Draw Green
    waveCtx.fillStyle = 'rgba(0, 255, 0, 0.1)';
    for (let y = 0; y < HIST_SIZE; y+=2) {
        for (let x = 0; x < HIST_SIZE; x+=2) {
            const idx = (y * HIST_SIZE + x) * 4;
            const g = pixels[idx+1];
            const plotX = (x / HIST_SIZE) * w;
            const plotY_G = h - (g / 255.0) * h;
            waveCtx.fillRect(plotX, plotY_G, 2, 2);
        }
    }
    
    // Draw Blue
    waveCtx.fillStyle = 'rgba(0, 150, 255, 0.1)';
    for (let y = 0; y < HIST_SIZE; y+=2) {
        for (let x = 0; x < HIST_SIZE; x+=2) {
            const idx = (y * HIST_SIZE + x) * 4;
            const b = pixels[idx+2];
            const plotX = (x / HIST_SIZE) * w;
            const plotY_B = h - (b / 255.0) * h;
            waveCtx.fillRect(plotX, plotY_B, 2, 2);
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
    const dminVal = parseFloat(sliders.dmin.el.value);
    const dmaxVal = parseFloat(sliders.dmax.el.value);
    const expVal = parseFloat(sliders.exposure.el.value);
    const exprVal = parseFloat(sliders.expr.el.value);
    const expgVal = parseFloat(sliders.expg.el.value);
    const expbVal = parseFloat(sliders.expb.el.value);
    const gammaVal = parseFloat(sliders.gamma.el.value);

    gl.uniform3f(u_base_density_loc, currentBaseDensity[0], currentBaseDensity[1], currentBaseDensity[2]);
    gl.uniform1f(u_dmin_loc, dminVal);
    gl.uniform1f(u_dmax_loc, dmaxVal);
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
    gl.viewport(0, 0, HIST_SIZE, HIST_SIZE);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
    
    const pixels = new Uint8Array(HIST_SIZE * HIST_SIZE * 4);
    gl.readPixels(0, 0, HIST_SIZE, HIST_SIZE, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    lastHistPixels = pixels;

    // Render to Main Canvas
    if (!isCropMode) {
        gl.uniform4f(u_crop_loc, current_geom.crop_rect.x, current_geom.crop_rect.y, current_geom.crop_rect.width, current_geom.crop_rect.height);
    } else {
        gl.uniform4f(u_crop_loc, 0.0, 0.0, 1.0, 1.0);
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

function updateUIFromParams(params, geom) {
    sliders.dmin.el.value = params.d_min;
    sliders.dmax.el.value = params.d_max;
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
    btnAutoColor.disabled = false;
    btnRotateLeft.disabled = false;
    btnRotateRight.disabled = false;
    btnFlipH.disabled = false;
    btnFlipV.disabled = false;
    
    document.getElementById('btn-copy-settings').disabled = false;
    if (copiedSettings) document.getElementById('btn-paste-settings').disabled = false;
    document.getElementById('btn-wb-eyedropper').disabled = false;
    
    canvasWrapper.style.display = 'block';
}

async function renderLibraryAndFilmstrip() {
    try {
        const items = await invoke('get_filmstrip');
        allLibraryItems = items;
        libraryGrid.innerHTML = '';
        filmstripContainer.innerHTML = '';
        
        if (items.length === 0) {
            libraryEmpty.classList.remove('hidden');
            libraryGrid.classList.add('hidden');
            btnSelectAll.classList.add('hidden');
            return;
        }
        
        libraryEmpty.classList.add('hidden');
        libraryGrid.classList.remove('hidden');
        btnSelectAll.classList.remove('hidden');
        
        // ensure selectedLibraryIds only contains valid items
        const validIds = new Set(items.map(i => i.id));
        for (const id of selectedLibraryIds) {
            if (!validIds.has(id)) selectedLibraryIds.delete(id);
        }
        
        if (selectedLibraryIds.size === 0 && items.length > 0) {
            selectedLibraryIds.add(items[0].id); // auto select first
        }
        
        updateLibrarySelectionUI();
        
        items.forEach(item => {
            // Library View Grid Item
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
            libImg.src = `data:image/jpeg;base64,${item.thumbnail_base64}`;
            libImg.className = 'w-full h-full object-cover pointer-events-none';
            
            const filenameLabel = document.createElement('div');
            filenameLabel.className = 'absolute bottom-0 left-0 w-full bg-black/60 backdrop-blur-sm text-[10px] text-zinc-300 p-1.5 truncate text-center pointer-events-none';
            filenameLabel.textContent = item.file_path.split(/[\\/]/).pop();
            
            libDiv.appendChild(libImg);
            libDiv.appendChild(filenameLabel);
            libraryGrid.appendChild(libDiv);

            // Develop View Filmstrip Item
            const stripDiv = document.createElement('div');
            stripDiv.className = `film-item shrink-0 ${item.id === activeId ? 'active' : ''}`;
            stripDiv.onclick = () => {
                selectImage(item.id);
                selectedLibraryIds.clear();
                selectedLibraryIds.add(item.id);
                updateLibrarySelectionUI();
            };
            
            const stripImg = document.createElement('img');
            stripImg.src = `data:image/jpeg;base64,${item.thumbnail_base64}`;
            stripImg.className = 'w-full h-full object-cover rounded-[2px] pointer-events-none';
            
            stripDiv.appendChild(stripImg);
            filmstripContainer.appendChild(stripDiv);
        });
    } catch (e) { console.error("Filmstrip error:", e); }
}

async function selectImage(id) {
    if (activeId === id) return;
    try {
        saveCurrentState(); // Save current state before switching

        let state;
        if (imageStates.has(id)) {
            state = imageStates.get(id);
            await invoke('switch_active_image', { id });
        } else {
            state = await invoke('switch_active_image', { id });
            imageStates.set(id, { params: state.params, geom: state.geom || { crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0.0, flip_h: false, flip_v: false, rotate_90_count: 0 } });
        }

        activeId = id;
        renderLibraryAndFilmstrip();
        
        enableUI();
        current_geom = JSON.parse(JSON.stringify(state.geom));
        updateUIFromParams(state.params, current_geom);
        updateCropOverlay();
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

const doImport = async () => {
    try {
        btnImport.textContent = "Importing...";
        btnImport.disabled = true;
        if (btnImportTrigger) {
            btnImportTrigger.textContent = "Importing...";
            btnImportTrigger.disabled = true;
        }
        
        console.log("Calling open_file_dialog...");
        const paths = await invoke('open_file_dialog');
        console.log("open_file_dialog returned:", paths);
        
        if (paths && paths.length > 0) {
            await invoke('import_images', { paths });
            const items = await invoke('get_filmstrip');
            if (items.length > 0) {
                if (!activeId) {
                    await selectImage(items[0].id);
                    switchView('library');
                } else {
                    await renderLibraryAndFilmstrip();
                }
            }
        }
    } catch (e) { 
        console.error("Import error:", e);
        showToast("Import failed: " + e, "error"); 
    } 
    finally {
        btnImport.textContent = "Import Roll";
        btnImport.disabled = false;
        if (btnImportTrigger) {
            btnImportTrigger.textContent = "Import From Disk";
            btnImportTrigger.disabled = false;
        }
    }
};

btnImport.addEventListener('click', doImport);
btnImportTrigger.addEventListener('click', doImport);

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

    if (isCropMode || isRotateMode) {
        canvasWrapper.style.aspectRatio = `${cw} / ${ch}`;
        dummyPusher.width = cw;
        dummyPusher.height = ch;
        
        previewCanvas.style.width = '100%';
        previewCanvas.style.height = '100%';
        previewCanvas.style.left = '0';
        previewCanvas.style.top = '0';
        
        cropOverlay.classList.remove('hidden');
        if (isCropMode) {
            updateCropOverlay();
        }
    } else {
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

btnRotateLeft.addEventListener('click', async () => {
    if (!activeId) return; pushUndoState();
    current_geom.rotate_90_count -= 1;
    current_geom.crop_rect = updateCropRectForRotation(current_geom.crop_rect, false, current_geom.flip_h, current_geom.flip_v);
    await invoke('update_geometry', { id: activeId, geom: current_geom }); await loadProxyImage(); requestThumbnailSync();
});

btnRotateRight.addEventListener('click', async () => {
    if (!activeId) return; pushUndoState();
    current_geom.rotate_90_count += 1;
    current_geom.crop_rect = updateCropRectForRotation(current_geom.crop_rect, true, current_geom.flip_h, current_geom.flip_v);
    await invoke('update_geometry', { id: activeId, geom: current_geom }); await loadProxyImage(); requestThumbnailSync();
});

btnFlipH.addEventListener('click', async () => {
    if (!activeId) return; pushUndoState(); current_geom.flip_h = !current_geom.flip_h;
    current_geom.crop_rect.x = 1.0 - current_geom.crop_rect.x - current_geom.crop_rect.width;
    await invoke('update_geometry', { id: activeId, geom: current_geom }); await loadProxyImage(); requestThumbnailSync();
});

btnFlipV.addEventListener('click', async () => {
    if (!activeId) return; pushUndoState(); current_geom.flip_v = !current_geom.flip_v;
    current_geom.crop_rect.y = 1.0 - current_geom.crop_rect.y - current_geom.crop_rect.height;
    await invoke('update_geometry', { id: activeId, geom: current_geom }); await loadProxyImage(); requestThumbnailSync();
});

async function doAutoColor() {
    if (!activeId || !lastHistPixels) return;

    let r_arr = [];
    let g_arr = [];
    let b_arr = [];

    for (let i = 0; i < lastHistPixels.length; i += 4) {
        r_arr.push(lastHistPixels[i]);
        g_arr.push(lastHistPixels[i+1]);
        b_arr.push(lastHistPixels[i+2]);
    }

    r_arr.sort((a,b)=>a-b);
    g_arr.sort((a,b)=>a-b);
    b_arr.sort((a,b)=>a-b);

    let start = Math.floor(r_arr.length * 0.1);
    let end = Math.floor(r_arr.length * 0.9);
    let count = end - start;
    if (count <= 0) return;

    let r_core = r_arr.slice(start, end);
    let g_core = g_arr.slice(start, end);
    let b_core = b_arr.slice(start, end);

    let mid = Math.floor(count / 2);
    let r_med = r_core[mid];
    let g_med = g_core[mid];
    let b_med = b_core[mid];

    const dminVal = parseFloat(sliders.dmin.el.value);
    const dmaxVal = parseFloat(sliders.dmax.el.value);
    const gammaVal = parseFloat(sliders.gamma.el.value);

    // Convert 8-bit back to normalized density
    function toDensity(val) {
        let norm = Math.pow(val / 255.0, gammaVal);
        return norm * (dmaxVal - dminVal) + dminVal;
    }

    let density_R = toDensity(r_med);
    let density_G = toDensity(g_med);
    let density_B = toDensity(b_med);

    let diff_R = density_G - density_R;
    let diff_B = density_G - density_B;

    let current_expr = parseFloat(sliders.expr.el.value);
    let current_expb = parseFloat(sliders.expb.el.value);

    let new_expr = current_expr + diff_R;
    let new_expb = current_expb + diff_B;

    document.getElementById('expr').value = new_expr;
    document.getElementById('val-expr').innerText = new_expr.toFixed(3);
    document.getElementById('expb').value = new_expb;
    document.getElementById('val-expb').innerText = new_expb.toFixed(3);

    document.getElementById('expr').dispatchEvent(new Event('input'));
    document.getElementById('expb').dispatchEvent(new Event('input'));
    requestRender();
}

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
        
        // Allow continuous rotation without clamping for slider
        current_geom.angle = newAngle;
        
        requestRender(); // real-time rotate rendering via uniforms
        return;
    } else {
        if (dragType.includes('w')) {
            const maxW = newRect.x + newRect.width; newRect.x = Math.max(0, Math.min(maxW - 0.05, newRect.x + dx)); newRect.width = maxW - newRect.x;
        }
        if (dragType.includes('e')) { newRect.width = Math.max(0.05, Math.min(1 - newRect.x, newRect.width + dx)); }
        if (dragType.includes('n')) {
            const maxH = newRect.y + newRect.height; newRect.y = Math.max(0, Math.min(maxH - 0.05, newRect.y + dy)); newRect.height = maxH - newRect.y;
        }
        if (dragType.includes('s')) { newRect.height = Math.max(0.05, Math.min(1 - newRect.y, newRect.height + dy)); }
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
