const assert = require('node:assert/strict');
const range = require('./range-math.js');

assert.equal(range.finiteNumber('1,25'), 1.25);
assert.equal(range.clampRangeValue(200, -100, 100, 1), 100);
assert.equal(range.clampRangeValue(-200, -100, 100, 1), -100);
assert.equal(range.clampRangeValue(0.126, 0, 1, 0.01), 0.13);
assert.equal(range.clampRangeValue(-0.499, -0.5, 0.5, 0.002), -0.498);
assert.equal(range.resetRangeValue({ min: 0.1, max: 3, step: 0.002, defaultValue: 1 }), 1);
assert.equal(range.getWheelRangeValue({ value: 0, min: -1.5, max: 1.5, step: 0.001, deltaY: -100 }), 0.03);
assert.equal(range.getWheelRangeValue({ value: 0, min: -1.5, max: 1.5, step: 0.001, deltaY: 100 }), -0.03);
assert.equal(range.getWheelRangeValue({ value: 0.49, min: -0.5, max: 0.5, step: 0.002, deltaY: -100 }), 0.5);
assert.equal(range.getWheelRangeValue({ value: 0, min: -1, max: 1, step: 0.01, deltaY: -100, shiftKey: true }), 0.1);

console.log('Range input math tests passed.');
