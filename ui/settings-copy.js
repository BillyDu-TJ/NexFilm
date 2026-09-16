(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmSettingsCopy = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    const SETTING_FIELDS = {
        filmMode: { params: ['film_mode'] },
        filmBase: { extra: ['base_density', 'base_color', 'base_source', 'invert_active'] },
        densityLimits: { params: ['d_min', 'd_max', 'd_min_offset', 'd_max_offset'] },
        printerRed: { params: ['exp_r'] },
        printerGreen: { params: ['exp_g'] },
        printerBlue: { params: ['exp_b'] },
        temperature: { params: ['temperature'] },
        tint: { params: ['tint'] },
        exposure: { params: ['exposure'] },
        gamma: { params: ['gamma'] },
        contrast: { params: ['contrast'] },
        highlights: { params: ['highlights'] },
        shadows: { params: ['shadows'] },
        saturation: { params: ['saturation'] },
        lut: { params: ['lut_path'] },
        lutOpacity: { params: ['lut_opacity'] },
        sprocketPoint: { params: ['sprocket_uv', 'sprocket_target_color'] },
        sprocketTolerance: { params: ['sprocket_tolerance'] },
        sprocketFeather: { params: ['sprocket_feather'] },
        workingSpace: { params: ['working_colorspace'] },
        crop: { geom: ['crop_rect', 'calibration_points', 'calibration_confirmed'] },
        rotateFlip: { geom: ['flip_h', 'flip_v', 'rotate_90_count'] },
        perspective: {
            geom: [
                'angle', 'perspective_vertical', 'perspective_horizontal',
                'perspective_aspect', 'lens_distortion', 'perspective_scale', 'constrain_crop'
            ]
        }
    };

    const LEGACY_GROUPS = {
        crop: ['crop'],
        transform: ['rotateFlip', 'perspective'],
        sprocket: ['sprocketPoint', 'sprocketTolerance', 'sprocketFeather'],
        edit: Object.keys(SETTING_FIELDS).filter(key => ![
            'crop', 'rotateFlip', 'perspective',
            'sprocketPoint', 'sprocketTolerance', 'sprocketFeather'
        ].includes(key))
    };

    function cloneSettingsValue(value) {
        return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
    }

    function mergeSelectedKeys(target, source, keys) {
        keys.forEach(key => {
            if (source?.[key] !== undefined) target[key] = cloneSettingsValue(source[key]);
        });
    }

    function normalizeSettings(settings) {
        const expanded = (settings || []).flatMap(setting => LEGACY_GROUPS[setting] || [setting]);
        return Array.from(new Set(expanded)).filter(setting => SETTING_FIELDS[setting]);
    }

    function createCopyPayload(params, geom, settings, extra = {}) {
        const selectedSettings = normalizeSettings(settings);
        if (selectedSettings.length === 0) throw new Error('At least one setting is required.');

        const selectedParams = {};
        const selectedGeom = {};
        const selectedExtra = {};
        selectedSettings.forEach(setting => {
            const fields = SETTING_FIELDS[setting];
            mergeSelectedKeys(selectedParams, params, fields.params || []);
            mergeSelectedKeys(selectedGeom, geom, fields.geom || []);
            mergeSelectedKeys(selectedExtra, extra, fields.extra || []);
        });

        const result = {
            settings: selectedSettings,
            params: selectedParams,
            geom: selectedGeom
        };
        if (Object.keys(selectedExtra).length > 0) {
            result.extra = selectedExtra;
        }
        return result;
    }

    function mergeCopyPayload(params, geom, payload, extra = {}) {
        const nextParams = cloneSettingsValue(params);
        const nextGeom = cloneSettingsValue(geom);
        const nextExtra = cloneSettingsValue(extra) || {};
        const selectedSettings = normalizeSettings(payload?.settings || payload?.modules);

        selectedSettings.forEach(setting => {
            const fields = SETTING_FIELDS[setting];
            mergeSelectedKeys(nextParams, payload?.params, fields.params || []);
            mergeSelectedKeys(nextGeom, payload?.geom, fields.geom || []);
            mergeSelectedKeys(nextExtra, payload?.extra, fields.extra || []);
        });

        return { params: nextParams, geom: nextGeom, extra: nextExtra };
    }

    return { cloneSettingsValue, createCopyPayload, mergeCopyPayload, normalizeSettings };
});
