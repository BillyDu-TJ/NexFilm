---
id: export-and-contact-sheets
track: user-manual
trackOrder: 2
chapterOrder: 6
kicker: "02 / User Manual"
title: "Master-Grade Exports, Color Space Management, and Contact Sheets"
subtitle: "A six-format decision matrix, seven output ICC color spaces, tokenized batch renaming, and authentic physical contact sheets."
readingTime: 13
inApp: true
sinceVersion: "v1.1.0"
---

## Six Export Formats Decision Matrix and Selection Guidelines {#format-decision-matrix}

In an authentic darkroom workflow, export represents the final serialization step where non-destructive grading adjustments, geometric transformations, and tone curves are permanently rendered into concrete raster assets. NexFilm rejects simplistic, one-size-fits-all output pipelines. To serve web publication, fine-art printing, archival preservation, and motion picture post-production, the engine provides an extensive export matrix spanning six distinct formats across deliverable and master classifications.

The technical specifications, data linearity models, and target applications for each format are detailed below:

<table class="doc-table">
    <thead>
        <tr><th>Export Format</th><th>Bit Depth and Encoding</th><th>Data Linearity</th><th>Embedded Adjustments</th><th>Recommended Downstream Workflow</th><th>Storage and Fidelity Tradeoffs</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>JPEG · 8-bit</strong></td><td>8-bit integer, lossy DCT compression</td><td>Non-linear display space (gamma encoded)</td><td>Yes (includes printer lights, curves, detail, and print LUTs)</td><td>Web browsers, mobile devices, social platforms, and instant sharing.</td><td>Compact footprint (typically 2–8 MB). Quality slider spans 40 to 100 (default 92). Ideal for rapid proofs.</td></tr>
        <tr><td><strong>PNG · 16-bit</strong></td><td>16-bit integer, lossless compression</td><td>Non-linear display space</td><td>Yes (complete positive grading baked in)</td><td>High-fidelity image viewers, digital archives, and publishing layouts.</td><td>Moderate footprint (~30–70 MB). Preserves 65,536 tonal levels per channel with zero lossy compression artifacts.</td></tr>
        <tr><td><strong>TIFF · 8-bit</strong></td><td>8-bit integer, lossless compression</td><td>Non-linear display space</td><td>Yes (complete positive grading baked in)</td><td>Legacy prepress workflows, desktop publishing, and raster prepress tools.</td><td>Moderate footprint. Engineered specifically for legacy prepress environments requiring TIFF containers under memory constraints.</td></tr>
        <tr><td><strong>TIFF · 16-bit</strong></td><td>16-bit integer, lossless compression</td><td>Non-linear display space</td><td>Yes (complete positive grading baked in)</td><td>Adobe Photoshop, Lightroom, Capture One, and fine-art giclée printing.</td><td>Substantial footprint (80–180 MB). The industry benchmark for master retouching, offering maximum headroom for layered color adjustments.</td></tr>
        <tr><td><strong>TIFF · 16-bit linear</strong></td><td>16-bit integer, lossless compression</td><td><strong>Pure linear optical space</strong> (display gamma omitted)</td><td>Yes (includes inversion and printer lights, but strips display curves)</td><td>DaVinci Resolve, Autodesk Flame, and Nuke visual effects pipelines.</td><td>Cinema delivery format. Embeds a linear ICC profile, where pixel values scale proportionally to scene transmission for wide-gamut grading.</td></tr>
        <tr><td><strong>DNG · linear</strong></td><td>16-bit linear integer/floating-point DNG</td><td><strong>Pure linear optical space</strong></td><td>Yes (serialized as an Adobe LinearRaw DNG container)</td><td>Adobe Camera Raw, Lightroom Classic, and Photoshop.</td><td>Packs the inverted positive into an Adobe LinearRaw container, allowing Lightroom to manipulate highlights and shadows natively.</td></tr>
        <tr><td><strong>DNG · camera RAW</strong></td><td>Raw sensor Bayer mosaic</td><td>Raw camera sensor linear data</td><td><strong>No (contains zero grading or inversion parameters)</strong></td><td>Any RAW processing utility supporting standard Adobe DNG.</td><td><strong>Pure archival data encapsulation</strong>. Repacks unmodified camera Bayer pixels directly into DNG for offsite negative preservation.</td></tr>
    </tbody>
