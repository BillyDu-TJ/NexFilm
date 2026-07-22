# Nexfilm Import Pipeline Refactor Plan

> This document is the original refactor baseline and preserves the reasoning and
> target contracts. Some observations under "Current Architecture" and "Open
> issues" have since been resolved. Use `docs/refactor-progress.md` as the
> authoritative current-status and verification record.

## Current Architecture

Nexfilm is a Rust + Tauri application with a single-page frontend in `ui/main.js`.

- `src/main.rs` initializes SQLite, background worker limits, `EngineState`, DB-backed image state, and `rolls.json`, then registers Tauri commands.
- `src/app_state.rs` owns the runtime model: `EngineState`, `FilmItem`, roll metadata, tuning params, geometry, proxy cache order, and thumbnail fields.
- `src/commands.rs` currently contains most orchestration: import, RAW thumbnail extraction, proxy decoding, SQLite persistence, history roll access, geometry, thumbnail sync, and export.
- `src/pipeline.rs` and `src/core_math.rs` hold part of the film math used by backend export and thumbnail generation.
- `ui/main.js` owns routing, Library/Develop/History state, WebGL shader rendering, parameter sync, thumbnail capture, import UI, and contact sheet generation.

## Current Data Flow

### Import

`import_images` now returns immediately after spawning producer/consumer threads. The producer deduplicates paths, checks a DB cache, and for new files calls LibRaw only through `libraw_unpack_thumb` + `libraw_dcraw_make_mem_thumb` to extract an embedded or bitmap thumbnail. The consumer is the SQLite writer and emits `import_progress`.

This is already close to the desired fast-import model, but there are still important leaks:

- After import flush, the backend schedules proxy decode for the first two images through `enqueue_proxy_job`.
- The frontend auto-opens the first imported image on `import_progress`, which can immediately trigger RAW proxy decoding.
- The fallback path may read a full RAW file into memory if `libraw_open_file` fails.
- Roll/history and loose import behavior are mixed in the same command, making lifecycle intent hard to enforce.

### Develop

`switch_active_image` returns params and geometry quickly, then queues a proxy job if the image has no proxy. The proxy worker calls `load_image_buffer(path, true, ...)`, which uses LibRaw with `half_size = 1`, `output_bps = 16`, gamma `(1, 1)`, then `dcraw_process` and `dcraw_make_mem_image`.

The frontend receives the proxy through `get_proxy_image_data`, uploads it to WebGL as `RGBA16UI`, and runs the inversion math in the fragment shader.

Open issues:

- The pipeline is "16-bit linear-ish", but not yet explicitly guaranteed: `no_auto_bright`, brightness, output color, camera matrix, and working colorspace policy need to be fixed and recorded.
- `load_image_buffer` currently sets default output color to sRGB for non-ACES paths even though `EngineState` defaults to `rec2020`.
- Proxy generation also computes `base_color` and `pristine_proxy`, adding extra work and memory before the user explicitly asks for auto color.
- Half-size output may be resized up to `PROXY_LONG_EDGE`, which can fabricate pixels instead of preserving decode output.

### WebGL And Math

The shader performs:

- 16-bit linear RGB normalization
- density conversion: `-log10(T)`
- base density subtraction
- Status M crosstalk matrix
- B&W branch
- exposure offsets
- Dmin/Dmax normalization
- highlights/shadows
- gamma
- LUT
- sprocket mask
- calibration homography

The backend export only implements a subset: density, base subtraction, Status M/B&W, exposure, Dmin/Dmax, gamma, geometry/crop, and basic output encoding. It does not currently match the shader for highlights/shadows, LUT, sprocket mask, or calibration homography.

This is the largest correctness risk for "what you see is what you export".

### Persistence

SQLite currently stores:

```sql
image_states(
  roll_id TEXT NOT NULL,
  file_path TEXT NOT NULL,
  thumbnail_base64 TEXT,
  params TEXT,
  geom TEXT,
  base_color TEXT,
  PRIMARY KEY (roll_id, file_path)
)
```

`rolls.json` stores roll lifecycle metadata. Loose images are intentionally skipped by `save_image_state_to_db`.

Open issues:

