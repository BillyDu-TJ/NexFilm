(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmInteractions = api;
}(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    function finite(value) {
        const number = Number(value);
        return Number.isFinite(number) ? number : 0;
    }

    function normalizeWheelDelta(event, pageSize = 800, lineHeight = 16) {
        const mode = Number(event?.deltaMode) || 0;
        const scale = mode === 1
            ? Math.max(1, finite(lineHeight))
            : mode === 2
                ? Math.max(1, finite(pageSize))
                : 1;
        return {
            x: finite(event?.deltaX) * scale,
            y: finite(event?.deltaY) * scale,
        };
    }

    function horizontalWheelDelta(delta) {
        const x = finite(delta?.x);
        const y = finite(delta?.y);
        return Math.abs(x) > Math.abs(y) ? x : y;
    }

    function canScrollBy(position, maximum, delta) {
        const current = finite(position);
        const limit = Math.max(0, finite(maximum));
        const movement = finite(delta);
        if (movement < 0) return current > 0;
        if (movement > 0) return current < limit;
        return false;
    }

    function getZoomViewUpdate(options) {
        const previousZoom = Math.max(0.0001, finite(options?.zoom) || 1);
        const minZoom = Math.max(0.0001, finite(options?.minZoom) || 0.1);
        const maxZoom = Math.max(minZoom, finite(options?.maxZoom) || 10);
        const delta = Math.max(-1000, Math.min(1000, finite(options?.deltaY)));
        const nextZoom = Math.max(minZoom, Math.min(maxZoom, previousZoom * Math.exp(-delta * 0.0015)));
        const ratio = nextZoom / previousZoom;
        const panX = finite(options?.panX);
        const panY = finite(options?.panY);
        const pointerX = finite(options?.pointerX);
        const pointerY = finite(options?.pointerY);
        const centerX = finite(options?.centerX);
        const centerY = finite(options?.centerY);
        return {
            zoom: nextZoom,
            panX: ratio * panX + (1 - ratio) * (pointerX - centerX),
            panY: ratio * panY + (1 - ratio) * (pointerY - centerY),
        };
    }

    function getCanvasCompositeTransform(options) {
        const zoom = Math.max(0.0001, finite(options?.zoom) || 1);
        const panX = finite(options?.panX);
        const panY = finite(options?.panY);
        return `translate3d(${panX}px, ${panY}px, 0) scale(${zoom})`;
    }

    function formatZoomPercent(zoom) {
        const safeZoom = Math.max(0.0001, finite(zoom) || 1);
        const percent = safeZoom * 100;
        const rounded = percent >= 100 ? Math.round(percent) : Math.round(percent * 10) / 10;
        return `${rounded}%`;
    }

    function getPreviewProxyTarget(options) {
        const displayLongEdge = Math.max(
            1,
            finite(options?.displayWidth),
            finite(options?.displayHeight)
        );
        const renderedLongEdge = Math.max(
            1,
            finite(options?.renderedWidth),
            finite(options?.renderedHeight)
        );
        const sourceLongEdge = Math.max(1, finite(options?.sourceLongEdge));
        const currentLongEdge = Math.max(1, finite(options?.currentLongEdge) || sourceLongEdge);
        const attemptedLongEdge = Math.max(0, finite(options?.attemptedLongEdge));
        const pixelRatio = Math.max(1, finite(options?.pixelRatio) || 1);
        const baseLongEdge = Math.max(1, finite(options?.baseLongEdge) || currentLongEdge);
        const maxLongEdge = Math.max(baseLongEdge, finite(options?.maxLongEdge) || baseLongEdge);
        const step = Math.max(1, finite(options?.step) || 1024);
        const requiredLongEdge = sourceLongEdge * displayLongEdge * pixelRatio / renderedLongEdge;

        if (requiredLongEdge <= currentLongEdge * 1.1 || requiredLongEdge <= attemptedLongEdge) {
            return null;
        }

        const bucketed = Math.ceil(requiredLongEdge / step) * step;
        // The base request may legitimately produce less than baseLongEdge
        // (for example LibRaw half-size output from a smaller RAW). Keep that
        // request cheap; an adaptive upgrade must move to the next bucket so
        // the backend can distinguish it from ordinary navigation prewarming.
        const minimumUpgrade = Math.min(maxLongEdge, baseLongEdge + step);
        const target = Math.min(maxLongEdge, Math.max(minimumUpgrade, bucketed));
        return target > currentLongEdge ? target : null;
    }

    function selectFilmAreaBatchTargets(options) {
        const items = Array.isArray(options?.items) ? options.items : [];
        const activeId = options?.activeId;
        const source = items.find(item => item?.id === activeId);
        if (!source) return [];
        const selectedIds = new Set(options?.selectedIds || []);
        const seen = new Set();
        return items.filter(item => {
            if (!item || item.id === activeId || !item.file_path) return false;
            const identity = `${item.roll_id || 'LOOSE_DEFAULT'}::${String(item.file_path).replaceAll('\\', '/').toLowerCase()}`;
            if (seen.has(identity)) return false;
            const eligible = options?.mode === 'selected'
                ? selectedIds.has(item.id)
                : item.roll_id === source.roll_id;
            if (eligible) seen.add(identity);
            return eligible;
        });
    }

    function getNormalizedCropAspect(preset, imageWidth, imageHeight) {
        if (!preset || preset === 'free') return null;
        if (preset === 'original') return 1;
        const match = /^(\d+(?:\.\d+)?):(\d+(?:\.\d+)?)$/.exec(String(preset));
        if (!match) return null;
        const outputAspect = Number(match[1]) / Number(match[2]);
        const width = Math.max(1, finite(imageWidth));
        const height = Math.max(1, finite(imageHeight));
        return outputAspect * height / width;
    }

    function fitCropRectToAspect(rect, aspect, minimumSize = 0.01) {
        const source = {
            x: finite(rect?.x),
            y: finite(rect?.y),
            width: Math.max(minimumSize, finite(rect?.width)),
            height: Math.max(minimumSize, finite(rect?.height)),
        };
        if (!(aspect > 0)) return source;
        let width = source.width;
        let height = source.height;
        if (width / height > aspect) width = height * aspect;
        else height = width / aspect;
        return {
            x: Math.max(0, Math.min(1 - width, source.x + (source.width - width) / 2)),
            y: Math.max(0, Math.min(1 - height, source.y + (source.height - height) / 2)),
            width,
            height,
        };
    }

    function constrainCropResize(startRect, candidateRect, dragType, aspect, minimumSize = 0.01) {
        if (!(aspect > 0) || dragType === 'box' || dragType === 'rotate') return candidateRect;
        const start = { ...startRect };
        const candidate = { ...candidateRect };
        const movesWest = dragType.includes('w');
        const movesEast = dragType.includes('e');
        const movesNorth = dragType.includes('n');
        const movesSouth = dragType.includes('s');
        const horizontal = movesWest || movesEast;
        const vertical = movesNorth || movesSouth;

        let width;
        let height;
        if (horizontal && vertical) {
            const widthDelta = Math.abs(candidate.width - start.width) / Math.max(start.width, minimumSize);
            const heightDelta = Math.abs(candidate.height - start.height) / Math.max(start.height, minimumSize);
            if (widthDelta >= heightDelta) {
                width = candidate.width;
                height = width / aspect;
            } else {
                height = candidate.height;
                width = height * aspect;
            }
        } else if (horizontal) {
            width = candidate.width;
            height = width / aspect;
        } else {
            height = candidate.height;
            width = height * aspect;
        }

        const anchorX = movesWest ? start.x + start.width : start.x;
        const anchorY = movesNorth ? start.y + start.height : start.y;
        const centerX = start.x + start.width / 2;
        const centerY = start.y + start.height / 2;
        const maxWidth = horizontal
            ? (movesWest ? anchorX : 1 - anchorX)
            : 2 * Math.min(centerX, 1 - centerX);
        const maxHeight = vertical
            ? (movesNorth ? anchorY : 1 - anchorY)
            : 2 * Math.min(centerY, 1 - centerY);
        width = Math.max(minimumSize, width);
        height = Math.max(minimumSize, height);
        const scale = Math.min(1, maxWidth / width, maxHeight / height);
        width *= scale;
        height *= scale;

        return {
            x: horizontal ? (movesWest ? anchorX - width : anchorX) : centerX - width / 2,
            y: vertical ? (movesNorth ? anchorY - height : anchorY) : centerY - height / 2,
            width,
            height,
        };
    }

    function thumbnailPixelsAreUsable(pixels, minimumChannel = 4) {
        if (!pixels || pixels.length < 4) return false;
        for (let index = 0; index + 3 < pixels.length; index += 4) {
            if (pixels[index + 3] > 0 && Math.max(
                pixels[index],
                pixels[index + 1],
                pixels[index + 2]
            ) > minimumChannel) {
                return true;
            }
        }
        return false;
    }

    function updateMatchingThumbnails(elements, id, update) {
        if (!id || typeof update !== 'function') return 0;
        let count = 0;
        for (const element of Array.from(elements || [])) {
            if (element?.dataset?.imgId !== id) continue;
            update(element);
            count += 1;
        }
        return count;
    }

    function getQuarterTurnAction(action) {
        if (action === 'left') return { clockwise: false, turnDelta: -1 };
        if (action === 'right') return { clockwise: true, turnDelta: 1 };
        return null;
    }

    function snapRotationDegrees(value, threshold = 2) {
        const degrees = Math.max(-180, Math.min(180, finite(value)));
        const snapDistance = Math.max(0, finite(threshold));
        const nearestQuarterTurn = Math.round(degrees / 90) * 90;
        return Math.abs(degrees - nearestQuarterTurn) <= snapDistance
            ? nearestQuarterTurn
            : degrees;
    }

    function decomposeRotationDegrees(value, threshold = 2) {
        const degrees = snapRotationDegrees(value, threshold);
        const quarterTurns = Math.round(degrees / 90);
        return {
            degrees,
            quarterTurns,
            angle: degrees - quarterTurns * 90,
        };
    }

    function composeRotationDegrees(geom) {
        let degrees = finite(geom?.angle) + Math.trunc(finite(geom?.rotate_90_count)) * 90;
        while (degrees > 180) degrees -= 360;
        while (degrees <= -180) degrees += 360;
        return Math.abs(degrees) < 1e-9 ? 0 : degrees;
    }

    return {
        normalizeWheelDelta,
        horizontalWheelDelta,
        canScrollBy,
        getZoomViewUpdate,
        getCanvasCompositeTransform,
        formatZoomPercent,
        getPreviewProxyTarget,
        selectFilmAreaBatchTargets,
        getNormalizedCropAspect,
        fitCropRectToAspect,
        constrainCropResize,
        thumbnailPixelsAreUsable,
        updateMatchingThumbnails,
        getQuarterTurnAction,
        snapRotationDegrees,
        decomposeRotationDegrees,
        composeRotationDegrees,
    };
}));