</table>

> [!IMPORTANT] Distinguishing Linear DNG from Camera RAW DNG
> **Linear DNG represents a fully processed positive master**: inversion, printer lights, and perspective corrections are completely rendered, while retaining linear dynamic range. **Camera RAW DNG is strictly an unmodified archival container**: it contains zero color corrections, inversions, or crops. Output color spaces and output sharpening are entirely bypassed for Camera RAW DNG, and this option is available exclusively when the source file is a native camera RAW (scanner TIFFs cannot be exported as Camera RAW DNG).

## Seven Output ICC Color Spaces and Gamut Mapping {#color-spaces-and-icc}

Color spaces govern how exported pixel values translate into visible spectral wavelengths on physical monitors or printing inks. NexFilm integrates seven standardized output color spaces spanning consumer displays to cinema archival standards, automatically embedding matching ICC profiles into file headers:

<table class="doc-table">
    <thead>
        <tr><th>Color Space Designation</th><th>Official Standard and Gamut Coverage</th><th>Target Delivery Medium</th><th>Operational Guidelines and Core Restrictions</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>sRGB</strong></td><td>sRGB IEC 61966-2.1 (standard web gamut)</td><td>Internet publishing, web galleries, mobile displays, and standard office monitors.</td><td>The safest, most universally compatible color space. Devices lacking color management assume sRGB, preventing unexpected shifts.</td></tr>
        <tr><td><strong>Display P3</strong></td><td>Apple Display P3 (DCI-P3 gamut paired with D65 white point)</td><td>MacBook Pro, iPhone, iPad, and wide-gamut OLED/Mini-LED displays.</td><td>Renders substantially richer reds and greens than sRGB, ideal for viewing high-fidelity digital portfolios across the Apple ecosystem.</td></tr>
        <tr><td><strong>Adobe RGB (1998)</strong></td><td>Adobe RGB (1998) (wide photography gamut)</td><td>Fine-art giclée printing, high-end book publishing, and professional photography displays.</td><td>Encompasses nearly all cyan and green printing ink pigments, representing the traditional gold standard for print prepress.</td></tr>
        <tr><td><strong>ITU-R BT.2020</strong></td><td>Rec.2020 (ultra-wide broadcast gamut)</td><td>4K/8K UHD television production, HDR workflows, and next-generation wide-gamut displays.</td><td>Extremely wide color envelope aligned with contemporary and future digital broadcast standards.</td></tr>
        <tr><td><strong>ProPhoto RGB</strong></td><td>ProPhoto RGB / ROMM RGB (ultra-wide master space)</td><td>16-bit archival masters, preservation of extreme negative dye saturation, and deep Photoshop retouching.</td><td>Covers over 90% of visible spectrum colors, easily accommodating intense emulsion dye saturation. <strong>Never use with 8-bit JPEG</strong>.</td></tr>
        <tr><td><strong>ACEScg</strong></td><td>ACEScg (AP1 color space, D60 white point)</td><td>Computer-generated imagery (CGI), visual effects compositing, and digital intermediate grading.</td><td>The global film industry interchange standard, facilitating handoffs into DaVinci Resolve color suites.</td></tr>
        <tr><td><strong>ACES2065-1</strong></td><td>ACES2065-1 (AP0 ultra-wide space, D60 white point)</td><td>The Academy of Motion Picture Arts and Sciences (AMPAS) archival and data exchange master.</td><td>Fully encompasses the entire visible spectrum and beyond, designated for long-term institutional preservation.</td></tr>
    </tbody>
</table>

> [!WARNING] Never Apply Wide-Gamut Profiles to 8-Bit JPEGs
> When exporting 8-bit JPEGs, color information is restricted to 256 discrete levels per channel. Applying ultra-wide gamuts such as ProPhoto RGB or ACES to an 8-bit file stretches these scarce quantization steps over vast color distances. This inevitably causes severe **color banding and posterization** across smooth skies and subtle skin tones. When exporting 8-bit JPEGs, restrict your color space strictly to **sRGB** or **Display P3**.

