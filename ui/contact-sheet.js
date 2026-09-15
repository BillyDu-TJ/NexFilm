(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmContactSheet = api;
}(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    // Real 35 mm (ISO 1007) geometry. The perforation pitch is measured along
    // the film, so a strip keeps one even sprocket rhythm instead of restarting
    // at every frame boundary.
    // KS perforations are 1.98 mm along the film and 2.79 mm across it, so a
    // hole is a slot standing across the strip, not a sideways rectangle.
    const FILM_135 = Object.freeze({
        frameWidthMm: 36,
        frameGapMm: 2,
        frameHeightMm: 24,
        filmWidthMm: 35,
        perforationPitchMm: 4.75,
        perforationWidthMm: 1.98,
        perforationHeightMm: 2.79,
    });

    // Paper, film base and print colors for the exported sheet. The paper stays
    // clearly lighter than the film base so strips read as separate objects.
    const contactSheetTheme = Object.freeze({
        paper: '#E9E9E7',
        filmBase: '#000000',
        ink: '#1B1B1C',
        muted: '#6E6E73',
        edgeCode: '#D97736',
        perforation: '#FFFFFF',
        brand: '#0A5AA8',
    });

    const formatProfiles = Object.freeze([
        // 120 frames are drawn along the film, so a 6x4.5 frame is portrait:
        // about 41.5 mm along the film against 56 mm across it.
        Object.freeze({ matches: ['6x4.5', '645'], framesPerRow: 4, aspect: 4 / 3 }),
        Object.freeze({ matches: ['6x6'], framesPerRow: 4, aspect: 1 }),
        Object.freeze({ matches: ['6x7'], framesPerRow: 3, aspect: 6 / 7 }),
        Object.freeze({ matches: ['6x9'], framesPerRow: 2, aspect: 2 / 3 }),
        Object.freeze({ matches: ['6x12'], framesPerRow: 2, aspect: 1 / 2 }),
        Object.freeze({ matches: ['6x17'], framesPerRow: 1, aspect: 6 / 17 }),
    ]);

    const formatGeometry = Object.freeze({
        '135': Object.freeze({
            frameGapRatio: FILM_135.frameGapMm / FILM_135.frameWidthMm,
            borderRatio: (FILM_135.filmWidthMm - FILM_135.frameHeightMm) / 2 / FILM_135.frameHeightMm,
            verticalGapRatio: 0.08,
            perforationPitchRatio: FILM_135.perforationPitchMm / FILM_135.frameWidthMm,
        }),
        '120': Object.freeze({
            // Two 6x7 frames sit about 2.5 mm apart on 61-62 mm wide roll film.
            frameGapRatio: 0.035,
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
        const availableWidth = canvasWidth - outerMargin * 2;
        // Inter-frame gaps belong to the film, not to the sheet: derive the
        // image width from the physical frame-to-gap ratio so one row reads as
        // a single continuous strip with an even perforation rhythm.
        const imageWidth = availableWidth / (
            profile.framesPerRow + profile.frameGapRatio * (profile.framesPerRow - 1)
        );

        return {
            ...profile,
            horizontalGap: imageWidth * profile.frameGapRatio,
            imageWidth,
            imageHeight: imageWidth * profile.aspect,
        };
    }

    // Lay out the perforation run for one whole strip. Spacing comes from the
    // frame width and the real 4.75 mm pitch, never from the frame count, which
    // is what keeps the sprockets even across frame boundaries.
    function getPerforationLayout(options) {
        const {
            stripLeft = 0,
            stripWidth,
            frameWidth,
            pitchRatio = FILM_135.perforationPitchMm / FILM_135.frameWidthMm,
        } = options || {};

        const pitch = Math.max(1, Number(frameWidth) * pitchRatio);
        const holeWidth = pitch * (FILM_135.perforationWidthMm / FILM_135.perforationPitchMm);
        const holeHeight = pitch * (FILM_135.perforationHeightMm / FILM_135.perforationPitchMm);
        const count = Math.max(1, Math.round(Number(stripWidth) / pitch));
        const inset = (Number(stripWidth) - (count - 1) * pitch) / 2;
        const centers = [];
        for (let i = 0; i < count; i += 1) centers.push(stripLeft + inset + i * pitch);

        return { pitch, count, inset, holeWidth, holeHeight, centers };
    }

    // Draws the rebate of one 135 strip: an even perforation run plus the edge
    // codes. Perforations sit next to the picture area and the codes print in
    // the strip between the holes and the film edge, the way 35 mm edge print
    // is laid out on a real strip.
    function draw135EdgeCodes(ctx, options) {
        const {
            stripLeft,
            stripTop,
            stripWidth,
            imageHeight,
            borderHeight,
            frames = [],
            color = contactSheetTheme.edgeCode,
            holeColor = contactSheetTheme.perforation,
            pitchRatio,
        } = options;

        const frameWidth = frames.length ? frames[0].width : stripWidth;
        const perforations = getPerforationLayout({
            stripLeft,
            stripWidth,
            frameWidth,
            pitchRatio: pitchRatio || FILM_135.perforationPitchMm / FILM_135.frameWidthMm,
        });

        const topBand = stripTop;
        const bottomBand = stripTop + borderHeight + imageHeight;
        // ~1 mm between the perforation and the picture area on the real strip.
        const holeInset = borderHeight * 0.15;
        const codeStrip = Math.max(0, borderHeight - holeInset - perforations.holeHeight);
        const topHoleY = topBand + borderHeight - holeInset - perforations.holeHeight / 2;
        const bottomHoleY = bottomBand + holeInset + perforations.holeHeight / 2;
        const topTextY = topBand + codeStrip / 2;
        const bottomTextY = bottomBand + borderHeight - codeStrip / 2;
        const fontSize = Math.max(11, Math.min(borderHeight * 0.24, codeStrip * 0.78));

        ctx.save();
        ctx.fillStyle = holeColor;
        const holeRadius = Math.min(perforations.holeWidth, perforations.holeHeight) * 0.28;
        for (const center of perforations.centers) {
            const holeX = center - perforations.holeWidth / 2;
            ctx.beginPath();
            ctx.roundRect(
                holeX,
                topHoleY - perforations.holeHeight / 2,
                perforations.holeWidth,
                perforations.holeHeight,
                holeRadius
            );
            ctx.fill();
            ctx.beginPath();
            ctx.roundRect(
                holeX,
                bottomHoleY - perforations.holeHeight / 2,
                perforations.holeWidth,
                perforations.holeHeight,
                holeRadius
            );
            ctx.fill();
        }

        ctx.fillStyle = color;
        ctx.font = `900 ${fontSize}px "Helvetica Neue Extended", "Helvetica Neue", Arial, sans-serif`;
        ctx.textBaseline = 'middle';
        ctx.textAlign = 'center';
        for (const frame of frames) {
            if (frame.label) {
                ctx.fillText(String(frame.label), frame.x + frame.width / 2, topTextY);
            }
            if (frame.frameNumber != null) {
                const number = String(frame.frameNumber);
                ctx.fillText(number, frame.x + frame.width * 0.25, bottomTextY);
                ctx.fillText(`${number}A`, frame.x + frame.width * 0.75, bottomTextY);
            }
        }
        ctx.restore();

        return perforations;
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

    // Brand assets (ui/assets/design-reference/nexfilm-logo.svg and
    // NEXFILM_word.svg) reduced to their path data so the footer can draw the
    // designed lockup instead of re-typing the name in a generic font.
    const NEXFILM_MARK_PATHS = Object.freeze([
        'M56 36H40V21.6L46 27l-6-7.36V0h16v36Zm-4-4v2h2v-2h-2Zm0-2h2v-2h-2v2Zm0-4h2v-2h-2v2Zm0-4h2v-2h-2v2Zm0-4h2v-2h-2v2Zm0-4h2v-2h-2v2Zm0-4h2V8h-2v2Zm0-4h2V4h-2v2Z',
        'M16 0 24 7.2v14.4L20 18l4 6v12H8V0h8Z',
        'M0 0h16l40 36H40L0 0Z',
    ]);
    const NEXFILM_MARK_SIZE = Object.freeze({ width: 56, height: 36 });
    const NEXFILM_WORDMARK_PATH = 'M5.37485e-05 7V-4.76837e-07H1.94005L5.50005 4.26H4.60005V-4.76837e-07H6.90005V7H4.96005L1.40005 2.74H2.30005V7H5.37485e-05ZM8.03716 7V-4.76837e-07H13.7272V1.78H10.3572V5.22H13.8572V7H8.03716ZM10.1972 4.3V2.6H13.3172V4.3H10.1972ZM14.0766 7L17.1966 2.64L17.1866 4.29L14.1666 -4.76837e-07H16.8166L18.5766 2.6L17.4466 2.61L19.1666 -4.76837e-07H21.7066L18.6866 4.2V2.56L21.8566 7H19.1566L17.3966 4.28L18.4866 4.27L16.7666 7H14.0766ZM22.1485 7V-4.76837e-07H27.8385V1.78H24.5085V7H22.1485ZM24.3485 4.76V2.98H27.4285V4.76H24.3485ZM28.584 7V-4.76837e-07H30.944V7H28.584ZM32.0801 7V-4.76837e-07H34.4401V5.17H37.6001V7H32.0801ZM38.252 7V-4.76837e-07H40.192L42.992 4.57H41.972L44.692 -4.76837e-07H46.632L46.652 7H44.502L44.482 3.24H44.822L42.962 6.37H41.922L39.982 3.24H40.402V7H38.252Z';
    const NEXFILM_WORDMARK_HEIGHT = 7;
    const NEXFILM_WORDMARK_WIDTH = 47;

    function canBuildSvgPath() {
        if (typeof Path2D !== 'function') return false;
        try {
            new Path2D(NEXFILM_WORDMARK_PATH);
            return true;
        } catch (error) {
            return false;
        }
    }

    // Draws the mark plus the designed wordmark, anchored on its bottom edge so
    // both share one optical baseline. Returns the occupied size.
    function drawNexfilmLockup(ctx, options) {
        const {
            x = 0,
            bottomY = 0,
            markHeight = 92,
            markColor = contactSheetTheme.brand,
            wordmarkColor = contactSheetTheme.ink,
            gap = markHeight * 0.34,
            wordmarkScale = 0.8,
        } = options || {};

        const markWidth = markHeight * (NEXFILM_MARK_SIZE.width / NEXFILM_MARK_SIZE.height);
        const wordmarkHeight = markHeight * wordmarkScale;
        const wordmarkWidth = wordmarkHeight * (NEXFILM_WORDMARK_WIDTH / NEXFILM_WORDMARK_HEIGHT);
        const totalWidth = markWidth + gap + wordmarkWidth;
        const markTop = bottomY - markHeight;

        if (!canBuildSvgPath()) {
            ctx.save();
            ctx.fillStyle = wordmarkColor;
            ctx.font = `800 ${Math.round(markHeight * 0.8)}px "Helvetica Neue Extended", "Helvetica Neue", Inter, sans-serif`;
            ctx.textAlign = 'left';
            ctx.textBaseline = 'alphabetic';
            ctx.fillText('NEXFILM', x, bottomY);
            ctx.restore();
            return { width: totalWidth, height: markHeight };
        }

        ctx.save();
        ctx.translate(x, markTop);
        const markScale = markHeight / NEXFILM_MARK_SIZE.height;
        ctx.scale(markScale, markScale);
        ctx.fillStyle = markColor;
        for (const pathData of NEXFILM_MARK_PATHS) ctx.fill(new Path2D(pathData));
        ctx.restore();

        ctx.save();
        ctx.translate(x + markWidth + gap, bottomY - wordmarkHeight);
        const wordmarkScaleFactor = wordmarkHeight / NEXFILM_WORDMARK_HEIGHT;
        ctx.scale(wordmarkScaleFactor, wordmarkScaleFactor);
        ctx.fillStyle = wordmarkColor;
        ctx.fill(new Path2D(NEXFILM_WORDMARK_PATH));
        ctx.restore();

        return { width: totalWidth, height: markHeight };
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
        getPerforationLayout,
        draw135EdgeCodes,
        get120EdgeCode,
        draw120EdgeCodes,
        drawNexfilmLockup,
        contactSheetTheme,
        createContactSheetFilename,
    };
}));
