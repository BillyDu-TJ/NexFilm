const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const main = fs.readFileSync(path.join(root, 'ui', 'main.js'), 'utf8');
const html = fs.readFileSync(path.join(root, 'ui', 'index.html'), 'utf8');
const css = fs.readFileSync(path.join(root, 'ui', 'style.css'), 'utf8');

for (const id of [
    'btn-delete-library-roll',
    'btn-delete-develop-roll',
    'btn-delete-current-roll',
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
assert.match(main, /invoke\('update_roll_metadata'/);
assert.match(main, /if \(importInProgress\)[\s\S]*?Wait for the current import to finish/);

assert.match(main, /libDiv\.onmousedown = event =>[\s\S]*?clearNativeSelection\(event\)/);
assert.match(main, /libDiv\.ondblclick = event =>[\s\S]*?clearNativeSelection\(event\)/);
assert.match(css, /\.library-item, \.film-item, \.roll-row[\s\S]*?user-select:\s*none/);

console.log('Library and roll management contract verified.');
