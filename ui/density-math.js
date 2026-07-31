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

    return {
        applyStatusMToPrintingDensity,
        getNeutralExposureOffsets,
    };
}));
