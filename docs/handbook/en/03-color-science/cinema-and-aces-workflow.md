---
id: cinema-and-aces-workflow
track: color-science
trackOrder: 3
chapterOrder: 4
kicker: "Color Science 04 / Cinema & ACES Pipeline"
title: "16-bit Linear Mastering & DaVinci Resolve / ACEScg Cinema Workflow"
subtitle: "Lossless mastering export truncation nodes, ACEScg transformation contracts, and DaVinci Resolve grade tree configuration."
readingTime: 14
inApp: true
sinceVersion: "v1.1.0"
---

## Linear Mastering Delivery vs Display-Referred Rendering {#linear-mastering-vs-display-rendering}

In professional film digitization, the designated role of an export format dictates the degree of creative latitude available during downstream color grading. Conventional inversion utilities typically produce display-referred finished deliverables, such as 8-bit JPEG files or standard TIFFs encoded with sRGB display transfer functions. Prior to export, these files bake in internal tone curves, print film emulation lookup tables, highlight rolloff compression, and monitor electro-optical transfer functions (OETFs). When a colorist subsequently attempts substantial exposure adjustments or shadow recovery, the image degrades rapidly with tonal posterization, hue shifts, and amplified sensor noise.

In contrast, **Linear Mastering Delivery** provides scene-referred intermediate deliverables tailored for theatrical post-production pipelines. The core mechanism involves tapping the data stream immediately following optical inversion and primary working space projection, prior to the application of any display curves or perceptual color grading.

```text
NexFilm Core Processing Pipeline
┌────────────────────────────────────────────────────────┐
│  Transmittance -> Net Density -> Printer Lights -> Inversion │
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼
               [ Linear Truncation Tap ]
              /                         \
             /                           \
     [ Lossless Master Exports ]      [ Display Render Pipeline ]
     - 16-bit Linear TIFF             - Four-channel tone curves
     - LinearRaw DNG                  - Print film LUTs (Kodak 2383)
     - Raw Mosaic DNG                 - Spatial sharpening & tone mapping
     (Linear gamma / full range)      - sRGB / Display P3 OETF encoding
             │                        - 8/16-bit TIFF / JPEG delivery
             ▼
     DaVinci Resolve / ACEScg
```

Exported through this linear truncation point, a 16-bit Linear ProPhoto RGB raster maintains pixel values that remain strictly proportional to original positive scene irradiance. This representation yields grading latitude equivalent to digital cinema camera raw footage (such as ARRI ALEXA LogC or RED Log3G10). In DaVinci Resolve, colorists can grade these wide-gamut, uncompressed positive rasters with full secondary control and color management fidelity.

## Three Master Export Formats & Truncation Architectures {#three-master-export-formats}

To satisfy diverse post-production requirements and long-term archival standards, the NexFilm backend (`src/dng_writer.rs` and `src/commands.rs`) implements three lossless mastering configurations:

1. **16-bit Linear TIFF**:  
   Upon completing net optical density inversion and base subtraction, the raster transforms into Linear ProPhoto RGB floating-point radiance and quantizes directly to uncompressed 16-bit integer TIFF. The file records an identity gamma of 1.0 alongside standard ROMM RGB chromaticity coordinates. This format provides the cleanest interchange standard for immediate ingestion into DaVinci Resolve, Nuke, or Baselight.
2. **LinearRaw DNG**:  
   For workflows coordinated through Adobe Camera Raw or Lightroom, users can export as LinearRaw DNG. Structured as a TIFF/EP container, the file writes `PhotometricInterpretation = 34892 (LinearRaw)` and embeds calibrated D50 and D65 color matrix tags. Third-party raw developers recognize the file as an un-demosaiced linear raw capture, allowing flexible adjustments of color temperature, white balance, and chromatic lens corrections.
3. **Raw Mosaic DNG**:  
   Tailored for archival institutions requiring strict preservation of original digitizer sensor data. Rather than performing demosaicing or non-linear adjustments, the engine re-wraps raw Bayer or X-Trans color filter array photosite samples directly into a compliant DNG container (`PhotometricInterpretation = 32803 (CFA)`). Hardware metadata, black levels, and white balance multipliers are preserved verbatim, guaranteeing uncompromised re-processing in future decades.

> [!IMPORTANT]
> When exporting 16-bit Linear TIFF or LinearRaw DNG files, active contrast sliders, shadow curves, and 3D print film LUTs within the NexFilm interface are automatically bypassed. In standard desktop image viewers, these files appear flat and dark, reflecting linear radiant energy distribution.

## DaVinci Resolve Color Management Configuration Guide {#davinci-resolve-color-management}

When importing NexFilm 16-bit Linear ProPhoto RGB masters into DaVinci Resolve, technical colorists should utilize color-managed pipelines to maintain consistent color rendering:

### Approach A: DaVinci YRGB Color Managed Pipeline (Recommended for Commercial Facilities)

