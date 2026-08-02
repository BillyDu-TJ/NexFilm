const assert = require('node:assert/strict');
const {
    createExportInvokeArgs,
    describeResize,
    findUnknownTokens,
    formatExportTemplate,
    sanitizeFileStem,
    validateExportSettings,
} = require('./export-settings.js');

assert.equal(sanitizeFileStem('  Roll: 01 / frame*  '), 'Roll_ 01 _ frame_');
assert.equal(sanitizeFileStem('CON'), '_CON');
assert.equal(sanitizeFileStem('...'), 'Export');
assert.equal(
    formatExportTemplate('{Date}_{Roll}_{Original}_{Seq}', {
        date: '2026-08-01', roll: 'R:01', original: 'DSC/001', seq: '007',
    }),
    '2026-08-01_R_01_DSC_001_007'
);
assert.deepEqual(findUnknownTokens('{Roll}_{Unknown}_{Other}'), ['{Unknown}', '{Other}']);
assert.equal(validateExportSettings({ namingTemplate: '', resizeMode: 'original' }), 'Enter a filename template.');
assert.match(
    validateExportSettings({ namingTemplate: '{Roll}', resizeMode: 'long_edge', longEdge: 64 }),
    /between 256 and 32768/
);
assert.equal(validateExportSettings({ namingTemplate: '{Roll}_{Seq}', resizeMode: 'long_edge', longEdge: 4096 }), null);
assert.equal(describeResize({ resizeMode: 'original' }), 'Original dimensions');
assert.equal(describeResize({ resizeMode: 'long_edge', longEdge: 2048, allowUpscale: true }), '2048 px long edge, enlargement allowed');

const invokeArgs = createExportInvokeArgs(['image-1'], 'C:\\Exports', {
    format: 'jpeg', colorSpace: 'srgb', resizeMode: 'long_edge', longEdge: '2048',
    allowUpscale: false, sharpening: 'standard', namingTemplate: '{Roll}_{Seq}',
    conflictPolicy: 'unique', quality: '92',
});
assert.deepEqual(invokeArgs, {
    exportIds: ['image-1'],
    outputDir: 'C:\\Exports',
    format: 'jpeg',
    colorSpace: 'srgb',
    resizeMode: 'long_edge',
    longEdge: 2048,
    allowUpscale: false,
    sharpening: 'standard',
    namingTemplate: '{Roll}_{Seq}',
    conflictPolicy: 'unique',
    quality: 92,
});
assert.ok(!Object.hasOwn(invokeArgs, 'export_ids'), 'Tauri arguments must use camelCase');

console.log('Export settings contract verified.');