## Resizing Strategies, Quality Protection, and Output Sharpening {#dimensions-and-sharpening}

Within the Dimensions section of the export dialogue, photographers can configure precise geometric scaling tailored to specific downstream destinations:

### 1. Original Dimensions
Renders the complete sensor or scanner matrix at full resolution without resampling, providing the foundation for archival masters and large-format exhibition prints.

### 2. Set Long Edge
Specifies a custom maximum long-edge dimension spanning **`256` to `32768` pixels**. The panel provides one-click quick presets for 1024, 2048, 4096, and 8192 pixels (with 2048 px representing the universal standard for high-density social web delivery). Scaling maintains exact aspect ratios to prevent image distortion.

### 3. Upscaling Protection (Allow Upscaling Off by Default)
The interface features an "Allow Upscaling" checkbox, **which remains strictly unselected by default**. When an original capture possesses long-edge dimensions smaller than the specified target, the engine maintains the native file size rather than performing synthetic interpolation. This policy protects native silver grain from being softened by interpolation blur.

### 4. Four-Tier Output Sharpening Matrix
Operating independently from the creative sharpening controls in the Develop workspace, output sharpening specifically counteracts resampling blur introduced during resolution scaling:
- **None**: Bypasses secondary sharpening entirely, preserving raw analog grain for specialized prepress workflows.
- **Low**: Mildly restores edge acuity for high-density Retina and OLED displays.
- **Standard (Default Recommendation)**: An optimized balance that delivers crisp, natural micro-contrast suitable for both screen viewing and photographic inkjets.
- **High**: Enhances edge transitions specifically for downsampled thumbnail indices below 1024 px.

## Batch Token Renaming Engine and Conflict Resolution {#naming-tokens-and-conflicts}

When exporting dozens of frames, consistent file naming is essential for catalog integrity. NexFilm integrates a token-based file naming engine that supports six core dynamic variables:

<table class="doc-table">
    <thead>
        <tr><th>Naming Token</th><th>Extracted Metadata Source</th><th>Example Output</th><th>Archival Utility</th></tr>
    </thead>
    <tbody>
        <tr><td><code>{Roll}</code></td><td>Unique roll identifier or assigned roll name.</td><td><code>Roll01</code>, <code>2024_Hokkaido</code></td><td>Locks the frame firmly to its source roll catalog.</td></tr>
        <tr><td><code>{Camera}</code></td><td>Camera body specified in the roll metadata.</td><td><code>LeicaM6</code>, <code>Hasselblad503CW</code></td><td>Organizes images by physical capture hardware.</td></tr>
        <tr><td><code>{Film}</code></td><td>Film emulsion type specified in the roll metadata.</td><td><code>Portra400</code>, <code>Tri-X400</code></td><td>Differentiates emulsion character across projects.</td></tr>
        <tr><td><code>{Date}</code></td><td>Capture or digitization date recorded at import.</td><td><code>2024-11-05</code>, <code>Undated</code></td><td>Establishes chronological image archives.</td></tr>
        <tr><td><code>{Original}</code></td><td>Original raw scan filename on disk.</td><td><code>DSC_0428</code>, <code>scan_frame01</code></td><td>Maintains provenance back to original raw scans.</td></tr>
        <tr><td><code>{Seq}</code></td><td>Three-digit zero-padded sequence index.</td><td><code>001</code>, <code>002</code>, <code>036</code></td><td>Ensures alphabetical sorting matches exposure sequence.</td></tr>
    </tbody>
</table>

**Practical Template Combinations**:
- `{Roll}_{Seq}` → Resolves to `Roll01_001.jpg` (standard clean template, default);
- `{Date}_{Film}_{Camera}_{Seq}` → Resolves to `2024-11-05_Portra400_LeicaM6_001.tif` (exhaustive archival template);
- `{Roll}_{Original}_{Seq}` → Resolves to `2024_Tokyo_DSC_1024_001.png` (source provenance template).

