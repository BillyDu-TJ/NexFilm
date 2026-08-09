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

    function getWheelRangeValue(options = {}) {
        const current = clampRangeValue(options.value, options.min, options.max, options.step);
        const deltaX = finiteNumber(options.deltaX, 0);
        const deltaY = finiteNumber(options.deltaY, 0);
        const primaryDelta = Math.abs(deltaX) > Math.abs(deltaY) ? deltaX : deltaY;
        if (primaryDelta === 0) return current;

        const lower = finiteNumber(options.min, Number.NEGATIVE_INFINITY);
        const upper = finiteNumber(options.max, Number.POSITIVE_INFINITY);
        const span = Number.isFinite(lower) && Number.isFinite(upper) ? Math.abs(upper - lower) : 0;
        const stepValue = options.step === 'any' ? 0 : Math.abs(finiteNumber(options.step, 0));
        const baseStep = Math.max(stepValue || 0, span > 0 ? span / 100 : 1);
        const wheelUnits = Math.min(6, Math.max(0.2, Math.abs(primaryDelta) / 100));
        const multiplier = options.shiftKey ? 5 : options.altKey ? 0.25 : 1;
        const direction = primaryDelta > 0 ? -1 : 1;

        return clampRangeValue(
            current + direction * baseStep * wheelUnits * multiplier,
            options.min,
            options.max,
            options.step
        );
    }

    return { finiteNumber, clampRangeValue, resetRangeValue, getWheelRangeValue };
}));