1. Navigate to DaVinci Resolve `Project Settings` -> `Color Management`.
2. Configure `Color Science` to `DaVinci YRGB Color Managed`.
3. Clear the `Automatic color management` checkbox and select `Custom` for the `Color processing mode`.
4. **Input Color Space**: Set to `Linear` transfer function with `ProPhoto RGB` (or `ROMM RGB`) chromatic primaries.
5. **Timeline Color Space**: Configure as `DaVinci Wide Gamut / Intermediate`. All grading operations throughout the node tree will execute within this high-dynamic-range logarithmic space.
6. **Output Color Space**: Select according to the mastering monitor target (e.g., `Rec.709 Gamma 2.4` for SDR broadcasting, or `P3-D65 ST2084 1000 nits` for HDR mastering).

### Approach B: Academy Color Encoding System (ACES) Pipeline (Recommended for Cinema Post)

1. In `Project Settings`, select `Color Science` as `ACEScc` or `ACEScct` (with `ACEScct` preferred for its smooth, photographic toe behavior in deep shadows).
2. **ACES Version**: Specify `ACES 1.3`.
3. **ACES Input Transform (IDT)**: Because Linear ProPhoto is not an embedded camera profile, set the global IDT to `No Input Transform`. Input transformation is applied explicitly via node-based Color Space Transform plugins.
4. **ACES Output Transform (ODT)**: Set to match the calibrated theatrical projection or mastering monitor standard (such as `Rec.709` or `DCI-P3 D65`).

## ACEScg Grading Node Tree & Secondary Grading Execution {#acescg-grading-node-tree}

With the color management baseline established, colorists can construct a structured node graph. The recommended node structure separates primary balancing from localized creative stylization:

```text
[Node 01: CST Input] -> [Node 02: Exposure Offset] -> [Node 03: Contrast Curve]
                                                            │
                                                            ▼
[Node 06: Output Transform] <- [Node 05: Film Grain] <- [Node 04: Secondaries]
```

Detailed operational guidelines for each node:

1. **Node 01: Input Color Space Transform (CST)**  
   Under an ACEScct project setup, load the `Color Space Transform` OFX plugin on the primary node. Set Input Color Space to `ProPhoto RGB` and Input Gamma to `Linear`. Set Output Color Space to `ACEScg` and Output Gamma to `ACEScct`. Ensure Tone Mapping is set to `None` to prevent uncalibrated compression.
2. **Node 02: Logarithmic Exposure & Printer Lights Balancing (Offset Wheel)**  
   Within ACEScct log space, adjustments made on the `Offset` color wheel correspond to additive optical filtration changes on a traditional motion picture contact printer. This control shifts the exposure linearly without introducing saturation distortion or highlight clipping.
3. **Node 03: Film Emulsion Contrast Curve (Custom Curves)**  
   Construct an S-shaped sensitometric curve emulating photographic print paper characteristics. Refine the highlight shoulder roll-off and shadow toe threshold to build optical negative depth.
4. **Node 04: Secondary Qualification & Skin Tone Isolation**  
   Utilize HSL qualifiers or 3D volumetric isolation masks to isolate subject skin tones or atmospheric sky values, purifying subtle tonal imbalances without affecting background neutrals.
5. **Node 05: Organic Texture & Halation Modeling**  
   Apply photochemically calibrated film grain or optical halation plugins to introduce physical silver halide grain texture and dye dispersion.
6. **Node 06: Output Limiting & Gamut Clamping**  
   Verify that all luminance signals conform to broadcast or theatrical delivery specifications without out-of-gamut clipping.

> [!NOTE]
> In ACEScct node operations, exposure trimming should be executed using the global Offset trackball on Node 02. Avoid using Gain or Lift wheels for primary exposure corrections, as they disrupt the proportionality between physical irradiance and color saturation.

## Master & Display Export Specification Matrix {#export-specification-matrix}

To assist post-production supervisors in selecting appropriate export formats, the table below outlines the core parameters and intended applications across the NexFilm export matrix:

| Export Specification | Bit Depth | Color Space & Gamma Transfer | Pipeline Truncation Point | Intended Production Application |
| --- | --- | --- | --- | --- |
| **16-bit Linear TIFF** | 16-bit integer | Linear ProPhoto RGB (Gamma 1.0) | Immediately following net density inversion; uncalibrated | DaVinci Resolve, Nuke, Flame cinema compositing and grading |
| **LinearRaw DNG** | 16-bit integer | LinearRaw space; embedded D50/D65 matrices | Linear positive stage with standard DNG tag encapsulation | Adobe Lightroom, Photoshop Camera Raw, Capture One workflows |
| **Raw Mosaic DNG** | Native (12/14/16-bit) | Camera native CFA Bayer / X-Trans matrix | File encapsulation only; raw samples unaltered | Long-term digital negative archiving and academic analysis |
| **8/16-bit Display TIFF** | 8 or 16-bit | sRGB / Display P3 / Adobe RGB with OETF | Full processing pipeline with tone curves and 3D LUTs | Fine art giclee printing, gallery catalogs, and publication |
| **High-Quality JPEG** | 8-bit integer | Standard sRGB (IEC 61966-2.1) | Full pipeline with perceptual tone compression | Web portfolios, editorial sharing, and client review |
