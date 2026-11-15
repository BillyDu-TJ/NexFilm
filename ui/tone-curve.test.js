const assert = require('node:assert/strict');
const test = require('node:test');

const curve = require('./tone-curve.js');

test('a default curve is an exact identity', () => {
    assert.deepEqual(curve.identityPoints(), [[0, 0], [1, 1]]);
    assert.equal(curve.isIdentity(curve.identityPoints()), true);
    assert.equal(curve.isIdentity([[0, 0.1], [1, 1]]), false);
    assert.equal(curve.isIdentity([[0.1, 0], [0.9, 1]]), false);
    const lut = curve.buildLut(curve.identityPoints());
    assert.equal(lut.length, curve.LUT_SIZE);
    for (const value of [0, 0.05, 0.5, 0.93, 1]) {
        assert.ok(Math.abs(curve.sampleLut(lut, value) - value) < 2e-3, `${value} moved`);
    }
});

test('control points are clamped, sorted and capped', () => {
    const normalized = curve.normalizePoints([[1.4, 2], [-0.3, -1], [0.5, 0.5]]);
    assert.deepEqual(normalized, [[0, 0], [0.5, 0.5], [1, 1]]);
    const freeEndpoints = [[0.2, 0.1], [0.55, 0.45], [0.8, 0.9]];
    assert.deepEqual(curve.normalizePoints(freeEndpoints), freeEndpoints);
    assert.deepEqual(curve.normalizePoints([[0.5, 0.4]]), [[0, 0.4], [1, 0.4]]);
    assert.deepEqual(curve.normalizePoints([]), curve.identityPoints());
    const many = Array.from({ length: 30 }, (unused, index) => [index / 29, 0.5]);
    assert.equal(curve.normalizePoints(many).length, curve.MAX_POINTS);
});

test('a dragged point cannot fold the curve backwards', () => {
    // Monotone control points: the interpolated curve has to stay monotone too,
    // which is what the Fritsch-Carlson tangents are for.
    const points = curve.normalizePoints([[0, 0], [0.35, 0.55], [0.65, 0.6], [1, 1]]);
    let previous = -1;
    for (let step = 0; step <= 200; step += 1) {
        const value = curve.evaluate(points, step / 200);
        assert.ok(value >= previous - 1e-4, `curve decreased at ${step / 200}`);
        assert.ok(value >= 0 && value <= 1);
        previous = value;
    }
});

test('a sharp S curve keeps its values inside the drawn range', () => {
    const points = curve.normalizePoints([[0, 0], [0.4, 0.05], [0.6, 0.95], [1, 1]]);
    for (let step = 0; step <= 100; step += 1) {
        const value = curve.evaluate(points, step / 100);
        assert.ok(value >= 0 && value <= 1, `value ${value} left the display range`);
    }
    assert.ok(curve.evaluate(points, 0.4) < 0.1);
    assert.ok(curve.evaluate(points, 0.6) > 0.9);
    assert.ok(curve.evaluate(points, 0.5) > 0.35 && curve.evaluate(points, 0.5) < 0.65);
});

test('free endpoints move in both directions and extend flat outside', () => {
    const lifted = curve.movePoint(curve.identityPoints(), 0, 0.4, 0.25);
    assert.deepEqual(lifted, [[0.4, 0.25], [1, 1]]);
    assert.ok(Math.abs(curve.evaluate(lifted, 0) - 0.25) < 1e-3);
    assert.ok(Math.abs(curve.evaluate(lifted, 0.2) - 0.25) < 1e-3);
    assert.ok(Math.abs(curve.evaluate(lifted, 0.4) - 0.25) < 1e-3);

    const lowered = curve.movePoint(curve.identityPoints(), 1, 0.2, 0.8);
    assert.deepEqual(lowered, [[0, 0], [0.2, 0.8]]);
    assert.ok(Math.abs(curve.evaluate(lowered, 0.2) - 0.8) < 1e-3);
    assert.ok(Math.abs(curve.evaluate(lowered, 0.5) - 0.8) < 1e-3);
    assert.ok(Math.abs(curve.evaluate(lowered, 1) - 0.8) < 1e-3);
});

test('endpoints keep a minimum gap from their neighbours and cannot cross', () => {
    const points = curve.normalizePoints([[0.2, 0.1], [0.5, 0.5], [0.8, 0.9]]);
    const first = curve.movePoint(points, 0, 1, 0.3);
    assert.deepEqual(first[0], [0.5 - curve.MIN_POINT_GAP, 0.3]);
    const last = curve.movePoint(points, 2, 0, 0.7);
    assert.deepEqual(last[2], [0.5 + curve.MIN_POINT_GAP, 0.7]);

    const crossedLeft = curve.movePoint(points, 1, -2, 0.4);
    assert.ok(crossedLeft[1][0] > crossedLeft[0][0]);
    assert.ok(crossedLeft[1][0] < crossedLeft[2][0]);
    const crossedRight = curve.movePoint(points, 1, 2, 0.4);
    assert.ok(crossedRight[1][0] > crossedRight[0][0]);
    assert.ok(crossedRight[1][0] < crossedRight[2][0]);
});