**Filename Sanitization**:
The template engine automatically sanitizes invalid filesystem characters (`< > : " / \ | ? *`) and control codes, converting them into clean underscores. It also detects Windows reserved filenames (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`), prefixing an underscore to guarantee cross-platform file write operations.

**Conflict Resolution Policies**:
If identically named files already occupy the target directory, the engine provides three deterministic options:
1. **Keep both · add a suffix (Default)**: Automatically appends `_1`, `_2` suffixes to prevent data loss;
2. **Replace existing file**: Overwrites prior files, useful when regenerating revised deliverables;
3. **Skip existing file**: Bypasses existing filenames, exporting only newly added frames.

Enabling **Write film, camera, and date to EXIF** injects roll metadata into standard Exif and IPTC fields, ensuring digital assets permanently retain analog provenance.

## Contact Sheet Physical Emulation and Generation Mechanics {#contact-sheet-generation}

In traditional darkrooms, contact sheets were made by placing cut negative strips directly emulsion-to-emulsion against an 8×10-inch sheet of photographic paper under an enlarger, exposing it briefly to produce a 1:1 index print. Contact sheets remain both a critical sorting tool and an enduring symbol of analog photographic practice.

Under **Rolls view → Roll Contents**, clicking **Export Contact Sheet** activates a digital contact sheet engine that renders physical negative geometry with historical precision:

### 1. 135 Full Frame Perforations and Edge Barcodes
For 35mm film, layout geometry adheres strictly to the ISO 1007 standard:
- Six frames per row with realistic 2mm inter-frame gaps;
- Authentic KS perforation holes along top and bottom borders, following a continuous 4.75mm pitch across frame boundaries;
- Rendered amber latent-image edge codes featuring frame numbers, half-frame indicators, and machine-readable barcodes.

### 2. 120 Medium Format Adaptive Multi-Aspect Layout
Because 120 film lacks perforations, the engine identifies frame dimensions and dynamically scales grid columns:
- `6x4.5` (vertical orientation): 4 frames per row;
- `6x6` (square format): 4 frames per row;
- `6x7`: 3 frames per row;
- `6x9` and `6x12`: 2 frames per row;
- `6x17` panoramic: 1 frame per row.
- Top and bottom borders render capitalized film stock labels, sequential frame numbers, and directional orientation triangles.

### 3. Layout Aesthetics and Unprocessed Frame Handling
- **Darkroom Presentation**: Contact sheets sit on a warm matte photographic paper ground (`#E9E9E7`) with pure black negative strips. The footer displays a clean vector lockup of the **NEXFILM** logo alongside Roll ID, Camera, Film Stock, and Date headers.
- **Handling Uninverted Frames**: If certain frames within a roll have not yet been graded in the Develop workspace, the engine avoids rendering raw negative scans. Instead, it fills the frame slot with an elegant neutral grey tone labeled with a understated **NOT DEVELOPED** indicator, maintaining sheet presentation while signaling remaining work.

Generated contact sheets save automatically as `contact_sheet_{roll_id}_{camera}.jpg` in your destination folder.

## Full-Roll Background Export Queue and Integrity Safeguards {#batch-export-queue}

Whether exporting dozens of 16-bit master TIFFs or generating large-format contact sheets, export operations are managed by a dedicated background concurrency queue:

- **Pre-Flight Integrity Check**: Before launching processing, the engine performs a pre-flight scan: it verifies source file availability on disk and validates inversion parameter integrity. If missing files are detected, the system pauses immediately and presents an itemized dialog, preventing wasted processing time.
- **Asynchronous Execution**: The worker pool utilizes multi-core hardware while regulating disk I/O to avoid head thrashing on mechanical drives. Photographers can continue refining other images in the Develop workspace while background exports proceed.
- **The Darkroom Test Print Rule**: Before launching a large-scale master export, **always select a single frame with rich tonal contrast to export as a test proof**. Inspecting this proof on a calibrated monitor verifies colors, sharpness, and dimensions before processing an entire roll.
