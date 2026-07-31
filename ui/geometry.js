(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmGeometry = api;
}(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    function numberOrZero(value) {
        const number = Number(value);
        return Number.isFinite(number) ? number : 0;
    }

    function normalizedQuarterTurns(value) {
        const turns = Math.trunc(numberOrZero(value));
        return ((turns % 4) + 4) % 4;
    }

    function getRotationLayout(width, height, angleDegrees) {
        const sourceWidth = Math.max(1, numberOrZero(width));
        const sourceHeight = Math.max(1, numberOrZero(height));
        const angle = Math.abs(angleDegrees) > 0.01 ? angleDegrees * Math.PI / 180 : 0;
        if (angle === 0) {
            return {
                angle,
                width: sourceWidth,
                height: sourceHeight,
                diagonal: 0,
                sourceOffsetX: 0,
                sourceOffsetY: 0,
                cropOffsetX: 0,
                cropOffsetY: 0,
            };
        }

        const sine = Math.sin(angle);
        const cosine = Math.cos(angle);
        const rotatedWidth = Math.ceil(sourceWidth * Math.abs(cosine) + sourceHeight * Math.abs(sine));
        const rotatedHeight = Math.ceil(sourceWidth * Math.abs(sine) + sourceHeight * Math.abs(cosine));
        const diagonal = Math.ceil(Math.hypot(sourceWidth, sourceHeight));
        return {
            angle,
            width: rotatedWidth,
            height: rotatedHeight,
            diagonal,
            sourceOffsetX: Math.trunc((diagonal - sourceWidth) / 2),
            sourceOffsetY: Math.trunc((diagonal - sourceHeight) / 2),
            cropOffsetX: Math.trunc((diagonal - rotatedWidth) / 2),
            cropOffsetY: Math.trunc((diagonal - rotatedHeight) / 2),
        };
    }

    function getOrientedDimensions(width, height, geom) {
        const layout = getRotationLayout(width, height, numberOrZero(geom && geom.angle));
        const turns = normalizedQuarterTurns(geom && geom.rotate_90_count);
        return turns % 2 === 0
            ? { width: layout.width, height: layout.height }
            : { width: layout.height, height: layout.width };
    }

    // Geometry is applied to pixels before crop in the Rust export pipeline.
    // This inverse maps a point in that oriented image back to canonical RAW UV.
    function mapOrientedPointToSource(point, width, height, geom) {
        const sourceWidth = Math.max(1, numberOrZero(width));
        const sourceHeight = Math.max(1, numberOrZero(height));
        const state = geom || {};
        const layout = getRotationLayout(sourceWidth, sourceHeight, numberOrZero(state.angle));
        const turns = normalizedQuarterTurns(state.rotate_90_count);
        const oriented = getOrientedDimensions(sourceWidth, sourceHeight, state);

        let x = numberOrZero(point[0]) * oriented.width;
        let y = numberOrZero(point[1]) * oriented.height;

        if (state.flip_h) x = oriented.width - x;
        if (state.flip_v) y = oriented.height - y;

        let rotatedX;
        let rotatedY;
        if (turns === 1) {
            rotatedX = y;
            rotatedY = layout.height - x;
        } else if (turns === 2) {
            rotatedX = layout.width - x;
            rotatedY = layout.height - y;
        } else if (turns === 3) {
            rotatedX = layout.width - y;
            rotatedY = x;
        } else {
            rotatedX = x;
            rotatedY = y;
        }

        if (layout.angle === 0) {
            return [rotatedX / sourceWidth, rotatedY / sourceHeight];
        }

        const expandedX = rotatedX + layout.cropOffsetX;
        const expandedY = rotatedY + layout.cropOffsetY;
        const dx = expandedX - layout.diagonal / 2;
        const dy = expandedY - layout.diagonal / 2;
        const sine = Math.sin(layout.angle);
        const cosine = Math.cos(layout.angle);
        const sourceX = cosine * dx + sine * dy + layout.diagonal / 2 - layout.sourceOffsetX;
        const sourceY = -sine * dx + cosine * dy + layout.diagonal / 2 - layout.sourceOffsetY;
        return [sourceX / sourceWidth, sourceY / sourceHeight];
    }

    function createInverseGeometryMatrix(width, height, geom) {
        const origin = mapOrientedPointToSource([0, 0], width, height, geom);
        const axisX = mapOrientedPointToSource([1, 0], width, height, geom);
        const axisY = mapOrientedPointToSource([0, 1], width, height, geom);
        return new Float32Array([
            axisX[0] - origin[0], axisX[1] - origin[1], 0,
            axisY[0] - origin[0], axisY[1] - origin[1], 0,
            origin[0], origin[1], 1,
        ]);
    }

    function getPreviewTransform(currentGeom, loadedGeom, editing) {
        if (!editing || !currentGeom) {
            return { angleDegrees: 0, angleRadians: 0, scaleX: 1, scaleY: 1 };
        }

        const loaded = loadedGeom || {};
        const angleDegrees = numberOrZero(currentGeom.angle) - numberOrZero(loaded.angle)
            + (numberOrZero(currentGeom.rotate_90_count) - numberOrZero(loaded.rotate_90_count)) * 90;

        return {
            angleDegrees,
            angleRadians: angleDegrees * Math.PI / 180,
            scaleX: !!currentGeom.flip_h !== !!loaded.flip_h ? -1 : 1,
            scaleY: !!currentGeom.flip_v !== !!loaded.flip_v ? -1 : 1,
        };
    }

    function proxyPixelTransformChanged(currentGeom, loadedGeom) {
        if (!currentGeom || !loadedGeom) return true;
        const angleChanged = Math.abs(numberOrZero(currentGeom.angle) - numberOrZero(loadedGeom.angle)) > 1e-4;
        const quarterTurnsChanged = (
            numberOrZero(currentGeom.rotate_90_count) - numberOrZero(loadedGeom.rotate_90_count)
        ) % 4 !== 0;
        return angleChanged
            || quarterTurnsChanged
            || !!currentGeom.flip_h !== !!loadedGeom.flip_h
            || !!currentGeom.flip_v !== !!loadedGeom.flip_v;
    }

    function createTransformMatrix(transform) {
        const sine = Math.sin(transform.angleRadians);
        const cosine = Math.cos(transform.angleRadians);
        return new Float32Array([
            cosine * transform.scaleX, sine * transform.scaleX, 0, 0,
            -sine * transform.scaleY, cosine * transform.scaleY, 0, 0,
            0, 0, 1, 0,
            0, 0, 0, 1,
        ]);
    }

    function invertDisplayPoint(x, y, transform) {
        const sine = Math.sin(-transform.angleRadians);
        const cosine = Math.cos(-transform.angleRadians);
        return {
            x: (x * cosine - y * sine) * transform.scaleX,
            y: (x * sine + y * cosine) * transform.scaleY,
        };
    }

    function transformPointForQuarterTurn(point, clockwise, flipH, flipV) {
        let x = numberOrZero(point[0]);
        let y = numberOrZero(point[1]);
        if (flipV) y = 1 - y;
        if (flipH) x = 1 - x;
        if (clockwise) {
            const oldX = x;
            x = 1 - y;
            y = oldX;
        } else {
            const oldX = x;
            x = y;
            y = 1 - oldX;
        }
        if (flipH) x = 1 - x;
        if (flipV) y = 1 - y;
        return [x, y];
    }

    function transformPointForFlip(point, flipH, flipV) {
        const x = numberOrZero(point[0]);
        const y = numberOrZero(point[1]);
        return [flipH ? 1 - x : x, flipV ? 1 - y : y];
    }

    function transformRect(rect, transformPoint) {
        const corners = [
            [rect.x, rect.y],
            [rect.x + rect.width, rect.y],
            [rect.x + rect.width, rect.y + rect.height],
            [rect.x, rect.y + rect.height],
        ].map(transformPoint);
        const xs = corners.map(point => point[0]);
        const ys = corners.map(point => point[1]);
        const x = Math.min(...xs);
        const y = Math.min(...ys);
        return {
            x,
            y,
            width: Math.max(...xs) - x,
            height: Math.max(...ys) - y,
        };
    }

    function reorderCalibrationPoints(points, transformPoint) {
        if (!Array.isArray(points) || points.length !== 4) return points;
        const canonical = [[0, 0], [1, 0], [1, 1], [0, 1]];
        const reordered = new Array(4);
        for (let index = 0; index < 4; index++) {
            const destination = transformPoint(canonical[index]);
            const destinationIndex = canonical.findIndex(point =>
                Math.abs(point[0] - destination[0]) < 1e-6
                && Math.abs(point[1] - destination[1]) < 1e-6
            );
            reordered[destinationIndex] = transformPoint(points[index]);
        }
        return reordered;
    }

    const calibrationEdgeIndices = Object.freeze([
        Object.freeze([0, 1]),
        Object.freeze([1, 2]),
        Object.freeze([2, 3]),
        Object.freeze([3, 0]),
    ]);

    function isValidCalibrationQuad(points) {
        if (!Array.isArray(points) || points.length !== 4) return false;
        if (points.some(point =>
            !Array.isArray(point)
            || !Number.isFinite(Number(point[0]))
            || !Number.isFinite(Number(point[1]))
            || Number(point[0]) < 0
            || Number(point[0]) > 1
            || Number(point[1]) < 0
            || Number(point[1]) > 1
        )) return false;

        let signedArea = 0;
        let orientation = 0;
        for (let index = 0; index < 4; index++) {
            const current = points[index];
            const next = points[(index + 1) % 4];
            const afterNext = points[(index + 2) % 4];
            signedArea += current[0] * next[1] - next[0] * current[1];
            const cross = (next[0] - current[0]) * (afterNext[1] - next[1])
                - (next[1] - current[1]) * (afterNext[0] - next[0]);
            if (Math.abs(cross) < 0.0001) return false;
            const sign = Math.sign(cross);
            if (orientation && sign !== orientation) return false;
            orientation = sign;
        }
        return Math.abs(signedArea) > 0.004;
    }

    function translateCalibrationEdge(points, edgeIndex, pointerDelta, viewport) {
        const indices = calibrationEdgeIndices[edgeIndex];
        if (!indices || !isValidCalibrationQuad(points)) return null;

        const viewportWidth = Math.max(1, numberOrZero(viewport && viewport[0]));
        const viewportHeight = Math.max(1, numberOrZero(viewport && viewport[1]));
        const deltaClientX = numberOrZero(pointerDelta && pointerDelta[0]);
        const deltaClientY = numberOrZero(pointerDelta && pointerDelta[1]);
        const candidate = points.map(point => [Number(point[0]), Number(point[1])]);
        const [startIndex, endIndex] = indices;
        const start = points[startIndex];
        const end = points[endIndex];
        const edgeX = (end[0] - start[0]) * viewportWidth;
        const edgeY = (end[1] - start[1]) * viewportHeight;
        const edgeLength = Math.hypot(edgeX, edgeY);
        if (edgeLength < 1) return null;

        const normalX = -edgeY / edgeLength;
        const normalY = edgeX / edgeLength;
        const projectedDistance = deltaClientX * normalX + deltaClientY * normalY;
        let deltaX = normalX * projectedDistance / viewportWidth;
        let deltaY = normalY * projectedDistance / viewportHeight;

        const minX = Math.min(start[0], end[0]);
        const maxX = Math.max(start[0], end[0]);
        const minY = Math.min(start[1], end[1]);
        const maxY = Math.max(start[1], end[1]);
        deltaX = Math.max(-minX, Math.min(1 - maxX, deltaX));
        deltaY = Math.max(-minY, Math.min(1 - maxY, deltaY));
        candidate[startIndex] = [start[0] + deltaX, start[1] + deltaY];
        candidate[endIndex] = [end[0] + deltaX, end[1] + deltaY];
        return isValidCalibrationQuad(candidate) ? candidate : null;
    }

    function transformGeometryForQuarterTurn(geom, clockwise) {
        const flipH = !!geom.flip_h;
        const flipV = !!geom.flip_v;
        const transformPoint = point => transformPointForQuarterTurn(point, clockwise, flipH, flipV);
        return {
            cropRect: transformRect(geom.crop_rect, transformPoint),
            calibrationPoints: reorderCalibrationPoints(geom.calibration_points, transformPoint),
            transformPoint,
        };
    }

    function transformGeometryForFlip(geom, flipH, flipV) {
        const transformPoint = point => transformPointForFlip(point, flipH, flipV);
        return {
            cropRect: transformRect(geom.crop_rect, transformPoint),
            calibrationPoints: reorderCalibrationPoints(geom.calibration_points, transformPoint),
            transformPoint,
        };
    }

    return {
        getOrientedDimensions,
        mapOrientedPointToSource,
        createInverseGeometryMatrix,
        getPreviewTransform,
        proxyPixelTransformChanged,
        createTransformMatrix,
        invertDisplayPoint,
        calibrationEdgeIndices,
        isValidCalibrationQuad,
        translateCalibrationEdge,
        transformGeometryForQuarterTurn,
        transformGeometryForFlip,
    };
}));
