const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const main = fs.readFileSync(path.join(root, 'ui', 'main.js'), 'utf8');
const html = fs.readFileSync(path.join(root, 'ui', 'index.html'), 'utf8');
const css = fs.readFileSync(path.join(root, 'ui', 'style.css'), 'utf8');

for (const id of [
    'btn-delete-library-images',
    'btn-delete-develop-image',
    'btn-delete-roll-images',
    'btn-edit-roll',
]) {
    assert.match(html, new RegExp(`id=["']${id}["']`), `${id} is missing`);
}

assert.match(main, /function createLooseImportRoll\(paths\)[\s\S]*?roll_id:\s*`loose_/);
assert.match(main, /format:\s*'Loose'/);
assert.match(main, /let currentRollViewId = null;\s*let historyRollViewId = null;/);
assert.match(main, /historyRollViewId === currentRollViewId/);
assert.match(
    main,
    /currentRollViewId = rollId;[\s\S]*?historyRollViewId = rollId;/,
    'Continue Editing must open the selected roll in the history view',
);
assert.doesNotMatch(
    main,
    /invoke\(['"]import_images['"]/,
    'Frontend imports must use import_roll so every batch remains manageable',
);

const beginImport = main.match(/async function beginWorkingImport\([\s\S]*?\n\}/)?.[0] || '';
assert.ok(beginImport.indexOf('resetWorkingLibrary();') >= 0, 'Import does not reset the working Library');
assert.ok(
    beginImport.indexOf('resetWorkingLibrary();') < beginImport.indexOf('currentRollViewId = rollId;'),
    'The previous Library must be reset before the new roll becomes active',
);

const promoteHandler = main.match(
    /document\.getElementById\('btn-promote-roll'\)\.addEventListener[\s\S]*?document\.getElementById\('btn-history-back'\)/,
)?.[0] || '';
for (const contract of [
    "await invoke('promote_roll'",
    "await invoke('get_filmstrip')",
    'resetWorkingLibrary();',
    'const promotedRollId = historyRollViewId;',
    'currentRollViewId = promotedRollId;',
    'isRollEditing = true;',
    "switchView('library');",
]) {
    assert.ok(promoteHandler.includes(contract), `Promote handler is missing: ${contract}`);
}

assert.match(main, /deleteSourceFiles:\s*choice === 'files'/);
assert.match(main, /data-delete-choice="catalog"/);
assert.match(main, /data-delete-choice="files"/);
assert.match(main, /invoke\('delete_images'/);
assert.match(main, /data-delete-image-choice="catalog"/);
assert.match(main, /data-delete-image-choice="files"/);
assert.match(main, /btnDeleteLibraryImages.*requestImageDeletion/);
assert.match(main, /btnDeleteDevelopImage.*requestImageDeletion/);
assert.match(main, /btnDeleteRollImages.*requestImageDeletion/);
assert.match(html, /id="temperature"[^>]*temperature-track/);
assert.match(html, /id="tint"[^>]*tint-track/);
assert.match(css, /\.temperature-track::[\s\S]*?#3976d2[\s\S]*?#d9682f/);
assert.match(css, /\.tint-track::[\s\S]*?#36a565[\s\S]*?#874ca2/);
const inspectorHtml = html.slice(
    html.indexOf('id="develop-inspector"'),
    html.indexOf('<!-- Sponsor Modal -->'),
);
const inspectorOrder = [
    'id="btn-copy-settings"',
    'id="btn-auto-color"',
    'id="btn-mode-color"',
    'Density Limits',
    'Printer Lights',
    'Aesthetics',
    'Sprocket Settings',
    'Input Color Science',
    'Print Film Emulation',
];
let previousInspectorPosition = -1;
for (const marker of inspectorOrder) {
    const position = inspectorHtml.indexOf(marker);
    assert.ok(position > previousInspectorPosition, `Develop inspector order is incorrect at: ${marker}`);
    previousInspectorPosition = position;
}
assert.match(css, /#develop-inspector\.calibration-locked\s*\{[\s\S]*?overflow:\s*hidden\s*!important/);
assert.match(main, /function setDevelopInspectorCalibrationLocked\(locked\)[\s\S]*?scrollTop = 0/);
assert.match(main, /function enterCalibrationMode\(\)[\s\S]*?setDevelopInspectorCalibrationLocked\(true\)/);
assert.match(main, /btn-confirm-calibration[\s\S]*?setDevelopInspectorCalibrationLocked\(false\)/);
assert.match(main, /invoke\('update_roll_metadata'/);
assert.match(main, /if \(importInProgress\)[\s\S]*?Wait for the current import to finish/);

assert.match(main, /libDiv\.onmousedown = event =>[\s\S]*?clearNativeSelection\(event\)/);
assert.match(main, /libDiv\.ondblclick = event =>[\s\S]*?clearNativeSelection\(event\)/);
assert.match(css, /\.library-item, \.film-item, \.roll-row[\s\S]*?user-select:\s*none/);

console.log('Library and roll management contract verified.');
