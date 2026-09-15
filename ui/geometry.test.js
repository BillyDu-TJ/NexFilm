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
    geometry.mapDisplayPointToSource(
        [0.25, 0.75],
        { x: 0.1, y: 0.2, width: 0.6, height: 0.4 },
        1000,
        500,
        { calibration_points: square }
    ),
    [0.25, 0.5],
    'film-area points must not apply perspective correction'
);
assert.notDeepEqual(
    geometry.mapDisplayPointToSource(
        [0.25, 0.75],
        { x: 0.1, y: 0.2, width: 0.6, height: 0.4 },
        1000,
        500,
        { calibration_points: square, perspective_horizontal: 60 }
    ),
    [0.25, 0.5],
    'explicit perspective controls must remain active'
);

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

// --- Picture-anchored rotation -------------------------------------------
// The oriented frame is normalised by the rotated bounding box. Anything placed
// in it has to be re-placed when the fine angle changes, or the crop box and the
// film area slide over the picture.
const anchorWidth = 3000;
const anchorHeight = 2000;
for (const angle of [0, 12.5, 45, -33]) {
    for (const rotate90 of [0, 1, 2, 3]) {
        for (const flipH of [false, true]) {
            const geom = { angle, rotate_90_count: rotate90, flip_h: flipH, flip_v: !flipH };
            for (const point of [[0.1, 0.2], [0.5, 0.5], [0.93, 0.04], [0, 1]]) {
                const picture = geometry.mapOrientedPointToSource(point, anchorWidth, anchorHeight, geom);
                const restored = geometry.mapSourcePointToOriented(picture, anchorWidth, anchorHeight, geom);
                assert.ok(Math.abs(restored[0] - point[0]) < 1e-9, 'source round trip x');
                assert.ok(Math.abs(restored[1] - point[1]) < 1e-9, 'source round trip y');
            }
        }
    }
}

const flattened = { angle: 0, rotate_90_count: 0, flip_h: false, flip_v: false };
const tilted = { angle: 18, rotate_90_count: 0, flip_h: false, flip_v: false };
const placedCrop = { x: 0.2, y: 0.25, width: 0.4, height: 0.35 };
const anchoredCrop = geometry.anchorRectForAngleChange(
    placedCrop, anchorWidth, anchorHeight, flattened, tilted
);
const placedCentre = geometry.mapOrientedPointToSource(
    [placedCrop.x + placedCrop.width / 2, placedCrop.y + placedCrop.height / 2],
    anchorWidth,
    anchorHeight,
    flattened
);
const anchoredCentre = geometry.mapOrientedPointToSource(
    [anchoredCrop.x + anchoredCrop.width / 2, anchoredCrop.y + anchoredCrop.height / 2],
    anchorWidth,
    anchorHeight,
    tilted
);
assert.ok(Math.abs(anchoredCentre[0] - placedCentre[0]) < 1e-9, 'crop centre stays on the picture');
assert.ok(Math.abs(anchoredCentre[1] - placedCentre[1]) < 1e-9, 'crop centre stays on the picture');
// The crop covers the same physical region, so it keeps scaling with the picture
// instead of zooming out from under the frame as the angle grows.
const tiltedExtent = geometry.getOrientedDimensions(anchorWidth, anchorHeight, tilted);
assert.ok(
    Math.abs(anchoredCrop.width * tiltedExtent.width - placedCrop.width * anchorWidth) < 1,
    'crop keeps its source-pixel width'
);
assert.ok(
    Math.abs(anchoredCrop.height * tiltedExtent.height - placedCrop.height * anchorHeight) < 1,
    'crop keeps its source-pixel height'
);
assert.ok(anchoredCrop.x >= 0 && anchoredCrop.y >= 0, 'anchored crop stays inside the frame');
assert.ok(anchoredCrop.x + anchoredCrop.width <= 1.000001);
assert.ok(anchoredCrop.y + anchoredCrop.height <= 1.000001);

const filmArea = [[0.15, 0.2], [0.85, 0.18], [0.88, 0.82], [0.12, 0.8]];
const anchoredFilmArea = geometry.anchorQuadForAngleChange(
    filmArea, anchorWidth, anchorHeight, flattened, tilted
);
filmArea.forEach((point, index) => {
    const before = geometry.mapOrientedPointToSource(point, anchorWidth, anchorHeight, flattened);
    const after = geometry.mapOrientedPointToSource(
        anchoredFilmArea[index], anchorWidth, anchorHeight, tilted
    );
    assert.ok(Math.abs(after[0] - before[0]) < 1e-9, 'film-area corner stays on the picture');
    assert.ok(Math.abs(after[1] - before[1]) < 1e-9, 'film-area corner stays on the picture');
});
assert.equal(geometry.isValidCalibrationQuad(anchoredFilmArea), true);

// --- Display frame -------------------------------------------------------
assert.deepEqual(geometry.getDisplayFrame(placedCrop, true), { x: 0, y: 0, width: 1, height: 1 });
assert.deepEqual(geometry.getDisplayFrame({ crop_rect: placedCrop }, false), placedCrop);
const displayFrame = { x: 0.25, y: 0.5, width: 0.5, height: 0.25 };
assert.deepEqual(geometry.orientedPointToDisplay([0.25, 0.5], displayFrame), [0, 0]);
assert.deepEqual(geometry.orientedPointToDisplay([0.75, 0.75], displayFrame), [1, 1]);
assert.deepEqual(geometry.displayPointToOriented([0.5, 0.5], displayFrame), [0.5, 0.625]);
assert.deepEqual(geometry.orientedRectToDisplay({ x: 0.25, y: 0.5, width: 0.25, height: 0.125 }, displayFrame), {
    x: 0,
    y: 0,
    width: 0.5,
    height: 0.5,
});
const fullFrameRect = geometry.orientedRectToDisplay(
    placedCrop,
    geometry.getDisplayFrame(placedCrop, true)
);
['x', 'y', 'width', 'height'].forEach(key => {
    assert.ok(
        Math.abs(fullFrameRect[key] - placedCrop[key]) < 1e-12,
        'the crop and film-area editing view keeps the oriented frame'
    );
});

console.log('Calibration edge geometry tests passed.');
