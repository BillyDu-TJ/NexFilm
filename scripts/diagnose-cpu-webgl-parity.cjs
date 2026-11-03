// Diagnostic harness: renders the CPU fixture through the real WebGL develop
// shader (ui/main.js fsSource/vsSource, SwiftShader headless) with the same
// transport texture, uniforms and curve LUT, then reports the numeric delta
// against the CPU render dumped by
// `cargo test --lib cpu_render_fixture_for_webgl_parity -- --ignored`.
// This is a diagnostic, not a contract: it prints per-channel deltas so the
// CPU export renderer and the canvas renderer can be compared stage by stage.
const childProcess = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const root = path.resolve(__dirname, '..');
const fixturePath = path.join(root, 'target', 'diag-webgl-parity', 'cpu.json');
const fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));

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
const toneCurveSource = fs.readFileSync(path.join(root, 'ui', 'tone-curve.js'), 'utf8');

const chromeCandidates = [
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
];
const browser = chromeCandidates.find(fs.existsSync);
if (!browser) throw new Error('Chrome or Edge was not found');

const { width, height, uniforms, curve } = fixture;
const payload = JSON.stringify({ vertex, fragment, width, height, uniforms, curve, transport: fixture.transport, rendered: fixture.rendered });
if (payload.includes('</script')) throw new Error('fixture contains a script terminator');

