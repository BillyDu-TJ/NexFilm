const assert = require('node:assert/strict');
const {
    getContactSheetFormat,
    getContactSheetLayout,
    getPerforationLayout,
    draw135EdgeCodes,
    get120EdgeCode,
    draw120EdgeCodes,
    drawNexfilmLockup,
    contactSheetTheme,
    createContactSheetFilename,
} = require('../ui/contact-sheet.js');

const format135 = getContactSheetLayout('135');
assert.equal(format135.framesPerRow, 6);
assert.ok(format135.horizontalGap > 0, '135 frames must have a visible black divider');
assert.ok(format135.imageWidth < (3000 - 200) / 6, '135 images must leave room for dividers');
assert.ok(
    Math.abs(format135.horizontalGap / format135.imageWidth - 2 / 36) < 1e-9,
    '135 frame gap must match the 2 mm gap of a 36 mm frame'
);
assert.ok(
    Math.abs(format135.borderRatio - 5.5 / 24) < 1e-9,
    '135 rebate must match the 5.5 mm of 35 mm film outside a 24 mm frame'
);

// Sprocket holes are laid out along the whole strip at the real 4.75 mm pitch.
// That is what keeps the perforations evenly spaced instead of restarting with
// a visible seam every time a frame ends.
const stripWidth = format135.imageWidth * 6 + format135.horizontalGap * 5;
const perforations = getPerforationLayout({
    stripLeft: 100,
    stripWidth,
    frameWidth: format135.imageWidth,
});
assert.equal(perforations.count, 48, 'a six frame 135 strip carries eight perforations per frame');
assert.ok(perforations.holeWidth < perforations.pitch, 'perforations must not touch each other');
assert.ok(
    Math.abs(perforations.holeWidth / perforations.pitch - 1.98 / 4.75) < 1e-9,
    'perforation width must be the 1.98 mm the hole takes along the film'
);
assert.ok(
    Math.abs(perforations.holeHeight / perforations.pitch - 2.79 / 4.75) < 1e-9,
    'perforation height must be the 2.79 mm it takes across the film'
);
assert.ok(
    perforations.holeHeight > perforations.holeWidth,
    'KS perforations are slots standing across the strip, not sideways rectangles'
);
for (let index = 1; index < perforations.centers.length; index += 1) {
    const delta = perforations.centers[index] - perforations.centers[index - 1];
    assert.ok(
        Math.abs(delta - perforations.pitch) < 1e-9,
        'sprocket pitch must stay uniform across frame boundaries'
    );
}
assert.ok(
    perforations.centers[perforations.centers.length - 1] > 100 + stripWidth - perforations.pitch,
    'perforations must run to the end of the strip'
);

const drawnHoles = [];
const stripFrames = [];
for (let frameIndex = 0; frameIndex < 6; frameIndex += 1) {
    stripFrames.push({
        x: 100 + frameIndex * (format135.imageWidth + format135.horizontalGap),
        width: format135.imageWidth,
        frameNumber: frameIndex + 1,
        label: frameIndex === 1 ? 'NEXFILM' : null,
    });
}
const stripCodes = [];
const stripContext = {
    save() {},
    restore() {},
    beginPath() {},
    fill() {},
    roundRect(x, y) { drawnHoles.push({ x, y }); },
    fillText(text) { stripCodes.push(text); },
};
draw135EdgeCodes(stripContext, {
    stripLeft: 100,
    stripTop: 0,
    stripWidth,
    imageHeight: format135.imageHeight,
    borderHeight: format135.imageHeight * format135.borderRatio,
    frames: stripFrames,
});
assert.equal(drawnHoles.length, perforations.count * 2, 'one even perforation run is drawn per strip edge');
assert.deepEqual(stripCodes, [
    '1', '1A', 'NEXFILM', '2', '2A', '3', '3A', '4', '4A', '5', '5A', '6', '6A',
]);

// Paper must stay clearly lighter than the film base, otherwise the strips
// disappear into the sheet.
function relativeLuminance(hex) {
    const value = hex.replace('#', '');
    const [r, g, b] = [0, 2, 4].map(offset => parseInt(value.slice(offset, offset + 2), 16) / 255);
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
assert.ok(
    relativeLuminance(contactSheetTheme.paper) - relativeLuminance(contactSheetTheme.filmBase) > 0.5,
    'contact sheet paper must contrast with the film base'
);

const lockupText = [];
const lockupContext = {
    save() {},
    restore() {},
    translate() {},
    scale() {},
    fill() {},
    fillText(text) { lockupText.push(text); },
};
const lockup = drawNexfilmLockup(lockupContext, { x: 100, bottomY: 400, markHeight: 92 });
assert.ok(lockup.width > 400, 'footer lockup must be a large mark plus wordmark');
// Node has no Path2D, so the documented fallback is a plain wordmark.
assert.deepEqual(lockupText, ['NEXFILM']);

const mediumFormatProfiles = [
    ['120 (645)', 4, 4 / 3],
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
assert.ok(format645.horizontalGap < format135.horizontalGap, '120 frame gaps must be narrower than 135 dividers');
assert.ok(format645.borderRatio < format135.borderRatio / 3, '120 rebates must be much narrower than 135 borders');
assert.ok(
    Math.abs((1 + format645.borderRatio * 2) - (61.7 / 56)) < 0.01,
    '120 film-to-image height ratio must stay close to physical film dimensions'
);

assert.deepEqual(
    getContactSheetFormat('120 (6x12)'),
    {
        is120: true,
        framesPerRow: 2,
        aspect: 1 / 2,
        frameGapRatio: 0.035,
        borderRatio: 0.052,
        verticalGapRatio: 0.11,
    }
);
assert.deepEqual(
    getContactSheetFormat('120 (6x17)'),
    {
        is120: true,
        framesPerRow: 1,
        aspect: 6 / 17,
        frameGapRatio: 0.035,
        borderRatio: 0.052,
        verticalGapRatio: 0.11,
    }
);

assert.deepEqual(
    get120EdgeCode(1, 'Kodak Portra 400'),
    {
        filmLabel: 'KODAK PORTRA 400',
        topFrameNumber: 41,
        bottomFrameNumber: 1,
        formatLabel: '120',
        markerDirection: 'right',
    }
);

const markerPath = [];
const mockContext = {
    save() {},
    restore() {},
    beginPath() { markerPath.length = 0; },
    moveTo(x, y) { markerPath.push(['moveTo', x, y]); },
    lineTo(x, y) { markerPath.push(['lineTo', x, y]); },
    closePath() {},
    stroke() {},
    fillText() {},
    measureText(text) { return { width: String(text).length * 8 }; },
};
draw120EdgeCodes(mockContext, {
    x: 100,
    y: 100,
    imageWidth: 650,
    imageHeight: 650,
    borderHeight: 34,
    frameNumber: 1,
    filmName: 'Kodak Portra 400',
});
assert.ok(
    markerPath[1][1] > markerPath[0][1] && markerPath[1][1] > markerPath[2][1],
    '120 orientation marker must point in the direction of increasing frame numbers'
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
