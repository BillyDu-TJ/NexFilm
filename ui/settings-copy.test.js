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
    ...currentParams, exposure: 0.4, saturation: 0.3, tint: 0.2,
    sprocket_uv: [0.2, 0.8], sprocket_tolerance: 0.2, sprocket_feather: 0.1
};
const currentGeom = {
    crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0, flip_h: false,
    flip_v: false, rotate_90_count: 0, calibration_points: null, calibration_confirmed: false
};
const sourceGeom = {
    crop_rect: { x: 0.1, y: 0.2, width: 0.7, height: 0.6 }, angle: 2.5, flip_h: true,
    flip_v: false, rotate_90_count: 1,
    calibration_points: [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
    calibration_confirmed: true
};

const editOnly = mergeCopyPayload(
    currentParams,
    currentGeom,
    createCopyPayload(sourceParams, sourceGeom, ['edit'])
);
assert.equal(editOnly.params.exposure, 0.4);
assert.equal(editOnly.params.saturation, 0.3);
assert.deepEqual(editOnly.params.sprocket_uv, [-1, -1]);
assert.deepEqual(editOnly.geom, currentGeom);

const sprocketOnly = mergeCopyPayload(
    currentParams,
    currentGeom,
    createCopyPayload(sourceParams, sourceGeom, ['sprocket'])
);
assert.deepEqual(sprocketOnly.params.sprocket_uv, [0.2, 0.8]);
assert.equal(sprocketOnly.params.exposure, 0);

const geometryGroups = mergeCopyPayload(
    currentParams,
    currentGeom,
    createCopyPayload(sourceParams, sourceGeom, ['crop', 'transform'])
);
assert.deepEqual(geometryGroups.geom, sourceGeom);
assert.deepEqual(geometryGroups.params, currentParams);

assert.throws(() => createCopyPayload(sourceParams, sourceGeom, []), /At least one/);
console.log('Settings copy module tests passed.');
