const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {
    applyStatusMToPrintingDensity,
    getNeutralExposureOffsets,
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
assert.match(
    commandSource,
    /highlight_fraction/,
    'The Roll white point must be persisted with the density anchors',
);
// The Master D-Min/D-Max sliders stay adjustable on the Roll Anchored route:
// the manual trim is applied on top of the sampled anchors instead of being
// locked out, and the same trim reaches the Rust renderer.
assert.match(mainSource, /currentDMinOffset = current;/);
assert.match(mainSource, /d_min_offset: currentDMinOffset/);
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

console.log('Density-domain white balance contract verified.');
