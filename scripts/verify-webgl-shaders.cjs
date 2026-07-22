const childProcess = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const root = path.resolve(__dirname, '..');
const frontend = fs.readFileSync(path.join(root, 'ui', 'main.js'), 'utf8');

function extractTemplate(name) {
    const marker = `const ${name} = \``;
    const start = frontend.indexOf(marker);
    if (start < 0) throw new Error(`Missing ${name} in ui/main.js`);
    const contentStart = start + marker.length;
    const end = frontend.indexOf('`;', contentStart);
    if (end < 0) throw new Error(`Unterminated ${name} in ui/main.js`);
    return frontend.slice(contentStart, end);
}

const vertex = extractTemplate('vsSource');
const fragment = extractTemplate('fsSource');
const chromeCandidates = [
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
];
const browser = chromeCandidates.find(fs.existsSync);
if (!browser) throw new Error('Chrome or Edge was not found');

const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'nexfilm-shader-'));
const htmlPath = path.join(tempDir, 'verify.html');
const scriptData = JSON.stringify({ vertex, fragment }).replaceAll('<', '\\u003c');
const html = `<!doctype html><meta charset="utf-8"><canvas id="c" width="8" height="8"></canvas>
<pre id="result">pending</pre><script>
const sources = ${scriptData};
const resultElement = document.getElementById('result');
const gl = document.getElementById('c').getContext('webgl2');
function finish(result) { resultElement.textContent = JSON.stringify(result); }
if (!gl) {
    finish({ ok: false, stage: 'context', log: 'WebGL2 unavailable' });
} else {
    function compile(type, source) {
        const shader = gl.createShader(type);
        gl.shaderSource(shader, source);
        gl.compileShader(shader);
        return { shader, ok: gl.getShaderParameter(shader, gl.COMPILE_STATUS), log: gl.getShaderInfoLog(shader) || '' };
    }
    const vs = compile(gl.VERTEX_SHADER, sources.vertex);
    const fs = compile(gl.FRAGMENT_SHADER, sources.fragment);
    if (!vs.ok) {
        finish({ ok: false, stage: 'vertex', log: vs.log });
    } else if (!fs.ok) {
        finish({ ok: false, stage: 'fragment', log: fs.log });
    } else {
        const program = gl.createProgram();
        gl.attachShader(program, vs.shader);
        gl.attachShader(program, fs.shader);
        gl.linkProgram(program);
        finish({ ok: gl.getProgramParameter(program, gl.LINK_STATUS), stage: 'link', log: gl.getProgramInfoLog(program) || '' });
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
    ], { encoding: 'utf8', timeout: 30000 });
    if (run.error) throw run.error;
    const match = run.stdout.match(/<pre id="result">([^<]+)<\/pre>/);
    if (!match) {
        throw new Error(`Browser did not return a shader result: ${run.stderr.trim()}`);
    }
    const result = JSON.parse(match[1].replaceAll('&quot;', '"').replaceAll('&amp;', '&'));
    if (!result.ok) {
        throw new Error(`${result.stage} shader validation failed: ${result.log}`);
    }
    console.log('WebGL2 shaders compiled and linked successfully.');
} finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
}
