const fs = require('fs');
const path = require('path');

function walk(dir) {
  let files = [];
  for (const item of fs.readdirSync(dir)) {
    const p = path.join(dir, item);
    if (fs.statSync(p).isDirectory()) {
      files = files.concat(walk(p));
    } else {
      files.push(p);
    }
  }
  return files;
}

const targetDir = path.resolve(__dirname, '../src');
const allFiles = walk(targetDir);

const re1 = /\b(h-15|w-15|py-0\.2|px-0\.2|shadow-glow)\b/i;
const re2 = /#(0f1117|0e1015|14161c|0b0d13|13161f|12141c|16181f|181b22|1d2029)/i;
const re3 = /\b(bg-amber-500|bg-amber-400)\b/i;
const re4 = /(href|src)=["\x27]\/(zh|en|assets|nexfilm-logo|favicon)/i;

let violations = 0;

for (const file of allFiles) {
  const content = fs.readFileSync(file, 'utf8');
  const lines = content.split('\n');
  lines.forEach((l, idx) => {
    if (re1.test(l)) {
      console.error(`[Illegal Class] ${file}:${idx + 1}: ${l.trim()}`);
      violations++;
    }
    if (re2.test(l)) {
      console.error(`[Cold Blue-Black] ${file}:${idx + 1}: ${l.trim()}`);
      violations++;
    }
    if (re3.test(l)) {
      console.error(`[Bright Neon/Amber] ${file}:${idx + 1}: ${l.trim()}`);
      violations++;
    }
    if (re4.test(l)) {
      console.error(`[Raw Path Without withBase] ${file}:${idx + 1}: ${l.trim()}`);
      violations++;
    }
  });
}

if (violations > 0) {
  console.error(`\n❌ Static lint failed: found ${violations} violations in website/src/`);
  process.exit(1);
} else {
  console.log(`\n✓ Static lint passed: 0 violations across ${allFiles.length} files in website/src/`);
}
