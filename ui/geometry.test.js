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

assert.deepEqual(
    geometry.resolveCalibrationRenderPoints(square, true),
    [[0, 0], [1, 0], [1, 1], [0, 1]],
    'calibration mode must keep the underlying image in its stable source coordinates'
);
assert.deepEqual(geometry.resolveCalibrationRenderPoints(square, false), square);

assert.deepEqual(geometry.normalizeGeometryState({}).crop_rect, { x: 0, y: 0, width: 1, height: 1 });
assert.equal(geometry.normalizeGeometryState({}).perspective_scale, 1);
assert.equal(geometry.normalizeGeometryState({}).lens_distortion, 0);
assert.equal(geometry.needsFilmAreaConfirmation({ calibration_confirmed: false }), true);
assert.equal(geometry.needsFilmAreaConfirmation({ calibration_confirmed: true }), false);
assert.equal(geometry.normalizeGeometryState({ calibration_confirmed: 1 }).calibration_confirmed, true);
assert.equal(geometry.normalizeGeometryState({ calibration_confirmed: 'true' }).calibration_confirmed, true);
assert.equal(geometry.needsFilmAreaConfirmation({}), true);
assert.deepEqual(
    geometry.getFilmAreaCalibrationDraft({ calibration_confirmed: false, calibration_points: square }),
    [[0, 0], [1, 0], [1, 1], [0, 1]],
    'first-time film-area setup must default to the full frame'
);
assert.deepEqual(
    geometry.getFilmAreaCalibrationDraft({ calibration_confirmed: true, calibration_points: square }),
    square,
    'manual recalibration must start from the saved film area'
);
const neutralPerspective = geometry.mapPerspectivePoint([0.2, 0.8], {});
assert.ok(Math.abs(neutralPerspective[0] - 0.2) < 1e-12);
assert.ok(Math.abs(neutralPerspective[1] - 0.8) < 1e-12);
const neutralDistortion = geometry.mapLensDistortionPoint([0.2, 0.8], {});
assert.ok(Math.abs(neutralDistortion[0] - 0.2) < 1e-12);
assert.ok(Math.abs(neutralDistortion[1] - 0.8) < 1e-12);
const barrelCorrection = geometry.mapLensDistortionPoint([0.1, 0.5], { lens_distortion: -50 });
const pincushionCorrection = geometry.mapLensDistortionPoint([0.1, 0.5], { lens_distortion: 50 });
assert.ok(barrelCorrection[0] > 0.1);
assert.ok(pincushionCorrection[0] < 0.1);
const constrainedScale = geometry.getConstrainedPerspectiveScale({
    perspective_vertical: 60,
    perspective_horizontal: -45,
    perspective_aspect: 20,
    perspective_scale: 1,
});
assert.ok(constrainedScale >= 1 && constrainedScale <= 3);
const croppedConstrainedScale = geometry.getConstrainedPerspectiveScale({
    crop_rect: { x: 0.25, y: 0.25, width: 0.5, height: 0.5 },
});
assert.ok(Math.abs(croppedConstrainedScale - 0.5) < 1e-12);

console.log('Calibration edge geometry tests passed.');
