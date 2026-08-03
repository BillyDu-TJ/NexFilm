const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const root = path.resolve(__dirname, '..');
const source = fs.readFileSync(path.join(root, 'ui', 'i18n.js'), 'utf8');
const stored = new Map();

const emptyRoot = {
    querySelectorAll: () => [],
};
const document = {
    ...emptyRoot,
    readyState: 'loading',
    addEventListener: () => {},
    createTreeWalker: () => ({ nextNode: () => null }),
    documentElement: {},
    body: { dataset: {} },
    title: '',
};
const sandbox = {
    console,
    document,
    NodeFilter: { SHOW_TEXT: 4 },
    localStorage: {
        getItem: key => stored.get(key) ?? null,
        setItem: (key, value) => stored.set(key, String(value)),
    },
    window: {},
};

vm.runInNewContext(source, sandbox, { filename: 'ui/i18n.js' });
const i18n = sandbox.window.NexFilmI18n;
assert.ok(i18n, 'i18n module did not expose its public API');

assert.equal(i18n.getLocale(), 'en');
assert.equal(i18n.t('nav.library'), 'Library');
assert.equal(i18n.t('library.selectedCount', { count: 4 }), '4 selected');
assert.equal(i18n.translateLegacy('Import failed: disk'), 'Import failed: disk');

i18n.setLocale('zh-CN');
assert.equal(i18n.getLocale(), 'zh-CN');
assert.equal(stored.get('nexfilm-language'), 'zh-CN');
assert.equal(i18n.t('nav.library'), '图库');
assert.equal(i18n.t('library.selectedCount', { count: 4 }), '已选 4 张');
assert.equal(i18n.translateLegacy('Import failed: disk'), '导入失败：disk');
assert.equal(document.documentElement.lang, 'zh-CN');
assert.equal(document.body.dataset.locale, 'zh-CN');

const mainSource = fs.readFileSync(path.join(root, 'ui', 'main.js'), 'utf8');
assert.equal(/[\u3400-\u9fff]/u.test(mainSource), false, 'translated UI copy must stay out of ui/main.js');

const htmlSource = fs.readFileSync(path.join(root, 'ui', 'index.html'), 'utf8');
const referencedKeys = new Set();
for (const match of htmlSource.matchAll(/data-i18n(?:-placeholder|-title|-aria-label|-alt)?="([A-Za-z0-9_.-]+)"/g)) {
    referencedKeys.add(match[1]);
}
for (const match of mainSource.matchAll(/i18nText\('([A-Za-z0-9_.-]+)'/g)) {
    referencedKeys.add(match[1]);
}

for (const key of referencedKeys) {
    assert.notEqual(i18n.t(key), key, `missing zh-CN translation for ${key}`);
    i18n.setLocale('en');
    assert.notEqual(i18n.t(key), key, `missing English translation for ${key}`);
    i18n.setLocale('zh-CN');
}

console.log(`i18n contract verified (${referencedKeys.size} referenced keys).`);
