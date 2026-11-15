(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmToneCurve = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    // These constants are shared with the Rust renderer in src/core_math.rs.
    // The preview builds the same lookup table and the export reads the same
    // table back, so a curve drawn here is the curve in the exported file.
    const LUT_SIZE = 1024;
    const MAX_POINTS = 16;
    const CHANNELS = ['rgb', 'red', 'green', 'blue'];
    const MIN_POINT_GAP = 0.01;
    const POINT_HIT_RADIUS = 0.075;
    const CHANNEL_STORAGE = {
        rgb: 'curve_rgb',
        red: 'curve_red',
        green: 'curve_green',
        blue: 'curve_blue',
    };
    const CHANNEL_LABELS = { rgb: 'RGB', red: 'R', green: 'G', blue: 'B' };
    const EPSILON = 1e-6;

    function identityPoints() {
        return [[0, 0], [1, 1]];
    }

    function clonePoints(points) {
        return (points || []).map(point => [Number(point[0]) || 0, Number(point[1]) || 0]);
    }

    function clamp01(value) {
        if (!Number.isFinite(value)) return 0;
        return Math.min(1, Math.max(0, value));
    }

    /// Sort, clamp and deduplicate the control points, exactly like the
    /// renderer. The first and last points are genuine curve endpoints: they
    /// may sit anywhere in the input range and the curve stays flat outside
    /// them.
    function normalizePoints(points) {
        const sorted = (points || [])
            .filter(point => Number.isFinite(Number(point?.[0])) && Number.isFinite(Number(point?.[1])))
            .map(point => [clamp01(Number(point[0])), clamp01(Number(point[1]))])
            .sort((left, right) => left[0] - right[0]);
        const deduplicated = [];
        for (const point of sorted) {
            const previous = deduplicated[deduplicated.length - 1];
            if (previous && Math.abs(previous[0] - point[0]) < EPSILON) {
                deduplicated[deduplicated.length - 1] = point;
            } else {
                deduplicated.push(point);
            }
        }
        if (deduplicated.length === 0) return identityPoints();
        if (deduplicated.length === 1) {
            const output = deduplicated[0][1];
            return [[0, output], [1, output]];
        }
        return deduplicated.slice(0, MAX_POINTS);
    }

    /// Fritsch-Carlson tangents: a monotone cubic that cannot overshoot between
    /// two control points.
    function monotoneTangents(points) {
        const count = points.length;
        if (count < 2) return points.map(() => 0);
        const slopes = [];
        for (let index = 0; index < count - 1; index += 1) {
            const dx = points[index + 1][0] - points[index][0];
            slopes.push(Math.abs(dx) < 1e-8 ? 0 : (points[index + 1][1] - points[index][1]) / dx);
        }
        const tangents = new Array(count).fill(0);
        tangents[0] = slopes[0];
        tangents[count - 1] = slopes[count - 2];
        for (let index = 1; index < count - 1; index += 1) {
            const previous = slopes[index - 1];
            const next = slopes[index];
            tangents[index] = previous * next <= 0 ? 0 : (previous + next) / 2;
        }
        for (let index = 0; index < count - 1; index += 1) {
            if (Math.abs(slopes[index]) < 1e-8) {
                tangents[index] = 0;
                tangents[index + 1] = 0;
                continue;
            }
            const alpha = tangents[index] / slopes[index];
            const beta = tangents[index + 1] / slopes[index];
            const magnitude = Math.hypot(alpha, beta);
            if (magnitude > 3) {
                const scale = 3 / magnitude;
                tangents[index] = scale * alpha * slopes[index];
                tangents[index + 1] = scale * beta * slopes[index];
            }
        }
        return tangents;
    }

    function evaluate(points, x, tangents = null) {
        const normalized = normalizePoints(points);
        if (normalized.length < 2) return clamp01(x);
        if (!Number.isFinite(x)) return normalized[0][1];
        if (x <= normalized[0][0]) return normalized[0][1];
        const last = normalized[normalized.length - 1];
        if (x >= last[0]) return last[1];
        const slopes = tangents || monotoneTangents(normalized);
        const clamped = Math.min(last[0], Math.max(normalized[0][0], x));
        let segment = normalized.length - 2;
        for (let index = 0; index < normalized.length - 1; index += 1) {
            if (clamped <= normalized[index + 1][0] + 1e-9) {
                segment = index;
                break;
            }
        }
        const [x0, y0] = normalized[segment];
        const [x1, y1] = normalized[segment + 1];
        const width = x1 - x0;
        if (Math.abs(width) < 1e-8) return y1;
        const t = Math.min(1, Math.max(0, (clamped - x0) / width));
        const t2 = t * t;
        const t3 = t2 * t;
        const h00 = 2 * t3 - 3 * t2 + 1;
        const h10 = t3 - 2 * t2 + t;
        const h01 = -2 * t3 + 3 * t2;
        const h11 = t3 - t2;
        const value = h00 * y0 + h10 * width * slopes[segment] + h01 * y1 + h11 * width * slopes[segment + 1];
        return clamp01(value);
    }

    function buildLut(points) {
        const normalized = normalizePoints(points);
        const slopes = monotoneTangents(normalized);
        const lut = new Float32Array(LUT_SIZE);
        for (let index = 0; index < LUT_SIZE; index += 1) {
            lut[index] = evaluate(normalized, index / (LUT_SIZE - 1), slopes);
        }
        return lut;
    }

    function sampleLut(lut, x) {
        const position = clamp01(x) * (LUT_SIZE - 1);
        const low = Math.floor(position);
        const high = Math.min(low + 1, LUT_SIZE - 1);
        const fraction = position - low;
        return lut[low] + (lut[high] - lut[low]) * fraction;
    }

    /// The renderer's identity test: the default curve has to be free of any
    /// rounding, otherwise every frame would carry a hidden tone move.
    function isIdentity(points) {
        const normalized = normalizePoints(points);
        return normalized.length === 2
            && Math.abs(normalized[0][0]) < EPSILON
            && Math.abs(normalized[0][1]) < EPSILON
            && Math.abs(normalized[1][0] - 1) < EPSILON
            && Math.abs(normalized[1][1] - 1) < EPSILON;
    }

    function findPointIndex(points, x, y, tolerance = POINT_HIT_RADIUS) {
        let best = -1;
        let bestDistance = tolerance;
        points.forEach((point, index) => {
            const distance = Math.hypot(point[0] - x, point[1] - y);
            if (distance <= bestDistance) {
                bestDistance = distance;
                best = index;
            }
        });
        return best;
    }

    /// Add a control point. Returns null when the curve already holds the
    /// maximum number of points or when the coordinates are invalid. Clicking
    /// inside EPSILON of an existing x replaces that column cleanly from
    /// either side instead of dropping the new click during sorting.
    function insertPoint(points, x, y) {
        const normalized = normalizePoints(points);
        if (normalized.length >= MAX_POINTS) return null;
        if (!Number.isFinite(Number(x)) || !Number.isFinite(Number(y))) return null;
        const nextX = clamp01(Number(x));
        const nextY = clamp01(Number(y));
        const existingIndex = normalized.findIndex(point => Math.abs(point[0] - nextX) < EPSILON);
        if (existingIndex >= 0) {
            const next = normalized.map(point => point.slice());
            next[existingIndex] = [nextX, nextY];
            return normalizePoints(next);
        }
        const next = normalized.slice();
        next.push([nextX, nextY]);
        return normalizePoints(next);
    }

    /// Move a control point. Both endpoints may move freely inside the input
    /// range, while every point keeps a small gap from its neighbours so the
    /// curve cannot fold or reorder.
    function movePoint(points, index, x, y) {
        const normalized = normalizePoints(points);
        if (index < 0 || index >= normalized.length) return normalized;
        const isFirst = index === 0;
        const isLast = index === normalized.length - 1;
        const next = normalized.map(point => point.slice());
        if (isFirst) {
            const upper = Math.max(0, next[1][0] - MIN_POINT_GAP);
            next[index][0] = Math.min(upper, clamp01(x));
        } else if (isLast) {
            const lower = Math.min(1, next[index - 1][0] + MIN_POINT_GAP);
            next[index][0] = Math.max(lower, clamp01(x));
        } else {
            const previous = next[index - 1][0];
            const following = next[index + 1][0];
            const lower = Math.min(previous + MIN_POINT_GAP, following - EPSILON);
            const upper = Math.max(previous + EPSILON, following - MIN_POINT_GAP);
            next[index][0] = Math.min(
                Math.max(lower, clamp01(x)),
                Math.max(lower, upper)
            );
        }
        next[index][1] = clamp01(y);
        return next;
    }

    /// Remove an interior control point. The endpoints always stay.
    function removePoint(points, index) {
        const normalized = normalizePoints(points);
        if (index <= 0 || index >= normalized.length - 1) return normalized;
        const next = normalized.slice();
        next.splice(index, 1);
        return normalizePoints(next);
    }

    function curveParamsFromState(curves) {
        const params = {};
        for (const channel of CHANNELS) {
            params[CHANNEL_STORAGE[channel]] = clonePoints(curves?.[channel] || identityPoints());
        }
        return params;
    }

    function curvesFromParams(params) {
        const curves = {};
        for (const channel of CHANNELS) {
            curves[channel] = normalizePoints(params?.[CHANNEL_STORAGE[channel]] || identityPoints());
        }
        return curves;
    }

    function defaultCurveState() {
        const curves = {};
        for (const channel of CHANNELS) curves[channel] = identityPoints();
        return curves;
    }

    function curvesAreIdentity(curves) {
        return CHANNELS.every(channel => isIdentity(curves?.[channel] || identityPoints()));
    }

    return {
        LUT_SIZE,
        MAX_POINTS,
        MIN_POINT_GAP,
        POINT_HIT_RADIUS,
        CHANNELS,
        CHANNEL_STORAGE,
        CHANNEL_LABELS,
        buildLut,
        clonePoints,
        curvesAreIdentity,
        curvesFromParams,
        curveParamsFromState,
        defaultCurveState,
        evaluate,
        findPointIndex,
        identityPoints,
        insertPoint,
        isIdentity,
        movePoint,
        normalizePoints,
        removePoint,
        sampleLut,
    };
});
