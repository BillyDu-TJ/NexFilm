const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const source = fs.readFileSync(path.join(__dirname, '..', 'ui', 'main.js'), 'utf8');

const listenDeclaration = source.indexOf('const listen = tauriEvents?.listen');
const firstListenCall = source.indexOf("listen('");
assert.ok(listenDeclaration >= 0, 'Tauri listen declaration is missing');
assert.ok(firstListenCall > listenDeclaration, 'Tauri listen is called before initialization');

for (const name of ['totalImportCount', 'currentImportCount', 'importFailedCount']) {
    const declaration = source.indexOf(`let ${name} = 0;`);
    const firstReference = source.indexOf(name);
    assert.ok(declaration >= 0, `${name} declaration is missing`);
    assert.equal(firstReference, declaration + 4, `${name} is referenced before initialization`);
    assert.equal(source.indexOf(`let ${name} = 0;`, declaration + 1), -1, `${name} is declared more than once`);
}

assert.doesNotMatch(
    source,
    /addEventListener\(\s*['"]change['"]\s*,\s*updateBackendParams\s*\)/,
    'A change event would be passed to updateBackendParams as the image id'
);

console.log('UI startup declaration order verified.');
