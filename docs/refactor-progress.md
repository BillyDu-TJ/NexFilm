# NexFilm Refactor Progress

Last updated: 2026-07-22

## Completed In The Current Worktree

- Import-stage camera RAW handling is embedded-thumbnail-only. TIFF prefers an
  embedded preview and falls back to a background 1024px decode for scanner
  files without a thumbnail IFD; no inversion or auto color runs during import.
- Develop proxy decoding uses explicit half-size, 16-bit, gamma 1.0, no-auto-bright LibRaw settings.
- Embedded and rendered thumbnails are stored separately and rendered previews are preferred by Library, Develop, History, and contact sheets.
- Per-image tuning state includes LUT path/opacity and RAW working color space.
- Export reads persisted roll parameters, rejects missing LUTs, runs in `spawn_blocking`, emits progress, prevents concurrent export jobs, and leaves the UI usable.
- CPU export mirrors the shader order for crop UV, calibration homography, nearest RAW sampling, sprocket masking, density/tone math, gamma, and LUT application.
- Export quality controls JPEG encoding only; it no longer silently resizes every output.
- Unsupported unprofiled wide-gamut export choices were removed. Current output contract is sRGB.
- Roll identity is `roll_id + file_path` in both frontend collection logic and backend deletion/preview lookup.
- Relocating a missing source file migrates the SQLite primary-key path and only updates the owning roll.
- History keeps showing persisted previews when source files are missing and marks them as missing.
- Release data uses a per-user data directory with one-time migration from legacy working-directory files.
- The LUT asset directory is declared as a Tauri bundle resource.
- The previous external DCP control was removed after source-level verification
  showed this vendored LibRaw build defines `NO_LCMS`; its `camera_profile`
  field was therefore ignored, and Adobe DCP files were never applied.
- Unsupported legacy working spaces are normalized to `linear-srgb`. The only
  exposed RAW decode choices are now LibRaw linear sRGB and LibRaw linear ACES;
  the previous fake Rec.2020-to-sRGB mapping and ACES-AP1 label are gone.
- Roll metadata now has a SQLite `rolls` table. Legacy `rolls.json` is migrated once,
  and roll deletion/append/relocation update roll metadata and image states in one
  transaction before changing the in-memory model. `rolls.json` remains only as a
  compatibility mirror.
- Import batches commit atomically. SQLite begin/insert/commit failures emit
  `import_error`, do not add non-persisted images to memory, and remove failed
  frontend placeholders. Immediate IPC failures also release the import controls.
- Proxy resizing is downscale-only; small half-size decode output is no longer
  enlarged to the proxy long-edge target.
- Release startup now creates its per-user data directory before opening SQLite.
  A release executable smoke test succeeded with an isolated writable `APPDATA`,
  and bundled LUT files were present under `target/release/assets/luts`.
- Import never unpacks RAW data. Entering Develop now starts one coalesced proxy
  preparation job in the background; Auto Invert waits for that same job instead
  of starting a duplicate decode.
- Image switching always validates the current roll and source path in the backend;
  cached frontend parameters can no longer bypass missing-file or identity errors.
- Continue Editing changes UI state only after the archived roll is loaded successfully.
  Frames without persisted state remain visible but cannot be silently re-imported.
- Failed image-state batches are removed from the owning roll metadata, preventing
  History entries that point at images which never committed to SQLite.
- Duplicate loose imports and immediate IPC failures no longer leave permanent
  `Importing` placeholders in Library or Develop.
- Frontend edit and rendered-thumbnail writes retain pending data on persistence
  failure, report the error, and do not switch away from an unsaved image.
- Obsolete `is_proxy_ready`, `precache_progress`, `thumbnail_updated`, and
  `proxy_ready` plumbing has been removed.
- Loose imports now use the same SQLite image-state contract as roll imports.
  Their thumbnails, parameters, geometry, base color, relocation, and export
  snapshots persist across restarts while remaining excluded from History Films.
- Drag-and-drop import now rejects unsupported paths, deduplicates loose images,
  and rolls back transient placeholders when IPC startup fails.
- Proxy reload and Auto Color are separate operations. Crop, rotation, flip,
  undo, and working-space refresh no longer overwrite an image's existing
  DMin/DMax or channel-exposure edits; Auto Color runs only from Auto Invert.
