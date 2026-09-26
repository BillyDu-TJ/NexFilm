const assert = require('node:assert/strict');
const test = require('node:test');

const curve = require('./tone-curve.js');

test('a default curve is an exact identity', () => {
    assert.deepEqual(curve.identityPoints(), [[0, 0], [1, 1]]);
    assert.equal(curve.isIdentity(curve.identityPoints()), true);
    assert.equal(curve.isIdentity([[0, 0.1], [1, 1]]), false);
    const lut = curve.buildLut(curve.identityPoints());
    assert.equal(lut.length, curve.LUT_SIZE);
    for (const value of [0, 0.05, 0.5, 0.93, 1]) {
        assert.ok(Math.abs(curve.sampleLut(lut, value) - value) < 2e-3, `${value} moved`);
    }
});

test('control points are clamped, sorted and capped', () => {
    const normalized = curve.normalizePoints([[1.4, 2], [-0.3, -1], [0.5, 0.5]]);
    assert.deepEqual(normalized, [[0, 0], [0.5, 0.5], [1, 1]]);
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

test('the endpoints keep their input value while their output moves', () => {
    const lifted = curve.movePoint(curve.identityPoints(), 0, 0.4, 0.25);
    assert.deepEqual(lifted, [[0, 0.25], [1, 1]]);
    assert.ok(Math.abs(curve.evaluate(lifted, 0) - 0.25) < 1e-3);

    const lowered = curve.movePoint(curve.identityPoints(), 1, 0.2, 0.8);
    assert.deepEqual(lowered, [[0, 0], [1, 0.8]]);
    assert.ok(Math.abs(curve.evaluate(lowered, 1) - 0.8) < 1e-3);
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

test('a point can be found by its screen position', () => {
    const points = curve.normalizePoints([[0, 0], [0.5, 0.4], [1, 1]]);
    assert.equal(curve.findPointIndex(points, 0.51, 0.42), 1);
    assert.equal(curve.findPointIndex(points, 0.2, 0.2), -1);
});
