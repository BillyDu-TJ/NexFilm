const assert = require('node:assert/strict');
const { createCopyPayload, mergeCopyPayload } = require('./settings-copy.js');
require('./color-science.js');

const colorScience = globalThis.NexFilmColorScience;

const currentParams = {
    film_mode: 'Color', d_min: [0.1, 0.1, 0.1], d_max: [2, 2, 2], exposure: 0,
    gamma: 1, contrast: 0, saturation: 0, temperature: 0, tint: 0, exp_r: 0, exp_g: 0, exp_b: 0,
    highlights: 0, shadows: 0, lut_path: null, lut_opacity: 1,
    working_colorspace: 'linear-srgb', sprocket_uv: [-1, -1], sprocket_tolerance: 0.1,
    sprocket_feather: 0.05
};
const sourceParams = {
    ...currentParams, film_mode: 'BW', d_min: [0.2, 0.2, 0.2], exposure: 0.4,
    contrast: 0.25, saturation: 0.3, tint: 0.2, exp_r: 0.1, lut_path: 'film.cube', lut_opacity: 0.7,
    working_colorspace: 'acescg', sprocket_uv: [0.2, 0.8],
    sprocket_tolerance: 0.2, sprocket_feather: 0.1,
    sprocket_target_color: [0.2, 0.3, 0.4], sprocket_target_color_version: 2
};
const currentGeom = {
    crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0, flip_h: false,
    flip_v: false, rotate_90_count: 0, calibration_points: null,
    calibration_confirmed: false, perspective_vertical: 0, perspective_horizontal: 0,
    perspective_aspect: 0, lens_distortion: 0, perspective_scale: 1, constrain_crop: false
};
const sourceGeom = {
    crop_rect: { x: 0.1, y: 0.2, width: 0.7, height: 0.6 }, angle: 2.5, flip_h: true,
    flip_v: false, rotate_90_count: 1,
    perspective_vertical: 24, perspective_horizontal: -12, perspective_aspect: 8,
    lens_distortion: -32,
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

const densityLimits = createCopyPayload(
    { ...sourceParams, d_min_offset: 0.05, d_max_offset: -0.05 },
    sourceGeom,
    ['densityLimits']
);
assert.deepEqual(densityLimits.params, {
    d_min: [0.2, 0.2, 0.2],
    d_max: [2, 2, 2],
    d_min_offset: 0.05,
    d_max_offset: -0.05
});

const perspectiveOnly = mergeCopyPayload(
    currentParams,
    currentGeom,
    createCopyPayload(sourceParams, sourceGeom, ['perspective'])
);
assert.equal(perspectiveOnly.geom.lens_distortion, -32);

const sprocketPoint = createCopyPayload(sourceParams, sourceGeom, ['sprocketPoint']);
assert.deepEqual(sprocketPoint.params, {
    sprocket_uv: [0.2, 0.8],
    sprocket_target_color: [0.2, 0.3, 0.4],
    sprocket_target_color_version: 2
});
const legacySprocketPoint = createCopyPayload(
    { ...sourceParams, sprocket_target_color_version: undefined },
    sourceGeom,
    ['sprocketPoint']
);
assert.deepEqual(legacySprocketPoint.params.sprocket_target_color, [0.2, 0.3, 0.4]);
assert.equal(legacySprocketPoint.params.sprocket_target_color_version, null);

for (const previousVersion of [undefined, null, 1]) {
    const targetParams = {
        ...currentParams,
        sprocket_uv: [0.4, 0.2],
        sprocket_target_color: [0.1, 0.1, 0.1],
        sprocket_target_color_version: previousVersion
    };
    const pasted = mergeCopyPayload(targetParams, currentGeom, sprocketPoint);
    assert.deepEqual(pasted.params.sprocket_uv, sourceParams.sprocket_uv);
    assert.deepEqual(pasted.params.sprocket_target_color, sourceParams.sprocket_target_color);
    assert.equal(
        pasted.params.sprocket_target_color_version,
        2,
        `a v2 sample must remain paired after replacing target version ${previousVersion}`
    );
}

const clearedSprocketPoint = createCopyPayload(
    { ...sourceParams, sprocket_target_color: null, sprocket_target_color_version: undefined },
    sourceGeom,
    ['sprocketPoint']
);
assert.equal(clearedSprocketPoint.params.sprocket_target_color, null);
assert.equal(clearedSprocketPoint.params.sprocket_target_color_version, null);
const clearedTarget = mergeCopyPayload(
    {
        ...currentParams,
        sprocket_target_color: [0.2, 0.3, 0.4],
        sprocket_target_color_version: 2
    },
    currentGeom,
    clearedSprocketPoint
);
assert.equal(clearedTarget.params.sprocket_target_color, null);
assert.equal(clearedTarget.params.sprocket_target_color_version, null);

const legacyClearedTarget = mergeCopyPayload(
    {
        ...currentParams,
        sprocket_target_color: [0.2, 0.3, 0.4],
        sprocket_target_color_version: 2
    },
    currentGeom,
    {
        settings: ['sprocketPoint'],
        params: { sprocket_uv: [0.2, 0.8], sprocket_target_color: null }
    }
);
assert.equal(legacyClearedTarget.params.sprocket_target_color_version, null);

const linearTarget = 0.8;
const v2SprocketPoint = createCopyPayload(
    {
        ...sourceParams,
        sprocket_target_color: [linearTarget, linearTarget, linearTarget],
        sprocket_target_color_version: 2
    },
    sourceGeom,
    ['sprocketPoint']
);
const v2Pasted = mergeCopyPayload(
    { ...currentParams, sprocket_target_color: null, sprocket_target_color_version: null },
    currentGeom,
    v2SprocketPoint
);
assert.equal(v2Pasted.params.sprocket_target_color_version, 2);
const retiredMigrationValue = colorScience.decodeProPhotoTransportSample(
    (linearTarget + 1.0) * 0.25
);
const pastedLinearTarget = v2Pasted.params.sprocket_target_color_version >= 2
    ? v2Pasted.params.sprocket_target_color[0]
    : retiredMigrationValue;
assert.ok(
    Math.abs(pastedLinearTarget - linearTarget) < 1e-6,
    'a v2 copied target must stay in linear range'
);
assert.ok(
    retiredMigrationValue < 0.05,
    'the retired v1 migration would visibly crush a 0.8 target'
);

const contrastOnly = createCopyPayload(sourceParams, sourceGeom, ['contrast']);
assert.deepEqual(contrastOnly.params, { contrast: 0.25 });
assert.equal(mergeCopyPayload(currentParams, currentGeom, contrastOnly).params.contrast, 0.25);
assert.equal(mergeCopyPayload(currentParams, currentGeom, contrastOnly).params.gamma, 1);

const curveOnly = createCopyPayload(
    {
        ...sourceParams,
        curve_rgb: [[0, 0], [0.5, 0.4], [1, 1]],
        curve_red: [[0, 0.1], [1, 1]],
        curve_green: [[0, 0], [1, 1]],
        curve_blue: [[0, 0], [1, 1]],
        sharpening_amount: 70,
        sharpening_radius: 1.4,
        sharpening_detail: 30,
        sharpening_masking: 20,
        texture: 12,
        clarity: -8,
        dehaze: 25
    },
    sourceGeom,
    ['curve']
);
assert.deepEqual(Object.keys(curveOnly.params).sort(), ['curve_blue', 'curve_green', 'curve_red', 'curve_rgb']);
assert.deepEqual(curveOnly.params.curve_rgb, [[0, 0], [0.5, 0.4], [1, 1]]);

const detailOnly = createCopyPayload(
    {
        ...sourceParams,
        curve_rgb: [[0, 0], [0.5, 0.4], [1, 1]],
        sharpening_amount: 70,
        sharpening_radius: 1.4,
        sharpening_detail: 30,
        sharpening_masking: 20,
        texture: 12,
        clarity: -8,
        dehaze: 25
    },
    sourceGeom,
    ['sharpen', 'clarity']
);
assert.deepEqual(detailOnly.params, {
    sharpening_amount: 70,
    sharpening_radius: 1.4,
    sharpening_detail: 30,
    sharpening_masking: 20,
    clarity: -8
});

const mergedDetail = mergeCopyPayload(
    { ...currentParams, texture: 5, clarity: 0 },
    currentGeom,
    detailOnly
);
assert.equal(mergedDetail.params.clarity, -8);
assert.equal(mergedDetail.params.texture, 5);
assert.equal(mergedDetail.params.sharpening_amount, 70);

const legacyEdit = mergeCopyPayload(
    currentParams,
    currentGeom,
    createCopyPayload(sourceParams, sourceGeom, ['edit'])
);
assert.equal(legacyEdit.params.exposure, 0.4);
assert.equal(legacyEdit.params.working_colorspace, 'acescg');
assert.deepEqual(legacyEdit.params.sprocket_uv, [-1, -1]);

const sourceExtra = {
    base_density: [0.85, 1.15, 1.45],
    base_color: { base_r: 9200, base_g: 4600, base_b: 2300 },
    base_source: 'detected_film_base',
    invert_active: true
};
const filmBasePayload = createCopyPayload(sourceParams, sourceGeom, ['filmBase'], sourceExtra);
assert.deepEqual(filmBasePayload.settings, ['filmBase']);
assert.deepEqual(filmBasePayload.extra, sourceExtra);

const mergedFilmBase = mergeCopyPayload(currentParams, currentGeom, filmBasePayload, {
    base_density: [0, 0, 0],
    invert_active: false
});
assert.deepEqual(mergedFilmBase.extra.base_density, [0.85, 1.15, 1.45]);
assert.equal(mergedFilmBase.extra.invert_active, true);

assert.throws(() => createCopyPayload(sourceParams, sourceGeom, []), /At least one/);
console.log('Settings copy module tests passed.');