- WebGL preview transforms are relative to the geometry already baked into the
  loaded proxy, preventing fine-angle rotation from being applied twice.
- Quarter-turn and flip operations keep crop bounds, calibration points, and
  sprocket samples in the same coordinate system. The pure geometry contract is
  shared by preview rendering, Auto Color sampling, and picker hit testing.
- RAW working-space changes invalidate the old proxy, base-color estimate, and
  rendered thumbnail before rebuilding them in the selected linear space.
- The tracked 19 MB scanner TIFF fixture now produces a real 1024px import
  preview even though it has no thumbnail IFD. The fallback runs only in the
  background import producer; its current timing has not been re-benchmarked.
- The same tracked 16-bit TIFF completes the Develop decode, base-color
  calculation, and shader-equivalent Rust export math path in a regression test.
- Frontend Tauri event APIs and import counters are initialized before listener
  registration. This fixes startup `listen` and roll-import `totalImportCount`
  temporal-dead-zone failures and is guarded by a startup-order check.
- Import thumbnail extraction uses a short-lived Rayon pool (up to eight
  workers) and sends each completed preview directly to the SQLite/UI consumer
  instead of sharing the smaller pool used by longer Develop/export work.
- Develop enters film-area calibration from the cached import thumbnail without
  waiting for the larger embedded preview. Stale proxy completions are ignored,
  and geometry undo renders immediately before asynchronous persistence.
- Master D-Min/D-Max change handlers no longer pass a DOM event object as the
  `update_tuning_parameters` image id; the IPC helper also guards that contract.
- Import previews use a 1024px long edge. Develop keeps an existing rendered
  positive visible while its matching proxy loads; undeveloped frames stay on
  the embedded negative until an explicit editing action needs a 16-bit proxy.
- Rendered thumbnails are no longer transformed a second time by legacy CSS or
  placeholder crop layout, keeping the Develop canvas and filmstrip framing consistent.
- Auto Invert waits for the proxy's first WebGL frame before reading its FBO and
  samples only the selected film-area coordinates, with a guarded fallback for
  invalid or stale geometry.
- During an active import, Library shows only the current batch. The batch view
  remains after completion and returns to the full library when Library is opened
  again; Develop and History filtering are unchanged.

## Current Verification

The following checks pass:

```powershell
cargo check
cargo test --lib
cargo fmt --check
node --check ui/main.js
node --check ui/geometry.js
node scripts/verify-geometry-contract.cjs
node scripts/verify-ui-startup-order.cjs
git diff --check
```

Current Rust library test count: 25.

The latest NSIS build and release startup smoke test were repeated after the DCP
and working-space cleanup. The isolated run created `nexfilm_user.db` and
`rolls.json` and remained running until the test process was intentionally stopped.

## Not Yet Proven

- Real import/develop/export timings for a 38-frame DNG roll.
- Camera coverage for DNG, NEF, ARW, RAF, CR3, and paths containing Chinese characters.
- Pixel-level WebGL-versus-export comparison captured from a real RAW file.
- A future real camera-profile implementation (DCP parsing or an LCMS-enabled
  ICC path); the previous no-op implementation is no longer exposed.
- Installer-driven first launch against the real `%APPDATA%`; direct release
  executable startup is proven with an isolated writable `APPDATA`.

The workspace currently contains TIFF files but no camera RAW fixtures, so RAW compatibility and performance must not be described as verified yet.

## Packaging Environment Blocker

The UI still loads `https://cdn.tailwindcss.com`. Vendoring the browser runtime was attempted, but the network approval service rejected the download. No partial file was created. Offline styling remains the only known packaging blocker that requires external dependency access or a separately supplied local Tailwind runtime.

`cargo tauri build --bundles nsis` succeeds and produces the NSIS installer. MSI bundling reaches WiX but fails its ICE validation with `LGHT0217/LGHT0216` because the current Windows Installer service is unavailable in this environment; this is not a project compilation or resource-path error.

## Next Work

1. Exercise the four business states in the running application with disposable test data.
2. Add a small curated RAW fixture matrix and record import/proxy/export timings.
3. Capture WebGL output and compare it against the CPU export golden fixture.
4. Design and verify a real camera-profile path before exposing profile controls again.
5. Vendor Tailwind locally, then test an installer-driven offline first launch.
