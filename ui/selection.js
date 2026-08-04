(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmSelection = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    function updateRangeSelection({ orderedIds, selectedIds, anchorId, targetId, shiftKey, additive }) {
        const order = Array.from(new Set((orderedIds || []).filter(Boolean)));
        const selected = new Set(selectedIds || []);
        if (!targetId || !order.includes(targetId)) {
            return { selectedIds: Array.from(selected), anchorId };
        }

        const anchorIndex = order.indexOf(anchorId);
        const targetIndex = order.indexOf(targetId);
        if (shiftKey && anchorIndex >= 0) {
            if (!additive) selected.clear();
            const start = Math.min(anchorIndex, targetIndex);
            const end = Math.max(anchorIndex, targetIndex);
            order.slice(start, end + 1).forEach(id => selected.add(id));
            return { selectedIds: Array.from(selected), anchorId };
        }

        if (additive) {
            if (selected.has(targetId)) selected.delete(targetId);
            else selected.add(targetId);
        } else {
            selected.clear();
            selected.add(targetId);
        }
        return { selectedIds: Array.from(selected), anchorId: targetId };
    }

    return { updateRangeSelection };
});
