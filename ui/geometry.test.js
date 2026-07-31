const assert = require('node:assert/strict');
const geometry = require('./geometry.js');

const square = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]];

assert.deepEqual(geometry.calibrationEdgeIndices, [[0, 1], [1, 2], [2, 3], [3, 0]]);

const movedTop = geometry.translateCalibrationEdge(square, 0, [0, 50], [1000, 500]);
assert.deepEqual(movedTop, [[0.1, 0.2], [0.9, 0.2], [0.9, 0.9], [0.1, 0.9]]);

const movedRight = geometry.translateCalibrationEdge(square, 1, [-100, 0], [1000, 500]);
assert.deepEqual(movedRight, [[0.1, 0.1], [0.8, 0.1], [0.8, 0.9], [0.1, 0.9]]);

const movedBottom = geometry.translateCalibrationEdge(square, 2, [0, -50], [1000, 500]);
assert.deepEqual(movedBottom, [[0.1, 0.1], [0.9, 0.1], [0.9, 0.8], [0.1, 0.8]]);

const movedLeft = geometry.translateCalibrationEdge(square, 3, [100, 0], [1000, 500]);
assert.deepEqual(movedLeft, [[0.2, 0.1], [0.9, 0.1], [0.9, 0.9], [0.2, 0.9]]);

assert.equal(geometry.isValidCalibrationQuad(square), true);
assert.equal(geometry.isValidCalibrationQuad([[0.1, 0.1], [0.9, 0.9], [0.9, 0.1], [0.1, 0.9]]), false);

console.log('Calibration edge geometry tests passed.');
