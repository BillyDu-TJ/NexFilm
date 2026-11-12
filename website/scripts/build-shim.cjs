const fs = require('fs');
const path = require('path');

const thumbs = JSON.parse(fs.readFileSync(path.join(__dirname, 'generated-thumbs.json'), 'utf8'));

const shimContent = `/**
 * NexFilm Web Shim for Interactive Workspace Sandbox
 * Emulates Tauri backend APIs (__TAURI__.core.invoke, convertFileSrc, listen)
 * Provides offline demo data, WebGL proxy texture with authentic C-41 / B&W physics,
 * real film strip thumbnails, and .cube 3D/1D LUT parsing.
 */
(() => {
  'use strict';

  // Preserve WebGL drawing buffer for headless testing and external readbacks
  const origGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (contextType, contextAttributes) {
    if (contextType === 'webgl2' || contextType === 'webgl') {
      contextAttributes = Object.assign({}, contextAttributes, { preserveDrawingBuffer: true });
    }
    return origGetContext.call(this, contextType, contextAttributes);
  };

  // 1. Language & Theme Synchronization from URL Search Params
  const searchParams = new URLSearchParams(window.location.search);
  const langParam = searchParams.get('lang');
  if (langParam === 'zh' || langParam === 'zh-CN') {
    localStorage.setItem('nexfilm-language', 'zh-CN');
  } else if (langParam === 'en') {
    localStorage.setItem('nexfilm-language', 'en');
  }

  // Darkroom dark theme by default
  const savedTheme = localStorage.getItem('nexfilm-theme');
  if (!savedTheme) {
    localStorage.setItem('nexfilm-theme', 'dark');
  }
  document.documentElement.dataset.theme = localStorage.getItem('nexfilm-theme') || 'dark';

  const isZh = (localStorage.getItem('nexfilm-language') || 'zh-CN').startsWith('zh');

  function showPreviewNotice(messageZh, messageEn) {
    const text = isZh ? messageZh : messageEn;
    if (typeof window.showToast === 'function') {
      window.showToast(text, 'info');
    } else {
      console.info('[NexFilm Preview Notice]', text);
    }
  }

  // 2. High Quality Photographic Base64 Thumbnails
  const demoThumbnails = {
    frame_portra_01: ${JSON.stringify(thumbs.portra)},
    frame_portra_02: ${JSON.stringify(thumbs.portra)},
    frame_portra_03: ${JSON.stringify(thumbs.superia)},
    frame_portra_04: ${JSON.stringify(thumbs.vision3)},
    frame_hp5_01: ${JSON.stringify(thumbs.hp5)},
    frame_hp5_02: ${JSON.stringify(thumbs.hp5)},
    frame_vision3_01: ${JSON.stringify(thumbs.vision3)},
    frame_vision3_02: ${JSON.stringify(thumbs.vision3)}
  };

  // 3. Demo Data: 3 Distinct Rolls with different formats & films
  const demoRolls = [
    {
      roll_id: 'roll_portra_400',
      name: 'Kodak Portra 400 · 135',
      film_stock: 'Kodak Portra 400',
      format: '135',
      camera: 'Leica M6',
      date: '2026-10-15',
      notes: 'Developed in CineStill Cs41 liquid kit @ 38°C (3.5 min). Scanned with Sony A7R IV + 90mm Macro.',
      image_paths: [
        '/demo/portra_01_street.dng',
        '/demo/portra_02_portrait.dng',
        '/demo/portra_03_architecture.dng',
        '/demo/portra_04_still_life.dng'
      ],
      density_anchors: {
        d_min_base: { density: [0.16, 0.44, 0.76], scope: 'Roll', source: 'SampledFilmBase' },
        d_max_full_exposure: { density: [1.86, 2.14, 2.46], scope: 'Roll', source: 'SampledLeader' }
      },
      calibration_profile_id: 'cal_led_panel_01',
      scanner_profile_id: 'profile_imacon_848'
    },
    {
      roll_id: 'roll_hp5_120',
      name: 'Ilford HP5 Plus · 120 6x7',
      film_stock: 'Ilford HP5 Plus',
      format: '120 (6x7)',
      camera: 'Mamiya 7 II',
      date: '2026-10-22',
      notes: 'Stand development in Rodinal 1:100 (60 min). Gentle inversions first 60s.',
      image_paths: [
        '/demo/hp5_01_mountains.dng',
        '/demo/hp5_02_alley.dng'
      ],
      density_anchors: {
        d_min_base: { density: [0.22, 0.22, 0.22], scope: 'Roll', source: 'SampledFilmBase' },
        d_max_full_exposure: { density: [2.10, 2.10, 2.10], scope: 'Roll', source: 'SampledLeader' }
      },
      calibration_profile_id: null,
      scanner_profile_id: null
    },
    {
      roll_id: 'roll_vision3_500t',
      name: 'Kodak Vision3 500T · 135',
      film_stock: 'Kodak Vision3 500T',
      format: '135',
      camera: 'Nikon FM2',
      date: '2026-10-28',
      notes: 'ECN-2 process with PB-1 pre-bath remjet wash @ 41°C. Rated at EI 400.',
      image_paths: [
        '/demo/vision3_01_night.dng',
        '/demo/vision3_02_neon.dng'
      ],
      density_anchors: {
        d_min_base: { density: [0.16, 0.44, 0.76], scope: 'Roll', source: 'SampledFilmBase' },
        d_max_full_exposure: { density: [1.86, 2.14, 2.46], scope: 'Roll', source: 'SampledLeader' }
      },
      calibration_profile_id: null,
      scanner_profile_id: null
    }
  ];

  // Demo Library items
  const demoItems = [
    {
      id: 'frame_portra_01',
      roll_id: 'roll_portra_400',
      file_path: '/demo/portra_01_street.dng',
      format: '135',
      camera: 'Leica M6',
      date: '2026-10-15',
      status: 'done',
      in_working_set: true,
      state_available: true,
      thumbnail_base64: demoThumbnails.frame_portra_01,
      embedded_thumbnail_base64: demoThumbnails.frame_portra_01,
      rendered_thumbnail_base64: demoThumbnails.frame_portra_01,
      thumbnail_kind: 'rendered'
    },
    {
      id: 'frame_portra_02',
      roll_id: 'roll_portra_400',
      file_path: '/demo/portra_02_portrait.dng',
      format: '135',
      camera: 'Leica M6',
      date: '2026-10-15',
      status: 'done',
      in_working_set: true,
      state_available: true,
      thumbnail_base64: demoThumbnails.frame_portra_02,
      embedded_thumbnail_base64: demoThumbnails.frame_portra_02,
      rendered_thumbnail_base64: demoThumbnails.frame_portra_02,
      thumbnail_kind: 'rendered'
    },
    {
      id: 'frame_portra_03',
      roll_id: 'roll_portra_400',
      file_path: '/demo/portra_03_architecture.dng',
      format: '135',
      camera: 'Leica M6',
      date: '2026-10-15',
      status: 'done',
      in_working_set: true,
      state_available: true,
      thumbnail_base64: demoThumbnails.frame_portra_03,
      embedded_thumbnail_base64: demoThumbnails.frame_portra_03,
      rendered_thumbnail_base64: demoThumbnails.frame_portra_03,
      thumbnail_kind: 'rendered'
    },
    {
      id: 'frame_portra_04',
      roll_id: 'roll_portra_400',
      file_path: '/demo/portra_04_still_life.dng',
      format: '135',
      camera: 'Leica M6',
      date: '2026-10-15',
      status: 'done',
      in_working_set: true,
      state_available: true,
      thumbnail_base64: demoThumbnails.frame_portra_04,
      embedded_thumbnail_base64: demoThumbnails.frame_portra_04,
      rendered_thumbnail_base64: demoThumbnails.frame_portra_04,
      thumbnail_kind: 'rendered'
    },
    {
      id: 'frame_hp5_01',
      roll_id: 'roll_hp5_120',
      file_path: '/demo/hp5_01_mountains.dng',
      format: '120 (6x7)',
      camera: 'Mamiya 7 II',
      date: '2026-10-22',
      status: 'done',
      in_working_set: false,
      state_available: true,
      thumbnail_base64: demoThumbnails.frame_hp5_01,
      embedded_thumbnail_base64: demoThumbnails.frame_hp5_01,
      rendered_thumbnail_base64: demoThumbnails.frame_hp5_01,
      thumbnail_kind: 'rendered'
    },
    {
      id: 'frame_hp5_02',
      roll_id: 'roll_hp5_120',
      file_path: '/demo/hp5_02_alley.dng',
      format: '120 (6x7)',
      camera: 'Mamiya 7 II',
      date: '2026-10-22',
      status: 'done',
      in_working_set: false,
      state_available: true,
      thumbnail_base64: demoThumbnails.frame_hp5_02,
      embedded_thumbnail_base64: demoThumbnails.frame_hp5_02,
      rendered_thumbnail_base64: demoThumbnails.frame_hp5_02,
      thumbnail_kind: 'rendered'
    },
    {
      id: 'frame_vision3_01',
      roll_id: 'roll_vision3_500t',
      file_path: '/demo/vision3_01_night.dng',
      format: '135',
      camera: 'Nikon FM2',
      date: '2026-10-28',
      status: 'done',
      in_working_set: false,
      state_available: true,
      thumbnail_base64: demoThumbnails.frame_vision3_01,
      embedded_thumbnail_base64: demoThumbnails.frame_vision3_01,
      rendered_thumbnail_base64: demoThumbnails.frame_vision3_01,
      thumbnail_kind: 'rendered'
    },
    {
      id: 'frame_vision3_02',
      roll_id: 'roll_vision3_500t',
      file_path: '/demo/vision3_02_neon.dng',
      format: '135',
      camera: 'Nikon FM2',
      date: '2026-10-28',
      status: 'done',
      in_working_set: false,
      state_available: true,
      thumbnail_base64: demoThumbnails.frame_vision3_02,
      embedded_thumbnail_base64: demoThumbnails.frame_vision3_02,
      rendered_thumbnail_base64: demoThumbnails.frame_vision3_02,
      thumbnail_kind: 'rendered'
    }
  ];

  // Calibration and Scanner Profiles
  const demoCalibrationProfiles = [
    {
      profile: {
        profile_id: 'cal_led_panel_01',
        name: 'Kaiser Slimlite Plano 5000K (CRI 95+)',
        created_at: '2026-09-01T12:00:00Z',
        calibration_level: 'basic',
        camera: 'Sony A7R IV',
        light_source: 'CRI 95+ Flat Panel LED',
        lens: 'FE 90mm f/2.8 Macro G',
        notes: 'Calibrated with dark frame and open gate flat field',
        references: []
      },
      availability: 'available',
      warnings: [],
      dark_frame: null,
      flat_field: null,
      channel_matrix: [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0]
      ]
    }
  ];

  const demoScannerProfiles = [
    {
      profile: {
        profile_id: 'profile_imacon_848',
        manufacturer: 'Hasselblad',
        model: 'Flextight 848',
        verified: true
      }
    }
  ];

  const builtinLutList = [
    'assets/luts/Kodak 2383.cube',
    'assets/luts/Kodak 2393.cube',
    'assets/luts/Kodak Ektacolor Edge.cube',
    'assets/luts/Kodak Endura Premier.cube',
    'assets/luts/Kodak Portra Endura.cube',
    'assets/luts/Kodak Supra Endura.cube',
    'assets/luts/Kodak Ultra Endura.cube',
    'assets/luts/Ilford MGFB 00.cube',
    'assets/luts/Ilford MGFB 0.cube',
    'assets/luts/Ilford MGFB 1.cube',
    'assets/luts/Ilford MGFB 2.cube',
    'assets/luts/Ilford MGFB 3.cube',
    'assets/luts/Ilford MGFB 4.cube',
    'assets/luts/Ilford MGFB 5.cube'
  ];

  // 4. Photographic Proxy Buffer Generation (28-byte Header + RGBA16UI)
  // Maps sample images to physical optical transmission T_c = 10^(-D_c), where D_c = D_base + L_c * span
  const proxyBlobCache = new Map();

  function createProxyBufferFromImageData(imgData, isBw = false) {
    const width = imgData.width;
    const height = imgData.height;
    const headerSize = 28;
    const pixelByteLength = width * height * 4 * 2;
    const buffer = new ArrayBuffer(headerSize + pixelByteLength);
    const view = new DataView(buffer);

    const dBase = isBw ? [0.22, 0.22, 0.22] : [0.16, 0.44, 0.76];
    const span = isBw ? [1.88, 1.88, 1.88] : [1.70, 1.70, 1.70];

    // 28-byte Header:
    // [0..3]: width (uint32)
    view.setUint32(0, width, true);
    // [4..7]: height (uint32)
    view.setUint32(4, height, true);
    // [8..19]: base_density (float32 x 3)
    view.setFloat32(8, dBase[0], true);
    view.setFloat32(12, dBase[1], true);
    view.setFloat32(16, dBase[2], true);
    // [20..23]: isFullProxy (uint32) -> 1
    view.setUint32(20, 1, true);
    // [24..27]: flags (uint32) -> bit 0 (baseAnalyzed = 1) | bit 2 (relative_transmission_rgb = 4) = 5
    view.setUint32(24, 1 | 4, true);

    const pixels = new Uint16Array(buffer, headerSize, width * height * 4);
    const pd = imgData.data;

    for (let i = 0; i < pd.length; i += 4) {
      const lr = Math.pow(pd[i] / 255.0, 2.2);
      const lg = Math.pow(pd[i + 1] / 255.0, 2.2);
      const lb = Math.pow(pd[i + 2] / 255.0, 2.2);

      if (isBw) {
        const luma = 0.299 * lr + 0.587 * lg + 0.114 * lb;
        const d = dBase[0] + span[0] * luma;
        const t = Math.pow(10, -d);
        const val = Math.min(65535, Math.max(1, Math.round(t * 65535)));
        pixels[i + 0] = val;
        pixels[i + 1] = val;
        pixels[i + 2] = val;
        pixels[i + 3] = 65535;
      } else {
        const dr = dBase[0] + span[0] * lr;
        const dg = dBase[1] + span[1] * lg;
        const db = dBase[2] + span[2] * lb;
        const tr = Math.pow(10, -dr);
        const tg = Math.pow(10, -dg);
        const tb = Math.pow(10, -db);
        pixels[i + 0] = Math.min(65535, Math.max(1, Math.round(tr * 65535)));
        pixels[i + 1] = Math.min(65535, Math.max(1, Math.round(tg * 65535)));
        pixels[i + 2] = Math.min(65535, Math.max(1, Math.round(tb * 65535)));
        pixels[i + 3] = 65535;
      }
    }

    const blob = new Blob([buffer], { type: 'application/octet-stream' });
    return URL.createObjectURL(blob);
  }

  // Pre-generate synchronous photographic fallback buffer (640x400)
  function createProceduralFallback(isBw = false) {
    const width = 640;
    const height = 400;
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext('2d');

    // Draw rich photographic scene: warm sky, architectural silhouettes, cobblestone ground
    const grad = ctx.createLinearGradient(0, 0, width, height);
    if (isBw) {
      grad.addColorStop(0, '#e8e8ea');
      grad.addColorStop(0.5, '#727278');
      grad.addColorStop(1, '#18181c');
    } else {
      grad.addColorStop(0, '#ffe8cc');
      grad.addColorStop(0.4, '#c89268');
      grad.addColorStop(0.7, '#6c4a38');
      grad.addColorStop(1, '#242028');
    }
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, width, height);

    // Warm sun / highlight glow
    const sunGrad = ctx.createRadialGradient(width * 0.72, height * 0.28, 10, width * 0.72, height * 0.28, 180);
    sunGrad.addColorStop(0, isBw ? 'rgba(255,255,255,0.95)' : 'rgba(255,248,220,0.95)');
    sunGrad.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = sunGrad;
    ctx.beginPath();
    ctx.arc(width * 0.72, height * 0.28, 180, 0, Math.PI * 2);
    ctx.fill();

    // Architecture / foliage silhouettes
    ctx.fillStyle = isBw ? '#202024' : '#281c16';
    ctx.beginPath();
    ctx.moveTo(0, height);
    ctx.lineTo(0, height * 0.52);
    ctx.bezierCurveTo(width * 0.2, height * 0.48, width * 0.35, height * 0.58, width * 0.5, height * 0.50);
    ctx.bezierCurveTo(width * 0.7, height * 0.42, width * 0.85, height * 0.54, width, height * 0.46);
    ctx.lineTo(width, height);
    ctx.closePath();
    ctx.fill();

    const imgData = ctx.getImageData(0, 0, width, height);
    return createProxyBufferFromImageData(imgData, isBw);
  }

  const defaultColorBlob = createProceduralFallback(false);
  const defaultBwBlob = createProceduralFallback(true);

  // Frame to gallery asset mapping
  const frameToGallery = {
    frame_portra_01: '/gallery/portra-400-pos.webp',
    frame_portra_02: '/gallery/portra-400-pos.webp',
    frame_portra_03: '/gallery/superia-400-pos.webp',
    frame_portra_04: '/gallery/vision3-500t-pos.webp',
    frame_hp5_01: '/gallery/hp5-plus-400-pos.webp',
    frame_hp5_02: '/gallery/hp5-plus-400-pos.webp',
    frame_vision3_01: '/gallery/vision3-500t-pos.webp',
    frame_vision3_02: '/gallery/vision3-500t-pos.webp'
  };

  // Asynchronously load real WebP gallery samples and generate high-fidelity 640x400 proxy buffers
  function loadRealProxyTextures() {
    const offCanvas = document.createElement('canvas');
    offCanvas.width = 640;
    offCanvas.height = 400;
    const ctx = offCanvas.getContext('2d', { willReadFrequently: true });

    for (const [frameId, assetUrl] of Object.entries(frameToGallery)) {
      const isBw = frameId.includes('hp5');
      const img = new Image();
      img.onload = () => {
        ctx.clearRect(0, 0, 640, 400);
        ctx.drawImage(img, 0, 0, 640, 400);
        const imgData = ctx.getImageData(0, 0, 640, 400);
        const blobUrl = createProxyBufferFromImageData(imgData, isBw);
        proxyBlobCache.set(frameId, blobUrl);
        const item = demoItems.find(it => it.id === frameId);
        if (item && item.file_path) {
          proxyBlobCache.set(item.file_path, blobUrl);
        }
      };
      img.src = assetUrl;
    }
  }

  // Start background decode immediately
  loadRealProxyTextures();

  function getProxyBlobUrl(key) {
    if (key && proxyBlobCache.has(key)) {
      return proxyBlobCache.get(key);
    }
    const isBw = typeof key === 'string' && key.includes('hp5');
    return isBw ? defaultBwBlob : defaultColorBlob;
  }

  // 5. Shared Default Development Parameters for Active Image
  const activeParamsState = {
    exposure: 0.0,
    gamma: 1.0,
    contrast: 0.0,
    saturation: 0.0,
    temperature: 0.0,
    tint: 0.0,
    exp_r: 0.0,
    exp_g: 0.0,
    exp_b: 0.0,
    highlights: 0.0,
    shadows: 0.0,
    d_min: [0.16, 0.44, 0.76],
    d_max: [1.86, 2.14, 2.46],
    d_min_offset: 0.0,
    d_max_offset: 0.0,
    film_mode: 'Color',
    lut_path: 'assets/luts/Kodak Portra Endura.cube',
    lut_opacity: 0.75,
    working_colorspace: 'linear-srgb',
    curve_rgb: [[0, 0], [1, 1]],
    curve_red: [[0, 0], [1, 1]],
    curve_green: [[0, 0], [1, 1]],
    curve_blue: [[0, 0], [1, 1]],
    sharpening_amount: 25,
    sharpening_radius: 1.0,
    sharpening_detail: 20,
    sharpening_masking: 0,
    texture: 0,
    clarity: 0,
    dehaze: 0,
    sprocket_uv: [-1.0, -1.0],
    sprocket_tolerance: 0.10,
    sprocket_feather: 0.05
  };

  const activeGeomState = {
    crop_rect: { x: 0.03, y: 0.03, width: 0.94, height: 0.94 },
    angle: 0.0,
    flip_h: false,
    flip_v: false,
    rotate_90_count: 0,
    calibration_points: [[0.03, 0.03], [0.97, 0.03], [0.97, 0.97], [0.03, 0.97]],
    calibration_confirmed: true
  };

  // 6. Mock Tauri Core and Events
  const mockInvoke = async (command, args = {}) => {
    switch (command) {
      case 'get_rolls':
        return JSON.parse(JSON.stringify(demoRolls));

      case 'get_filmstrip':
        return JSON.parse(JSON.stringify(demoItems));

      case 'get_roll_filmstrip': {
        const rollId = args.rollId;
        const matching = demoItems.filter(item => item.roll_id === rollId);
        return JSON.parse(JSON.stringify(matching));
      }

      case 'get_roll_previews': {
        const rollId = args.rollId;
        const matching = demoItems.filter(item => item.roll_id === rollId);
        return matching.map(item => item.thumbnail_base64 || item.rendered_thumbnail_base64);
      }

      case 'promote_roll':
        return true;

      case 'get_calibration_profiles':
        return JSON.parse(JSON.stringify(demoCalibrationProfiles));

      case 'get_scanner_profiles':
        return JSON.parse(JSON.stringify(demoScannerProfiles));

      case 'get_builtin_luts':
        return builtinLutList.slice();

      case 'load_3d_lut': {
        const lutPath = args.path;
        try {
          const res = await fetch(lutPath);
          if (!res.ok) throw new Error(\`HTTP \${res.status}\`);
          const content = await res.text();

          let size3d = 0;
          let size1d = 0;
          const dataFloats = [];

          const lines = content.split('\\n');
          for (let i = 0; i < lines.length; i++) {
            const line = lines[i].trim();
            if (!line || line.startsWith('#')) continue;
            if (line.startsWith('LUT_3D_SIZE')) {
              const parts = line.split(/\\s+/);
              if (parts.length >= 2) size3d = parseInt(parts[1], 10);
            } else if (line.startsWith('LUT_1D_SIZE')) {
              const parts = line.split(/\\s+/);
              if (parts.length >= 2) size1d = parseInt(parts[1], 10);
            } else if (line.startsWith('DOMAIN_MIN') || line.startsWith('DOMAIN_MAX') || line.startsWith('TITLE')) {
              continue;
            } else {
              const parts = line.split(/\\s+/);
              if (parts.length >= 3) {
                const r = parseFloat(parts[0]);
                const g = parseFloat(parts[1]);
                const b = parseFloat(parts[2]);
                if (!Number.isNaN(r) && !Number.isNaN(g) && !Number.isNaN(b)) {
                  dataFloats.push(r, g, b);
                }
              }
            }
          }

          let maxVal = 0;
          for (let i = 0; i < dataFloats.length; i++) {
            if (dataFloats[i] > maxVal) maxVal = dataFloats[i];
          }
          if (maxVal > 1.0) {
            for (let i = 0; i < dataFloats.length; i++) {
              dataFloats[i] /= 1023.0;
            }
          }

          let finalSize = size3d;
          let is1d = false;
          if (size1d > 0 && size3d === 0) {
            finalSize = size1d;
            is1d = true;
          }

          const rgbaFloats = new Float32Array((dataFloats.length / 3) * 4);
          let dst = 0;
          for (let i = 0; i < dataFloats.length; i += 3) {
            rgbaFloats[dst++] = dataFloats[i];
            rgbaFloats[dst++] = dataFloats[i + 1];
            rgbaFloats[dst++] = dataFloats[i + 2];
            rgbaFloats[dst++] = 1.0;
          }

          return {
            size: finalSize,
            is_1d: is1d,
            data: Array.from(new Uint8Array(rgbaFloats.buffer))
          };
        } catch (err) {
          console.debug('[Tauri Shim load_3d_lut fallback]', err);
          const size = 2;
          const floatData = new Float32Array(size * size * size * 4);
          let ptr = 0;
          for (let b = 0; b < size; b++) {
            for (let g = 0; g < size; g++) {
              for (let r = 0; r < size; r++) {
                floatData[ptr++] = r / (size - 1);
                floatData[ptr++] = g / (size - 1);
                floatData[ptr++] = b / (size - 1);
                floatData[ptr++] = 1.0;
              }
            }
          }
          return {
            size,
            is_1d: false,
            data: Array.from(new Uint8Array(floatData.buffer))
          };
        }
      }

      case 'switch_active_image': {
        const targetId = args.id || 'frame_portra_01';
        const isBw = targetId.includes('hp5');
        const dBase = isBw ? [0.22, 0.22, 0.22] : [0.16, 0.44, 0.76];
        const dMax = isBw ? [2.10, 2.10, 2.10] : [1.86, 2.14, 2.46];
        const span = isBw ? [1.88, 1.88, 1.88] : [1.70, 1.70, 1.70];

        activeParamsState.d_min = dBase.slice();
        activeParamsState.d_max = dMax.slice();
        activeParamsState.film_mode = isBw ? 'BW' : 'Color';

        return {
          params: { ...activeParamsState },
          geom: { ...activeGeomState },
          pipeline_state: {
            contract: 'capture_corrected_v11',
            density_anchors: {
              d_min_base: { density: dBase.slice(), scope: 'Roll', source: 'SampledFilmBase' },
              d_max_full_exposure: { density: dMax.slice(), scope: 'Roll', source: 'SampledLeader' }
            },
            render_mapping: {
              mode: 'roll_anchored',
              density_low: [0.0, 0.0, 0.0],
              density_high: span.slice()
            }
          },
          developed: true,
          base_analyzed: true
        };
      }

      case 'auto_detect_film_border':
        return {
          crop_rect: { x: 0.04, y: 0.04, width: 0.92, height: 0.92 },
          angle: 0.0,
          flip_h: false,
          flip_v: false,
          rotate_90_count: 0
        };

      case 'analyze_proxy_base_color':
        return [0.16, 0.44, 0.76];

      case 'analyze_proxy_density_limits':
        return {
          d_min: [0.16, 0.44, 0.76],
          d_max: [1.86, 2.14, 2.46]
        };

      case 'apply_film_base':
        return true;

      case 'update_tuning_parameters':
        if (args.params) Object.assign(activeParamsState, args.params);
        return true;

      case 'update_geometry':
        if (args.geom) Object.assign(activeGeomState, args.geom);
        return true;

      case 'sync_thumbnail_buffer':
      case 'set_thumbnail_data':
      case 'clear_invalid_rendered_thumbnail':
      case 'reset_image_development':
        return true;

      case 'batch_copy_settings':
        showPreviewNotice('参数已同步至选中底片', 'Settings copied to selected frames.');
        return { applied_count: 3 };

      case 'auto_invert_roll':
        showPreviewNotice('整卷批量反转处理完成 (4 帧)', 'Roll batch inversion complete (4 frames).');
        return { total: 4, processed: 4, succeeded: 4, failed: 0, done: true };

      case 'save_contact_sheet':
        showPreviewNotice('接触印样已生成预览', 'Contact sheet generated.');
        return 'NexFilm_ContactSheet_Demo.jpg';

      case 'batch_export_images':
        showPreviewNotice('当前为网页交互预览，下载桌面版即可无损导出 16-bit TIFF/DNG', 'Web demo: Download desktop app for 16-bit TIFF/DNG export.');
        return { exported_count: 4 };

      case 'open_file_dialog':
      case 'open_lut_dialog':
      case 'choose_scanner_profile_file':
      case 'choose_calibration_reference':
      case 'locate_missing_file':
        showPreviewNotice('当前为网页交互预览，下载桌面版即可导入与导出本地 RAW 原片', 'Web demo: Download desktop app to import & export local RAW files.');
        return null;

      case 'select_export_dir':
        return '/Users/apple/Pictures/NexFilm Exports';

      case 'get_user_cameras':
        return ['Leica M6', 'Mamiya 7 II', 'Nikon FM2', 'Hasselblad 500C/M'];

      case 'get_user_films':
        return ['Kodak Portra 400', 'Ilford HP5 Plus', 'Kodak Vision3 500T', 'Fujifilm Superia 400'];

      case 'prepare_proxy':
        return true;

      default:
        console.debug('[Tauri Shim invoke fallback]', command, args);
        return null;
    }
  };

  const mockConvertFileSrc = (filePath, protocol) => {
    if (protocol === 'nexfilm-proxy') {
      return getProxyBlobUrl(filePath);
    }
    return protocol + '://localhost/' + encodeURIComponent(filePath);
  };

  const mockListen = async (eventName, callback) => {
    return () => {};
  };

  // Expose global window.__TAURI__ before other application scripts load
  window.__TAURI__ = {
    core: {
      invoke: mockInvoke,
      convertFileSrc: mockConvertFileSrc
    },
    event: {
      listen: mockListen
    }
  };

  // 7. Auto-activate first roll into working set editing mode on startup
  window.addEventListener('load', () => {
    setTimeout(async () => {
      try {
        const continueBtn = document.getElementById('btn-continue-roll');
        const select = document.getElementById('continue-roll-select');
        const confirmBtn = document.getElementById('btn-confirm-continue');
        if (continueBtn && select && confirmBtn) {
          continueBtn.click();
          select.value = 'roll_portra_400';
          confirmBtn.click();
        }
        await new Promise(r => setTimeout(r, 120));
        // Reset historyRollViewId to null so Rolls overview grid is rendered with all 3 demo rolls
        const backBtn = document.getElementById('btn-history-back');
        if (backBtn) {
          backBtn.click();
        }
        await new Promise(r => setTimeout(r, 80));
        const firstFilm = document.querySelector('#filmstrip-container .film-item');
        if (firstFilm) {
          firstFilm.click();
        }
      } catch (err) {
        console.debug('[NexFilm AutoFocus]', err);
      }
    }, 180);
  });

  // Ensure clicking Develop view automatically selects first frame if none active
  window.addEventListener('DOMContentLoaded', () => {
    const navDev = document.getElementById('nav-develop');
    if (navDev) {
      navDev.addEventListener('click', () => {
        setTimeout(() => {
          const activeItem = document.querySelector('#filmstrip-container .film-item.active');
          if (!activeItem) {
            const firstItem = document.querySelector('#filmstrip-container .film-item');
            if (firstItem) firstItem.click();
          }
        }, 50);
      });
    }
  });
})();
`;

fs.writeFileSync(path.join(__dirname, 'tauri-web-shim.js'), shimContent);
console.log('Successfully generated website/scripts/tauri-web-shim.js');
