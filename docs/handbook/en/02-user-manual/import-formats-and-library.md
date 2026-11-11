---
id: import-formats-and-library
track: user-manual
trackOrder: 2
chapterOrder: 2
kicker: "02 / User Manual"
title: "Format Support Matrix, Tri-State Import, and Roll Archive Management"
subtitle: "A comprehensive format support matrix across camera RAW, Flextight .fff, and scanned TIFF, coupled with tri-state import workflows and missing file relinking."
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
        <tr><td><strong>Camera RAW</strong></td><td><code>.dng</code>, <code>.nef</code>, <code>.cr2</code>, <code>.cr3</code>, <code>.arw</code>, <code>.raf</code>, <code>.rw2</code>, <code>.orf</code>, <code>.pef</code>, <code>.3fr</code>, <code>.iiq</code>, etc.</td><td>12-bit, 14-bit, or 16-bit linear sensor Bayer mosaic data</td><td>The premier input format for camera scanning. Bypasses in-camera white balance and tone curves, retaining the full linear sensor dynamic range for surgical base mask subtraction.</td></tr>
        <tr><td><strong>Hasselblad Flextight / Imacon</strong></td><td><code>.fff</code></td><td>16-bit linear optical scanner transmission data</td><td>As of <strong>v1.1.0</strong>, the engine natively decodes <code>.fff</code> files as scanner transmission data without relying on legacy FlexColor hardware ID strings, preventing misidentification as camera RAWs.</td></tr>
        <tr><td><strong>Production Scanner TIFF</strong></td><td><code>.tif</code>, <code>.tiff</code></td><td>16-bit or 8-bit uncompressed or LZW compressed</td><td>Natively handles single-channel monochrome and three-channel positive TIFFs from Noritsu, Frontier, Epson, and Plustek units. Streams large multi-gigabyte files with stable memory usage.</td></tr>
        <tr><td><strong>Standard Bitmap Files</strong></td><td><code>.png</code>, <code>.jpg</code>, <code>.jpeg</code></td><td>8-bit or 16-bit sRGB or Display P3 encodings</td><td>Ideal for rapid composition checks. Because 8-bit JPEGs feature non-linear tone curves and clipped base data, automatic color precision cannot match RAW captures.</td></tr>
    </tbody>
</table>

> [!WARNING] Nikon Z8 / Z9 High-Efficiency NEF Restrictions
> The integrated LibRaw decoder does not currently support high-efficiency `HE` or `HE*` compressed NEF files produced by the Nikon Z8 and Z9. When digitizing negatives with these cameras, set the camera RAW recording format to conventional Lossless Compressed RAW, or convert captures to linear DNG using the free Adobe DNG Converter utility prior to import.

With 8-bit JPEG scans, non-linear gamma curves and harsh black/white clipping applied by scanning software often crush subtle shadow transitions and blur the distinction between orange mask dyes and dark details. When working with JPEGs, if the automatic inversion produces an unexpected color cast, use the Printer Lights panel to manually balance the RGB exposure offsets.

## Bounded Proxy Architecture and Low-Memory Indexing {#bounded-proxy-architecture}

When processing a full roll of 36 high-resolution 60-megapixel camera RAW files or massive 4×5 drum scans, traditional raw converters routinely launch full-resolution decoding on import. This causes massive memory spikes, sustained high CPU loads, and sluggish responsiveness.

NexFilm overcomes this performance bottleneck through a **Two-Stage Bounded Proxy Architecture**:

1. **Stage 1: Instantaneous Metadata Ingestion and Preview Extraction**
   When a folder is selected for import, the engine reads only the file headers to extract pre-existing embedded JPEG previews and Exif camera metadata. An entire roll of several dozen frames is cataloged into the local database in two to three seconds. The application footprint during this stage remains under 200 MB of system RAM.
2. **Stage 2: On-Demand Bounded Proxy Generation**
   Full decoding occurs strictly on demand when a user double-clicks a frame to open the Develop workspace. The engine renders a high-fidelity bounded proxy constrained to a long edge of `2560 px`. This resolution provides crisp rendering across 4K and 5K displays while capping active GPU texture memory consumption.

When final exports are requested, the engine bypasses the proxy entirely, streaming full-resolution source scan data from disk through the physical color pipeline at maximum bit depth.

## Tri-State Ingestion and Metadata Enrollment {#tri-state-import}

To accommodate distinct shooting and scanning routines, NexFilm provides three complementary ingestion workflows that establish proper darkroom cataloging at the point of entry:

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

### 3. Append to Existing Roll
If additional frames from a previously cataloged roll are digitized later, navigate to the Rolls view, select the roll, click "Continue Editing Roll", and trigger "Import Roll". The newly scanned frames will be appended directly to the end of the existing filmstrip, automatically inheriting the roll camera and emulsion metadata.

## Rolls Archive Management and Multi-Dimensional Filtering {#rolls-management}

Switching to the Rolls view opens your persistent digital negative archive. Over time, this section serves as an organized catalog of all historical film projects processed through the engine.

Key management tools within the Rolls view include:
- **Multi-Dimensional Faceted Filtering**: The left panel offers interactive filtering across film formats, camera hardware, and capture dates. The header reflects the exact count of matching rolls and total frames in real time, with a **Clear** button to reset filters.
- **Roll Card Catalog Grid**: The central canvas displays visual roll cards highlighting cover thumbnails, frame counts, format badges, camera gear, emulsion types, and creation dates.
- **Roll Management Hub**: Entering a specific roll card reveals its complete thumbnail sequence and unlocks critical actions:
  1. **Promote to Library**: Loads the entire archive roll into the primary Library workspace for renewed grading sessions.
  2. **Edit Info**: Allows updating camera names, film stocks, dates, and roll notes. Modifications automatically synchronize with subsequent export operations.
  3. **Export Contact Sheet**: Generates an index sheet featuring authentic sprockets, frame numbering, and clean metadata headers.
  4. **Delete Roll**: Offers two protective options: unregistering the roll metadata from the local database, or simultaneously sending the underlying source files to the operating system trash. If source files are referenced by other projects, they are safely preserved.

## Offline Source Files and Broken Link Recovery {#locate-missing-files}

NexFilm Engine adheres to a strict non-destructive, reference-based asset architecture: **the application never duplicates or moves your raw capture files**. The local database `nexfilm_user.db` stores only absolute filesystem path references to your source assets.

If folders are renamed, rearranged, or disconnected on external storage drives, the associated frames transition into an offline state:
- An offline indicator icon appears on the frame thumbnail, noting that the source file could not be resolved at its recorded path.
- Thanks to local cached proxies, thumbnails remain viewable, but full-resolution exports and new inversions are temporarily disabled.

**Broken Link Recovery Procedure (Locate File)**:
1. Select the offline frame and click the **Locate File** button on the interface.
2. In the native system file picker, browse to the new location of the file and select it.
3. NexFilm updates the file path reference and scans the directory for other offline frames belonging to the same roll. If relative paths and filenames match, the engine automatically reconnects the entire roll in a single step.

With robust format decoding, lightweight proxy memory management, and secure asset cataloging, NexFilm provides a dependable foundation for serious analog archival workflows.
