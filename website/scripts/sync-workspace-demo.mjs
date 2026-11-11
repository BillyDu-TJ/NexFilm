import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const uiSourceDir = path.resolve(__dirname, '../../ui');
const targetDemoDir = path.resolve(__dirname, '../public/workspace-demo');
const shimSourcePath = path.resolve(__dirname, 'tauri-web-shim.js');
const repoLutsDir = path.resolve(__dirname, '../../assets/luts');

if (!fs.existsSync(uiSourceDir)) {
  console.error(`[sync-workspace-demo] Source directory not found: ${uiSourceDir}`);
  process.exit(1);
}

console.log(`[sync-workspace-demo] Syncing UI assets to ${targetDemoDir}...`);

// Ensure target dir exists
fs.mkdirSync(targetDemoDir, { recursive: true });

function copyRecursive(src, dest) {
  if (!fs.existsSync(src)) return;
  const stat = fs.statSync(src);
  if (stat.isDirectory()) {
    fs.mkdirSync(dest, { recursive: true });
    for (const child of fs.readdirSync(src)) {
      copyRecursive(path.join(src, child), path.join(dest, child));
    }
  } else {
    fs.copyFileSync(src, dest);
  }
}

// Copy top-level CSS & JS from ui/
const entries = fs.readdirSync(uiSourceDir, { withFileTypes: true });

for (const entry of entries) {
  const srcPath = path.join(uiSourceDir, entry.name);
  const destPath = path.join(targetDemoDir, entry.name);

  if (entry.isDirectory()) {
    if (entry.name === 'assets') {
      copyRecursive(srcPath, destPath);
    }
  } else if (entry.isFile()) {
    if (entry.name.endsWith('.test.js')) {
      continue;
    }
    if (entry.name === 'index.html') {
      let html = fs.readFileSync(srcPath, 'utf8');
      html = html.replace('data-theme="light"', 'data-theme="dark"');
      const targetScript = '<script src="geometry.js"></script>';
      const injectedShim = '<script src="tauri-web-shim.js"></script>\n    ' + targetScript;
      if (html.includes(targetScript)) {
        html = html.replace(targetScript, injectedShim);
      } else {
        html = html.replace('</body>', '<script src="tauri-web-shim.js"></script>\n</body>');
      }
      fs.writeFileSync(destPath, html, 'utf8');
    } else if (entry.name.endsWith('.css') || entry.name.endsWith('.js')) {
      fs.copyFileSync(srcPath, destPath);
    }
  }
}

// Copy tauri-web-shim.js
fs.copyFileSync(shimSourcePath, path.join(targetDemoDir, 'tauri-web-shim.js'));

// If repository assets/luts exist, copy them to targetDemoDir/assets/luts
if (fs.existsSync(repoLutsDir)) {
  const destLuts = path.join(targetDemoDir, 'assets/luts');
  copyRecursive(repoLutsDir, destLuts);
}

console.log(`[sync-workspace-demo] Successfully synced workspace demo assets.`);