- `thumbnail_base64` is overloaded: sometimes embedded orange negative, sometimes processed positive preview.
- Params have no schema/math version.
- Export uses in-memory `FilmItem.params`, not a DB-read source of truth.
- History/contact sheet can miss roll thumbnails because history roll items are merged into a local render list but not consistently into `allLibraryItems`.
- Continue-edit historical import can create fallback DB rows for missing cache entries instead of either loading existing state or doing a real thumbnail import.

## Target Architecture

### Stage 1: Library Placeholder Import

Goal: import a roll or loose set without RAW unpack, demosaic, color conversion, proxy generation, or auto-base calculation.

Backend responsibilities:

- Open file metadata safely.
- Extract only embedded thumbnail via `libraw_unpack_thumb`.
- Downscale/re-encode bitmap thumbs to a small JPEG if needed.
- Persist path, roll id, embedded thumbnail, default params, geometry, and decode status.
- Emit progress events for UI replacement of placeholders.

Frontend responsibilities:

- Create all visual placeholders immediately.
- Replace placeholders as embedded thumbs arrive.
- Never auto-select into Develop as a side effect of import.

Hard rule:

- `import_images` and `import_roll` must not call, enqueue, or indirectly trigger
  RAW `libraw_unpack`, `dcraw_process`, demosaic, `compute_auto_base`, or
  `compute_pristine_proxy`. TIFF files without a thumbnail IFD may be decoded in
  the background producer only to create a 256px import preview.

### Stage 2: Develop Proxy And Auto Color

Goal: defer RAW work to the moment the user opens or actively edits one image.

Backend responsibilities:

- Provide an explicit `ensure_develop_proxy(id)` or equivalent command.
- Decode half-size 16-bit linear proxy with a documented LibRaw parameter set.
- Return proxy bytes plus metadata: dimensions, raw/color mode, max value, black/white levels if available, camera/output matrix policy, decode version.
- Cache only a bounded number of proxies in memory.

Frontend responsibilities:

- Show embedded thumbnail immediately.
- Upload proxy to WebGL as integer texture only after proxy readiness.
- Keep slider rendering local and 60fps.
- Persist params and geometry with bounded debounce and a final flush before export/navigation-sensitive actions.

Decision to make before implementation:

- Either decode proxy on first Develop open, or wait until "Auto Color" is clicked. The current UX and the user's proposed Stage 2 favor first Develop open with thumbnail masking. If decode time remains near 5 seconds, move the decode trigger behind Auto Color and keep Develop as a crop-on-thumbnail staging view.

### Stage 3: Background Export

Goal: full-size output that matches WebGL visually.

Backend responsibilities:

- Run export as a background job with progress events instead of a blocking IPC call.
- Read params/geometry/base color from SQLite at job start for roll images.
- Use in-memory state only for loose images, unless loose-session persistence is added.
- Decode full-size RAW at highest precision.
- Apply the same math as WebGL, including tone, LUT, calibration homography, and sprocket masking.
- Write TIFF/JPEG while UI remains responsive.

Hard rule:

- WebGL and Rust must share a single math contract. Any shader change must have a Rust parity test or generated golden comparison.

## Business State Machines

### 1. Import By Roll

- Create roll metadata.
- Create image rows with embedded negative thumbs.
- Show the imported roll in the current working set.
- History card can appear immediately, but inner roll thumbnails should prefer processed positive previews when available.
- Editing updates SQLite and processed thumbnails.
- Contact sheet uses processed positive thumbnails, not embedded orange negatives.

### 2. Loose Import

- Create session-only items.
- Show in Library and Develop filmstrip.
- Do not add to History Films.
- Export uses current in-memory params.
- Optional later improvement: session recovery table separate from roll archive.

### 3. Continue Editing

- Load roll metadata and DB image states.
- Add those images to the working set without re-importing or mutating missing DB rows.
- Decode proxies lazily only when opened in Develop.
- Save edits back to SQLite.

### 4. Browse History Only

- Never trigger RAW decode.
- Never import missing physical files.
- Display roll cards and inner-roll thumbnails from SQLite.
- Export contact sheet from processed thumbnails; show a clear placeholder if a frame has never been processed.

## Concrete Work Plan

### Phase 0: Instrument And Freeze Contracts

