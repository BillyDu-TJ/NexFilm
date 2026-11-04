// In-vitro reproduction of the washed-canvas state: renders the proxy buffer
// built from the user's persisted frame state (target/diag-washed/proxy.bin,
// produced by `cargo test --lib washed_open_state_diagnostic -- --ignored`)
// through the real develop shader with exactly the uniforms the app would set,
// then reports the resulting cast next to the known-correct render.
const childProcess = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const root = path.resolve(__dirname, '..');
const washDir = path.join(root, 'target', 'diag-washed');
const proxyBytes = fs.readFileSync(path.join(washDir, 'proxy.bin'));
const uniforms = JSON.parse(fs.readFileSync(path.join(washDir, 'uniforms.json'), 'utf8'));

function extractTemplate(name) {
    const frontend = fs.readFileSync(path.join(root, 'ui', 'main.js'), 'utf8');
    const marker = `const ${name} = \``;
    const start = frontend.indexOf(marker);
    if (start < 0) throw new Error(`Missing ${name} in ui/main.js`);
    const contentStart = start + marker.length;
    const end = frontend.indexOf('`;', contentStart);
    if (end < 0) throw new Error(`Unterminated ${name}`);
    return frontend.slice(contentStart, end);
}

const vertex = extractTemplate('vsSource');
const fragment = extractTemplate('fsSource');

// Parse the proxy exactly like loadProxyImage does.
const header = new DataView(proxyBytes.buffer, proxyBytes.byteOffset, proxyBytes.byteLength);
const width = header.getUint32(0, true);
const height = header.getUint32(4, true);
const flags = header.getUint32(24, true);
const domain = (flags & 4) !== 0
    ? 'relative_transmission_rgb'
    : (flags & 2) !== 0 ? 'prophoto_estimate' : 'legacy_linear_srgb';
const allPixels = new Uint16Array(proxyBytes.buffer, proxyBytes.byteOffset + 28, width * height * 4);
// Half-resolution nearest subsample keeps the cast statistics exact while the
// payload stays small enough for a headless --dump-dom snapshot.
const subW = Math.floor(width / 2);
const subH = Math.floor(height / 2);
const pixels = new Uint16Array(subW * subH * 4);
for (let y = 0; y < subH; y += 1) {
    for (let x = 0; x < subW; x += 1) {
        const src = ((y * 2) * width + (x * 2)) * 4;
        const dst = (y * subW + x) * 4;
        pixels[dst] = allPixels[src];
        pixels[dst + 1] = allPixels[src + 1];
        pixels[dst + 2] = allPixels[src + 2];
        pixels[dst + 3] = allPixels[src + 3];
    }
}
console.log(`[WASH] parsed header: ${width}x${height} flags=${flags} domain=${domain} (rendered at ${subW}x${subH})`);

const chromeCandidates = [
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
];
const browser = chromeCandidates.find(fs.existsSync);
if (!browser) throw new Error('Chrome or Edge was not found');

const payload = JSON.stringify({ vertex, fragment, width: subW, height: subH, uniforms, domain, flags });
if (payload.includes('</script')) throw new Error('payload contains a script terminator');
const pixelBase64 = Buffer.from(pixels.buffer, pixels.byteOffset, pixels.byteLength).toString('base64');

