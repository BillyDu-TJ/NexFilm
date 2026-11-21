const assert = require('node:assert/strict');
const fs = require('node:fs');

const mainSource = fs.readFileSync(__dirname + '/main.js', 'utf8');

assert.match(mainSource, /window\.__TAURI__\?\.app/);
assert.match(mainSource, /getVersion\?\.\(\)/);
assert.doesNotMatch(mainSource, /NexFilm Engine v1\.0/);
assert.match(mainSource, /`NexFilm Engine v\$\{version\}`/);

console.log('About version follows Tauri app metadata.');
