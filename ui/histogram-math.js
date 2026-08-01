(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmHistogram = api;
}(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    const DEFAULT_SCALE_PERCENTILE = 0.98;

    function getHistogramScale(histograms, percentile = DEFAULT_SCALE_PERCENTILE) {
        const counts = [];
        for (const histogram of histograms) {
            for (let bin = 1; bin < histogram.length - 1; bin++) {
                const count = histogram[bin];
                if (Number.isFinite(count) && count > 0) counts.push(count);
            }
        }
        if (counts.length === 0) return 1;

        counts.sort((a, b) => a - b);
        const clampedPercentile = Math.max(0, Math.min(1, percentile));
        const index = Math.floor((counts.length - 1) * clampedPercentile);
        return Math.max(1, counts[index]);
    }

    function getHistogramY(count, scale, height, headroom = 0.9) {
        if (!Number.isFinite(count) || !Number.isFinite(scale) || scale <= 0 || height <= 0) {
            return Math.max(0, height);
        }
        const normalized = Math.max(0, Math.min(1, count / scale));
        return height - normalized * height * headroom;
    }

    return {
        DEFAULT_SCALE_PERCENTILE,
        getHistogramScale,
        getHistogramY,
    };
}));
