(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmGeometry = api;
}(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    function numberOrZero(value) {
        const number = Number(value);
        return Number.isFinite(number) ? number : 0;
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
        getPreviewTransform,
        proxyPixelTransformChanged,
        createTransformMatrix,
        invertDisplayPoint,
        transformGeometryForQuarterTurn,
        transformGeometryForFlip,
    };
}));
