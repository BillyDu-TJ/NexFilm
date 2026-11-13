const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const context = { globalThis: {} };
vm.runInNewContext(fs.readFileSync(__dirname + '/color-science.js', 'utf8'), context);
const science = context.globalThis.NexFilmColorScience;

for (const space of [
    'linear-srgb', 'linear-display-p3', 'linear-adobe-rgb', 'linear-rec2020',
    'linear-prophoto-rgb', 'linear-aces', 'linear-acescg',
]) {
    const matrix = science.getWorkingToSrgbMatrix(space);
    assert.equal(matrix.length, 9);
    assert.ok([...matrix].every(Number.isFinite));
    assert.deepEqual(science.getWorkingLuma(space).length, 3);
}

const srgb = science.getWorkingToSrgbMatrix('linear-srgb');
assert.ok(Math.abs(srgb[0] - 1) < 1e-5);
assert.ok(Math.abs(srgb[4] - 1) < 1e-5);
assert.ok(Math.abs(srgb[8] - 1) < 1e-5);

const commandSource = fs.readFileSync(__dirname + '/../src/commands.rs', 'utf8');
for (const [name, expected] of Object.entries(science.PROPHOTO_TRANSPORT)) {
    const match = commandSource.match(
        new RegExp(`const PROPHOTO_TRANSPORT_${name}: f32 = ([^;]+);`)
    );
    assert.ok(match, `Rust constant PROPHOTO_TRANSPORT_${name} is missing`);
    assert.equal(Number(match[1]), expected, `PROPHOTO_TRANSPORT_${name} drifted from Rust`);
}

function assertClose(actual, expected, epsilon = 1e-12) {
    assert.ok(Math.abs(actual - expected) < epsilon, `${actual} != ${expected}`);
}

for (const [encoded, expected] of [
    [0.0, -1.0],
    [0.0625, -0.5],
    [0.124, -0.008],
    [0.125, 0.0],
    [0.8335432183158692, 0.5],
    [0.8772000733509779, 0.8],
    [0.8979294365914244, 1.0],
    [1.0, 3.0],
]) {
    assertClose(science.decodeProPhotoTransportSample(encoded), expected);
}

const linearAverage = (
    science.decodeProPhotoTransportSample(0.8335432183158692)
    + science.decodeProPhotoTransportSample(0.8979294365914244)
) / 2;
const decodedEncodedAverage = science.decodeProPhotoTransportSample(
    (0.8335432183158692 + 0.8979294365914244) / 2
);
assertClose(linearAverage, 0.75);
assert.ok(
    Math.abs(linearAverage - decodedEncodedAverage) > 0.01,
    'the sprocket sampler must average decoded linear samples, not encoded values'
);

const mainSource = fs.readFileSync(__dirname + '/main.js', 'utf8');
assert.match(
    mainSource,
    /NexFilmColorScience\.decodeProPhotoTransportSample\(sample \/ 65535\.0\)/
);
assert.match(mainSource, /sumR \+= toSprocketTargetSample\(proxyPixels\[idx\]\);/);
assert.match(
    mainSource,
    /decodeProPhotoTransportSample\(\(value \+ 1\.0\) \* 0\.25\)/
);
assert.match(mainSource, /function migrateRetiredSprocketTargetColor\(\)/);
assert.ok(
    (mainSource.match(/migrateRetiredSprocketTargetColor\(\);/g) || []).length >= 2,
    'sprocket target migration must run after proxy load and parameter restore'
);
assert.match(mainSource, /params\.sprocket_target_color_version != null/);
const jsTargetVersion = mainSource.match(
    /const PROPHOTO_TRANSPORT_TARGET_COLOR_VERSION = (\d+);/
);
const rustTargetVersion = commandSource.match(
    /const PROPHOTO_TRANSPORT_TARGET_COLOR_VERSION: u32 = (\d+);/
);
assert.ok(jsTargetVersion, 'the frontend sprocket target version is missing');
assert.ok(rustTargetVersion, 'the Rust sprocket target version is missing');
assert.equal(jsTargetVersion[1], rustTargetVersion[1]);
assert.doesNotMatch(mainSource, /avgR \* 4\.0 - 1\.0/);
console.log('Color science UI contract verified.');
