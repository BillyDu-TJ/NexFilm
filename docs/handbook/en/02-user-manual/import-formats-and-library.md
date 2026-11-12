---
id: import-formats-and-library
track: user-manual
trackOrder: 2
chapterOrder: 2
kicker: "02 / User Manual"
title: "Format Support, Import Options, and Roll Archive Management"
subtitle: "Camera RAW, Flextight .fff, and scanned TIFF support, the actual roll and loose import paths, and missing-file relinking."
readingTime: 9
inApp: true
sinceVersion: "v1.1.0"
aliases: ["import-and-rolls"]
---

## Input Format Support Matrix and Decoder Specifications {#format-matrix}

NexFilm Engine is architected to unify the fragmented landscape of negative digitization, bridging the gap between high-resolution camera scanning workflows and classic drum/flatbed scanner outputs. Built with an optimized Rust image ingestion pipeline, the software directly parses uncompressed digital captures from modern digital sensors as well as industrial scanning systems.

The matrix below provides complete specifications for all supported input file formats:

<table class="doc-table">
    <thead>
        <tr><th>Format Class</th><th>File Extensions</th><th>Color Space and Bit Depth</th><th>Decoding Characteristics and Practical Advice</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Camera RAW</strong></td><td><code>.dng</code>, <code>.nef</code>, <code>.cr2</code>, <code>.cr3</code>, <code>.arw</code>, <code>.raf</code>, <code>.rw2</code>, <code>.orf</code>, <code>.pef</code>, <code>.3fr</code>, <code>.iiq</code>, etc.</td><td>12-bit, 14-bit, or 16-bit linear sensor Bayer mosaic data</td><td>The best input format for camera scanning. It bypasses in-camera white balance and tone curves, keeping the full linear dynamic range for precise base-mask subtraction.</td></tr>
        <tr><td><strong>Hasselblad Flextight / Imacon</strong></td><td><code>.fff</code></td><td>16-bit linear optical scanner transmission data</td><td>Since <strong>v1.1.0</strong>, the engine decodes <code>.fff</code> files as scanner transmission data without relying on the old FlexColor hardware ID string. That fixes the old problem where these scans could be mistaken for camera RAW files.</td></tr>
        <tr><td><strong>Production Scanner TIFF</strong></td><td><code>.tif</code>, <code>.tiff</code></td><td>16-bit or 8-bit uncompressed or LZW compressed</td><td>Handles single-channel monochrome and three-channel positive TIFFs from Noritsu, Frontier, Epson, and Plustek scanners. Large multi-gigabyte files are read in streams, keeping memory use steady.</td></tr>
        <tr><td><strong>Standard Bitmap Files</strong></td><td><code>.png</code>, <code>.jpg</code>, <code>.jpeg</code></td><td>8-bit or 16-bit sRGB or Display P3 encodings</td><td>Ideal for rapid composition checks. Because 8-bit JPEGs feature non-linear tone curves and clipped base data, automatic color precision cannot match RAW captures.</td></tr>
    </tbody>
</table>

> [!WARNING] Nikon Z8 / Z9 High-Efficiency NEF Restrictions
> The integrated LibRaw decoder does not currently support high-efficiency `HE` or `HE*` compressed NEF files produced by the Nikon Z8 and Z9. When digitizing negatives with these cameras, set the camera RAW recording format to conventional Lossless Compressed RAW, or convert captures to linear DNG using the free Adobe DNG Converter utility prior to import.

With 8-bit JPEG scans, non-linear gamma curves and harsh black/white clipping applied by scanning software often crush subtle shadow transitions and blur the distinction between orange mask dyes and dark details. When working with JPEGs, if the automatic inversion produces an unexpected color cast, use the Printer Lights panel to manually balance the RGB exposure offsets.

## Bounded Proxy Architecture and Low-Memory Indexing {#bounded-proxy-architecture}

When you import a full roll of 36 high-resolution 60-megapixel camera RAW files, or a set of large 4×5 TIFF scans, traditional converters often decode everything at full resolution during import. Memory use spikes, the CPU stays busy, and the interface becomes sluggish.

NexFilm overcomes this performance bottleneck through a **Two-Stage Bounded Proxy Architecture**:

<div class="doc-callout">
    <div class="doc-callout-title">What Is a Bounded Proxy?</div>
    <div class="doc-callout-body">Think of it as a working copy sized for the screen. Its long edge is capped at <code>2560 px</code>, which is enough for zooming, panning, and real-time grading on a 4K display without keeping the full-resolution scan in GPU memory. Export ignores the proxy and reads the original file again at full resolution, so the proxy affects preview speed, not final image quality.</div>
</div>

