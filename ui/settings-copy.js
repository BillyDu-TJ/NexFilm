(function (root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.NexFilmSettingsCopy = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
    const EDIT_PARAM_KEYS = [
        'film_mode', 'd_min', 'd_max', 'exposure', 'gamma', 'saturation', 'temperature', 'tint',
        'exp_r', 'exp_g', 'exp_b', 'highlights', 'shadows', 'lut_path', 'lut_opacity',
        'working_colorspace'
    ];
    const SPROCKET_PARAM_KEYS = ['sprocket_uv', 'sprocket_tolerance', 'sprocket_feather'];
    const CROP_GEOM_KEYS = ['crop_rect', 'calibration_points', 'calibration_confirmed'];
    const TRANSFORM_GEOM_KEYS = [
        'angle', 'perspective_vertical', 'perspective_horizontal',
        'perspective_aspect', 'perspective_scale', 'constrain_crop',
        'flip_h', 'flip_v', 'rotate_90_count'
    ];
    const VALID_MODULES = new Set(['crop', 'edit', 'transform', 'sprocket']);

    function cloneSettingsValue(value) {
        return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
    }

    function mergeSelectedKeys(target, source, keys) {
        keys.forEach(key => {
            if (source[key] !== undefined) target[key] = cloneSettingsValue(source[key]);
        });
    }

    function normalizeModules(modules) {
        return Array.from(new Set(modules || [])).filter(module => VALID_MODULES.has(module));
    }

    function createCopyPayload(params, geom, modules) {
        const selectedModules = normalizeModules(modules);
        if (selectedModules.length === 0) throw new Error('At least one settings group is required.');
        return {
            modules: selectedModules,
            params: cloneSettingsValue(params),
            geom: cloneSettingsValue(geom)
        };
    }

    function mergeCopyPayload(params, geom, payload) {
        const nextParams = cloneSettingsValue(params);
        const nextGeom = cloneSettingsValue(geom);
        const modules = new Set(normalizeModules(payload?.modules));

        if (modules.has('edit')) mergeSelectedKeys(nextParams, payload.params, EDIT_PARAM_KEYS);
        if (modules.has('sprocket')) mergeSelectedKeys(nextParams, payload.params, SPROCKET_PARAM_KEYS);
        if (modules.has('crop')) mergeSelectedKeys(nextGeom, payload.geom, CROP_GEOM_KEYS);
        if (modules.has('transform')) mergeSelectedKeys(nextGeom, payload.geom, TRANSFORM_GEOM_KEYS);

        return { params: nextParams, geom: nextGeom };
    }

    return { cloneSettingsValue, createCopyPayload, mergeCopyPayload };
});
