const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const geometry = require('../ui/geometry.js');

const root = path.resolve(__dirname, '..');
const html = fs.readFileSync(path.join(root, 'ui/index.html'), 'utf8');
const main = fs.readFileSync(path.join(root, 'ui/main.js'), 'utf8');
const css = fs.readFileSync(path.join(root, 'ui/style.css'), 'utf8');

assert.match(
    html,
    /data-film-aspect="3:2"[\s\S]*?data-film-aspect="1:1"[\s\S]*?data-film-aspect="6:7"[\s\S]*?data-film-aspect="free"/,
    'Film-area tool must expose 3:2, 1:1, 6:7 and Free ratio locks'
);
assert.match(
    html,
    /data-film-aspect="free"[^>]*aria-pressed="true"/,
    'Free must be the initial ratio lock'
);
assert.match(
    main,
    /let filmAreaAspectLock = 'free'/,
    'Film-area editing must keep ratio-lock state'
);
assert.match(
    main,
    /translateCalibrationQuad\(/,
    'Dragging the film-area body must translate the whole quad'
);
assert.match(
    main,
    /resizeCalibrationCornerLocked\(/,
    'Corner drags must honor the selected ratio lock'
);
assert.match(
    main,
    /resizeCalibrationEdgeLocked\(/,
    'Single-edge drags must honor the selected ratio lock'
);
assert.match(
    main,
    /type: isCorner \? 'corner' : \(isEdge \? 'edge' : 'move'\)/,
    'The polygon body must start a move drag while edges and corners keep their own handles'
);
assert.match(
    css,
    /#calibration-polygon\s*\{\s*cursor:\s*move;/,
    'The film-area body must advertise that it can be dragged'
);
assert.match(
    css,
    /\.film-area-aspect-button\.is-active/,
    'The active ratio lock must have visible feedback'
);

const square = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]];
const moved = geometry.translateCalibrationQuad(square, [100, 50], [1000, 500]);
assert.deepEqual(moved.map(point => point.map(value => Number(value.toFixed(6)))), [
    [0.2, 0.2],
    [1, 0.2],
    [1, 1],
    [0.2, 1],
]);

const squareLock = geometry.aspectLockedCalibrationQuad(square, 1, [1000, 500]);
assert.deepEqual(squareLock.map(point => point.map(value => Number(value.toFixed(6)))), [
    [0.3, 0.1],
    [0.7, 0.1],
    [0.7, 0.9],
    [0.3, 0.9],
]);

const cornerLock = geometry.resizeCalibrationCornerLocked(
    square,
    2,
    [0.8, 0.6],
    [1000, 1000],
    1
);
assert.deepEqual(cornerLock.map(point => point.map(value => Number(value.toFixed(6)))), [
    [0.1, 0.1],
    [0.6, 0.1],
    [0.6, 0.6],
    [0.1, 0.6],
]);

const edgeLock = geometry.resizeCalibrationEdgeLocked(
    square,
    0,
    [0, 100],
    [1000, 1000],
    1
);
assert.deepEqual(edgeLock.map(point => point.map(value => Number(value.toFixed(6)))), [
    [0.15, 0.2],
    [0.85, 0.2],
    [0.85, 0.9],
    [0.15, 0.9],
]);

console.log('Film-area UI contracts passed.');
