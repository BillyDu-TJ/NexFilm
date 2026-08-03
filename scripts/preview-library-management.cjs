const fs = require('node:fs');
const http = require('node:http');
const path = require('node:path');

const root = path.join(__dirname, '..', 'ui');
const port = Number(process.env.NEXFILM_PREVIEW_PORT || 4177);
const qrBase64 = fs.readFileSync(path.join(root, 'assets', 'author-sponsor-qr.jpg')).toString('base64');

const mock = `
<script>
(() => {
    const thumb = (label, color) => 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(
        '<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 0 800 600">' +
        '<rect width="800" height="600" fill="' + color + '"/>' +
        '<path d="M0 430L190 250l130 125 150-190 330 300v115H0z" fill="rgba(255,255,255,.24)"/>' +
        '<circle cx="635" cy="135" r="62" fill="rgba(255,255,255,.58)"/>' +
        '<text x="40" y="555" fill="white" font-family="Segoe UI,sans-serif" font-size="34">' + label + '</text>' +
        '</svg>'
    );
    const params = {
        film_mode: 'Color', d_min: [0.1, 0.1, 0.1], d_max: [2, 2, 2],
        exposure: 0, gamma: 1, exp_r: 0, exp_g: 0, exp_b: 0,
        highlights: 0, shadows: 0, lut_path: null, lut_opacity: 1,
        working_colorspace: 'linear-srgb', sprocket_uv: null,
        sprocket_tolerance: 0.1, sprocket_feather: 0.05
    };
    const geom = {
        crop_rect: { x: 0, y: 0, width: 1, height: 1 }, angle: 0,
        flip_h: false, flip_v: false, rotate_90_count: 0,
        calibration_points: null, calibration_confirmed: false
    };
    let rolls = [
        { roll_id: 'roll-a', date: '2026-07-12', format: '135', film_stock: 'Kodak Gold 200', camera: 'Nikon F3', image_paths: ['mock/a-1.dng', 'mock/a-2.dng'] },
        { roll_id: 'loose-b', date: '2026-07-29', format: 'Loose', film_stock: 'Loose Import', camera: '', image_paths: ['mock/b-1.tif', 'mock/b-2.tif'] }
    ];
    let items = [
        { id: 'a-1', roll_id: 'roll-a', file_path: 'mock/a-1.dng', thumbnail_base64: thumb('Frame 01', '#50666a'), embedded_thumbnail_base64: thumb('Frame 01', '#50666a'), rendered_thumbnail_base64: null, thumbnail_kind: 'embedded', state_available: true, file_missing: false },
        { id: 'a-2', roll_id: 'roll-a', file_path: 'mock/a-2.dng', thumbnail_base64: thumb('Frame 02', '#9a6d4f'), embedded_thumbnail_base64: thumb('Frame 02', '#9a6d4f'), rendered_thumbnail_base64: null, thumbnail_kind: 'embedded', state_available: true, file_missing: false },
        { id: 'b-1', roll_id: 'loose-b', file_path: 'mock/b-1.tif', thumbnail_base64: thumb('Loose 01', '#54647e'), embedded_thumbnail_base64: thumb('Loose 01', '#54647e'), rendered_thumbnail_base64: null, thumbnail_kind: 'embedded', state_available: true, file_missing: false },
        { id: 'b-2', roll_id: 'loose-b', file_path: 'mock/b-2.tif', thumbnail_base64: thumb('Loose 02', '#6e7651'), embedded_thumbnail_base64: thumb('Loose 02', '#6e7651'), rendered_thumbnail_base64: null, thumbnail_kind: 'embedded', state_available: true, file_missing: false }
    ];
    let libraryRollId = null;
    const clone = value => structuredClone(value);
    const invoke = async (command, args = {}) => {
        if (command === 'get_rolls') return clone(rolls);
        if (command === 'get_filmstrip') return clone(items.filter(item => item.roll_id === libraryRollId));
        if (command === 'get_roll_filmstrip') return clone(items.filter(item => item.roll_id === args.rollId));
        if (command === 'get_roll_previews') return ['${qrBase64}'];
        if (command === 'get_user_cameras') return ['Nikon F3', 'Contax RTS II'];
        if (command === 'get_user_films') return ['Kodak Gold 200', 'Fujifilm C400'];
        if (command === 'get_builtin_luts') return [];
        if (command === 'promote_roll') { libraryRollId = args.rollId; return null; }
        if (command === 'delete_rolls') {
            const ids = new Set(args.rollIds || []);
            const removed = items.filter(item => ids.has(item.roll_id));
            rolls = rolls.filter(roll => !ids.has(roll.roll_id));
            items = items.filter(item => !ids.has(item.roll_id));
            if (ids.has(libraryRollId)) libraryRollId = null;
            return { removed_rolls: ids.size, removed_records: removed.length, deleted_source_files: args.deleteSourceFiles ? removed.length : 0, missing_source_files: 0, protected_source_files: 0, failed_source_files: [] };
        }
        if (command === 'update_roll_metadata') {
            const roll = rolls.find(candidate => candidate.roll_id === args.rollId);
            Object.assign(roll, { date: args.date, format: args.format, film_stock: args.filmStock, camera: args.camera });
            return clone(roll);
        }
        if (command === 'switch_active_image') return { params: clone(params), geom: clone(geom), base_analyzed: false };
        if (command === 'get_embedded_preview') return items.find(item => item.id === args.id)?.thumbnail_base64 || '';
        if (command === 'prepare_proxy') return true;
        if (command === 'get_proxy_image_data') throw 'PROXY_NOT_READY';
        if (command === 'auto_detect_film_border') return { confidence: 0, used_fallback: true, points: null };
        if (command === 'open_file_dialog') return [];
        return null;
    };
    window.__TAURI__ = {
        core: { invoke },
        event: { listen: async () => () => {} }
    };
})();
</script>`;

const mime = {
    '.css': 'text/css; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.jpg': 'image/jpeg',
    '.html': 'text/html; charset=utf-8',
};

http.createServer((request, response) => {
    const pathname = new URL(request.url, `http://127.0.0.1:${port}`).pathname;
    if (pathname === '/' || pathname === '/index.html') {
        const html = fs.readFileSync(path.join(root, 'index.html'), 'utf8')
            .replace('<script src="geometry.js"></script>', `${mock}\n    <script src="geometry.js"></script>`);
        response.writeHead(200, { 'Content-Type': mime['.html'] });
        response.end(html);
        return;
    }
    const relative = decodeURIComponent(pathname).replace(/^\/+/, '');
    const file = path.resolve(root, relative);
    if (!file.startsWith(`${path.resolve(root)}${path.sep}`) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
        response.writeHead(404);
        response.end('Not found');
        return;
    }
    response.writeHead(200, { 'Content-Type': mime[path.extname(file).toLowerCase()] || 'application/octet-stream' });
    fs.createReadStream(file).pipe(response);
}).listen(port, '127.0.0.1', () => {
    console.log(`NexFilm management preview: http://127.0.0.1:${port}`);
});
