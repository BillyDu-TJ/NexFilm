// Contract checks for two user-visible features that span the UI and the Rust
// side: the per-roll note written at import time, and the linear / RAW export
// formats. The Rust half is verified by cargo tests; this script keeps the
// dialog wiring and the interface labels from drifting away from them.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const html = fs.readFileSync(path.join(root, 'ui/index.html'), 'utf8');
const main = fs.readFileSync(path.join(root, 'ui/main.js'), 'utf8');
const i18n = fs.readFileSync(path.join(root, 'ui/i18n.js'), 'utf8');
const commands = fs.readFileSync(path.join(root, 'src/commands.rs'), 'utf8');
const dngWriter = fs.readFileSync(path.join(root, 'src/dng_writer.rs'), 'utf8');
const persistence = fs.readFileSync(path.join(root, 'src/persistence.rs'), 'utf8');

// --- Roll note ------------------------------------------------------------

assert.match(
    html,
    /id="roll-notes"[^>]*maxlength="400"/,
    'The roll import dialog must offer a bounded note field',
);
assert.match(html, /id="history-roll-notes"/, 'The roll details panel must show the note');
assert.match(
    main,
    /notes: document\.getElementById\('roll-notes'\)\.value/,
    'Editing a roll must send the note back to the backend',
);
assert.match(
    main,
    /const notes = document\.getElementById\('roll-notes'\)\.value\.trim\(\);/,
    'Importing a roll must read the note field',
);
assert.match(
    main,
    /film_stock: film, camera, notes,/,
    'The imported roll must carry its note',
);
assert.match(
    commands,
    /pub async fn update_roll_metadata\([\s\S]*?notes: Option<String>/,
    'update_roll_metadata must accept the note',
);
assert.match(
    persistence,
    /add_roll_column_if_missing\(connection, "notes", "TEXT NOT NULL DEFAULT ''"\)/,
    'Existing databases must gain the note column without a rebuild',
);
assert.match(persistence, /roll\.notes,/, 'Roll saves must persist the note');

// --- Linear / RAW export --------------------------------------------------

for (const format of ['tiff16_linear', 'dng_linear', 'dng']) {
    assert.match(html, new RegExp(`<option value="${format}">`), `The export dialog must offer ${format}`);
    assert.match(
        main,
        new RegExp(`${format}: '[^']+'`),
        `${format} must have a display label`,
    );
    assert.match(
        commands,
        new RegExp(`"${format}"[^=]*=> Ok\\(Self::`),
        `The backend must parse the ${format} format id`,
    );
}
assert.match(main, /if \(format === 'dng' \|\| format === 'dng_linear'\) return 'dng';/, 'DNG exports need the dng extension');

assert.match(commands, /fn encode_export_buffer_linear\(/, 'Linear output must skip the display transfer curve');
assert.match(
    commands,
    /crate::color_science::build_linear_icc_profile\(output_space\)/,
    'A linear TIFF must be tagged with a linear ICC profile',
);
assert.match(
    commands,
    /xyz_d50_to_color_space_matrix\(output_space\)/,
    'A linear DNG must declare the matrix of its colour space',
);
assert.match(dngWriter, /PHOTOMETRIC_LINEAR_RAW: u16 = 34892/, 'Linear DNG output must use the LinearRaw photometric interpretation');
assert.match(dngWriter, /PHOTOMETRIC_CFA: u16 = 32803/, 'Camera RAW DNG output must declare a CFA image');
assert.match(
    commands,
    /if !export_format\.is_raw_container\(\) \{\s*export_dimensions\(/,
    'Raw DNG output must not be resized',
);
assert.match(
    commands,
    /export_format\.is_raw_container\(\) \{\s*\/\/ The source scan/,
    'Raw DNG output must bypass the Develop pipeline',
);

// The dialog has to disable the controls a raw DNG cannot honour, otherwise a
// user would set an output colour and a resize policy that silently do nothing.
assert.match(main, /const isRawDng = settings\.format === 'dng'/, 'The export dialog must recognize the raw DNG format');
for (const control of ['exportColorSpace', 'exportSharpening', 'exportResizeMode', 'exportUpscale']) {
    assert.match(main, new RegExp(`${control}\\.disabled = isRawDng`), `${control} must be disabled for raw DNG exports`);
}
assert.match(main, /exportFormatNote\.textContent = isRawDng/, 'The export dialog must explain what the selected format does');

// Every new label has to be translatable in both locales.
for (const key of [
    'import.rollNotes',
    'export.formatRawDngHint',
    'export.formatLinearHint',
    'export.formatDngRaw',
]) {
    const occurrences = i18n.split(`'${key}':`).length - 1;
    assert.equal(occurrences, 2, `${key} must exist in the English and Chinese dictionaries`);
}

console.log('Roll notes and linear/RAW export contract verified.');
