const assert = require('node:assert/strict');
const histogram = require('./histogram-math.js');

assert.equal(histogram.normalizeScopeChannel('RED'), 'red');
assert.equal(histogram.normalizeScopeChannel('unknown'), 'rgb');
assert.deepEqual(
    histogram.getToneZones().map(zone => zone.id),
    ['shadows', 'lowlights', 'midtones', 'highlights', 'bright']
);
assert.equal(histogram.classifyTone(0.01), 'shadows');
assert.equal(histogram.classifyTone(0.25), 'lowlights');
assert.equal(histogram.classifyTone(0.5), 'midtones');
assert.equal(histogram.classifyTone(0.75), 'highlights');
assert.equal(histogram.classifyTone(1), 'bright');

const overflow = histogram.countOverflow(new Uint8Array([
    0, 0, 0, 255,
    128, 128, 128, 255,
    255, 255, 255, 255,
]), 0.02, 0.98);
assert.deepEqual(overflow, { shadows: 1, highlights: 1, total: 3 });

console.log('Professional scope math tests passed.');
