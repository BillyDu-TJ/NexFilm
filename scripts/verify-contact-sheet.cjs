const assert = require('node:assert/strict');
const {
    getContactSheetFormat,
    getContactSheetLayout,
    createContactSheetFilename,
} = require('../ui/contact-sheet.js');

const format135 = getContactSheetLayout('135');
assert.equal(format135.framesPerRow, 6);
assert.ok(format135.horizontalGap > 0, '135 frames must have a visible black divider');
assert.ok(format135.imageWidth < (3000 - 200) / 6, '135 images must leave room for dividers');

const mediumFormatProfiles = [
    ['120 (645)', 4, 3 / 4],
    ['120 (6x6)', 4, 1],
    ['120 (6x7)', 3, 6 / 7],
    ['120 (6x9)', 2, 2 / 3],
    ['120 (6x12)', 2, 1 / 2],
    ['120 (6x17)', 1, 6 / 17],
];
for (const [format, framesPerRow, aspect] of mediumFormatProfiles) {
    const layout = getContactSheetLayout(format);
    assert.equal(layout.framesPerRow, framesPerRow, `${format} row density is incorrect`);
    assert.equal(layout.aspect, aspect, `${format} aspect ratio is incorrect`);
    const occupiedWidth = layout.imageWidth * framesPerRow
        + layout.horizontalGap * (framesPerRow - 1);
    assert.ok(Math.abs(occupiedWidth - 2800) < 1e-9, `${format} must fit inside the margins`);
}

const format645 = getContactSheetLayout('120 (645)');
assert.ok(format645.horizontalGap > format135.horizontalGap, '120 dividers must be wider than 135 dividers');

assert.deepEqual(
    getContactSheetFormat('120 (6x12)'),
    { is120: true, framesPerRow: 2, aspect: 1 / 2, horizontalGapRatio: 0.02 }
);
assert.deepEqual(
    getContactSheetFormat('120 (6x17)'),
    { is120: true, framesPerRow: 1, aspect: 6 / 17, horizontalGapRatio: 0.02 }
);

assert.equal(
    createContactSheetFilename({ roll_id: 'roll_42', camera: 'Contax RTS 2' }),
    'contact_sheet_roll_42_Contax_RTS_2.jpg'
);
assert.equal(
    createContactSheetFilename({ roll_id: 'roll:42', camera: 'Mamiya/RZ 67' }),
    'contact_sheet_roll_42_Mamiya_RZ_67.jpg'
);

console.log('Contact sheet layout and filename contract verified.');
