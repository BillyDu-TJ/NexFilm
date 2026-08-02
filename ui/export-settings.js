(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmExport = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    const TOKENS = ['{Roll}', '{Camera}', '{Film}', '{Date}', '{Original}', '{Seq}'];
    const INVALID_FILENAME_CHARS = /[<>:"/\\|?*\u0000-\u001f]/g;
    const WINDOWS_RESERVED_NAMES = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i;

    function sanitizeFileStem(value) {
        let stem = String(value || '')
            .replace(INVALID_FILENAME_CHARS, '_')
            .replace(/\s+/g, ' ')
            .trim()
            .replace(/[. ]+$/g, '');
        if (!stem) stem = 'Export';
        if (WINDOWS_RESERVED_NAMES.test(stem)) stem = '_' + stem;
        return Array.from(stem).slice(0, 180).join('');
    }

    function formatExportTemplate(template, metadata = {}) {
        const replacements = {
            '{Roll}': metadata.roll || 'Roll',
            '{Camera}': metadata.camera || 'Camera',
            '{Film}': metadata.film || 'Film',
            '{Date}': metadata.date || 'Undated',
            '{Original}': metadata.original || 'Image',
            '{Seq}': metadata.seq || '001',
        };
        let result = String(template || '');
        for (const token of TOKENS) result = result.split(token).join(replacements[token]);
        return sanitizeFileStem(result);
    }

    function findUnknownTokens(template) {
        return [...String(template || '').matchAll(/\{[^{}]+\}/g)]
            .map(match => match[0])
            .filter(token => !TOKENS.includes(token));
    }

    function validateExportSettings(settings) {
        if (!String(settings.namingTemplate || '').trim()) {
            return 'Enter a filename template.';
        }
        const unknownTokens = findUnknownTokens(settings.namingTemplate);
        if (unknownTokens.length) return 'Unknown filename token: ' + unknownTokens[0];
        if (settings.resizeMode === 'long_edge') {
            const edge = Number(settings.longEdge);
            if (!Number.isInteger(edge) || edge < 256 || edge > 32768) {
                return 'Long edge must be between 256 and 32768 pixels.';
            }
        }
        return null;
    }

    function describeResize(settings) {
        if (settings.resizeMode !== 'long_edge') return 'Original dimensions';
        return settings.longEdge + ' px long edge' + (settings.allowUpscale ? ', enlargement allowed' : '');
    }

    function createExportInvokeArgs(exportIds, outputDir, settings) {
        return {
            exportIds: Array.from(exportIds || []),
            outputDir,
            format: settings.format,
            colorSpace: settings.colorSpace,
            resizeMode: settings.resizeMode,
            longEdge: Number(settings.longEdge),
            allowUpscale: Boolean(settings.allowUpscale),
            sharpening: settings.sharpening,
            namingTemplate: settings.namingTemplate,
            conflictPolicy: settings.conflictPolicy,
            quality: Number(settings.quality),
        };
    }

    return {
        TOKENS,
        createExportInvokeArgs,
        describeResize,
        findUnknownTokens,
        formatExportTemplate,
        sanitizeFileStem,
        validateExportSettings,
    };
});
