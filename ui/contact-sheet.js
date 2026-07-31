(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmContactSheet = api;
}(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    const formatProfiles = Object.freeze([
        Object.freeze({ matches: ['6x4.5', '645'], framesPerRow: 4, aspect: 3 / 4 }),
        Object.freeze({ matches: ['6x6'], framesPerRow: 4, aspect: 1 }),
        Object.freeze({ matches: ['6x7'], framesPerRow: 3, aspect: 6 / 7 }),
        Object.freeze({ matches: ['6x9'], framesPerRow: 2, aspect: 2 / 3 }),
        Object.freeze({ matches: ['6x12'], framesPerRow: 2, aspect: 1 / 2 }),
        Object.freeze({ matches: ['6x17'], framesPerRow: 1, aspect: 6 / 17 }),
    ]);

    function getContactSheetFormat(format) {
        const normalized = String(format || '135').toLowerCase().replace(/\u00d7/g, 'x');
        const is120 = normalized.includes('120');
        if (!is120) {
            return { is120: false, framesPerRow: 6, aspect: 2 / 3, horizontalGapRatio: 0.012 };
        }

        const profile = formatProfiles.find(candidate =>
            candidate.matches.some(pattern => normalized.includes(pattern))
        );
        return {
            is120: true,
            framesPerRow: profile ? profile.framesPerRow : 3,
            aspect: profile ? profile.aspect : 1,
            horizontalGapRatio: 0.02,
        };
    }

    function getContactSheetLayout(format, canvasWidth = 3000, outerMargin = 100) {
        const profile = getContactSheetFormat(format);
        const horizontalGap = canvasWidth * profile.horizontalGapRatio;
        const availableWidth = canvasWidth - outerMargin * 2;
        const imageWidth = (
            availableWidth - horizontalGap * (profile.framesPerRow - 1)
        ) / profile.framesPerRow;

        return {
            ...profile,
            horizontalGap,
            imageWidth,
            imageHeight: imageWidth * profile.aspect,
        };
    }

    function safeFilenamePart(value, fallback) {
        const normalized = String(value || '')
            .trim()
            .replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_')
            .replace(/\s+/g, '_')
            .replace(/[. ]+$/g, '')
            .replace(/_+/g, '_');
        return (normalized || fallback).slice(0, 80);
    }

    function createContactSheetFilename(roll) {
        const rollId = safeFilenamePart(roll && roll.roll_id, 'unknown_roll');
        const camera = safeFilenamePart(roll && roll.camera, 'unknown_camera');
        return `contact_sheet_${rollId}_${camera}.jpg`;
    }

    return {
        getContactSheetFormat,
        getContactSheetLayout,
        createContactSheetFilename,
    };
}));
