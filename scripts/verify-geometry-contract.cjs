const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {
    getPreviewTransform,
    proxyPixelTransformChanged,
    createTransformMatrix,
    invertDisplayPoint,
    mapDisplayPointToSource,
    normalizeGeometryState,
    mapPerspectivePoint,
    getConstrainedPerspectiveScale,
    getOrientedDimensions,
    transformGeometryForQuarterTurn,
    transformGeometryForFlip,
    mapOrientedPointToSource,
    mapSourcePointToOriented,
    anchorPointForAngleChange,
    anchorQuadForAngleChange,
    anchorRectForAngleChange,
    getDisplayFrame,
    orientedPointToDisplay,
    orientedRectToDisplay,
} = require('../ui/geometry.js');

assert.equal(normalizeGeometryState({}).perspective_scale, 1);
assert.deepEqual(mapPerspectivePoint([0.25, 0.75], {}), [0.25, 0.75]);
assert.ok(getConstrainedPerspectiveScale({ perspective_vertical: 50 }) >= 1);
assert.equal(getConstrainedPerspectiveScale({
    crop_rect: { x: 0.25, y: 0.25, width: 0.5, height: 0.5 },
}), 0.5);

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

assert.deepEqual(
    mapDisplayPointToSource(
        [0.25, 0.75],
        { x: 0.1, y: 0.2, width: 0.6, height: 0.4 },
        1000,
        500,
        {}
    ),
    [0.25, 0.5]
);

assert.deepEqual(
    mapDisplayPointToSource(
        [0.25, 0.75],
        { x: 0.1, y: 0.2, width: 0.6, height: 0.4 },
        1000,
        500,
        { calibration_points: [[0.1, 0.2], [0.9, 0.1], [0.8, 0.9], [0.2, 0.8]] }
    ).map(value => Number(value.toFixed(6))),
    [0.25, 0.5],
    'film-area points must not change display-to-source geometry'
);

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

// --- Rotation must keep the crop and the film area on the picture ----------
// The oriented frame is normalised by the rotated bounding box, so a fixed crop
// rect or film-area quad would slide and rescale over the picture as the fine
// angle changes. Rotating has to re-place both through the picture's own frame.
const flat = { angle: 0, rotate_90_count: 0, flip_h: false, flip_v: false };
const turned = { angle: 24, rotate_90_count: 0, flip_h: false, flip_v: false };
const frameSource = { width: 3000, height: 2000 };

const anchoredCrop = anchorRectForAngleChange(
    { x: 0.2, y: 0.2, width: 0.5, height: 0.5 },
    frameSource.width,
    frameSource.height,
    flat,
    turned
);
const flatCentre = mapOrientedPointToSource(
    [0.45, 0.45],
    frameSource.width,
    frameSource.height,
    flat
);
const turnedCentre = mapOrientedPointToSource(
    [anchoredCrop.x + anchoredCrop.width / 2, anchoredCrop.y + anchoredCrop.height / 2],
    frameSource.width,
    frameSource.height,
    turned
);
assertClose(turnedCentre[0], flatCentre[0], 1e-9);
assertClose(turnedCentre[1], flatCentre[1], 1e-9);
const turnedExtent = getOrientedDimensions(frameSource.width, frameSource.height, turned);
assertClose(anchoredCrop.width * turnedExtent.width, 0.5 * frameSource.width, 1);
assertClose(anchoredCrop.height * turnedExtent.height, 0.5 * frameSource.height, 1);

const filmArea = [[0.1, 0.1], [0.9, 0.12], [0.88, 0.9], [0.12, 0.88]];
const anchoredFilmArea = anchorQuadForAngleChange(
    filmArea,
    frameSource.width,
    frameSource.height,
    flat,
    turned
);
filmArea.forEach((point, index) => {
    const picture = mapOrientedPointToSource(point, frameSource.width, frameSource.height, flat);
    const restored = mapOrientedPointToSource(
        anchoredFilmArea[index],
        frameSource.width,
        frameSource.height,
        turned
    );
    assertClose(restored[0], picture[0], 1e-9);
    assertClose(restored[1], picture[1], 1e-9);
});

// --- One display frame for the picture and the editing overlays ------------
const displayCrop = { x: 0.3, y: 0.2, width: 0.4, height: 0.5 };
assert.deepEqual(getDisplayFrame({ crop_rect: displayCrop }, true), { x: 0, y: 0, width: 1, height: 1 });
assert.deepEqual(getDisplayFrame({ crop_rect: displayCrop }, false), displayCrop);
assert.deepEqual(orientedPointToDisplay([0.3, 0.2], displayCrop), [0, 0]);
const displayBottomRight = orientedPointToDisplay([0.7, 0.7], displayCrop);
assertClose(displayBottomRight[0], 1, 1e-12);
assertClose(displayBottomRight[1], 1, 1e-12);
assertRectClose(orientedRectToDisplay({ x: 0.4, y: 0.3, width: 0.2, height: 0.25 }, displayCrop), {
    x: 0.25,
    y: 0.2,
    width: 0.5,
    height: 0.5,
});
assertRectClose(
    orientedRectToDisplay(displayCrop, getDisplayFrame({ crop_rect: displayCrop }, true)),
    displayCrop
);

const frontend = fs.readFileSync(path.join(__dirname, '..', 'ui', 'main.js'), 'utf8');
for (const [pattern, message] of [
    [/function fullFrameEditView\(\)/, 'the canvas view needs one shared edit-frame decision'],
    [/if \(!fullFrameEditView\(\)\) \{\n\s+gl\.uniform4f\(u_crop_loc/, 'the preview must draw the full oriented frame while editing'],
    [/current_geom\.crop_rect = clampCropRect\(\s*\n?\s*NexFilmGeometry\.anchorRectForAngleChange/, 'a rotated crop has to be re-anchored to the picture'],
    [/anchorQuadForAngleChange/, 'a rotated film area has to be re-anchored to the picture'],
    [/applyGeometryAngle\(target\.angle\)/, 'the crop rotation slider must anchor the geometry'],
    [/applyGeometryAngle\(rawValue\)/, 'the straighten slider must anchor the geometry'],
    [/applyGeometryAngle\(Math\.max\(-45/, 'the crop rotate gesture must anchor the geometry'],
    [/orientedRectToDisplay\(/, 'the crop overlay must convert out of the oriented frame'],
    [/orientedPointToDisplay\(p, displayFrame\)/, 'the film-area overlay must convert out of the oriented frame'],
    [/if \(isCalibrationMode\) updateCalibrationPolygon\(\);/, 'the canvas layout must refresh the film-area overlay'],
    [/imageSizeIsProvisional/, 'a cached thumbnail must not resize a canvas the proxy already measured'],
    [/gl\.uniform4f\(u_crop_loc, crop\.x, crop\.y, crop\.width, crop\.height\);/, 'captured thumbnails must carry the finished framing'],
]) {
    assert.match(frontend, pattern, message);
}

console.log('Geometry preview contract verified.');
