---
id: system-architecture
track: architecture
trackOrder: 4
chapterOrder: 1
kicker: "Architecture 01 / System Overview"
title: "System Architecture & Front-Back Rendering Equivalence Contract"
subtitle: "Tauri 2 runtime, asynchronous two-phase pipeline, and WebGL2 shader vs Rust CPU export WYSIWYG contract."
readingTime: 12
inApp: false
sinceVersion: "v1.1.0"
---

## Process Model & Technical Architecture Topology {#process-model-and-tech-stack}

NexFilm is engineered as a modern hybrid desktop application, balancing high-throughput raw image processing with a responsive darkroom user interface. The system combines an unmanaged Rust native core with a WebGL2-accelerated web view, integrated across cross-platform environments using the Tauri 2 framework.

The architecture is supported by four core pillars:

1. **Tauri 2 Host & Process Model**: The main process executes within a native Rust environment, managing window lifecycles, operating system menus, sandboxed file system access, and asynchronous worker orchestration. The presentation layer runs inside the native operating system web view (WebKit on macOS, WebView2 on Windows, and WebKitGTK 4.1 on Linux). Communication between the front-end interface and native backend occurs strictly across a type-safe asynchronous IPC boundary.
2. **Rust Multithreaded Processing Core**: High-throughput tasks—such as demosaicing raw files, projective geometric transformations, relative transmittance normalization, and master file encoding—are handled by compiled Rust routines. Leveraging the Rayon data-parallelism framework, computation saturates multi-core CPU architectures and SIMD vector registers without runtime garbage collection overhead.
3. **Vanilla HTML5 / CSS / WebGL2 UI**: The frontend avoids monolithic single-page frameworks and relies on plain JavaScript, Tailwind CSS, and WebGL2 shaders. Printer-light changes, tone curves, and film-gate handles are drawn on the GPU canvas with little perceptible delay.
4. **SQLite Local Persistence & State Engine**: Roll indexes, crop coordinates, printer-light offsets, base-density measurements, and calibration profiles live in a local SQLite database (`nexfilm_user.db`). Edits stay structured and non-destructive.

| Architecture Layer | Technology & Runtime | Primary Responsibilities | Performance & Safety Guarantees |
| --- | --- | --- | --- |
| Host Runtime | Tauri 2 (Rust) | Native window management, sandboxed IPC routing, file drag-and-drop | Minimal memory footprint; zero bundled Chromium dependencies |
| Native Core | Rust 2021 + Rayon | LibRaw / Rawler decoding, streaming TIFF I/O, mathematical pipeline | Data parallelism across CPU cores; thread safety; zero segmentation faults |
| Hardware Shaders | WebGL2 (GLSL ES 3.00) | Fragment shaders, logarithmic density inversion, tone curve evaluation | GPU-accelerated 60fps real-time viewport; smooth UI updates |
| Local Database | SQLite 3 (rusqlite) | Library roll metadata, film crop polygons, grading recipes, undo history | ACID transaction safety; idempotent schema migrations; local data privacy |

## Two-Phase Asynchronous Pipeline: Rapid Ingestion & On-Demand Proxies {#two-phase-asynchronous-pipeline}

Film digitization workflows involve large high-resolution RAW files or multi-hundred-megabyte scanner TIFFs. Decoding and inverting an entire roll at full resolution during import would use too much memory and make the interface unresponsive.

To keep roll loading responsive, NexFilm uses a **Two-Phase Asynchronous Pipeline**:

```text
Phase 1: Library Ingestion (Near-Instantaneous Indexing)
User drops roll folder -> Scan directory contents
      │
      ├─> Extract embedded camera RAW JPEG preview (milliseconds per frame)
      ├─> Stream first-strip IFD thumbnail from TIFF / FFF files
      └─> Populate SQLite roll index; render filmstrip immediately

Phase 2: Develop Workbench (On-Demand Decoding)
User selects a negative frame to enter the Develop view
      │
      ├─> Background thread pool executes full RAW demosaic (LibRaw / Rawler)
      ├─> Linearize into 16-bit linear positive proxy (bounded LRU cache)
      ├─> Upload proxy to WebGL2 texture unit
      └─> Engage real-time 60fps GPU fragment shader grading
```

During ingestion, the pipeline reads solely the embedded preview thumbnails stored in the raw container headers. The system indexes dozens of film frames in seconds, populating the interactive filmstrip while keeping memory utilization minimal.

