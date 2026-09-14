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
assert.match(main, /function runAutoInvertRoll\(rollId\)/);
assert.match(main, /invoke\('auto_invert_roll', \{\s*rollId,\s*frameId:/);
assert.match(main, /auto_invert_roll_progress/);
assert.match(html, /id="btn-auto-color-roll"/);
assert.match(main, /btnAutoColorRoll\.addEventListener\('click'/);
assert.match(
    main,
    /const priorityId = activeId;[\s\S]*const frameItems = \[\.\.\.fetchedFrameItems\]\.sort/,
    'Roll auto invert must prioritize the active frame before the persisted roll order',
);
assert.match(
    main,
    /await invoke\('prepare_proxy',[\s\S]*?await invoke\('auto_invert_roll',[\s\S]*?await refreshRollFilmstripThumbnails\(rollId\)/,
    'Roll auto invert must prepare, process, and refresh each frame in sequence',
);
assert.match(
    main,
    /progressCardStack\(\)\.appendChild\(autoInvertRollProgress\)/,
    'Roll progress must share the export-style lower-right panel column',
);
// A batch and an export can run at the same time; sharing one column keeps the
// two cards from covering each other, which made the export look frozen.
assert.match(
    main,
    /function progressCardStack\(\)[\s\S]*?fixed bottom-6 right-6 z-\[100\] flex w-72 flex-col items-end gap-3/,
    'Progress cards must stack in one column',
);
// A full-resolution frame takes seconds: the export card names the frame it is
// working on, and the backend reports the frame before it starts decoding it.
assert.match(
    main,
    /export-progress-file/,
    'Export progress must name the frame being written',
);
const commandSource = fs.readFileSync(path.join(__dirname, '..', 'src', 'commands.rs'), 'utf8');
assert.match(
    commandSource,
    /export_snapshots\.iter\(\)\.for_each\(\|snapshot\| \{[\s\S]{0,900}?"stage": "decoding"/,
    'The export must report each frame before it starts decoding',
);
// Orientation belongs to the renderer: a flipped frame must export the same way
// up as the Develop preview shows it, and the filmstrip thumbnail must match.
assert.match(
    commandSource,
    /fn render_f32_shader_equivalent\([\s\S]{0,1200}?map_oriented_uv_to_source\(uv, source_width, source_height, geom\)/,
    'The export renderer must apply the frame geometry',
);
assert.match(
    commandSource,
    /let oriented_thumb = orient_display_image\(thumb_8bit, &item\.geom\)/,
    'The filmstrip thumbnail must apply the frame geometry',
);
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
assert.match(
    main,
    /function pipelineRequiresFilmArea\(state = currentPipelineState\) \{\s*return !pipelineHasCompleteRollAnchors\(state\);\s*\}/,
    'Film Area may only be skipped when both roll density anchors are present',
);
const pipelineRequiresFilmArea = main.match(
    /function pipelineRequiresFilmArea\(state = currentPipelineState\) \{[\s\S]*?\n\}/,
)?.[0] || '';
assert.doesNotMatch(
    pipelineRequiresFilmArea,
    /isLooseImportRoll/,
    'Loose imports must not bypass Film Area when a density endpoint is missing',
);

assert.match(main, /libDiv\.onmousedown = event =>[\s\S]*?clearNativeSelection\(event\)/);
assert.match(main, /libDiv\.ondblclick = event =>[\s\S]*?clearNativeSelection\(event\)/);
assert.match(css, /\.library-item, \.film-item, \.roll-row[\s\S]*?user-select:\s*none/);
assert.match(
    main,
    /isDensityReferenceSelectionMode && scope === 'library'[\s\S]*?additive:\s*true/,
    'Density reference selection must toggle multiple Library images without modifier keys',
);
const selectAllHandler = main.match(
    /btnSelectAll\.addEventListener\('click',[\s\S]*?\n\}\);/,
)?.[0] || '';
assert.ok(
    selectAllHandler && !selectAllHandler.includes('if (isDensityReferenceSelectionMode) return;'),
    'Select All must remain usable for density reference multi-selection',
);
assert.match(
    main,
    /isDensityReferenceSelectionMode \? 'calibration\.confirmSelection' : 'calibration\.action'/,
    'The calibration action must become Confirm while selecting references',
);
assert.match(
    main,
    /if \(items\.length === 0\) \{[\s\S]{0,400}?showToast\(i18nText\('calibration\.selectFramesFirst'\)/,
    'Confirming without a selection must explain what to pick instead of doing nothing',
);
// Entering the selection must not throw away the frames the user already
// picked, which is what made the flow ask for the selection twice.
assert.match(
    main,
    /function setDensityReferenceSelectionMode\(enabled\) \{[\s\S]*?if \(enabled\) \{[\s\S]*?lastSelectionScope = 'density-reference';[\s\S]*?\} else \{[\s\S]*?selectedLibraryIds\.clear\(\);/,
    'Entering the density reference selection must keep the existing selection',
);
const librarySelectionUi = main.match(
    /function updateLibrarySelectionUI\(\) \{[\s\S]*?\n\}/,
)?.[0] || '';
assert.ok(
    librarySelectionUi.includes("btnCalibrateDensity.disabled = importInProgress")
        && !librarySelectionUi.includes('calibratableItems.length'),
    'The calibration entry must not depend on a preselected or already loaded image',
);
assert.match(
    main,
    /function openDensityCalibration\(items\)[\s\S]*?selectedRollItems\s*=\s*items\.filter\(candidate => candidate\.roll_id === item\.roll_id\)[\s\S]*?itemIds:\s*selectedRollItems\.map\(candidate => candidate\.id\)/,
    'Density calibration must retain every selected reference image',
);
assert.match(main, /aggregate_roll_density_references/);
assert.match(main, /auto_invert_roll/);
assert.doesNotMatch(
    html,
    /id="btn-calibrate-density"[^>]*\sdisabled(?:\s|=|>)/,
    'The calibration entry must be clickable before any Library image is selected',
);
assert.match(
    main,
    /densityCalibrationModal\.classList\.add\('is-open'\)[\s\S]*?void selectDensityCalibrationSource\(item\.id\)/,
    'Calibration modal must open before the full-resolution preview finishes decoding',
);
assert.match(html, /id="density-calibration-source-list"/);
assert.match(
    css,
    /\.density-calibration-preview img\s*\{[\s\S]*?width:\s*100%[\s\S]*?height:\s*100%[\s\S]*?object-fit:\s*contain/,
    'Density calibration preview must show the complete frame without stretching',
);
assert.match(
    main,
    /function densityCalibrationContainGeometry\(\)[\s\S]*?Math\.min\(previewRect\.width \/ sourceWidth, previewRect\.height \/ sourceHeight\)/,
    'Density calibration sampling must account for contain letterboxing',
);
assert.match(
    main,
    /function densityCalibrationSourcePoint\(clientX, clientY\)[\s\S]*?geometry\.offsetX[\s\S]*?geometry\.renderedWidth/,
    'Density calibration clicks must map back to source coordinates',
);
// Calibration is about placing two points. It must show the preview that is
// already in memory and let the sampling command decode in the background,
// instead of blocking the modal on a RAW decode and swapping the picture under
// the user's cursor.
assert.doesNotMatch(
    main,
    /await invoke\('get_density_calibration_preview'/,
    'Opening the calibration view must not wait for a RAW decode',
);
assert.match(
    main,
    /const source = item\.embedded_thumbnail_base64 \|\| item\.thumbnail_base64/,
    'Calibration must show the frame preview that is already loaded',
);
assert.match(
    main,
    /setDensitySampleMarker\(kind, x, y\);[\s\S]*?await invoke\('sample_roll_density_reference'/,
    'A calibration click must record its position before the sample is decoded',
);
assert.doesNotMatch(
    css,
    /\.density-reference-selection \.library-item\s*\{[^}]*cursor:\s*crosshair/,
    'Selecting the calibration source image must not imply pixel sampling',
);
assert.match(
    css,
    /\.density-calibration-preview\.is-sampling img\s*\{[^}]*cursor:\s*crosshair/,
    'The crosshair must be limited to active sampling inside the calibration modal',
);

console.log('Library and roll management contract verified.');
