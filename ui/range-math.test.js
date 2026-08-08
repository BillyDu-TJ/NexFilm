const assert = require('node:assert/strict');
const range = require('./range-math.js');

assert.equal(range.finiteNumber('1,25'), 1.25);
assert.equal(range.clampRangeValue(200, -100, 100, 1), 100);
assert.equal(range.clampRangeValue(-200, -100, 100, 1), -100);
assert.equal(range.clampRangeValue(0.126, 0, 1, 0.01), 0.13);
assert.equal(range.clampRangeValue(-0.499, -0.5, 0.5, 0.002), -0.498);
assert.equal(range.resetRangeValue({ min: 0.1, max: 3, step: 0.002, defaultValue: 1 }), 1);

console.log('Range input math tests passed.');