Full-resolution demosaicing happens only when a frame is opened in the `Develop` workspace. The backend draws worker threads from the Rayon pool, decodes the sensor samples, generates a 16-bit linear proxy, and stores it in a bounded LRU cache. Heavy work stays close to the frame you are using, which greatly reduces the chance of running out of memory.

## What-You-See-Is-What-You-Get: The WebGL2 and CPU Equivalence Contract {#wysiwyg-rendering-parity-contract}

A critical vulnerability in digital darkroom software is perceptual divergence between interactive screen previews and exported high-resolution files. This discrepancy arises when front-end viewports utilize simplified shader approximations while backend export routines employ distinct CPU math, causing subtle shifts across highlight rolloffs, tonal curves, and color gamuts.

NexFilm enforces an architectural **WYSIWYG Mathematical Equivalence Contract**:

1. **Shared Source Formulation**: When rendering master deliverables, the Rust backend invokes the foundational routine `render_shader_equivalent_core` in `commands.rs`. The front-end WebGL2 fragment shader transcribes this exact formula into GLSL code line by line.
2. **Unified Axioms & Numeric Precision**:
   - **Transmittance to Density Mapping**: Both pathways apply the identical numerical threshold $\epsilon = 1.0 \times 10^{-6}$ in evaluating $D = -\log_{10}(\max(T, \epsilon))$.
   - **Neutral Base Anchor**: When a valid film-base measurement exists, both runtimes set channel density offsets to zero (`channel_offsets = 0`), so the clear base produces neutral display white.
   - **Bounded Span Response**: The dynamic compensation branch in the GLSL shader mirrors the $1.15\times$ deadzone and $1.35\times$ / $3.0\times$ bounding thresholds of the CPU implementation.
   - **Tone Curve Spline Interpolation**: The viewport shader and export core both sample tone curves through 256-entry or 4096-entry cubic spline LUTs, keeping highlight transitions consistent and avoiding tonal banding.
3. **Automated Cross-Platform Verification**: Using `scripts/verify-density-contract.cjs` and shader syntax regression suites, every release validates that shader parameters, uniform bindings, and backend memory structures remain synchronized.

> [!IMPORTANT]
> Any new rendering feature must be implemented in both the WebGL2 fragment shader and the Rust CPU core. Do not add an unaligned heuristic to only one side.

## System Architecture Diagram & End-to-End Signal Topology {#system-architecture-signal-flow}

The diagram below presents the comprehensive system architecture and module topology of the NexFilm Engine:

