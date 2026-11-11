---
id: core-modules-walkthrough
track: architecture
trackOrder: 4
chapterOrder: 2
kicker: "Architecture 02 / Core Modules Tour"
title: "Core Codebase Modules Walkthrough & Dataflow Analysis"
subtitle: "Deep dive into Rust backend crates, streaming scanline decoders, and SQLite engineering migrations."
readingTime: 14
inApp: false
sinceVersion: "v1.1.0"
---

## Rust Native Backend Module Topology & Responsibilities {#rust-backend-module-topology}

The native codebase of NexFilm is organized under the `src/` directory, structured with high cohesion and minimal inter-module coupling. Source files maintain distinct boundaries separating IPC orchestration, file decoding, optical mathematics, and database storage:

```text
src/
├── main.rs & lib.rs             # Tauri 2 application entry and runtime setup
├── commands.rs                  # Front-end IPC command orchestrator and coordinator
├── app_state.rs                 # Primary data structures, pipeline state & contracts
├── core_math.rs                 # Mathematical foundation, nalgebra linear algebra
├── pipeline.rs                  # Negative density inversion pipeline (FilmPipeline)
├── color_science.rs             # Linear color space conversions, primaries, and ICC
├── raw_backend.rs               # LibRaw 0.22+ bindings with C++ mosaic shim
├── rawler_backend.rs            # Pure Rust fallback RAW decoding engine
├── calibration_fit.rs           # SVD channel separation fitting with condition guards
├── dng_writer.rs                # Minimal single-IFD LinearRaw & CFA DNG encoder
├── scanner_profile.rs           # Scanner profile registry and container plist parser
├── io_processor.rs              # Streaming scanline strip decoders and memory managers
├── film_border.rs & geometry.rs # Automatic film gate detection and projective quad logic
├── batch_settings.rs            # Whole-roll parameter replication and merging
└── persistence.rs               # SQLite project database persistence and migrations
```

During application bootstrap, `lib.rs` registers all Tauri command handlers and binds the shared `AppState` container. Acting as the top-level orchestrator, `commands.rs` coordinates interactions across `raw_backend`, `pipeline`, and `persistence`. The `app_state.rs` module defines the data model schemas—including `PipelineState`, `Roll`, `BaseColor`, and `ProcessingContract`—establishing the structural foundation for all cross-thread message passing. Auxiliary vector mathematics, dot products, and empirical luminance coefficients are concentrated within `core_math.rs`, using `nalgebra` for optimized linear algebra routines. This clear demarcation ensures that compute-intensive image transformations run inside dedicated background worker threads without stalling the main user interface event loop.

## Density Inversion Pipeline & Color Science Internals {#pipeline-and-color-science-internals}

Within the optical density and color conversion layer, NexFilm enforces stateless functional calling conventions, natively compatible with Rayon multi-threaded data parallelism:

1. **`src/pipeline.rs` (Core Inversion Pipeline)**:  
   The `FilmPipeline` struct encapsulates optical density mathematics. The pipeline evaluates the active contract through the `contract` enum:
   - **Legacy Path (`ProcessingContract::LegacyV1`)**: Invokes `status_m_crosstalk_matrix()` to apply the historical empirical crosstalk matrix, preserving backward compatibility for legacy projects.
   - **Modern Unified Path (`ProcessingContract::SmartAutoV11` / `CaptureCorrectedV11`)**: Sets the crosstalk matrix to the $3 \times 3$ identity matrix (`Matrix3::identity()`), operating upon measured relative transmittance and declared input domains without unverified assumptions.
2. **`src/color_science.rs` (Linear Color Conversion Core)**:  
   This module governs chromatic definitions for all supported color spaces (`ColorSpaceId`), including sRGB, Display P3, Adobe RGB (1998), ITU-R BT.2020, Linear ProPhoto RGB (ROMM RGB), ACES 2065-1, and ACEScg. Color space transformation matrices are calculated under double-precision floating point using Bradford chromatic adaptation transforms, preserving precision across pipeline stages. Furthermore, the module embeds an autonomous binary ICC profile generator for embedding compliant tags into exported 16-bit deliverables.
3. **`src/calibration_fit.rs` (Hardware Calibration Optimization)**:  
   In `Capture Characterized` workflows, this module solves regularized matrix equations via `nalgebra::DMatrix` and singular value decomposition (`SVD`). The implementation enforces three safety guards: `MIN_PATCHES = 4`, `MAX_CONDITION_NUMBER = 1.0e5`, and `MAX_VALIDATION_RMSE = 0.08`, intercepting ill-conditioned acquisitions and documenting diagnostic telemetry.

> [!NOTE]
> `FilmPipeline` is completely free of global mutable locks. During batch processing or multi-frame export, the Rayon thread pool instantiates multiple isolated pipeline workers concurrently, scaling throughput linearly across available physical CPU cores.

## Streaming Scanline Decoders & Bounded Memory Footprint {#streaming-scanline-decoders}

For high-resolution drum scans generating multi-hundred-megabyte or gigabyte-scale TIFF and Flextight `.fff` images, reading entire uncompressed rasters directly into RAM can trigger excessive memory consumption and operating system paging. To address these demands, the I/O subsystems incorporate specialized streaming decoders:

1. **Strip-Based Streaming I/O (`src/io_processor.rs`)**:  
   Interfacing with low-level TIFF strip and tile accessors, the decoder loads image rasters in narrow vertical increments (strip buffers). Rather than buffering entire image rasters, color space linearization, film base subtraction, and density window mapping execute directly within this small memory window.
