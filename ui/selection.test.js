const assert = require('node:assert/strict');
const { updateRangeSelection } = require('./selection.js');

const order = ['a', 'b', 'c', 'd', 'e'];

let result = updateRangeSelection({
    orderedIds: order, selectedIds: [], anchorId: null, targetId: 'b', shiftKey: false, additive: false
});
assert.deepEqual(result, { selectedIds: ['b'], anchorId: 'b' });

result = updateRangeSelection({
    orderedIds: order, selectedIds: result.selectedIds, anchorId: result.anchorId, targetId: 'e', shiftKey: true, additive: false
});
assert.deepEqual(result, { selectedIds: ['b', 'c', 'd', 'e'], anchorId: 'b' });

result = updateRangeSelection({
    orderedIds: order, selectedIds: ['a'], anchorId: 'b', targetId: 'd', shiftKey: true, additive: true
});
assert.deepEqual(result, { selectedIds: ['a', 'b', 'c', 'd'], anchorId: 'b' });

result = updateRangeSelection({
    orderedIds: order, selectedIds: ['a', 'c'], anchorId: 'c', targetId: 'c', shiftKey: false, additive: true
});
assert.deepEqual(result, { selectedIds: ['a'], anchorId: 'c' });

result = updateRangeSelection({
    orderedIds: order, selectedIds: ['a'], anchorId: 'missing', targetId: 'd', shiftKey: true, additive: false
});
assert.deepEqual(result, { selectedIds: ['d'], anchorId: 'd' });

console.log('Selection module tests passed.');
