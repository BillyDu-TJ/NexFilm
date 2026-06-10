const invoke = window.__TAURI__.core.invoke;

// DOM
const btnImport = document.getElementById('btn-import');
const btnExportBatch = document.getElementById('btn-export-batch');
const filmstripContainer = document.getElementById('filmstrip-container');
const previewCanvas = document.getElementById('preview-canvas');
const ctx = previewCanvas.getContext('2d');
const placeholder = document.getElementById('placeholder');

const sliders = {
    dmin: { el: document.getElementById('dmin'), val: document.getElementById('val-dmin') },
    dmax: { el: document.getElementById('dmax'), val: document.getElementById('val-dmax') },
    exposure: { el: document.getElementById('exposure'), val: document.getElementById('val-exposure') },
    gamma: { el: document.getElementById('gamma'), val: document.getElementById('val-gamma') },
    expr: { el: document.getElementById('expr'), val: document.getElementById('val-expr') },
    expg: { el: document.getElementById('expg'), val: document.getElementById('val-expg') },
    expb: { el: document.getElementById('expb'), val: document.getElementById('val-expb') },
};

let activeId = null;

// CSS Variable updater for Track
function updateSliderTrack(el) {
    const min = parseFloat(el.min);
    const max = parseFloat(el.max);
    const val = parseFloat(el.value);
    const percent = ((val - min) / (max - min)) * 100;
    el.style.setProperty('--val', `${percent}%`);
}

// Throttled UI logic
const applyTuning = _.throttle(async () => {
    if (!activeId) return;
    
    const params = {
        d_min: parseFloat(sliders.dmin.el.value),
        d_max: parseFloat(sliders.dmax.el.value),
        exposure: parseFloat(sliders.exposure.el.value),
        gamma: parseFloat(sliders.gamma.el.value),
        exp_r: parseFloat(sliders.expr.el.value),
        exp_g: parseFloat(sliders.expg.el.value),
        exp_b: parseFloat(sliders.expb.el.value)
    };

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
    } catch (e) {
        console.error("Error applying tuning:", e);
    }
}, 32, { leading: true, trailing: true });

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
        s.val.textContent = parseFloat(s.el.value).toFixed(3);
        updateSliderTrack(s.el);
    }
}

for (const key in sliders) {
    const s = sliders[key];
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
    placeholder.style.display = 'none';
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
        const params = await invoke('switch_active_image', { id });
        activeId = id;
        
        // Quick UI update for filmstrip active state
        document.querySelectorAll('.film-item').forEach(el => el.classList.remove('active'));
        renderFilmstrip();
        
        enableUI();
        updateUIFromParams(params);
        applyTuning(); // Trigger render for this image
    } catch(e) {
        console.error("Select image error:", e);
    }
}

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
        alert("Import failed: " + e);
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
        alert(`Successfully exported ${count} image(s) to:\n${outputDir}`);
    } catch (e) {
        alert("Batch export failed: " + e);
    } finally {
        btnExportBatch.textContent = "Batch Export";
        btnExportBatch.disabled = false;
    }
});

// Init tracks
for (const key in sliders) {
    updateSliderTrack(sliders[key].el);
}