test('interior points stay between their neighbours', () => {
    const points = curve.insertPoint(curve.identityPoints(), 0.5, 0.5);
    assert.deepEqual(points, [[0, 0], [0.5, 0.5], [1, 1]]);
    const pushedLeft = curve.movePoint(points, 1, -3, 0.4);
    assert.ok(pushedLeft[1][0] > pushedLeft[0][0]);
    assert.ok(pushedLeft[1][0] < pushedLeft[2][0]);
    const pushedRight = curve.movePoint(points, 1, 4, 0.4);
    assert.ok(pushedRight[1][0] < pushedRight[2][0]);
});

test('points can be added and removed but the endpoints stay', () => {
    let points = curve.insertPoint(curve.identityPoints(), 0.25, 0.3);
    points = curve.insertPoint(points, 0.5, 0.5);
    points = curve.insertPoint(points, 0.75, 0.8);
    assert.equal(points.length, 5);
    assert.equal(curve.removePoint(points, 0).length, 5);
    assert.equal(curve.removePoint(points, points.length - 1).length, 5);
    const without = curve.removePoint(points, 2);
    assert.equal(without.length, 4);
    assert.deepEqual(without[0], [0, 0]);
    assert.deepEqual(without[without.length - 1], [1, 1]);
});

test('endpoints can be reset independently while interior removal keeps bounds', () => {
    const points = curve.normalizePoints([[0.2, 0.15], [0.45, 0.4], [0.75, 0.8]]);
    assert.deepEqual(curve.movePoint(points, 0, 0, 0), [[0, 0], [0.45, 0.4], [0.75, 0.8]]);
    assert.deepEqual(curve.movePoint(points, 2, 1, 1), [[0.2, 0.15], [0.45, 0.4], [1, 1]]);
    assert.deepEqual(curve.removePoint(points, 1), [[0.2, 0.15], [0.75, 0.8]]);
    assert.equal(curve.removePoint(points, 0).length, 3);
    assert.equal(curve.removePoint(points, 2).length, 3);
});

test('the control point limit stops new points', () => {
    let points = curve.identityPoints();
    for (let index = 1; index < curve.MAX_POINTS - 1; index += 1) {
        points = curve.insertPoint(points, index / (curve.MAX_POINTS - 1), 0.5);
    }
    assert.equal(points.length, curve.MAX_POINTS);
    assert.equal(curve.insertPoint(points, 0.33, 0.4), null);
});

test('parameter round-trips cover all four channels', () => {
    const curves = curve.defaultCurveState();
    curves.red = curve.normalizePoints([[0, 0.2], [1, 1]]);
    const params = curve.curveParamsFromState(curves);
    assert.deepEqual(Object.keys(params).sort(), ['curve_blue', 'curve_green', 'curve_red', 'curve_rgb']);
    const restored = curve.curvesFromParams(params);
    assert.deepEqual(restored, curves);
    assert.equal(curve.curvesAreIdentity(curves), false);
    assert.equal(curve.curvesAreIdentity(curve.defaultCurveState()), true);
});

test('a missing channel falls back to the identity curve', () => {
    const restored = curve.curvesFromParams({ curve_red: [[0, 0.4], [1, 1]] });
    assert.deepEqual(restored.rgb, [[0, 0], [1, 1]]);
    assert.deepEqual(restored.red, [[0, 0.4], [1, 1]]);
});

test('historical curves with custom endpoints load without insertions', () => {
    const restored = curve.curvesFromParams({
        curve_red: [[0.15, 0.2], [0.85, 0.8]],
    });
    assert.deepEqual(restored.red, [[0.15, 0.2], [0.85, 0.8]]);
});

test('a point can be found by its screen position', () => {
    const points = curve.normalizePoints([[0, 0], [0.5, 0.4], [1, 1]]);
    assert.equal(curve.findPointIndex(points, 0.51, 0.42), 1);
    assert.equal(curve.findPointIndex(points, 0.57, 0.4), 1);
    assert.equal(curve.findPointIndex(points, 0.58, 0.4), -1);
    assert.equal(curve.findPointIndex(points, 0.2, 0.2), -1);
});

test('the LUT samples the same free-endpoint curve as the renderer', () => {
    const points = [[0.18, 0.08], [0.42, 0.34], [0.68, 0.72], [0.86, 0.92]];
    const lut = curve.buildLut(points);
    const expected = new Map([
        [0, 0.07999999821186066],
        [64, 0.07999999821186066],
        [128, 0.07999999821186066],
        [333, 0.23107102513313293],
        [512, 0.45339617133140564],
        [700, 0.7254473567008972],
        [900, 0.9200000166893005],
        [1023, 0.9200000166893005],
    ]);
    for (const [index, value] of expected) {
        assert.ok(
            Math.abs(lut[index] - value) < 1e-4,
            `LUT sample ${index}: ${lut[index]} != ${value}`
        );
    }
});

test('inserting on either side of an existing column keeps the new click findable', () => {
    const base = [[0, 0], [0.5, 0.2], [1, 1]];
    const leftClick = curve.insertPoint(base, 0.5 - 5e-7, 0.7);
    assert.deepEqual(leftClick, [[0, 0], [0.5 - 5e-7, 0.7], [1, 1]]);
    assert.equal(curve.findPointIndex(leftClick, 0.5 - 5e-7, 0.7), 1);

    const rightClick = curve.insertPoint(base, 0.5 + 5e-7, 0.7);
    assert.deepEqual(rightClick, [[0, 0], [0.5 + 5e-7, 0.7], [1, 1]]);
    assert.equal(curve.findPointIndex(rightClick, 0.5 + 5e-7, 0.7), 1);
});
