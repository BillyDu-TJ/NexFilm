const assert = require('node:assert/strict');
const {
    getContactSheetFormat,
    getContactSheetLayout,
    getContactSheetPageLayout,
    getContactSheetPreviewSize,
    getPerforationLayout,
    draw135EdgeCodes,
    get120EdgeCode,
    draw120EdgeCodes,
    drawNexfilmLockup,
    drawContactSheet,
    contactSheetTheme,
    createContactSheetFilename,
    createContactSheetFilenameStem,
    validateContactSheetFilename,
    contactSheetExtension,
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
        fullFrameAspect: 0.552,
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
        fullFrameAspect: (6 / 17) * 1.104,
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
assert.equal(
    createContactSheetFilename({ roll_id: 'roll_42', camera: 'Contax RTS 2' }, 'png'),
    'contact_sheet_roll_42_Contax_RTS_2.png'
);
assert.equal(
    createContactSheetFilenameStem({ roll_id: 'roll_42', camera: 'Contax RTS 2' }),
    'contact_sheet_roll_42_Contax_RTS_2'
);
assert.equal(contactSheetExtension('png'), 'png');
assert.equal(contactSheetExtension('jpeg'), 'jpg');
assert.deepEqual(validateContactSheetFilename('  contact_sheet_01  ', 'jpeg'), {
    valid: true,
    code: '',
    stem: 'contact_sheet_01',
    filename: 'contact_sheet_01.jpg',
    extension: 'jpg',
});
assert.equal(validateContactSheetFilename('', 'jpeg').code, 'empty');
assert.equal(validateContactSheetFilename('bad/name', 'jpeg').code, 'invalidCharacters');
assert.equal(validateContactSheetFilename('bad.', 'jpeg').code, 'trailingDotOrSpace');
assert.equal(validateContactSheetFilename('CON', 'jpeg').code, 'reservedName');

// Borderless mode uses the same frame + inter-frame packing as the film strip,
// but the cell height comes from the complete strip ratio: the exposure
// aperture plus the physical rebate above and below it.
const bordered135 = getContactSheetLayout('135', 3000, 100);
const borderless135 = getContactSheetLayout('135', 3000, 100, { borderless: true });
assert.ok(borderless135.horizontalGap > 0, 'borderless 135 frames must keep the film gap');
assert.equal(borderless135.framesPerRow, bordered135.framesPerRow);
assert.equal(borderless135.imageWidth, bordered135.imageWidth);
assert.equal(borderless135.horizontalGap, bordered135.horizontalGap);
assert.ok(Math.abs(
    borderless135.imageHeight / borderless135.imageWidth - 35 / 36
) < 1e-12, 'borderless 135 cells must use the full 35 mm film strip height');
assert.ok(Math.abs(
    borderless135.imageWidth * 6 + borderless135.horizontalGap * 5 - (3000 - 200)
) < 1e-9, 'six frames and five gaps must fill the row width exactly');

const borderedPage = getContactSheetPageLayout({
    format: '135',
    canvasWidth: 3000,
    frameCount: 14,
});
const borderlessPage = getContactSheetPageLayout({
    format: '135',
    canvasWidth: 3000,
    frameCount: 14,
    borderless: true,
});
assert.equal(borderlessPage.rows, borderedPage.rows, 'borderless mode must not change row wrapping');
assert.ok(Math.abs(borderlessPage.canvasHeight - borderedPage.canvasHeight) < 1e-9);
assert.equal(
    borderlessPage.rowLayouts[0].height,
    borderlessPage.layout.imageHeight,
    'borderless rows must be exactly as tall as the packed frames'
);
for (let row = 0; row < borderlessPage.rows; row += 1) {
    assert.ok(
        Math.abs(borderlessPage.rowLayouts[row].top - borderedPage.rowLayouts[row].top) < 1e-9
    );
}
for (let column = 1; column < borderlessPage.layout.framesPerRow; column += 1) {
    const previous = borderlessPage.slots[column - 1];
    const current = borderlessPage.slots[column];
    assert.ok(
        Math.abs(
            (current.x - (previous.x + previous.width)) - borderlessPage.layout.horizontalGap
        ) < 1e-9,
        'adjacent borderless 135 frames must keep the 2 mm inter-frame gap'
    );
}

const borderless120 = getContactSheetLayout('120 (6x7)', 3000, 100, { borderless: true });
assert.ok(borderless120.horizontalGap > 0, 'borderless 120 frames must keep the film gap');
assert.equal(borderless120.framesPerRow, 3);
assert.ok(
    Math.abs(borderless120.imageHeight / borderless120.imageWidth - (6 / 7) * 1.104) < 1e-12,
    'borderless 120 frames must include the physical rebate'
);

function recordingContext() {
    const calls = {
        fillRect: [],
        roundRect: [],
        fillText: [],
        drawImage: [],
        shadowColor: '',
        shadowBlur: 0,
        shadowOffsetY: 0,
    };
    return {
        calls,
        save() {},
        restore() {},
        beginPath() {},
        closePath() {},
        rect() {},
        clip() {},
        translate() {},
        rotate() {},
        fill() {},
        stroke() {},
        moveTo() {},
        lineTo() {},
        fillRect(x, y, width, height) { calls.fillRect.push({ x, y, width, height }); },
        roundRect(x, y) { calls.roundRect.push({ x, y }); },
        fillText(text) { calls.fillText.push(String(text)); },
        drawImage(...args) { calls.drawImage.push(args); },
        measureText(text) { return { width: String(text).length * 8 }; },
        set shadowColor(value) { calls.shadowColor = value; },
        set shadowBlur(value) { calls.shadowBlur = value; },
        set shadowOffsetY(value) { calls.shadowOffsetY = value; },
    };
}

const borderlessContext = recordingContext();
drawContactSheet(borderlessContext, {
    format: '135',
    canvasWidth: 3000,
    frameCount: 6,
    borderless: true,
    images: Array.from({ length: 6 }, () => ({ width: 600, height: 400 })),
    roll: {
        roll_id: 'roll_1',
        format: '135',
        film_stock: 'Kodak Portra 400',
        camera: 'Contax RTS II',
        date: '2026-11-13',
    },
});
assert.equal(
    borderlessContext.calls.fillRect.length,
    2,
    'borderless drawing must paint the paper and one shadowed film-base strip'
);
assert.ok(
    Math.abs(borderlessContext.calls.fillRect[1].width - borderlessPage.stripWidth) < 1e-9
        && Math.abs(borderlessContext.calls.fillRect[1].height - borderlessPage.layout.rowHeight) < 1e-9,
    'the borderless film-base strip must provide the black frame gaps'
);
assert.equal(
    borderlessContext.calls.shadowColor,
    'rgba(18, 18, 20, 0.28)',
    'borderless frames must keep the same strip shadow as bordered frames'
);
assert.ok(borderlessContext.calls.shadowBlur > 0);
assert.equal(
    borderlessContext.calls.roundRect.length,
    0,
    'borderless drawing must not draw sprocket perforations'
);
assert.equal(
    borderlessContext.calls.fillText.includes('1A'),
    false,
    'borderless drawing must not draw 135 edge codes'
);
assert.equal(
    borderlessContext.calls.drawImage.length,
    6,
    'borderless drawing must place every available frame into the same slots'
);
const packedDraw = borderlessContext.calls.drawImage[0];
assert.ok(
    packedDraw[3] >= borderlessPage.slots[0].width - 1e-9
        && Math.abs(packedDraw[4] - borderlessPage.slots[0].height) < 1e-9,
    'borderless frames must fill their full-strip cells while preserving the gap'
);

const tallPreview = getContactSheetPreviewSize({
    intrinsicWidth: 1800,
    intrinsicHeight: 7000,
    availableWidth: 1400,
    availableHeight: 600,
});
assert.ok(tallPreview.width <= 1400, 'a tall preview must still fit the available width');
assert.ok(tallPreview.height > 600, 'a many-row preview must scroll instead of shrinking to fit');

const fullSheetPreview = getContactSheetPreviewSize({
    intrinsicWidth: 1800,
    intrinsicHeight: 830,
    availableWidth: 1400,
    availableHeight: 600,
});
assert.ok(Math.abs(fullSheetPreview.width / fullSheetPreview.height - 1800 / 830) < 1e-2);
assert.ok(fullSheetPreview.height <= 600, 'a normal contact sheet should fit without scrolling');
assert.equal(fullSheetPreview.scrollsVertically, false);

let aspectDraw = null;
const aspectContext = recordingContext();
aspectContext.drawImage = (...args) => { aspectDraw = args; };
drawContactSheet(aspectContext, {
    format: '135',
    canvasWidth: 3000,
    frameCount: 1,
    borderless: true,
    images: [{ width: 600, height: 300 }],
    roll: { roll_id: 'roll_1', format: '135', film_stock: 'Kodak Portra 400' },
});
assert.ok(aspectDraw, 'borderless drawing must place the frame image');
assert.ok(
    Math.abs(aspectDraw[3] / aspectDraw[4] - 2) < 1e-9,
    'borderless frames must preserve the source image aspect ratio'
);
assert.ok(
    aspectDraw[3] > borderlessPage.slots[0].width + 1e-9
        && Math.abs(aspectDraw[4] - borderlessPage.slots[0].height) < 1e-9,
    'a non-standard source must fill the cell before its excess edge is cropped'
);

let portraitRotation = 0;
const portraitContext = recordingContext();
portraitContext.rotate = angle => { portraitRotation += angle; };
drawContactSheet(portraitContext, {
    format: '135',
    canvasWidth: 3000,
    frameCount: 1,
    borderless: true,
    images: [{ width: 400, height: 700 }],
    roll: { roll_id: 'roll_1', format: '135', film_stock: 'Kodak Portra 400' },
});
assert.ok(
    Math.abs(portraitRotation - Math.PI / 2) < 1e-12,
    'portrait frames must still turn clockwise 90 degrees in borderless mode'
);

const borderedContext = recordingContext();
drawContactSheet(borderedContext, {
    format: '135',
    canvasWidth: 3000,
    frameCount: 6,
    images: Array.from({ length: 6 }, () => ({ width: 600, height: 400 })),
    roll: { roll_id: 'roll_1', format: '135', film_stock: 'Kodak Portra 400' },
});
assert.ok(borderedContext.calls.fillRect.length > 1, 'bordered drawing must paint the film base');
assert.ok(borderedContext.calls.roundRect.length > 0, 'bordered drawing must paint sprockets');
assert.ok(borderedContext.calls.fillText.includes('1A'), 'bordered drawing must paint edge codes');

console.log('Contact sheet layout and filename contract verified.');