- Add timing logs around import thumbnail extraction, proxy decode, GL upload, auto color, thumbnail sync, and export.
- Add a debug assertion/log if import calls any full RAW decode path.
- Define `math_version` and `raw_decode_version` constants.
- Add a small parity test fixture for Rust math vs shader-equivalent JS/math.

### Phase 1: Persistence Cleanup

Migrate `image_states` toward explicit fields:

- `embedded_thumb_base64`
- `rendered_thumb_base64`
- `thumb_kind` or `rendered_thumb_updated_at`
- `params`
- `geom`
- `base_color`
- `math_version`
- `raw_decode_version`
- `updated_at`

Keep backward compatibility by reading old `thumbnail_base64` as:

- processed thumbnail if params/base_color are non-default and it was saved after editing;
- otherwise embedded thumbnail.

The first implementation can be conservative: add columns, preserve `thumbnail_base64`, then update reads to prefer `rendered_thumb_base64`.

### Phase 2: Pure Fast Import

- Split thumbnail extraction into a small helper module or function, e.g. `extract_embedded_thumb_base64`.
- Remove proxy scheduling from import flush.
- Remove first-image auto-open from `import_progress`.
- Replace full-file fallback with a safer path strategy, ideally Windows wide-path `libraw_open_wfile`; only use `open_buffer` when the file is small enough or explicitly required.
- Fix `is_historical` behavior so Continue Editing never creates fake fallback DB rows.

### Phase 3: Develop Proxy Boundary

- Rename `load_image_buffer` into explicit decode modes:
  - `decode_develop_proxy_half_linear`
  - `decode_export_full_linear`
  - `load_tiff_linear`
- Set LibRaw parameters explicitly:
  - `half_size = 1` for develop only
  - `output_bps = 16`
  - `gamm = [1.0, 1.0]`
  - `no_auto_bright = 1`
  - `bright = 1.0`
  - fixed highlight policy
  - documented `output_color` policy
- Do not upscale proxies after decode.
- Return metadata in the proxy response header instead of only width/base density/full flag.

### Phase 4: Thumbnail Synchronization

- Separate embedded and rendered thumbnails in frontend state.
- After any successful render/auto-color/param change, update the visible thumbnail immediately from canvas capture.
- Persist rendered thumbnail to SQLite after debounce.
- For History Films and contact sheets, prefer rendered thumbnail; fall back to embedded thumbnail only for unedited frames with a clear internal state.

### Phase 5: Export Parity

- Extract the shared CPU processing path into a function that mirrors the shader.
- Add missing backend support for highlights/shadows first.
- Add calibration homography and crop/rotate parity.
- Decide whether LUT and sprocket mask are export-critical for v1 of this refactor; if they remain UI-only temporarily, block or warn before export.
- Change export to a background job with progress events.
- Ensure export starts by flushing any pending frontend params to SQLite.

### Phase 6: Verification

- Benchmark 38 DNG import: target is UI placeholders under 100 ms and no full RAW decode during import.
- Benchmark first Develop open: proxy decode time, memory use, GL upload time.
- Benchmark export: UI remains interactive while export runs.
- Add tests:
  - import path never calls full decode
  - DB migration preserves existing rows
  - roll/history thumbnails prefer rendered previews
  - Rust math matches shader for known pixels
  - export reads persisted roll params

## Immediate Next Engineering Steps

1. Remove import-time proxy prefetch and first-image auto-open.
2. Add explicit embedded-vs-rendered thumbnail fields while preserving old DB data.
3. Make develop proxy decode an explicit boundary with hard LibRaw linear-output settings.
4. Fix Continue Editing and History browsing so they never mutate archive rows unintentionally.
5. Bring backend export math to parity with the WebGL shader, starting with highlights/shadows and calibration.

## Main Risks

- LibRaw output may not be the "absolute physical linear" data we think it is unless parameters and color policy are frozen and tested.
- WebGL/Rust parity will drift unless math is centralized or golden-tested.
- `thumbnail_base64` overloading can keep causing History/contact-sheet bugs until split.
- Frequent SQLite writes from slider movement can race with export unless export forces a final params flush.
- Windows path handling can silently fall back to full-file reads, destroying import performance on non-ASCII paths.