1. **Stage 1: Instantaneous Metadata Ingestion and Preview Extraction**
   Once you select the scans, the engine reads only the file headers and pulls any embedded previews and Exif camera metadata. A few dozen frames usually enter the local database within seconds, and memory use stays modest while this happens.
2. **Stage 2: On-Demand Bounded Proxy Generation**
   Full decoding occurs strictly on demand when a user double-clicks a frame to open the Develop workspace. The engine renders a high-fidelity bounded proxy constrained to a long edge of `2560 px`. This resolution provides crisp rendering across 4K and 5K displays while capping active GPU texture memory consumption.

When you export, the engine skips the proxy and streams the full-resolution source from disk through the color pipeline at the highest available bit depth.

## The Three Import Dialog Options and Metadata Enrollment {#tri-state-import}

To accommodate different scanning routines, NexFilm offers three options in the **Import Method** dialog. They look like peers, but they do different jobs: the first two bring new files into the workspace, while the third simply reopens an archived roll.

<div class="doc-ui-demo doc-ui-demo-import">
  <div class="doc-ui-demo-topbar">
    <span class="doc-ui-demo-window-dots" aria-hidden="true"><i></i><i></i><i></i></span>
    <span class="doc-ui-demo-window-title">NEXFILM / IMPORT METHOD</span>
    <span class="doc-ui-demo-badge">INTERACTIVE PREVIEW</span>
  </div>
  <input class="doc-ui-demo-radio" type="radio" name="doc-import-mode-en" id="doc-import-roll-en" checked>
  <input class="doc-ui-demo-radio" type="radio" name="doc-import-mode-en" id="doc-import-loose-en">
  <input class="doc-ui-demo-radio" type="radio" name="doc-import-mode-en" id="doc-import-continue-en">
  <div class="doc-ui-demo-choices">
    <label class="doc-ui-demo-choice" for="doc-import-roll-en">
      <span class="doc-ui-demo-index">01</span>
      <span class="doc-ui-demo-choice-copy"><strong>Import by Roll</strong><small>Archive a full roll with camera and film metadata</small></span>
      <span class="doc-ui-demo-arrow">→</span>
    </label>
    <label class="doc-ui-demo-choice" for="doc-import-loose-en">
      <span class="doc-ui-demo-index">02</span>
      <span class="doc-ui-demo-choice-copy"><strong>Loose Import</strong><small>Open one or more files without creating a roll</small></span>
      <span class="doc-ui-demo-arrow">→</span>
    </label>
    <label class="doc-ui-demo-choice" for="doc-import-continue-en">
      <span class="doc-ui-demo-index">03</span>
      <span class="doc-ui-demo-choice-copy"><strong>Continue Editing Roll</strong><small>Reopen an archived roll for grading</small></span>
      <span class="doc-ui-demo-arrow">→</span>
    </label>
  </div>
  <div class="doc-ui-demo-panels">
    <section class="doc-ui-demo-panel doc-ui-demo-panel-roll" aria-label="Import by Roll preview">
      <div class="doc-ui-demo-window">
        <div class="doc-ui-demo-window-head"><span>ROLL METADATA</span><span>Next: select images</span></div>
        <div class="doc-ui-demo-fields">
          <div class="doc-ui-demo-field"><span>FORMAT</span><strong>135 (35mm)</strong></div>
          <div class="doc-ui-demo-field"><span>CAMERA</span><strong>Leica M6</strong></div>
          <div class="doc-ui-demo-field"><span>FILM</span><strong>Kodak Portra 400</strong></div>
          <div class="doc-ui-demo-field"><span>DATE</span><strong>2026-11-12</strong></div>
        </div>
        <div class="doc-ui-demo-window-foot"><span>Enables roll-wide processing and a shared white point</span><span class="doc-ui-demo-primary">Select images →</span></div>
      </div>
    </section>
    <section class="doc-ui-demo-panel doc-ui-demo-panel-loose" aria-label="Loose Import preview">
      <div class="doc-ui-demo-window">
        <div class="doc-ui-demo-window-head"><span>LOOSE IMPORT</span><span>Next: select files</span></div>
        <div class="doc-ui-demo-dropzone"><strong>Drop image files here</strong><span>RAW / TIFF / PNG / JPEG</span></div>
        <div class="doc-ui-demo-window-foot"><span>No roll metadata; opens directly in Library</span><span class="doc-ui-demo-primary">Select files →</span></div>
      </div>
    </section>
    <section class="doc-ui-demo-panel doc-ui-demo-panel-continue" aria-label="Continue Editing Roll preview">
      <div class="doc-ui-demo-window">
        <div class="doc-ui-demo-window-head"><span>CONTINUE EDITING ROLL</span><span>Choose a roll archive</span></div>
        <div class="doc-ui-demo-select"><span>ROLL ARCHIVE</span><strong>Portra 400 · 2026-10-18</strong><b>⌄</b></div>
        <p class="doc-ui-demo-note">This option only opens an archived roll; it does not add new scans to it.</p>
        <div class="doc-ui-demo-window-foot"><span>Return to the original roll</span><span class="doc-ui-demo-primary">Open →</span></div>
      </div>
    </section>
  </div>
  <p class="doc-ui-demo-caption">Click a mode above to preview the next step. This is a UI preview and does not perform a real import.</p>
