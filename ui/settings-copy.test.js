const assert = require('node:assert/strict');
const { createCopyPayload, mergeCopyPayload } = require('./settings-copy.js');

const currentParams = {
    film_mode: 'Color', d_min: [0.1, 0.1, 0.1], d_max: [2, 2, 2], exposure: 0,
    gamma: 1, saturation: 0, temperature: 0, tint: 0, exp_r: 0, exp_g: 0, exp_b: 0,
    highlights: 0, shadows: 0, lut_path: null, lut_opacity: 1,
    working_colorspace: 'linear-srgb', sprocket_uv: [-1, -1], sprocket_tolerance: 0.1,
    sprocket_feather: 0.05
};
const sourceParams = {
    ...currentParams, film_mode: 'BW', d_min: [0.2, 0.2, 0.2], exposure: 0.4,
    saturation: 0.3, tint: 0.2, exp_r: 0.1, lut_path: 'film.cube', lut_opacity: 0.7,
    working_colorspace: 'acescg', sprocket_uv: [0.2, 0.8],
    sprocket_tolerance: 0.2, sprocket_feather: 0.1
};
const currentGeom = {
    crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0, flip_h: false,
    flip_v: false, rotate_90_count: 0, calibration_points: null,
    calibration_confirmed: false, perspective_vertical: 0, perspective_horizontal: 0,
    perspective_aspect: 0, perspective_scale: 1, constrain_crop: false
};
const sourceGeom = {
    crop_rect: { x: 0.1, y: 0.2, width: 0.7, height: 0.6 }, angle: 2.5, flip_h: true,
    flip_v: false, rotate_90_count: 1,
    perspective_vertical: 24, perspective_horizontal: -12, perspective_aspect: 8,
    perspective_scale: 1.18, constrain_crop: true,
    calibration_points: [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
    calibration_confirmed: true
};

const selectedPayload = createCopyPayload(sourceParams, sourceGeom, ['exposure', 'tint', 'crop']);
assert.deepEqual(selectedPayload.settings, ['exposure', 'tint', 'crop']);
assert.deepEqual(selectedPayload.params, { exposure: 0.4, tint: 0.2 });
assert.deepEqual(selectedPayload.geom, {
    crop_rect: sourceGeom.crop_rect,
    calibration_points: sourceGeom.calibration_points,
    calibration_confirmed: true
});

const selectedResult = mergeCopyPayload(currentParams, currentGeom, selectedPayload);
assert.equal(selectedResult.params.exposure, 0.4);
assert.equal(selectedResult.params.tint, 0.2);
assert.equal(selectedResult.params.saturation, 0);
assert.deepEqual(selectedResult.geom.crop_rect, sourceGeom.crop_rect);
assert.equal(selectedResult.geom.flip_h, false);

const rotateOnly = mergeCopyPayload(
    currentParams,
    currentGeom,
    createCopyPayload(sourceParams, sourceGeom, ['rotateFlip'])
);
assert.equal(rotateOnly.geom.flip_h, true);
assert.equal(rotateOnly.geom.rotate_90_count, 1);
assert.equal(rotateOnly.geom.angle, 0);

const legacyEdit = mergeCopyPayload(
    currentParams,
    currentGeom,
    createCopyPayload(sourceParams, sourceGeom, ['edit'])
);
assert.equal(legacyEdit.params.exposure, 0.4);
assert.equal(legacyEdit.params.working_colorspace, 'acescg');
assert.deepEqual(legacyEdit.params.sprocket_uv, [-1, -1]);

assert.throws(() => createCopyPayload(sourceParams, sourceGeom, []), /At least one/);
console.log('Settings copy module tests passed.');
