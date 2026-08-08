(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmRangeMath = api;
}(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    function finiteNumber(value, fallback = 0) {
        const parsed = Number.parseFloat(String(value).trim().replace(',', '.'));
        return Number.isFinite(parsed) ? parsed : fallback;
    }

    function decimalPlaces(value) {
        const text = String(value || '');
        if (/e-/i.test(text)) return Math.max(0, Number(text.split(/e-/i)[1]) || 0);
        return (text.split('.')[1] || '').length;
    }

    function clampRangeValue(value, min, max, step = 'any') {
        const lower = finiteNumber(min, Number.NEGATIVE_INFINITY);
        const upper = finiteNumber(max, Number.POSITIVE_INFINITY);
        let next = Math.min(upper, Math.max(lower, finiteNumber(value, lower)));
        const increment = finiteNumber(step, 0);
        if (step !== 'any' && increment > 0 && Number.isFinite(lower)) {
            next = lower + Math.round((next - lower) / increment) * increment;
            next = Math.min(upper, Math.max(lower, next));
            next = Number(next.toFixed(Math.max(decimalPlaces(step), decimalPlaces(min))));
        }
        return next;
    }

    function resetRangeValue({ min, max, step, defaultValue }) {
        return clampRangeValue(defaultValue, min, max, step);
    }

    return { finiteNumber, clampRangeValue, resetRangeValue };
}));
