const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { getHistogramScale, getHistogramY } = require('../ui/histogram-math.js');

const normal = new Uint32Array(256);
for (let bin = 20; bin < 230; bin++) normal[bin] = 20 + (bin % 17);
normal[120] = 100000;
normal[255] = 500000;
const scale = getHistogramScale([normal]);
assert.ok(scale >= 20 && scale < 1000, 'isolated clipping peaks must not flatten the histogram');
assert.equal(getHistogramY(normal[255], scale, 100), 10);
assert.ok(getHistogramY(25, scale, 100) >= 0);

const clipped = new Uint32Array(256);
clipped[0] = 1000;
clipped[255] = 1000;
assert.equal(getHistogramScale([clipped]), 1);
assert.equal(getHistogramY(clipped[0], 1, 100), 10);

const mainSource = fs.readFileSync(path.join(__dirname, '..', 'ui', 'main.js'), 'utf8');
const indexSource = fs.readFileSync(path.join(__dirname, '..', 'ui', 'index.html'), 'utf8');
assert.match(mainSource, /final_rgb = vec3\(getLuma\(final_rgb\)\)/);
assert.match(mainSource, /float bw_dmin =/);
assert.match(indexSource, /id="viz-mode-tabs"[^>]+role="tablist"/);
assert.match(indexSource, /id="btn-viz-histogram"[^>]+aria-selected="true"/);
assert.match(indexSource, /id="btn-viz-waveform"[^>]+aria-selected="false"/);

console.log('Histogram scaling and B&W shader contracts verified.');