</div>

### 1. Import as Roll
The primary workflow for cataloging whole rolls captured or scanned in unified batches:
- Open the import dialogue and choose "Import as Roll".
- **Format Hierarchy Selection**: Select from standard analog format presets:
  - 135 Full Frame (35mm), 135 Half-frame, APS;
  - 120 Medium Format range: `120 (645)`, `120 (6x6)`, `120 (6x7)`, `120 (6x9)`, `120 (6x12)`, `120 (6x17)`;
  - Large Format Sheet Film: `4x5`, `5x7`, `8x10`.
- **Hardware and Emulation Data**: Assign or enter the camera body (e.g., Leica M6, Pentax 67) and emulsion profile (including Kodak Portra, Gold 200, Fujifilm C200, Ilford HP5 Plus, and Kodak Vision3 cinema stocks).
- **Date and Roll Notes**: Record capture dates and freeform context notes (e.g., "Roll 03, E100 pushed +1 stop, studio session"). These attributes feed batch file renaming templates and output Exif fields.

### 2. Loose Import
Intended for single-frame test shots, scanner calibration targets, or isolated frames not tied to a specific roll:
- Select "Loose Import" in the dialogue or simply drag and drop image files directly onto the main application window.
- Loose frames appear in the Library as standalone items with full inversion capabilities, but they do not participate in roll-wide physical calibrations.
- Duplicate detection is automatic: if an imported file already exists within the active project, the engine skips it to avoid duplicate records.

### 3. Continue Editing an Archived Roll
If a roll was archived earlier and you want to return to it for more grading, choose **Continue Editing Roll** in the **Import Method** dialog. Pick the roll from the **Roll Archive** list and click **Open & Edit**. NexFilm brings that roll back into the Library and filmstrip.

To be clear: **Continue Editing Roll only opens the archived roll; it does not add new files to it.** If you scanned a few more frames later, import them as a separate roll for now and manage the two scan batches separately. The documentation should not describe an append workflow that is not exposed in the interface.

## Rolls Archive Management and Multi-Dimensional Filtering {#rolls-management}

Switching to the Rolls view opens your persistent digital negative archive. Over time, this section serves as an organized catalog of all historical film projects processed through the engine.

Key management tools within the Rolls view include:
- **Multi-Dimensional Faceted Filtering**: The left panel offers interactive filtering across film formats, camera hardware, and capture dates. The header reflects the exact count of matching rolls and total frames in real time, with a **Clear** button to reset filters.
- **Roll Card Catalog Grid**: The central canvas displays visual roll cards highlighting cover thumbnails, frame counts, format badges, camera gear, emulsion types, and creation dates.
- **Roll Management Hub**: Entering a specific roll card reveals its complete thumbnail sequence and unlocks critical actions:
  1. **Promote to Library**: Loads the entire archive roll into the primary Library workspace for renewed grading sessions.
  2. **Edit Info**: Allows updating camera names, film stocks, dates, and roll notes. Modifications automatically synchronize with subsequent export operations.
  3. **Export Contact Sheet**: Generates an index sheet featuring authentic sprockets, frame numbering, and clean metadata headers.
  4. **Delete Roll**: Offers two protective options: unregistering the roll metadata from the local database, or sending the underlying source files to the operating system trash as well. If other projects still reference those source files, they are preserved.

## Offline Source Files and Broken Link Recovery {#locate-missing-files}

NexFilm Engine follows a non-destructive, reference-based asset model: **the application does not duplicate or move your raw capture files**. The local database `nexfilm_user.db` stores only absolute filesystem paths to your source assets.

If folders are renamed, rearranged, or disconnected on external storage drives, the associated frames transition into an offline state:
- An offline indicator icon appears on the frame thumbnail, noting that the source file could not be resolved at its recorded path.
- Thanks to local cached proxies, thumbnails remain viewable, but full-resolution exports and new inversions are temporarily disabled.

**Broken Link Recovery Procedure (Locate File)**:
1. Select the offline frame and click the **Locate File** button on the interface.
2. In the native system file picker, browse to the new location of the file and select it.
3. NexFilm updates the file path reference and scans the directory for other offline frames belonging to the same roll. If relative paths and filenames match, the engine automatically reconnects the entire roll in a single step.

With robust format decoding, lightweight proxy memory management, and secure asset cataloging, NexFilm provides a dependable foundation for serious analog archival workflows.
