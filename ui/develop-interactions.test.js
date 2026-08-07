const assert = require('node:assert/strict');
const interactions = require('./develop-interactions.js');

assert.deepEqual(
    interactions.normalizeWheelDelta({ deltaX: 2, deltaY: -3, deltaMode: 1 }, 900, 18),
    { x: 36, y: -54 }
);
assert.deepEqual(
    interactions.normalizeWheelDelta({ deltaX: 0, deltaY: 1, deltaMode: 2 }, 720),
    { x: 0, y: 720 }
);
assert.equal(interactions.horizontalWheelDelta({ x: 80, y: 20 }), 80);
assert.equal(interactions.horizontalWheelDelta({ x: 5, y: -40 }), -40);
assert.equal(interactions.canScrollBy(0, 100, -20), false);
assert.equal(interactions.canScrollBy(0, 100, 20), true);
assert.equal(interactions.canScrollBy(100, 100, 20), false);

const zoomed = interactions.getZoomViewUpdate({
    zoom: 1,
    deltaY: -120,
    minZoom: 0.1,
    maxZoom: 10,
    panX: 0,
    panY: 0,
    pointerX: 750,
    pointerY: 300,
    centerX: 500,
    centerY: 300,
});
assert.ok(zoomed.zoom > 1);
assert.ok(zoomed.panX < 0, 'zooming at the right side must keep that image point under the pointer');
assert.equal(zoomed.panY, 0);

const clamped = interactions.getZoomViewUpdate({ zoom: 10, deltaY: -1000, minZoom: 0.1, maxZoom: 10 });
assert.equal(clamped.zoom, 10);

assert.equal(interactions.getPreviewProxyTarget({
    displayWidth: 1200, displayHeight: 800, renderedWidth: 2560, renderedHeight: 1700,
    sourceLongEdge: 2560, currentLongEdge: 2560, pixelRatio: 2,
    baseLongEdge: 2560, maxLongEdge: 4096, step: 1024,
}), null, 'a proxy that already covers the display pixels must not be redecoded');
assert.equal(interactions.getPreviewProxyTarget({
    displayWidth: 1800, displayHeight: 1200, renderedWidth: 1280, renderedHeight: 850,
    sourceLongEdge: 2560, currentLongEdge: 2560, pixelRatio: 1,
    baseLongEdge: 2560, maxLongEdge: 4096, step: 1024,
}), 4096, 'a zoomed crop must request enough source pixels for its rendered area');
assert.equal(interactions.getPreviewProxyTarget({
    displayWidth: 1800, displayHeight: 1200, renderedWidth: 1280, renderedHeight: 850,
    sourceLongEdge: 2560, currentLongEdge: 2560, attemptedLongEdge: 4096, pixelRatio: 1,
    baseLongEdge: 2560, maxLongEdge: 4096, step: 1024,
}), null, 'a native-size limit must not cause the same upgrade to loop');
assert.equal(interactions.getPreviewProxyTarget({
    displayWidth: 1500, displayHeight: 1000, renderedWidth: 1200, renderedHeight: 800,
    sourceLongEdge: 1200, currentLongEdge: 1200, pixelRatio: 1,
    baseLongEdge: 2560, maxLongEdge: 4096, step: 1024,
}), 3584, 'an adaptive request must be distinguishable from base proxy prewarming');

const batchItems = [
    { id: 'source', roll_id: 'roll-a', file_path: 'A/source.nef', base_analyzed: true },
    { id: 'fresh', roll_id: 'roll-a', file_path: 'A/fresh.nef', base_analyzed: false },
    { id: 'visited', roll_id: 'roll-a', file_path: 'A/visited.nef', base_analyzed: false, rendered_thumbnail_base64: 'negative-preview' },
    { id: 'done', roll_id: 'roll-a', file_path: 'A/done.nef', base_analyzed: true },
    { id: 'other', roll_id: 'roll-b', file_path: 'B/other.nef', base_analyzed: false },
];
assert.deepEqual(
    interactions.selectFilmAreaBatchTargets({ items: batchItems, activeId: 'source', mode: 'roll-unprocessed' }).map(item => item.id),
    ['fresh', 'visited', 'done'],
    'batch apply must allow every other frame in the source roll'
);
assert.deepEqual(
    interactions.selectFilmAreaBatchTargets({
        items: batchItems, activeId: 'source', mode: 'selected', selectedIds: ['source', 'done', 'other'],
    }).map(item => item.id),
    ['done', 'other']
);

const normalized23 = interactions.getNormalizedCropAspect('2:3', 6000, 4000);
assert.ok(Math.abs(normalized23 - 4 / 9) < 1e-12);
assert.equal(interactions.getNormalizedCropAspect('original', 6000, 4000), 1);
assert.equal(interactions.getNormalizedCropAspect('free', 6000, 4000), null);

const fitted = interactions.fitCropRectToAspect(
    { x: 0.1, y: 0.1, width: 0.8, height: 0.8 },
    normalized23
);
assert.ok(Math.abs(fitted.width / fitted.height - normalized23) < 1e-12);
assert.ok(fitted.x >= 0 && fitted.y >= 0 && fitted.x + fitted.width <= 1 && fitted.y + fitted.height <= 1);

const resized = interactions.constrainCropResize(
    { x: 0.2, y: 0.2, width: 0.6, height: 0.6 },
    { x: 0.1, y: 0.15, width: 0.7, height: 0.65 },
    'nw',
    normalized23
);
assert.ok(Math.abs(resized.width / resized.height - normalized23) < 1e-12);
assert.ok(Math.abs(resized.x + resized.width - 0.8) < 1e-12, 'opposite horizontal edge stays anchored');
assert.ok(Math.abs(resized.y + resized.height - 0.8) < 1e-12, 'opposite vertical edge stays anchored');

assert.equal(
    interactions.thumbnailPixelsAreUsable(new Uint8Array([0, 0, 0, 255, 4, 4, 4, 255])),
    false,
    'an all-black GPU readback must never replace a valid thumbnail'
);
assert.equal(
    interactions.thumbnailPixelsAreUsable(new Uint8Array([0, 0, 0, 255, 5, 1, 1, 255])),
    true,
    'a genuinely dark image remains usable when it contains visible image data'
);
const thumbnailNodes = [
    { dataset: { imgId: 'frame-a' }, location: 'library' },
    { dataset: { imgId: 'frame-a' }, location: 'filmstrip' },
    { dataset: { imgId: 'frame-b' }, location: 'other' },
    { dataset: { imgId: 'frame-a' }, location: 'history' },
];
const updatedLocations = [];
assert.equal(
    interactions.updateMatchingThumbnails(thumbnailNodes, 'frame-a', node => {
        updatedLocations.push(node.location);
    }),
    3,
    'one live thumbnail publication must update every visible instance of that image'
);
assert.deepEqual(updatedLocations, ['library', 'filmstrip', 'history']);
assert.deepEqual(
    interactions.getQuarterTurnAction('left'),
    { clockwise: false, turnDelta: -1 },
    'Rotate Left must apply a counter-clockwise quarter turn'
);
assert.deepEqual(
    interactions.getQuarterTurnAction('right'),
    { clockwise: true, turnDelta: 1 },
    'Rotate Right must apply a clockwise quarter turn'
);

console.log('Develop mouse interaction tests passed.');