const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nexfilm-parity-'));
const htmlPath = path.join(tempDir, 'parity.html');
const html = `<!doctype html><meta charset="utf-8"><canvas id="c" width="${width}" height="${height}"></canvas>
<script type="application/json" id="fixture">${payload}</script>
<pre id="result">pending</pre><script>
const spec = JSON.parse(document.getElementById('fixture').textContent);
const transport = spec.transport;
const renderedCpu = spec.rendered;
const resultElement = document.getElementById('result');
function finish(result) { resultElement.textContent = JSON.stringify(result); }
${toneCurveSource}
const gl = document.getElementById('c').getContext('webgl2', { preserveDrawingBuffer: true });
if (!gl) {
    finish({ ok: false, stage: 'context', log: 'WebGL2 unavailable' });
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

        // Mirror the app's quad exactly: two separate attributes, drawArrays.
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

        // Transport texture: RGBA16UI, NEAREST, exactly like loadProxyImage.
        const rgba = new Uint16Array(spec.width * spec.height * 4);
        for (let i = 0; i < spec.width * spec.height; i += 1) {
            rgba[i * 4] = transport[i * 3];
            rgba[i * 4 + 1] = transport[i * 3 + 1];
            rgba[i * 4 + 2] = transport[i * 3 + 2];
            rgba[i * 4 + 3] = 65535;
        }
        const imageTexture = gl.createTexture();
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, imageTexture);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16UI, spec.width, spec.height, 0, gl.RGBA_INTEGER, gl.UNSIGNED_SHORT, rgba);
        const imageError = gl.getError();
        if (imageError !== gl.NO_ERROR) throw new Error('image texture upload failed: ' + imageError);
        gl.uniform1i(gl.getUniformLocation(program, 'u_image'), 0);

        // Curve LUT: RGBA16F LINEAR, built exactly like ensureCurveLutTexture.
        const size = NexFilmToneCurve.LUT_SIZE;
        const lutPixels = new Float32Array(size * 4);
        const luts = {
            red: NexFilmToneCurve.buildLut(spec.curve.red),
            green: NexFilmToneCurve.buildLut(spec.curve.green),
            blue: NexFilmToneCurve.buildLut(spec.curve.blue),
            rgb: NexFilmToneCurve.buildLut(spec.curve.rgb),
        };
        for (let i = 0; i < size; i += 1) {
            lutPixels[i * 4] = luts.red[i];
            lutPixels[i * 4 + 1] = luts.green[i];
            lutPixels[i * 4 + 2] = luts.blue[i];
            lutPixels[i * 4 + 3] = luts.rgb[i];
        }
        const curveTexture = gl.createTexture();
        gl.activeTexture(gl.TEXTURE3);
        gl.bindTexture(gl.TEXTURE_2D, curveTexture);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA16F, size, 1, 0, gl.RGBA, gl.FLOAT, lutPixels);
        const curveError = gl.getError();
        if (curveError !== gl.NO_ERROR) throw new Error('curve texture upload failed: ' + curveError);
        gl.uniform1i(gl.getUniformLocation(program, 'u_curve_lut'), 3);
        gl.uniform1i(gl.getUniformLocation(program, 'u_has_curve'), 1);

        const u = spec.uniforms;
        const loc = (name) => gl.getUniformLocation(program, name);
        gl.uniform1i(loc('u_mode'), 0);
        gl.uniform1i(loc('u_invert_enabled'), 1);
        gl.uniform1i(loc('u_legacy_pipeline'), 0);
        gl.uniform1i(loc('u_capture_corrected_pipeline'), 0);
        gl.uniform3fv(loc('u_base_density'), u.base_density);
        gl.uniform3fv(loc('u_dmin'), u.dmin);
        gl.uniform3fv(loc('u_dmax'), u.dmax);
        gl.uniform1f(loc('u_dmin_trim'), 0.0);
        gl.uniform1f(loc('u_dmax_trim'), 0.0);
        gl.uniform1f(loc('u_master_exposure'), u.master_exposure);
        gl.uniform3fv(loc('u_exposure'), u.exposure);
        gl.uniform1f(loc('u_gamma'), u.gamma);
        gl.uniform1f(loc('u_contrast'), u.contrast);
        gl.uniform1f(loc('u_saturation'), u.saturation);
        gl.uniform1f(loc('u_temperature'), u.temperature);
        gl.uniform1f(loc('u_tint'), u.tint);
        gl.uniform1f(loc('u_highlights'), u.highlights);
        gl.uniform1f(loc('u_shadows'), u.shadows);
        gl.uniform1i(loc('u_has_lut'), 0);
        gl.uniform1i(loc('u_lut_is_1d'), 0);
        gl.uniform1f(loc('u_lut_opacity'), 0.0);
        // Unbound float samplers default to unit 0 where the integer transport
        // texture lives, which is a draw-time type conflict. The app binds
        // placeholder textures for exactly this reason; mirror it.
        const placeholder = new Uint8Array([0, 0, 0, 255]);
        const lut1dPlaceholder = gl.createTexture();
        gl.activeTexture(gl.TEXTURE1);
        gl.bindTexture(gl.TEXTURE_2D, lut1dPlaceholder);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, placeholder);
        gl.uniform1i(loc('u_lut1d'), 1);
        const lut3dPlaceholder = gl.createTexture();
        gl.activeTexture(gl.TEXTURE2);
        gl.bindTexture(gl.TEXTURE_3D, lut3dPlaceholder);
        gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGBA8, 1, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, placeholder);
        gl.uniform1i(loc('u_lut3d'), 2);
        gl.uniform4f(loc('u_perspective'), 0, 0, 0, 1);
        gl.uniform1f(loc('u_lens_distortion'), 0);
        gl.uniformMatrix3fv(loc('u_geometry_uv'), false, [1, 0, 0, 0, 1, 0, 0, 0, 1]);
        gl.uniform4f(loc('u_crop'), 0, 0, 1, 1);
        gl.uniform2f(loc('u_sprocket_uv'), -1, -1);
        gl.uniform1f(loc('u_sprocket_tolerance'), 0.1);
        gl.uniform1f(loc('u_sprocket_feather'), 0.05);
        gl.uniform1i(loc('u_has_sprocket_target_color'), 0);
        gl.uniform3f(loc('u_sprocket_target_color'), 0, 0, 0);
        gl.uniform2fv(loc('u_calib_pts[0]'), new Float32Array([
            0, 0, 1, 0, 1, 1, 0, 1,
        ]));
        gl.uniform1i(loc('u_scope_warning'), 0);
        gl.uniform1f(loc('u_shadow_threshold'), 0.0);
        gl.uniform1f(loc('u_highlight_threshold'), 1.0);
        gl.uniform1i(loc('u_alpha_guard'), 0);

        gl.viewport(0, 0, spec.width, spec.height);
        while (gl.getError() !== gl.NO_ERROR) { /* drain pending errors before the draw */ }
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        const drawError = gl.getError();
        if (drawError !== gl.NO_ERROR) throw new Error('draw failed: ' + drawError);

        const pixels = new Uint8Array(spec.width * spec.height * 4);
        gl.readPixels(0, 0, spec.width, spec.height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);

        // The Film Area selection changes analysis only: re-render with a
        // small quad and require the same picture. This is the WebGL side of
        // the "a selection must not whiten anything" regression pin.
        const cornerPts = new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]);
        gl.uniform2fv(loc('u_calib_pts[0]'), new Float32Array([
            0.18, 0.12, 0.86, 0.08, 0.88, 0.92, 0.22, 0.88,
        ]));
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        const quadPixels = new Uint8Array(spec.width * spec.height * 4);
        gl.readPixels(0, 0, spec.width, spec.height, gl.RGBA, gl.UNSIGNED_BYTE, quadPixels);
        let quadDelta = 0;
        for (let i = 0; i < quadPixels.length; i += 4) {
            for (let channel = 0; channel < 3; channel += 1) {
                quadDelta = Math.max(quadDelta, Math.abs(quadPixels[i + channel] - pixels[i + channel]));
            }
        }
        if (quadDelta !== 0) throw new Error('film area selection changed the WebGL render by ' + quadDelta + '/255');
        gl.uniform2fv(loc('u_calib_pts[0]'), cornerPts);

        // Compare against the CPU render: readPixels is bottom-up.
        const totals = [0, 0, 0];
        const maxima = [0, 0, 0];
        let worstIndex = 0;
        let worstChannel = 0;
        for (let y = 0; y < spec.height; y += 1) {
            const flippedY = spec.height - 1 - y;
            for (let x = 0; x < spec.width; x += 1) {
                const glIndex = (flippedY * spec.width + x) * 4;
                const cpuIndex = (y * spec.width + x) * 3;
                for (let channel = 0; channel < 3; channel += 1) {
                    const glValue = pixels[glIndex + channel] / 255.0;
                    const cpuValue = renderedCpu[cpuIndex + channel] / 65535.0;
                    const delta = Math.abs(glValue - cpuValue);
                    totals[channel] += delta;
                    if (delta > maxima[channel]) maxima[channel] = delta;
                    if (delta > maxima[worstChannel]) {
                        maxima[worstChannel] = delta;
                    }
                    if (delta >= maxima[worstChannel]) {
                        worstChannel = channel;
                        worstIndex = cpuIndex / 3;
                    }
                }
            }
        }
        const count = spec.width * spec.height;
        const means = totals.map((value) => value / count);
        finish({
            ok: true,
            means,
            maxima,
            worst: { pixel: worstIndex, channel: worstChannel, delta: maxima[worstChannel] },
            sample: {
                gl: [pixels[0], pixels[1], pixels[2]],
                cpu: [
                    Math.round(renderedCpu[(count - spec.width) * 3] / 257),
                    Math.round(renderedCpu[(count - spec.width) * 3 + 1] / 257),
                    Math.round(renderedCpu[(count - spec.width) * 3 + 2] / 257),
                ],
            },
        });
    } catch (error) {
        finish({ ok: false, stage: 'render', log: String(error && error.message || error) });
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
        pathToFileURL(htmlPath).href,
    ], { encoding: 'utf8', timeout: 60000, maxBuffer: 64 * 1024 * 1024 });
    if (run.error) throw run.error;
    const match = run.stdout.match(/<pre id="result">([^<]+)<\/pre>/);
    if (!match) {
        throw new Error(`Browser did not return a result: ${run.stderr.trim().slice(0, 400)}`);
    }
    const result = JSON.parse(match[1].replaceAll('&quot;', '"').replaceAll('&amp;', '&'));
    if (!result.ok) {
        throw new Error(`${result.stage} failed: ${result.log}`);
    }
    const [mr, mg, mb] = result.means;
    const [xr, xg, xb] = result.maxima;
    console.log(`[PARITY] WebGL vs CPU render (${width}x${height}, 8-bit canvas readback):`);
    console.log(`[PARITY]   mean |delta| R=${mr.toFixed(5)} G=${mg.toFixed(5)} B=${mb.toFixed(5)} (of 1.0)`);
    console.log(`[PARITY]   max  |delta| R=${xr.toFixed(4)} G=${xg.toFixed(4)} B=${xb.toFixed(4)}`);
    console.log(`[PARITY]   worst pixel #${result.worst.pixel} channel ${result.worst.channel}`);
    const tolerance = 0.02;
    if (Math.max(...result.maxima) > tolerance) {
        console.log(`[PARITY]   OVER tolerance ${tolerance}: renderers disagree beyond 8-bit quantisation.`);
    } else {
        console.log(`[PARITY]   within ${tolerance} tolerance: canvas and CPU renderers agree.`);
    }
} finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
}
