(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmDensity = api;
}(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    const statusMToPrintingDensity = Object.freeze([
        Object.freeze([1.0197, 0.0317, 0.0091]),
        Object.freeze([-0.0052, 0.8933, 0.0521]),
        Object.freeze([0.0131, -0.0011, 0.9712]),
    ]);
    const lumaCoefficients = Object.freeze([0.2126, 0.7152, 0.0722]);

    function applyStatusMToPrintingDensity(density) {
        return statusMToPrintingDensity.map(row =>
            row[0] * density[0] + row[1] * density[1] + row[2] * density[2]
        );
    }

    function getNeutralExposureOffsets(rawDensity, greenExposure = 0) {
        const corrected = applyStatusMToPrintingDensity(rawDensity);
        return [
            corrected[1] + greenExposure - corrected[0],
            greenExposure,
            corrected[1] + greenExposure - corrected[2],
        ];
    }

    function densityLuma(rgb) {
        return lumaCoefficients.reduce(
            (sum, coefficient, channel) => sum + coefficient * rgb[channel],
            0
        );
    }

    // The display endpoints are per channel, and a sampled Roll measures its own
    // base-to-leader span in each of them. Adding the same raw density offset to
    // all three therefore changes each channel's gain by a different relative
    // amount, which tints the print: raising D-Max by one uniform offset
    // visibly turns the frame green. Scaling each channel's offset by its share
    // of the span keeps the trim equal in normalised display units, so the
    // sliders stay a neutral exposure and contrast move.
    function trimDensityEndpoints(dMin, dMax, minOffset, maxOffset) {
        const span = [
            dMax[0] - dMin[0],
            dMax[1] - dMin[1],
            dMax[2] - dMin[2],
        ];
        const spanLuma = densityLuma(span);
        if (!Number.isFinite(spanLuma) || spanLuma <= 1e-4) {
            // A collapsed window has no share to scale by; keep the plain shift.
            return {
                dMin: dMin.map(value => value + minOffset),
                dMax: dMax.map(value => value + maxOffset),
            };
        }
        const scale = span.map(value => (Number.isFinite(value) ? value / spanLuma : 1));
        return {
            dMin: dMin.map((value, channel) => value + minOffset * scale[channel]),
            dMax: dMax.map((value, channel) => value + maxOffset * scale[channel]),
        };
    }

    return {
        applyStatusMToPrintingDensity,
        getNeutralExposureOffsets,
        trimDensityEndpoints,
    };
}));