2. **Zero-Copy Export Stream (`src/commands.rs`)**:  
   When exporting 16-bit Linear TIFF or JPEG files, the CPU export engine streams finished scanlines directly to the output encoder. Throughout the entire export process, physical memory residence remains constrained within a few dozen megabytes regardless of input canvas dimensions.
3. **Dual RAW Decoding Architecture (`src/raw_backend.rs` & `src/rawler_backend.rs`)**:  
   The engine defaults to the industry-standard `LibRaw 0.22+` dynamic library, paired with a custom C++ bridge (`raw_mosaic_shim.cpp`) to capture pure Bayer mosaic sensor readings prior to demosaicing. When deployed in lightweight environments without system C++ dependencies, the backend falls back smoothly to the pure Rust `Rawler` decoder.
4. **DNG Container Serialization (`src/dng_writer.rs`)**:  
   The custom DNG writer implements a single-IFD little-endian TIFF/EP encoder. Supporting both mosaic CFA buffers and demosaiced 16-bit LinearRaw arrays, the writer stores calibration illuminants, active sensor crops, and rational matrix tags compliant with ISO 12234-2.

## SQLite Schema Evolution & Seamless Recipe Migration {#sqlite-schema-and-recipe-migration}

Session persistence is maintained by `src/persistence.rs`. To accommodate internal algorithmic improvements across versions, the database architecture follows an idempotent migration contract:

```text
Application Launch -> Open nexfilm_user.db
              │
              ├─> Query user_version PRAGMA against MATH_VERSION (currently 5)
              │
              ├─> Execute migrate_retired_density_recipe()
              │     ├─> Query frames tagged with legacy_linear_srgb
              │     ├─> Preserve LegacyV1 sessions safely
              │     └─> Promote remaining frames to linear_prophoto_estimate
              │           ├─> Reset compatibility_base to unresolved
              │           ├─> Clear stale base_color and thumbnails
              │           └─> Log retired_density_recipe_migrated event
              │
              └─> Commit transaction; complete idempotent bootstrap
```

During the v1.1.0 rollout of the unified density pipeline, legacy `legacy_linear_srgb` approximations were deprecated. To prevent obsolete density parameters from affecting new sessions, `migrate_retired_density_recipe` scans the database during startup, removes invalidated derivative states, and ensures the front end triggers a clean re-inversion upon next access. The migration is idempotent and produces zero side effects on repeated runs. Database transactions wrap entire roll grading operations, guaranteeing atomic state changes across editing sessions.

## Front-End UI Architecture & Module Responsibility Matrix {#frontend-rendering-and-ui-modules}

The front-end interface avoids bloated application frameworks, organizing darkroom lifecycle events through focused modular scripts:

- **`ui/main.js` (Application Controller & Navigation)**: Manages view transitions across Library, Develop, Rolls, Hardware Calibration, and Documentation; coordinates the virtualized filmstrip, global keyboard shortcuts, and asynchronous Tauri IPC dispatch.
- **`ui/tone-curve.js` (Printer Lights & Curve Editor)**: Handles interactive editing across four-channel (Master/R/G/B) tone curves, calculating 256-step LUTs via cubic spline interpolation and synchronizing printer lights additive offsets.
- **`ui/geometry.js` (Film Gate & Projective Warping)**: Implements four-corner polygon drag handles, coordinates automated gate boundary detection, and renders perspective alignment grids.
- **`ui/contact-sheet.js` (Contact Sheet Layout Engine)**: Organizes multi-frame negative rolls into row-based contact sheet layouts, supporting darkroom paper emulation, frame borders, and metadata typographic stamping.
- **`ui/density-math.js` (Client-Side Density Utilities)**: Provides density coordinate conversions for real-time histogram waveforms, clipping warnings, and inspection readouts.

The table below outlines the core module responsibilities across the application:

| Source File Path | Language & Environment | Core Module Responsibility | System Dependencies & Consumers |
| --- | --- | --- | --- |
| `src/commands.rs` | Rust (Tauri Commands) | Async command dispatch, pipeline orchestration, master export | Invokes pipeline, persistence, raw_backend |
| `src/pipeline.rs` | Rust (Rayon parallel) | Core density math, base subtraction, bounded span response | Consumes app_state; feeds raster export |
| `src/color_science.rs` | Rust | Color conversions, ProPhoto/ACES matrices, binary ICC generation | Utilized by pipeline and dng_writer |
| `src/raw_backend.rs` | Rust + C++ Shim | LibRaw bridge, CFA Bayer extraction, dark field subtraction | Supplies raw image buffers to commands |
| `src/persistence.rs` | Rust (rusqlite) | SQLite database I/O, schema versioning, recipe migrations | Provides roll and frame data persistence |
| `ui/main.js` | Vanilla JavaScript | View routing, filmstrip virtual list, event dispatcher | Interfaces with Tauri invoke and UI modules |
| `ui/tone-curve.js` | Vanilla JS + Canvas | Four-channel spline curves, printer lights slider bindings | Syncs curve LUTs with WebGL shader and backend |
| `ui/geometry.js` | Vanilla JS + SVG | Four-point projective handles, auto-gate visualization | Synchronizes crop polygon coordinates |
| `ui/contact-sheet.js` | Vanilla JS + Canvas | Contact sheet pagination, border masking, high-res export | Pulls roll frames from SQLite to generate layouts |
