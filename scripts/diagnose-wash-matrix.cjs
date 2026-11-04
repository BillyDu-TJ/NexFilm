// Variant matrix for the washed-canvas report: render the user's persisted
// proxy through the real develop shader under every base/window combination
// the app could plausibly assemble, then score each against the user's
// screenshot statistics (white fraction, dark fraction, centre colour).
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
    return frontend.slice(contentStart, end);
}
const vertex = extractTemplate('vsSource');
const fragment = extractTemplate('fsSource');

const header = new DataView(proxyBytes.buffer, proxyBytes.byteOffset, proxyBytes.byteLength);
const width = header.getUint32(0, true);
const height = header.getUint32(4, true);
const flags = header.getUint32(24, true);
const allPixels = new Uint16Array(proxyBytes.buffer, proxyBytes.byteOffset + 28, width * height * 4);
const subW = Math.floor(width / 2);
const subH = Math.floor(height / 2);
const pixels = new Uint16Array(subW * subH * 4);
for (let y = 0; y < subH; y += 1) {
    for (let x = 0; x < subW; x += 1) {
        const src = ((y * 2) * width + (x * 2)) * 4;
        const dst = (y * subW + x) * 4;
        for (let c = 0; c < 4; c += 1) pixels[dst + c] = allPixels[src + c];
    }
}

const V = (r, g, b) => [r, g, b];
const HALF = V(0.30103, 0.30103, 0.30103); // -log10(32768/65535): BaseColor::default()
const TRUE_BASE = uniforms.base_density;
const W1808 = { dmin: uniforms.dmin, dmax: uniforms.dmax };
const W1739 = {
    dmin: [-0.00029333285, -0.0003713463, -0.0024184394],
    dmax: [0.9161181, 0.9581729, 1.2603792],
};
const PLACEHOLDER = { dmin: V(0.1, 0.1, 0.1), dmax: V(2.0, 2.0, 2.0) };

const variants = [
    { name: 'persisted_1808', base: TRUE_BASE, ...W1808 },
    { name: 'half_base_1808', base: HALF, ...W1808 },
    { name: 'zero_base_1808', base: V(0, 0, 0), ...W1808 },
    { name: 'true_base_1739win', base: TRUE_BASE, ...W1739 },
    { name: 'half_base_1739win', base: HALF, ...W1739 },
    { name: 'zero_base_1739win', base: V(0, 0, 0), ...W1739 },
    { name: 'true_base_placeholder', base: TRUE_BASE, ...PLACEHOLDER },
    { name: 'half_base_placeholder', base: HALF, ...PLACEHOLDER },
    { name: 'staging_fixed', base: TRUE_BASE, ...W1808, invert: 0 },
];

const chromeCandidates = [
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
];
const browser = chromeCandidates.find(fs.existsSync);

