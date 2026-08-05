const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '..', 'ui', 'main.js'), 'utf8');
const sliderSetupStart = source.indexOf(
    'for (const key in sliders) {',
    source.indexOf('function scheduleBackendSync')
);
const sliderSetupEnd = source.indexOf('function setupEditableSliderValues()');
assert.ok(sliderSetupStart >= 0, 'Native slider setup is missing');
assert.ok(sliderSetupEnd > sliderSetupStart, 'Native slider setup boundary is missing');

const sliderSetup = source.slice(sliderSetupStart, sliderSetupEnd);
assert.doesNotMatch(
    sliderSetup,
    /setPointerCapture|lostpointercapture/,
    'Native range inputs must keep the browser drag implementation'
);
assert.match(source, /window\.addEventListener\('pointerup',\s*\(\) => activeSliderEnd\?\.\(\)\)/);
assert.match(source, /window\.addEventListener\('pointercancel',\s*\(\) => activeSliderEnd\?\.\(\)\)/);

console.log('Native slider drag contract verified.');
