const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const context = { globalThis: {} };
vm.runInNewContext(fs.readFileSync(__dirname + '/color-science.js', 'utf8'), context);
const science = context.globalThis.NexFilmColorScience;

for (const space of [
    'linear-srgb', 'linear-display-p3', 'linear-adobe-rgb', 'linear-rec2020',
    'linear-prophoto-rgb', 'linear-aces', 'linear-acescg',
]) {
    const matrix = science.getWorkingToSrgbMatrix(space);
    assert.equal(matrix.length, 9);
    assert.ok([...matrix].every(Number.isFinite));
    assert.deepEqual(science.getWorkingLuma(space).length, 3);
}

const srgb = science.getWorkingToSrgbMatrix('linear-srgb');
assert.ok(Math.abs(srgb[0] - 1) < 1e-5);
assert.ok(Math.abs(srgb[4] - 1) < 1e-5);
assert.ok(Math.abs(srgb[8] - 1) < 1e-5);
console.log('Color science UI contract verified.');
