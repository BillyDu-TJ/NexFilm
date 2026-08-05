const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const runtimeAssets = [
    'ui/assets/design-reference/nexfilm-logo.svg',
    'ui/assets/design-reference/nexfilm-logo-dark.svg',
];

for (const relativePath of runtimeAssets) {
    const absolutePath = path.join(root, relativePath);
    assert.ok(fs.existsSync(absolutePath), 'Runtime asset is missing: ' + relativePath);
    assert.ok(fs.statSync(absolutePath).size > 0, 'Runtime asset is empty: ' + relativePath);
}

console.log('Runtime UI assets verified.');
