const fs = require('fs');
let code = fs.readFileSync('E:/Code/NegativeConverter/ui/main.js', 'utf8');

const proxyReadyEndIndex = code.indexOf("listen('import_progress'");
if (proxyReadyEndIndex !== -1) {
    let proxyReadySection = code.substring(0, proxyReadyEndIndex);
    if (!proxyReadySection.includes('// Update import progress bar based on proxy readiness')) {
        proxyReadySection = proxyReadySection.replace('    }\n});\n', '    }\n\n    // Update import progress bar based on proxy readiness\n    if (precacheToast && totalImportCount > 0) {\n        currentImportCount++;\n        const pct = (currentImportCount / totalImportCount) * 100;\n        const bar = document.getElementById(\'precache-bar\');\n        const txt = document.getElementById(\'precache-text\');\n        if (bar) bar.style.width = `${pct}%`;\n        if (txt) txt.textContent = `Processing ${currentImportCount} / ${totalImportCount}`;\n        \n        if (currentImportCount >= totalImportCount) {\n            setTimeout(() => {\n                if (precacheToast) {\n                    precacheToast.remove();\n                    precacheToast = null;\n                }\n                btnImport.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-4 h-4"><path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15" /></svg>IMPORT`;\n                restoreImportButtons();\n                renderLibraryAndFilmstrip(false);\n            }, 1000);\n        }\n    }\n});\n');
        code = proxyReadySection + code.substring(proxyReadyEndIndex);
    }
}

const importProgressStart = code.indexOf('    currentImportCount = Number.isFinite(payload.processed)');
const importProgressEnd = code.indexOf("});\n\nlisten('import_complete'");
if (importProgressStart !== -1 && importProgressEnd !== -1) {
    const replacement = '    // currentImportCount is now tracking actual proxy readiness, not just DB inserts.\n    // We only track totalImportCount here for the max value.\n    if (Number.isFinite(payload.total) && payload.total > 0) {\n        totalImportCount = payload.total;\n    }\n';
    code = code.substring(0, importProgressStart) + replacement + code.substring(importProgressEnd);
}

fs.writeFileSync('E:/Code/NegativeConverter/ui/main.js', code, 'utf8');
console.log('Modified main.js successfully');
