const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {
    applyStatusMToPrintingDensity,
    getNeutralExposureOffsets,
    trimDensityEndpoints,
} = require('../ui/density-math.js');

function assertClose(actual, expected, epsilon = 1e-12) {
    assert.ok(Math.abs(actual - expected) < epsilon, `${actual} != ${expected}`);
}

const sampleDensity = [0.42, 0.58, 0.71];
const corrected = applyStatusMToPrintingDensity(sampleDensity);
assertClose(corrected[0], 1.0197 * 0.42 + 0.0317 * 0.58 + 0.0091 * 0.71);
assertClose(corrected[1], -0.0052 * 0.42 + 0.8933 * 0.58 + 0.0521 * 0.71);
assertClose(corrected[2], 0.0131 * 0.42 - 0.0011 * 0.58 + 0.9712 * 0.71);

const mainSource = fs.readFileSync(path.join(__dirname, '..', 'ui', 'main.js'), 'utf8');
const commandSource = fs.readFileSync(path.join(__dirname, '..', 'src', 'commands.rs'), 'utf8');
const shaderMatrixMatch = mainSource.match(/const mat3 STATUS_M = mat3\(([\s\S]*?)\);/);
assert.ok(shaderMatrixMatch, 'WebGL STATUS_M matrix was not found');
// A colour negative gains density where the scene is bright, so every route
// ramps the density upward from D-min. The shader must not invert the
// roll-anchored route a second time.
assert.match(mainSource, /\(density - effective_dmin\) \/ safe_range/);
assert.doesNotMatch(mainSource, /effective_dmax - density/);
assert.match(commandSource, /RollAnchoredDirectInvert/);
assert.match(commandSource, /roll_density_mapping_with_frame_base\(/);
assert.match(
    commandSource,
    /fn detect_frame_base_density\(/,
    'Roll rendering must measure the film base on each frame',
);
assert.match(
    commandSource,
    /fn detect_frame_highlight_fraction\(/,
    'Roll rendering must place the white point from the scene highlights',
);
// A pasted film base is a starting point, not a constant: one Roll can record
// a different clear-film density on either side of a scan pass, and
// subtracting another frame's figure tints that frame end to end. The paste
// therefore measures the target frame and only falls back to the copied figure
// when the frame has no analysis of its own.
assert.match(commandSource, /fn choose_pasted_film_base\(/);
assert.match(
    fs.readFileSync(path.join(__dirname, '..', 'src', 'pipeline.rs'), 'utf8'),
    /const INHERITED_FILM_BASE_SOURCE: &str = "inherited_film_base";/,
);
assert.match(
    commandSource,
    /let measured = item[\s\S]{0,220}?estimate_film_base_f32\(proxy, &item\.geom\)/,
    'Pasting a film base must measure the target frame',
);
const pasteHandler =
    mainSource.match(/btnPasteSettings\.addEventListener\('click',[\s\S]*?\n\}\);/)?.[0] || '';
assert.ok(pasteHandler, 'the Paste Settings handler was not found');
assert.ok(
    pasteHandler.indexOf('await updateBackendParams(targetId, nextParams);') <
        pasteHandler.indexOf("invoke('apply_film_base'"),
    'Pasted geometry and density limits must be persisted before the film base is measured',
);
assert.match(
    pasteHandler,
    /applied\?\.base_density/,
    'The preview must render the film base the backend installed',
);
assert.doesNotMatch(
    pasteHandler,
    /currentBaseDensity = copiedSettings\.extra\.base_density\.slice\(\);/,
    'The copied film base must not be rendered directly',
);
// Batch Apply and Copy Settings stay separated: the batch carries the frame's
// physical settings (Film Area, film base), while the display endpoints belong
// to Copy Settings. Each target measures its own base the next time it is
// decoded, which is why the batch never opens an image by itself.
const batchSource = fs.readFileSync(
    path.join(__dirname, '..', 'src', 'batch_settings.rs'),
    'utf8',
);
const batchCore = batchSource.split('#[cfg(test)]')[0];
assert.doesNotMatch(
    batchCore,
    /d_min|d_max/,
    'Batch Apply must not move another frame\'s display endpoints',
);
assert.match(batchSource, /fn mark_inherited_film_base\(/);
assert.match(batchSource, /fn target_keeps_its_own_base\(/);
assert.doesNotMatch(
    batchSource,
    /estimate_film_base_f32|decode_/,
    'Batch Apply must not decode or measure anything',
);
assert.match(
    commandSource,
    /fn remeasure_inherited_film_base\(/,
    'An inherited film base must be re-measured on the frame that inherited it',
);
assert.match(commandSource, /fn inherited_film_base_measurement\(/);
// The measurement runs where the pixels arrive — an already prepared proxy, a
// freshly decoded one, and the export decode — never in the batch itself.
assert.match(
    commandSource,
    /if current_long_edge >= target_long_edge[\s\S]{0,400}?remeasure_inherited_film_base\(&mut item\)/,
    'An already prepared frame must still re-measure an inherited base',
);
assert.match(
    commandSource,
    /item\.runtime_pipeline_key = Some\(final_resolution_key\);[\s\S]{0,400}?remeasure_inherited_film_base\(&mut item\)/,
    'A freshly decoded frame must re-measure an inherited base',
);
assert.match(
    commandSource,
    /inherited_film_base_measurement\(&render_pipeline_state, &geom_owned, &input\)/,
    'Exporting a frame must not print another frame\'s film base',
);
// Opening a frame must never de-mask it: stored analysis state is not a user
// action, so only Auto Invert or a Paste Settings commit may show a positive.
assert.doesNotMatch(
    mainSource,
    /storedDevelopedFrame/,
    'Opening a frame must not enable inversion by itself',
);
assert.match(
    mainSource,
    /autoInvertAppliedActiveImage = hasRenderedPreview;/,
    'Only a rendered preview may mark a frame as developed when it is opened',
);
// The Roll white point is one value shared by every frame, so it has to come
// from the brightest frame of the Roll. Sampling a few frames can only
// under-estimate it, and a white point below a frame's highlights clips them.
assert.match(
    commandSource,
    /fn measure_roll_highlight_frames\(/,
    'The Roll white point must be measured frame by frame on the whole Roll',
);
assert.doesNotMatch(
    commandSource,
    /median_value\(/,
    'A sampled median cannot be the Roll white point',
);
assert.doesNotMatch(
    commandSource,
    /let sample_step = \(item_arcs\.len\(\) \/ 5\)/,
    'The Roll white point must not be sampled from five frames',
);
assert.match(
    commandSource,
    /highlight_fraction/,
    'The Roll white point must be persisted with the density anchors',
);
assert.match(
    commandSource,
    /current\.max\(fraction\)/,
    'The Roll white point must keep the brightest frame',
);
// The Master D-Min/D-Max sliders stay adjustable on the Roll Anchored route:
// the manual trim is applied on top of the sampled anchors instead of being
// locked out, and the same trim reaches the Rust renderer.
assert.match(mainSource, /currentDMinOffset = current;/);
assert.match(mainSource, /d_min_offset: currentDMinOffset/);
// The trim is a whole-frame exposure/contrast move, and a sampled Roll gives
// every channel a different span: the slider amount is scaled by each channel's
// share of the span, in the shader and in the Rust renderers alike, so the same
// raw offset cannot tint the frame green.
assert.match(mainSource, /u_dmin_trim/);
assert.match(mainSource, /endpoint_span \/ endpoint_span_luma/);
assert.match(mainSource, /gl\.uniform1f\(u_dmax_trim_loc, currentDMaxOffset\)/);
assert.match(commandSource, /fn trim_density_endpoints|trim_density_endpoints\(/);
assert.doesNotMatch(
    commandSource,
    /\.map\(\|value\| value \+ density_max_offset\)/,
    'The Master trim must not be a uniform raw density shift',
);
assert.doesNotMatch(
    mainSource,
    /masterDmin\.el\.disabled\s*=/,
    'The Master D-Min slider must stay adjustable after Roll sampling',
);
assert.doesNotMatch(
    mainSource,
    /masterDmax\.el\.disabled\s*=/,
    'The Master D-Max slider must stay adjustable after Roll sampling',
);
assert.match(commandSource, /params\.density\.d_min_offset/);
// There is exactly one density maths for every input class, and it carries no
// per-channel display response: the shared density window plus the working-space
// conversion is the whole mapping. A per-channel film-response curve was tried
// and removed because it changed an already-validated look.
assert.doesNotMatch(
    commandSource,
    /display_response|DisplayResponse/,
    'The unified pipeline must not carry a per-channel display response',
);
assert.doesNotMatch(
    mainSource,
    /display_response|u_display_response/,
    'The WebGL shader must not apply a per-channel display response',
);
assert.match(commandSource, /pipeline_state\.content_range = None/);
assert.match(mainSource, /pipelineHasCompleteRollAnchors\(currentPipelineState\)\s*&&\s*!hasRenderedPreview/);
assert.match(
    mainSource,
    /autoInvertAppliedActiveImage\s*=\s*true;[\s\S]*proxyHasAnalyzedBase\s*=\s*true;[\s\S]*await reloadDevelopProxy/,
    'Single-frame Auto Invert must reload the analyzed proxy before rendering',
);
const shaderMatrix = shaderMatrixMatch[1]
    .split(',')
    .map(value => Number.parseFloat(value.trim()));
assert.equal(shaderMatrix.length, 9);
const shaderCorrected = [
    shaderMatrix[0] * sampleDensity[0] + shaderMatrix[3] * sampleDensity[1] + shaderMatrix[6] * sampleDensity[2],
    shaderMatrix[1] * sampleDensity[0] + shaderMatrix[4] * sampleDensity[1] + shaderMatrix[7] * sampleDensity[2],
    shaderMatrix[2] * sampleDensity[0] + shaderMatrix[5] * sampleDensity[1] + shaderMatrix[8] * sampleDensity[2],
];
shaderCorrected.forEach((value, channel) => assertClose(corrected[channel], value));

const greenExposure = 0.035;
const offsets = getNeutralExposureOffsets(sampleDensity, greenExposure);
assertClose(corrected[0] + offsets[0], corrected[1] + offsets[1]);
assertClose(corrected[2] + offsets[2], corrected[1] + offsets[1]);

// A Roll batch owns its own token: looking at another frame while it runs must
// leave the work alone, and only the progress card's cancel button or a newer
// batch may stop it.
const selectImageStart = mainSource.indexOf('async function selectImage(');
const selectImageTail = mainSource.slice(selectImageStart + 10);
const selectImageBody = mainSource.slice(
    selectImageStart,
    selectImageStart + 10 + selectImageTail.search(/\n(?:async )?function /)
);
assert.ok(selectImageBody.length > 200, 'selectImage body was not found');
assert.doesNotMatch(
    selectImageBody,
    /cancel_auto_invert_roll/,
    'Selecting a frame must not cancel the Roll batch',
);
assert.match(mainSource, /let autoInvertRollRevision = 0;/);
assert.match(mainSource, /batchToken !== autoInvertRollRevision\) break;/);
assert.match(
    mainSource,
    /invoke\('calibrate_roll_highlight_fraction'[\s\S]*?for \(const frame of frameItems\)/,
    'The Roll white point must be measured before the frames are rendered',
);

// The Master trim is scaled per channel, so a neutral frame stays neutral even
// though a sampled Roll measures a different span in every channel. The plain
// uniform shift the sliders used to apply splits the channels instead.
const rollLow = [0.02, -0.01, -0.05];
const rollHigh = [1.22, 1.54, 1.95];
const trimmedRoll = trimDensityEndpoints(rollLow, rollHigh, -0.03, 0.09);
const normalizedRoll = [0, 1, 2].map(channel => {
    const span = rollHigh[channel] - rollLow[channel];
    const density = rollLow[channel] + 0.5 * span;
    return (density - trimmedRoll.dMin[channel]) / (trimmedRoll.dMax[channel] - trimmedRoll.dMin[channel]);
});
assert.ok(
    Math.max(...normalizedRoll) - Math.min(...normalizedRoll) < 1e-9,
    `the Master trim split a neutral frame: ${normalizedRoll}`,
);
const shiftedRoll = [0, 1, 2].map(channel => {
    const span = rollHigh[channel] - rollLow[channel];
    const density = rollLow[channel] + 0.5 * span;
    return (density - (rollLow[channel] - 0.03)) / ((rollHigh[channel] + 0.09) - (rollLow[channel] - 0.03));
});
assert.ok(
    Math.max(...shiftedRoll) - Math.min(...shiftedRoll) > 1e-3,
    'a uniform raw density shift must split the channels',
);

console.log('Density-domain white balance contract verified.');