<svg viewBox="0 0 860 420" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg" style="background:#161618; border-radius:12px; border:1px solid #27272a; font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;">
<rect x="25" y="25" width="245" height="370" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="40" y="55" fill="#e4e4e7" font-size="12" font-weight="bold">Native Front-End (WebView2 / WebKit)</text>
<text x="40" y="73" fill="#a1a1aa" font-size="10">Darkroom UI · WebGL2 Viewport</text>
<rect x="40" y="95" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="52" y="116" fill="#fafafa" font-size="11">View Controllers (main.js)</text>
<text x="52" y="132" fill="#a1a1aa" font-size="9">Library / Develop / Rolls / Calib</text>
<rect x="40" y="155" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="52" y="176" fill="#fafafa" font-size="11">Grading Controls (tone-curve.js)</text>
<text x="52" y="192" fill="#a1a1aa" font-size="9">Printer lights sliders / cubic splines</text>
<rect x="40" y="215" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="52" y="236" fill="#fafafa" font-size="11">Film Crop Geometry (geometry.js)</text>
<text x="52" y="252" fill="#a1a1aa" font-size="9">Gate detection / projective quad warping</text>
<rect x="40" y="275" width="215" height="52" rx="6" fill="#3f3f46" stroke="#fbbf24" stroke-width="1.5"/>
<text x="52" y="297" fill="#fbbf24" font-size="11" font-weight="bold">WebGL2 Fragment Shader (Canvas)</text>
<text x="52" y="313" fill="#fef08a" font-size="9">60fps real-time grading · GLSL ES 3.00</text>
<rect x="40" y="338" width="215" height="38" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="52" y="358" fill="#fafafa" font-size="10">IPC Client (invoke / listen)</text>
<line x1="270" y1="357" x2="305" y2="357" stroke="#fbbf24" stroke-width="2"/>
<rect x="305" y="25" width="250" height="370" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="320" y="55" fill="#e4e4e7" font-size="12" font-weight="bold">Tauri 2 Host &amp; Orchestration</text>
<text x="320" y="73" fill="#a1a1aa" font-size="10">System Services · Async IPC Layer</text>
<rect x="320" y="95" width="220" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="116" fill="#fafafa" font-size="11">IPC Dispatcher (commands.rs)</text>
<text x="332" y="132" fill="#a1a1aa" font-size="9">resolve_input_domain / analyze_base</text>
<rect x="320" y="155" width="220" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="176" fill="#fafafa" font-size="11">Application State (app_state.rs)</text>
<text x="332" y="192" fill="#a1a1aa" font-size="9">PipelineState / Roll / TuningParams</text>
<rect x="320" y="215" width="220" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="236" fill="#fafafa" font-size="11">Async Worker Pool (Rayon)</text>
<text x="332" y="252" fill="#a1a1aa" font-size="9">Background decoding · 16-bit proxy LRU</text>
<rect x="320" y="275" width="220" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="296" fill="#fafafa" font-size="11">Persistence Engine (persistence.rs)</text>
<text x="332" y="312" fill="#a1a1aa" font-size="9">nexfilm_user.db · schema migrations</text>
<rect x="320" y="335" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="355" fill="#fafafa" font-size="10">File System &amp; Security Boundary</text>
<line x1="555" y1="179" x2="590" y2="179" stroke="#34d399" stroke-width="2"/>
<rect x="590" y="25" width="245" height="370" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="605" y="55" fill="#e4e4e7" font-size="12" font-weight="bold">Rust Core &amp; Mastering Engine</text>
<text x="605" y="73" fill="#a1a1aa" font-size="10">High-Performance Native Compute</text>
<rect x="605" y="95" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="116" fill="#fafafa" font-size="11">Dual RAW Decoders (raw_backend.rs)</text>
<text x="617" y="132" fill="#a1a1aa" font-size="9">LibRaw 0.22+ &amp; Rawler Rust engine</text>
<rect x="605" y="155" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="176" fill="#fafafa" font-size="11">Streaming Readers (io_processor.rs)</text>
<text x="617" y="192" fill="#a1a1aa" font-size="9">Scanline strip reader for large TIFF/FFF</text>
<rect x="605" y="215" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="236" fill="#fafafa" font-size="11">Color Science Library (color_science.rs)</text>
<text x="617" y="252" fill="#a1a1aa" font-size="9">Linear ProPhoto / ACEScg / ICC profiles</text>
<rect x="605" y="275" width="215" height="52" rx="6" fill="#3f3f46" stroke="#34d399" stroke-width="1.5"/>
<text x="617" y="297" fill="#34d399" font-size="11" font-weight="bold">CPU Export Core (render_shader...)</text>
<text x="617" y="313" fill="#a7f3d0" font-size="9">Full-resolution scanline export</text>
<rect x="605" y="338" width="215" height="38" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="358" fill="#fafafa" font-size="10">DNG Writer (dng_writer.rs)</text>
</svg>

## Local Persistence & Non-Destructive Editing Guarantees {#local-persistence-and-safety}

In photographic digitization, crop shapes, printer lights, and tone curves represent real creative work. NexFilm follows a non-destructive editing policy: **the software does not alter the binary contents of original RAW captures, TIFF scans, or master files on disk**.

All session metadata and grading parameters are recorded as lightweight declarative state records in a local SQLite database (`nexfilm_user.db`). The persistence architecture (`src/persistence.rs`) is built upon three structural safeguards:

1. **Versioned Schema Migrations**: The database tracks explicit integer revisions for `MATH_VERSION` and `RAW_DECODE_VERSION`. When optical models evolve (such as the removal of legacy `legacy_linear_srgb` approximations in v1.1.0), the database executes idempotent migrations via `migrate_retired_density_recipe`. Older session records are updated transparently while preserving audit histories.
2. **ACID Transactions & Atomic Consistency**: Batch grading, parameter replication, and export tagging execute within single database transactions. If an unexpected power outage or system interruption occurs during processing, the database rolls back cleanly, preventing corrupted or half-written session files.
3. **Operating System AppData Segregation**: In debug configurations, the database sits beside the repository to facilitate diagnostic testing. In release binaries, the database is stored in standard application directories (`~/Library/Application Support/NexFilm Engine/` on macOS and `%APPDATA%/NexFilm Engine/` on Windows). This ensures system cleanup utilities do not delete user calibration data.
