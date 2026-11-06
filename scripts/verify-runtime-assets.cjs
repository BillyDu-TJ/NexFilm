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

// Phase 3.1 guardrail: the vendored rsraw-sys builds bindings from its own
// LibRaw 0.22.2 headers at build time. The upstream 0.1.1 pregenerated
// bindings describe LibRaw 0.21.3 struct layouts and must never come back
// into the vendor tree (a HOST != TARGET fallback once copied them, which
// mislaid every struct against the 0.22.2 library; see vendor/rsraw-sys/README.md).
const staleBindings = [
    'vendor/rsraw-sys/src/bindings.rs',
    'vendor/rsraw-sys/bindings.rs',
];
for (const relativePath of staleBindings) {
    assert.ok(
        !fs.existsSync(path.join(root, relativePath)),
        'Stale pre-generated LibRaw bindings must not exist in vendor: ' + relativePath
    );
}

// The HOST != TARGET fallback must stay a panic, never a copy of stale bindings.
const buildRs = fs.readFileSync(path.join(root, 'vendor/rsraw-sys/build.rs'), 'utf8');
assert.ok(
    !/fs::copy\(\s*"src\/bindings\.rs"/.test(buildRs),
    'vendor/rsraw-sys/build.rs must not copy pregenerated src/bindings.rs'
);
assert.ok(
    /panic!\(/.test(buildRs) && /HOST\s*!=\s*TARGET|host\s*!=\s*target/.test(buildRs),
    'vendor/rsraw-sys/build.rs must panic on cross builds (HOST != TARGET)'
);

console.log('Runtime UI assets verified.');