const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nexfilm-wash-'));
const htmlPath = path.join(tempDir, 'wash.html');
const html = `<!doctype html><meta charset="utf-8"><canvas id="c" width="${subW}" height="${subH}"></canvas>
<script type="application/json" id="spec">${payload}</script>
<script type="application/json" id="pixels">${JSON.stringify(pixelBase64)}</script>
<pre id="result">pending</pre><script>
window.addEventListener('error', (event) => {
    document.getElementById('result').textContent = JSON.stringify({ ok: false, log: 'page: ' + event.message + ' @' + (event.lineno || 0) });
});
const spec = JSON.parse(document.getElementById('spec').textContent);
const pixelB64 = JSON.parse(document.getElementById('pixels').textContent);
const bin = atob(pixelB64);
const pixels = new Uint16Array(bin.length / 2);
for (let i = 0; i < pixels.length; i += 1) {
    pixels[i] = bin.charCodeAt(i * 2) | (bin.charCodeAt(i * 2 + 1) << 8);
}
const resultElement = document.getElementById('result');
function finish(result) { resultElement.textContent = JSON.stringify(result); }
const gl = document.getElementById('c').getContext('webgl2', { preserveDrawingBuffer: true });
if (!gl) {
    finish({ ok: false, log: 'WebGL2 unavailable' });
} else {
    try {
        const compile = (type, source) => {
            const shader = gl.createShader(type);
            gl.shaderSource(shader, source);
            gl.compileShader(shader);
            if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
                throw new Error(gl.getShaderInfoLog(shader) || 'compile failed');
            }
            return shader;
        };
        const program = gl.createProgram();
        gl.attachShader(program, compile(gl.VERTEX_SHADER, spec.vertex));
        gl.attachShader(program, compile(gl.FRAGMENT_SHADER, spec.fragment));
        gl.linkProgram(program);
        if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
            throw new Error(gl.getProgramInfoLog(program) || 'link failed');
        }
        gl.useProgram(program);

        const vao = gl.createVertexArray();
        gl.bindVertexArray(vao);
        const positionLoc = gl.getAttribLocation(program, 'a_position');
        const posBuffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, posBuffer);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);
        gl.enableVertexAttribArray(positionLoc);
        gl.vertexAttribPointer(positionLoc, 2, gl.FLOAT, false, 0, 0);
        const texcoordLoc = gl.getAttribLocation(program, 'a_texcoord');
        const texBuffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, texBuffer);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1]), gl.STATIC_DRAW);
        gl.enableVertexAttribArray(texcoordLoc);
        gl.vertexAttribPointer(texcoordLoc, 2, gl.FLOAT, false, 0, 0);

        const texture = gl.createTexture();
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, texture);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16UI, spec.width, spec.height, 0, gl.RGBA_INTEGER, gl.UNSIGNED_SHORT, pixels);
        const textureError = gl.getError();
        if (textureError !== gl.NO_ERROR) throw new Error('texture upload failed: ' + textureError);
        gl.uniform1i(gl.getUniformLocation(program, 'u_image'), 0);

        const placeholder = new Uint8Array([0, 0, 0, 255]);
        const lut1d = gl.createTexture();
        gl.activeTexture(gl.TEXTURE1);
        gl.bindTexture(gl.TEXTURE_2D, lut1d);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, placeholder);
        gl.uniform1i(gl.getUniformLocation(program, 'u_lut1d'), 1);
        const lut3d = gl.createTexture();
        gl.activeTexture(gl.TEXTURE2);
        gl.bindTexture(gl.TEXTURE_3D, lut3d);
        gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGBA8, 1, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, placeholder);
        gl.uniform1i(gl.getUniformLocation(program, 'u_lut3d'), 2);

        const u = spec.uniforms;
        const loc = (name) => gl.getUniformLocation(program, name);
        gl.uniform1i(loc('u_mode'), 0);
        gl.uniform1i(loc('u_invert_enabled'), 1);
        gl.uniform1i(loc('u_legacy_pipeline'), u.legacy ? 1 : 0);
        gl.uniform1i(loc('u_capture_corrected_pipeline'), 0);
        gl.uniform3fv(loc('u_base_density'), u.base_density);
        gl.uniform3fv(loc('u_dmin'), u.dmin);
        gl.uniform3fv(loc('u_dmax'), u.dmax);
        gl.uniform1f(loc('u_dmin_trim'), 0.0);
        gl.uniform1f(loc('u_dmax_trim'), 0.0);
        gl.uniform1f(loc('u_master_exposure'), u.exposure);
        gl.uniform3fv(loc('u_exposure'), [
            u.exp_r * 0.5, u.exp_g * 0.5, u.exp_b * 0.5,
        ]);
        gl.uniform1f(loc('u_gamma'), u.gamma);
        gl.uniform1f(loc('u_contrast'), u.contrast);
        gl.uniform1f(loc('u_saturation'), u.saturation);
        gl.uniform1f(loc('u_temperature'), u.temperature);
        gl.uniform1f(loc('u_tint'), u.tint);
        gl.uniform1f(loc('u_highlights'), u.highlights);
        gl.uniform1f(loc('u_shadows'), u.shadows);
        gl.uniform1i(loc('u_has_lut'), 0);
        gl.uniform1i(loc('u_has_curve'), 0);
        const curvePlaceholder = gl.createTexture();
        gl.activeTexture(gl.TEXTURE3);
        gl.bindTexture(gl.TEXTURE_2D, curvePlaceholder);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, placeholder);
        gl.uniform1i(loc('u_curve_lut'), 3);
        gl.uniform1f(loc('u_lut_opacity'), 0.0);
        gl.uniform4f(loc('u_perspective'), 0, 0, 0, 1);
        gl.uniform1f(loc('u_lens_distortion'), 0);
        gl.uniformMatrix3fv(loc('u_geometry_uv'), false, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
        gl.uniform4f(loc('u_crop'), 0, 0, 1, 1);
        gl.uniform2f(loc('u_sprocket_uv'), -1, -1);
        gl.uniform1f(loc('u_sprocket_tolerance'), 0.1);
        gl.uniform1f(loc('u_sprocket_feather'), 0.05);
        gl.uniform1i(loc('u_has_sprocket_target_color'), 0);
        gl.uniform3f(loc('u_sprocket_target_color'), 0, 0, 0);
        gl.uniform2fv(loc('u_calib_pts[0]'), new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]));
        gl.uniform1i(loc('u_scope_warning'), 0);
        gl.uniform1f(loc('u_shadow_threshold'), 0.0);
        gl.uniform1f(loc('u_highlight_threshold'), 1.0);
        gl.uniform1i(loc('u_alpha_guard'), 0);

        gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
        const renderOnce = () => {
            gl.drawArrays(gl.TRIANGLES, 0, 6);
            const drawError = gl.getError();
            if (drawError !== gl.NO_ERROR) throw new Error('draw failed: ' + drawError);
            const out = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4);
            gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, out);
            const means = [0, 0, 0];
            for (let i = 0; i < out.length; i += 4) {
                for (let channel = 0; channel < 3; channel += 1) means[channel] += out[i + channel];
            }
            const count = out.length / 4;
            return means.map((value) => value / count);
        };
        const variants = {};
        variants.persisted = renderOnce();
        // A: placeholder window [0.1, 2.0] with the real base
        gl.uniform3fv(loc('u_dmin'), [0.1, 0.1, 0.1]);
        gl.uniform3fv(loc('u_dmax'), [2.0, 2.0, 2.0]);
        variants.placeholder_window = renderOnce();
        // B: zero base with the persisted window
        gl.uniform3fv(loc('u_dmin'), u.dmin);
        gl.uniform3fv(loc('u_dmax'), u.dmax);
        gl.uniform3fv(loc('u_base_density'), [0.0, 0.0, 0.0]);
        variants.zero_base = renderOnce();
        // C: zero base AND placeholder window
        gl.uniform3fv(loc('u_dmin'), [0.1, 0.1, 0.1]);
        gl.uniform3fv(loc('u_dmax'), [2.0, 2.0, 2.0]);
        variants.zero_base_placeholder_window = renderOnce();
        // D: staging (invert off) with the persisted state restored
        gl.uniform3fv(loc('u_dmin'), u.dmin);
        gl.uniform3fv(loc('u_dmax'), u.dmax);
        gl.uniform3fv(loc('u_base_density'), u.base_density);
        gl.uniform1i(loc('u_invert_enabled'), 0);
        variants.staging = renderOnce();
        finish({ ok: true, variants });
    } catch (error) {
        finish({ ok: false, log: String(error && error.message || error) });
    }
}
</script>`;