const payload = JSON.stringify({ vertex, fragment, width: subW, height: subH, variants });
const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nexfilm-washmx-'));
const htmlPath = path.join(tempDir, 'wash.html');
const html = `<!doctype html><meta charset="utf-8"><canvas id="c" width="${subW}" height="${subH}"></canvas>
<script type="application/json" id="spec">${payload}</script>
<script type="application/json" id="pixels">${JSON.stringify(Buffer.from(pixels.buffer, pixels.byteOffset, pixels.byteLength).toString('base64'))}</script>
<pre id="result">pending</pre><script>
window.addEventListener('error', (e) => {
    document.getElementById('result').textContent = JSON.stringify({ ok: false, log: 'page: ' + e.message });
});
const spec = JSON.parse(document.getElementById('spec').textContent);
const bin = atob(JSON.parse(document.getElementById('pixels').textContent));
const pixels = new Uint16Array(bin.length / 2);
for (let i = 0; i < pixels.length; i += 1) pixels[i] = bin.charCodeAt(i * 2) | (bin.charCodeAt(i * 2 + 1) << 8);
const finish = (r) => document.getElementById('result').textContent = JSON.stringify(r);
const gl = document.getElementById('c').getContext('webgl2', { preserveDrawingBuffer: true });
try {
    const compile = (t, s) => { const sh = gl.createShader(t); gl.shaderSource(sh, s); gl.compileShader(sh);
        if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(sh)); return sh; };
    const program = gl.createProgram();
    gl.attachShader(program, compile(gl.VERTEX_SHADER, spec.vertex));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, spec.fragment));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
    gl.useProgram(program);
    const vao = gl.createVertexArray(); gl.bindVertexArray(vao);
    const pos = gl.getAttribLocation(program, 'a_position');
    const posBuf = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, posBuf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1,1,-1,-1,1,-1,1,1,-1,1,1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(pos); gl.vertexAttribPointer(pos, 2, gl.FLOAT, 0, 0, 0);
    const tex = gl.getAttribLocation(program, 'a_texcoord');
    const texBuf = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, texBuf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0,0,1,0,0,1,0,1,1,0,1,1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(tex); gl.vertexAttribPointer(tex, 2, gl.FLOAT, 0, 0, 0);
    const texture = gl.createTexture();
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16UI, spec.width, spec.height, 0, gl.RGBA_INTEGER, gl.UNSIGNED_SHORT, pixels);
    if (gl.getError() !== gl.NO_ERROR) throw new Error('texture upload failed');
    gl.uniform1i(gl.getUniformLocation(program, 'u_image'), 0);
    const placeholder = new Uint8Array([0, 0, 0, 255]);
    for (const [unit, target, kind] of [[1, gl.TEXTURE_2D, 'u_lut1d'], [2, gl.TEXTURE_3D, 'u_lut3d'], [3, gl.TEXTURE_2D, 'u_curve_lut']]) {
        const t = gl.createTexture();
        gl.activeTexture(gl.TEXTURE0 + unit);
        gl.bindTexture(target, t);
        if (target === gl.TEXTURE_3D) gl.texImage3D(target, 0, gl.RGBA8, 1, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, placeholder);
        else gl.texImage2D(target, 0, gl.RGBA8, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, placeholder);
        gl.uniform1i(gl.getUniformLocation(program, kind), unit);
    }
    const loc = (n) => gl.getUniformLocation(program, n);
    gl.uniform1i(loc('u_mode'), 0);
    gl.uniform1i(loc('u_invert_enabled'), 1);
    gl.uniform1i(loc('u_legacy_pipeline'), 0);
    gl.uniform1i(loc('u_capture_corrected_pipeline'), 0);
    gl.uniform1f(loc('u_dmin_trim'), 0); gl.uniform1f(loc('u_dmax_trim'), 0);
    gl.uniform1f(loc('u_master_exposure'), 0);
    gl.uniform3f(loc('u_exposure'), 0, 0, 0);
    gl.uniform1f(loc('u_gamma'), 1);
    gl.uniform1f(loc('u_contrast'), 0); gl.uniform1f(loc('u_saturation'), 0);
    gl.uniform1f(loc('u_temperature'), 0); gl.uniform1f(loc('u_tint'), 0);
    gl.uniform1f(loc('u_highlights'), 0); gl.uniform1f(loc('u_shadows'), 0);
    gl.uniform1i(loc('u_has_lut'), 0); gl.uniform1i(loc('u_has_curve'), 0);
    gl.uniform1f(loc('u_lut_opacity'), 0);
    gl.uniform4f(loc('u_perspective'), 0, 0, 0, 1);
    gl.uniform1f(loc('u_lens_distortion'), 0);
    gl.uniformMatrix3fv(loc('u_geometry_uv'), false, [1,0,0,0,1,0,0,0,1]);
    gl.uniform4f(loc('u_crop'), 0, 0, 1, 1);
    gl.uniform2f(loc('u_sprocket_uv'), -1, -1);
    gl.uniform1f(loc('u_sprocket_tolerance'), 0.1);
    gl.uniform1f(loc('u_sprocket_feather'), 0.05);
    gl.uniform1i(loc('u_has_sprocket_target_color'), 0);
    gl.uniform3f(loc('u_sprocket_target_color'), 0, 0, 0);
    gl.uniform2fv(loc('u_calib_pts[0]'), new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]));
    gl.uniform1i(loc('u_scope_warning'), 0);
    gl.uniform1f(loc('u_shadow_threshold'), 0);
    gl.uniform1f(loc('u_highlight_threshold'), 1);
    gl.uniform1i(loc('u_alpha_guard'), 0);
    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
    const out = {};
    for (const v of spec.variants) {
        gl.uniform1i(loc('u_invert_enabled'), v.invert === 0 ? 0 : 1);
        gl.uniform3fv(loc('u_base_density'), v.base);
        gl.uniform3fv(loc('u_dmin'), v.dmin);
        gl.uniform3fv(loc('u_dmax'), v.dmax);
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        const buf = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4);
        gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, buf);
        const n = buf.length / 4;
        let mean = [0, 0, 0], white = 0, dark = 0, alphaSum = 0;
        const cx0 = Math.floor(gl.drawingBufferWidth * 0.4), cx1 = Math.floor(gl.drawingBufferWidth * 0.6);
        const cy0 = Math.floor(gl.drawingBufferHeight * 0.4), cy1 = Math.floor(gl.drawingBufferHeight * 0.6);
        let center = [0, 0, 0], centerN = 0;
        for (let i = 0; i < buf.length; i += 4) {
            const r = buf[i], g = buf[i+1], b = buf[i+2];
            mean[0] += r; mean[1] += g; mean[2] += b; alphaSum += buf[i + 3];
            const lum = (r + g + b) / 3;
            if (lum > 240) white += 1;
            if (lum < 80) dark += 1;
            const p = i / 4;
            const px = p % gl.drawingBufferWidth, py = Math.floor(p / gl.drawingBufferWidth);
            if (px >= cx0 && px < cx1 && py >= cy0 && py < cy1) { center[0] += r; center[1] += g; center[2] += b; centerN += 1; }
        }
        out[v.name] = {
            mean: mean.map(x => +(x / n).toFixed(1)),
            center: center.map(x => +(x / centerN).toFixed(1)),
            whitePct: +(white / n * 100).toFixed(1),
            darkPct: +(dark / n * 100).toFixed(1),
            alphaMean: +(alphaSum / n).toFixed(1),
        };
    }
    finish({ ok: true, out });
} catch (e) { finish({ ok: false, log: String(e && e.message || e) }); }
</script>`;

