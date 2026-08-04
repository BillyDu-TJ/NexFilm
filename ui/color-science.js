(function (root) {
    const D50 = [0.3457, 0.3585];
    const D60 = [0.32168, 0.33767];
    const D65 = [0.3127, 0.3290];
    const SPACES = {
        'linear-srgb': { p: [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]], w: D65 },
        'linear-display-p3': { p: [[0.68, 0.32], [0.265, 0.69], [0.15, 0.06]], w: D65 },
        'linear-adobe-rgb': { p: [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06]], w: D65 },
        'linear-rec2020': { p: [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]], w: D65 },
        'linear-prophoto-rgb': { p: [[0.7347, 0.2653], [0.1596, 0.8404], [0.0366, 0.0001]], w: D50 },
        'linear-aces': { p: [[0.7347, 0.2653], [0, 1], [0.0001, -0.0770]], w: D60 },
        'linear-acescg': { p: [[0.713, 0.293], [0.165, 0.830], [0.128, 0.044]], w: D60 },
    };

    function multiply(a, b) {
        const out = new Array(9).fill(0);
        for (let row = 0; row < 3; row++) {
            for (let col = 0; col < 3; col++) {
                for (let k = 0; k < 3; k++) out[row * 3 + col] += a[row * 3 + k] * b[k * 3 + col];
            }
        }
        return out;
    }

    function inverse(m) {
        const det = m[0] * (m[4] * m[8] - m[5] * m[7])
            - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6]);
        return [
            (m[4] * m[8] - m[5] * m[7]) / det, (m[2] * m[7] - m[1] * m[8]) / det, (m[1] * m[5] - m[2] * m[4]) / det,
            (m[5] * m[6] - m[3] * m[8]) / det, (m[0] * m[8] - m[2] * m[6]) / det, (m[2] * m[3] - m[0] * m[5]) / det,
            (m[3] * m[7] - m[4] * m[6]) / det, (m[1] * m[6] - m[0] * m[7]) / det, (m[0] * m[4] - m[1] * m[3]) / det,
        ];
    }

    function vector(m, v) {
        return [
            m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
            m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
            m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
        ];
    }

    function whiteXyz([x, y]) { return [x / y, 1, (1 - x - y) / y]; }

    function toXyz(profile) {
        const columns = profile.p.map(([x, y]) => [x / y, 1, (1 - x - y) / y]);
        const primaries = [
            columns[0][0], columns[1][0], columns[2][0],
            columns[0][1], columns[1][1], columns[2][1],
            columns[0][2], columns[1][2], columns[2][2],
        ];
        const scale = vector(inverse(primaries), whiteXyz(profile.w));
        return [
            primaries[0] * scale[0], primaries[1] * scale[1], primaries[2] * scale[2],
            primaries[3] * scale[0], primaries[4] * scale[1], primaries[5] * scale[2],
            primaries[6] * scale[0], primaries[7] * scale[1], primaries[8] * scale[2],
        ];
    }

    function adapt(source, target) {
        if (source[0] === target[0] && source[1] === target[1]) return [1,0,0,0,1,0,0,0,1];
        const b = [0.8951, 0.2664, -0.1614, -0.7502, 1.7135, 0.0367, 0.0389, -0.0685, 1.0296];
        const sourceLms = vector(b, whiteXyz(source));
        const targetLms = vector(b, whiteXyz(target));
        return multiply(multiply(
            [0.9869929, -0.1470543, 0.1599627, 0.4323053, 0.5183603, 0.0492912, -0.0085287, 0.0400428, 0.9684867],
            [targetLms[0] / sourceLms[0], 0, 0, 0, targetLms[1] / sourceLms[1], 0, 0, 0, targetLms[2] / sourceLms[2]]
        ), b);
    }

    const srgb = SPACES['linear-srgb'];
    const srgbXyzInverse = inverse(toXyz(srgb));
    const matrices = new Map();
    Object.entries(SPACES).forEach(([id, space]) => {
        matrices.set(id, multiply(srgbXyzInverse, multiply(adapt(space.w, srgb.w), toXyz(space))));
    });

    root.NexFilmColorScience = {
        getWorkingToSrgbMatrix(id) {
            const rowMajor = matrices.get(id) || matrices.get('linear-srgb');
            return new Float32Array([
                rowMajor[0], rowMajor[3], rowMajor[6],
                rowMajor[1], rowMajor[4], rowMajor[7],
                rowMajor[2], rowMajor[5], rowMajor[8],
            ]);
        },
        getWorkingLuma(id) {
            const matrix = toXyz(SPACES[id] || srgb);
            return [matrix[3], matrix[4], matrix[5]];
        },
    };
})(typeof globalThis !== 'undefined' ? globalThis : this);
