const assert = require('node:assert/strict');
const {
    getPreviewTransform,
    proxyPixelTransformChanged,
    createTransformMatrix,
    invertDisplayPoint,
    transformGeometryForQuarterTurn,
    transformGeometryForFlip,
} = require('../ui/geometry.js');

function assertClose(actual, expected, epsilon = 1e-12) {
    assert.ok(Math.abs(actual - expected) < epsilon, `${actual} != ${expected}`);
}

function assertRectClose(actual, expected) {
    assertClose(actual.x, expected.x);
    assertClose(actual.y, expected.y);
    assertClose(actual.width, expected.width);
    assertClose(actual.height, expected.height);
}

const loaded = { angle: 7.5, rotate_90_count: 1, flip_h: true, flip_v: false };

assert.deepEqual(getPreviewTransform(loaded, loaded, true), {
    angleDegrees: 0,
    angleRadians: 0,
    scaleX: 1,
    scaleY: 1,
});
assert.equal(proxyPixelTransformChanged(loaded, loaded), false);
assert.equal(proxyPixelTransformChanged({ ...loaded, angle: 7.6 }, loaded), true);
assert.equal(proxyPixelTransformChanged({ ...loaded, crop_rect: { x: 0.2, y: 0.2, width: 0.5, height: 0.5 } }, loaded), false);
assert.equal(proxyPixelTransformChanged({ ...loaded, rotate_90_count: 5 }, loaded), false);

const changed = getPreviewTransform({
    angle: 10,
    rotate_90_count: 2,
    flip_h: false,
    flip_v: true,
}, loaded, true);
assert.equal(changed.angleDegrees, 92.5);
assert.equal(changed.scaleX, -1);
assert.equal(changed.scaleY, -1);

assert.deepEqual(getPreviewTransform({ angle: 15 }, null, false), {
    angleDegrees: 0,
    angleRadians: 0,
    scaleX: 1,
    scaleY: 1,
});

const transform = getPreviewTransform({ angle: 30, flip_h: true }, null, true);
const matrix = createTransformMatrix(transform);
const source = { x: 0.25, y: -0.4 };
const displayed = {
    x: matrix[0] * source.x + matrix[4] * source.y,
    y: matrix[1] * source.x + matrix[5] * source.y,
};
const restored = invertDisplayPoint(displayed.x, displayed.y, transform);
assert.ok(Math.abs(restored.x - source.x) < 1e-6);
assert.ok(Math.abs(restored.y - source.y) < 1e-6);

const geometry = {
    crop_rect: { x: 0.1, y: 0.2, width: 0.3, height: 0.4 },
    calibration_points: [[0.1, 0.2], [0.8, 0.1], [0.9, 0.75], [0.2, 0.9]],
    flip_h: false,
    flip_v: false,
};
const rotated = transformGeometryForQuarterTurn(geometry, true);
assertRectClose(rotated.cropRect, { x: 0.4, y: 0.1, width: 0.4, height: 0.3 });
assert.deepEqual(rotated.calibrationPoints, [[0.09999999999999998, 0.2], [0.8, 0.1], [0.9, 0.8], [0.25, 0.9]]);

const identity = {
    crop_rect: { x: 0, y: 0, width: 1, height: 1 },
    calibration_points: [[0, 0], [1, 0], [1, 1], [0, 1]],
    flip_h: false,
    flip_v: false,
};
assert.deepEqual(transformGeometryForQuarterTurn(identity, true).calibrationPoints, identity.calibration_points);
assert.deepEqual(transformGeometryForFlip(identity, true, false).calibrationPoints, identity.calibration_points);

const flipped = transformGeometryForFlip(geometry, true, false);
assertRectClose(flipped.cropRect, { x: 0.6, y: 0.2, width: 0.3, height: 0.4 });
assert.deepEqual(flipped.calibrationPoints, [[0.19999999999999996, 0.1], [0.9, 0.2], [0.8, 0.9], [0.09999999999999998, 0.75]]);

console.log('Geometry preview contract verified.');