try {
    fs.writeFileSync(htmlPath, html);
    const run = childProcess.spawnSync(browser, [
        '--headless=new', '--no-sandbox', '--disable-gpu-sandbox', '--enable-webgl',
        '--ignore-gpu-blocklist', '--use-angle=swiftshader', '--dump-dom',
        '--virtual-time-budget=20000', pathToFileURL(htmlPath).href,
    ], { encoding: 'utf8', timeout: 60000, maxBuffer: 64 * 1024 * 1024 });
    const match = run.stdout.match(/<pre id="result">([^<]+)<\/pre>/);
    if (!match) throw new Error('no result: ' + run.stderr.slice(0, 300));
    const result = JSON.parse(match[1].replaceAll('&quot;', '"').replaceAll('&amp;', '&'));
    if (!result.ok) throw new Error(result.log);
    console.log('[WASH-MX] header flags=' + flags);
    console.log('[WASH-MX] target (user screenshot image area): center=(106,116,124) white%≈22-26 dark%≈0');
    for (const [name, s] of Object.entries(result.out)) {
        console.log(`[WASH-MX] ${name.padEnd(24)} mean=(${s.mean}) center=(${s.center}) white%=${s.whitePct} dark%=${s.darkPct} alpha=${s.alphaMean}`);
    }
} finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
}
