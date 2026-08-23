const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const html = fs.readFileSync(path.join(root, 'ui', 'index.html'), 'utf8');
const main = fs.readFileSync(path.join(root, 'ui', 'main.js'), 'utf8');
const styleCss = fs.readFileSync(path.join(root, 'ui', 'style.css'), 'utf8');
const referenceCss = fs.readFileSync(path.join(root, 'ui', 'reference.css'), 'utf8');

assert.match(html, /class="view-toolbar calibration-toolbar"/, 'Calibration toolbar must reuse the shared view-toolbar');
assert.match(html, /class="calibration-profile-panel"/, 'Profile list must remain in its own panel');
assert.match(html, /class="calibration-detail-panel"/, 'Profile detail must remain in its own panel');
assert.match(html, /class="calibration-metadata-grid"/, 'Profile detail must expose metadata');
assert.match(html, /id="calibration-pipeline" class="calibration-pipeline"/, 'Profile detail must expose the pipeline');
assert.doesNotMatch(html, /id="calibration-profile-level"/, 'Calibration level must not be user-selectable');
assert.doesNotMatch(main, /calibrationProfileLevel/, 'Profile save must not accept a user-selected level');
assert.doesNotMatch(main, /calibrationReferenceKinds[\s\S]*?'film_base'/, 'Film base must stay in Roll calibration');
assert.doesNotMatch(main, /calibrationReferenceKinds[\s\S]*?'full_exposure'/, 'Full exposure must stay in Roll calibration');
assert.match(main, /calibrationProfiles\.unshift\(saved\)/, 'Saved Profile must immediately enter the visible list');
assert.match(main, /renderCalibrationWorkspace\(\);[\s\S]*?await loadCalibrationProfiles\(\)/, 'Saved Profile must render before database refresh');
assert.match(main, /selectedCalibrationProfileId = saved\.profile\.profile_id/, 'Saved Profile must become the selected detail');
assert.match(main, /calibrationPipeline\.replaceChildren/, 'Selected Profile must render pipeline stages');
assert.match(styleCss, /\.calibration-profile-panel,[\s\S]*?\.calibration-detail-panel[\s\S]*?border-radius:/, 'Both workspace panels must be rounded cards');
assert.match(styleCss, /\.calibration-stage:not\(:last-child\)::before[\s\S]*?width: 1px/, 'Profile pipeline must retain its vertical guide');
assert.match(referenceCss, /#view-calibration > \.calibration-toolbar[\s\S]*?border-radius: var\(--ui-radius\)/, 'Calibration toolbar must keep the shared rounded geometry');

console.log('Calibration Profile UI contract verified.');