try {
    fs.writeFileSync(htmlPath, html);
    const run = childProcess.spawnSync(browser, [
        '--headless=new',
        '--no-sandbox',
        '--disable-gpu-sandbox',
        '--enable-webgl',
        '--ignore-gpu-blocklist',
        '--use-angle=swiftshader',
        '--dump-dom',
        '--virtual-time-budget=20000',
        pathToFileURL(htmlPath).href,
    ], { encoding: 'utf8', timeout: 60000, maxBuffer: 64 * 1024 * 1024 });
    if (run.error) throw run.error;
    const match = run.stdout.match(/<pre id="result">([^<]+)<\/pre>/);
    if (!match) throw new Error(`Browser did not return a result: ${run.stderr.trim().slice(0, 300)}`);
    const result = JSON.parse(match[1].replaceAll('&quot;', '"').replaceAll('&amp;', '&'));
    if (!result.ok) throw new Error(result.log);
    console.log(`[WASH] in-vitro variants (real shader, user's persisted frame):`);
    for (const [name, [r, g, b]] of Object.entries(result.variants)) {
        console.log(`[WASH]   ${name.padEnd(28)} mean=(${r.toFixed(1)}, ${g.toFixed(1)}, ${b.toFixed(1)}) B/G=${(b / g).toFixed(3)}`);
    }
    console.log(`[WASH] user's washed screenshots: (118,126,134) B/G=1.06 and (141,142,154) B/G=1.09`);
} finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
}
