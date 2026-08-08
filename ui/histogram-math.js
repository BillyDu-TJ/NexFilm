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

    const SCOPE_CHANNELS = Object.freeze(['rgb', 'red', 'green', 'blue', 'luma']);
    const TONE_ZONES = Object.freeze([
        Object.freeze({ id: 'shadows', label: '阴影', start: 0, end: 0.2 }),
        Object.freeze({ id: 'lowlights', label: '低光', start: 0.2, end: 0.4 }),
        Object.freeze({ id: 'midtones', label: '中灰', start: 0.4, end: 0.6 }),
        Object.freeze({ id: 'highlights', label: '高光', start: 0.6, end: 0.8 }),
        Object.freeze({ id: 'bright', label: '亮部', start: 0.8, end: 1 }),
    ]);

    function normalizeScopeChannel(value) {
        const channel = String(value || '').toLowerCase();
        return SCOPE_CHANNELS.includes(channel) ? channel : 'rgb';
    }

    function getToneZones() {
        return TONE_ZONES.map(zone => ({ ...zone }));
    }

    function classifyTone(value) {
        const normalized = Math.max(0, Math.min(0.999999, Number(value) || 0));
        return TONE_ZONES[Math.min(TONE_ZONES.length - 1, Math.floor(normalized * TONE_ZONES.length))].id;
    }

    function countOverflow(pixels, shadowThreshold = 0.02, highlightThreshold = 0.98) {
        let shadows = 0;
        let highlights = 0;
        let total = 0;
        const low = Math.max(0, Math.min(1, Number(shadowThreshold) || 0));
        const high = Math.max(low, Math.min(1, Number(highlightThreshold) || 1));
        for (let index = 0; index + 3 < pixels.length; index += 4) {
            const luma = (0.2126 * pixels[index] + 0.7152 * pixels[index + 1] + 0.0722 * pixels[index + 2]) / 255;
            total++;
            if (luma <= low) shadows++;
            if (luma >= high) highlights++;
        }
        return { shadows, highlights, total };
    }

    return {
        DEFAULT_SCALE_PERCENTILE,
        SCOPE_CHANNELS,
        TONE_ZONES,
        getHistogramScale,
        getHistogramY,
        normalizeScopeChannel,
        getToneZones,
        classifyTone,
        countOverflow,
    };
}));
