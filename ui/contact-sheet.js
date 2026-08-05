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

    const formatGeometry = Object.freeze({
        '135': Object.freeze({
            horizontalGapRatio: 0.012,
            borderRatio: 0.18,
            verticalGapRatio: 0.08,
        }),
        '120': Object.freeze({
            // A 120 frame is about 56 mm high on 61-62 mm wide film.
            horizontalGapRatio: 0.006,
            borderRatio: 0.052,
            verticalGapRatio: 0.11,
        }),
    });

    function getContactSheetFormat(format) {
        const normalized = String(format || '135').toLowerCase().replace(/\u00d7/g, 'x');
        const is120 = normalized.includes('120');
        if (!is120) {
            return { is120: false, framesPerRow: 6, aspect: 2 / 3, ...formatGeometry['135'] };
        }

        const profile = formatProfiles.find(candidate =>
            candidate.matches.some(pattern => normalized.includes(pattern))
        );
        return {
            is120: true,
            framesPerRow: profile ? profile.framesPerRow : 3,
            aspect: profile ? profile.aspect : 1,
            ...formatGeometry['120'],
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

    function get120EdgeCode(frameNumber, filmName) {
        const bottomFrameNumber = Math.max(1, Math.trunc(Number(frameNumber) || 1));
        return {
            filmLabel: String(filmName || 'UNKNOWN FILM').trim().toUpperCase() || 'UNKNOWN FILM',
            topFrameNumber: 40 + bottomFrameNumber,
            bottomFrameNumber,
            formatLabel: '120',
            markerDirection: 'right',
        };
    }

    function draw120OrientationMarker(ctx, centerX, centerY, size) {
        const halfHeight = size * 0.42;
        const left = centerX - size * 0.44;
        const right = centerX + size * 0.44;

        ctx.beginPath();
        ctx.moveTo(left, centerY - halfHeight);
        ctx.lineTo(right, centerY);
        ctx.lineTo(left, centerY + halfHeight);
        ctx.closePath();
        ctx.stroke();
    }

    function draw120EdgeCodes(ctx, options) {
        const {
            x,
            y,
            imageWidth,
            imageHeight,
            borderHeight,
            frameNumber,
            filmName,
            color = '#D97736',
        } = options;
        const code = get120EdgeCode(frameNumber, filmName);
        const edgeFontSize = Math.max(11, Math.min(16, borderHeight * 0.34));
        const numberFontSize = Math.max(12, Math.min(18, borderHeight * 0.4));
        const inset = Math.max(12, borderHeight * 0.42);
        const topY = y + borderHeight * 0.52;
        const bottomY = y + borderHeight + imageHeight + borderHeight * 0.5;

        ctx.save();
        ctx.fillStyle = color;
        ctx.strokeStyle = color;
        ctx.lineWidth = Math.max(1.5, borderHeight * 0.055);
        ctx.lineJoin = 'round';
        ctx.textBaseline = 'middle';

        ctx.font = `700 ${edgeFontSize}px "Helvetica Neue Extended", "Helvetica Neue", Arial, sans-serif`;
        ctx.textAlign = 'center';
        ctx.fillText(code.filmLabel, x + imageWidth / 2, topY);
        ctx.font = `700 ${Math.max(10, edgeFontSize * 0.82)}px "Helvetica Neue", Arial, sans-serif`;
        ctx.textAlign = 'left';
        ctx.fillText(String(code.topFrameNumber), x + inset, topY);
        ctx.textAlign = 'right';
        ctx.fillText(code.formatLabel, x + imageWidth - inset, topY);

        ctx.font = `800 ${numberFontSize}px "Helvetica Neue Extended", "Helvetica Neue", Arial, sans-serif`;
        const numberText = String(code.bottomFrameNumber);
        const markerSize = numberFontSize * 0.72;
        const markerGap = numberFontSize * 0.42;
        const numberWidth = ctx.measureText(numberText).width;
        const groupWidth = markerSize + markerGap + numberWidth;
        const markerCenterX = x + (imageWidth - groupWidth) / 2 + markerSize / 2;
        draw120OrientationMarker(ctx, markerCenterX, bottomY, markerSize);
        ctx.textAlign = 'left';
        ctx.fillText(numberText, markerCenterX + markerSize / 2 + markerGap, bottomY);
        ctx.restore();
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
        get120EdgeCode,
        draw120EdgeCodes,
        createContactSheetFilename,
    };
}));
