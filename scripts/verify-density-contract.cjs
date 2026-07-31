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
const shaderMatrixMatch = mainSource.match(/const mat3 STATUS_M = mat3\(([\s\S]*?)\);/);
assert.ok(shaderMatrixMatch, 'WebGL STATUS_M matrix was not found');
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
