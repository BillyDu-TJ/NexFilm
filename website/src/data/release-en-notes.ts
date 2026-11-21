/**
 * English Release Notes translations for historical releases.
 * For future releases without manual translation mappings or <!-- en --> splitters,
 * the system will automatically fall back to the original release notes with an indicator.
 */

export const RELEASE_EN_NOTES: Record<string, string> = {
  'v1.2.1': `From a feature standpoint, **v1.2.1** adds no new editing panels—its user-facing focus is **decoding stability, broader source compatibility, film-base consistency, and clear error diagnostics**. Beneath the surface, however, it represents a substantial internal overhaul of direct RGB linearization, LinearRaw/merged DNG decoding, and film-base anchoring so demanding real-world scans and camera captures run reliably through one unified physical pipeline.

### Highlights & Engineering Improvements
- **Direct RGB & Scanned JPEG / PNG / TIFF Straight to ProPhoto f32**: TIFF, JPEG, and PNG inputs bypass the legacy 16-bit linear-sRGB transport and luminance chroma compression, linearizing directly into \`f32\` ProPhoto RGB. Paired with real ICC chromaticity/TRC decoding via \`moxcms\` (including linear TRC guards and EOTF fixes), \`f32\` / \`f64\` float TIFF preservation, EXIF orientation handling, and explicit memory budget checks, **standard scanner JPEGs and borderless scans now invert cleanly out of the box in v1.2.1**.
- **Full LinearRaw & Lightroom Stitched / Merged DNG Support**: Handles both IFD0 and SubIFD LinearRaw layouts, decodes compressed LinearRaw frames through \`rawler\`, executes DNG Stage-2 \`OpcodeList2\` (\`MapTable\` / \`MapPolynomial\`) inverse tone mapping prior to cropping or downsampling, and resolves linear color via embedded ICC or dual-illuminant \`ColorMatrix1/2\` + \`AsShotNeutral\`—fixing color casts, highlight distortion, and decode failures on stitched panoramas and merged DNGs.
- **Unified Modern Decode Entry & Film-Base State Separation**: Unifies the modern decode path shared by preview proxies, Roll anchor sampling, Contact Sheets, and batch export (\`RAW_DECODE_VERSION = 15\`, \`DENSITY_WINDOW_RULE_VERSION = 6\`). Clearly separates \`Trusted\` bases (strictly anchored at \`d_min = 0\`) from \`ContentFallback\` (preserving per-channel lows without blowing highlights), invalidating affected legacy caches and Roll anchors cleanly on upgrade.
- **Canon \`.CR2\` Preview Extraction & 135 Sprocket Border Recovery**: Tightens embedded JPEG preview probing to 8-bit 1/3-channel baseline/progressive JPEGs and skips raw slice (\`0xC640\`) or CFA/LinearRaw IFDs so high-resolution \`.CR2\` files no longer mistake 50+ MB lossless sensor slices for thumbnails; adds inward top/bottom border recovery on horizontal \`3:2\` 135 scans so framing does not lock onto outer sprocket edges.
- **Degraded-Pipeline Warnings & Copyable Diagnostics**: Surfacing clear stage-specific errors and one-click **Copy details** diagnostics (\`NexFilm v1.2.1 | stage=...\`) in Toast notifications for ICC parse failures, degraded camera matrices, DNG/scanner metadata fallbacks, calibration fallbacks, and import / auto-invert / film-area / Develop / batch-export failures.

### Upgrade Notice
- Because \`v1.2.1\` updates direct RGB decoding and unified density window rules, affected caches and Roll anchors refresh on open; re-run **Auto Invert** if an existing roll prompts for anchor recalibration.

### Known Limitations
- Hardware calibration features (Capture Corrected, Capture Characterized, Scanner Input Profiles) remain experimental.
- Very large JPEG/PNG files remain subject to explicit decoder memory ceilings; 16-bit TIFF or DNG is still recommended for ultra-high-resolution archives.
- Full-resolution export of massive scans requires sufficient free RAM.
- Nikon proprietary **HE/HE*** compressed NEF files remain unsupported by upstream open-source decoders (LibRaw / \`rawler\`).`,

  'v1.1.0': `### What's New
- **Unified Density Pipeline**: Input domain (embedded ICC, scanner container tags, RAW metadata, model recognition, or sRGB fallback) is parsed per frame. Inversion math is strictly unified: per-channel film base subtraction → optical density domain → shared window origin → bounded channel span response. Normal materials remain completely consistent.
- **Roll-wide White Point & Batch Inversion**: Calibrate a shared white point from the brightest frame across an entire roll. Frames sampled for film base or leader are marked as calibration frames, automatically excluded from Develop, white point measurement, and batch inversion, with skipped count reported in progress.
- **Hardware Calibration (Experimental)**: New Hardware Calibration view and profile management to capture dark / open gate / flat / transmission targets bound to rolls. Capture Corrected enables dark/open correction in pixel math; Capture Separation 3×3 can be fitted from measured color targets, gracefully falling back to Smart Auto with descriptive reason if constraints fail.
- **Scanner Input Profiles**: Import local scanner profiles and bind to rolls, affecting both live preview and master export.
- **New Master Delivery Formats**: 16-bit linear TIFF, linear DNG, and untouched camera RAW DNG container encapsulating original scan data.
- **Roll Notes & Built-in Documentation**: Roll notes tracking and 9 comprehensive offline bilingual documentation chapters.

### Fixes & Improvements
- **Line-by-line Streaming Scan Decoder**: Hundreds of MB to GB-sized FFF, scanner TIFF, and linear DNG files now open smoothly; fixes the issue where large scans rendered in Develop but failed during full export.
- \`.fff\` recognition no longer relies on device strings from FlexColor, preventing Imacon/Flextight scans from being misidentified as Hasselblad camera RAWs (the root cause of import failures in 1.0.2).
- Added geometry and file length consistency checks in the streaming TIFF reader, failing gracefully with clear errors instead of crashing the process.
- Inversion adjustments: Low-key images preserve original tonal character; borders, backlight margins, and highlight clip are excluded from window endpoints; inherited film base is re-measured on current frame pixels.
- Loose import and uncalibrated roll views no longer diverge; RAW is no longer decoded during import, generating bounded proxies only upon entering Develop.
- UI: Develop inspector reorganized with new Contrast control and tightened Exposure/Highlights/Shadows ranges; Crop and Film Area now share unified frame coordinates; switching frames no longer drops masks.

### Upgrade Notice
- After upgrading, please run **Auto Invert** once on existing rolls to refresh thumbnail caches rendered with legacy density formulas.

### Known Limitations
- Hardware calibration features (Capture Corrected, Capture Characterized, Scanner Profiles) remain experimental.
- Extremely large JPEG/PNG files are constrained by decoder memory limits; TIFF or DNG is recommended.
- Full-resolution export of massive scans requires sufficient free RAM.
- Nikon Z8 HE/HE* high-efficiency RAW files cannot be imported yet due to LibRaw upstream support.`,

  'v1.0.2': `### Fixes & Improvements
- Optimized camera RAW pipeline: extracts embedded preview during import, creates bounded 16-bit linear proxy on demand in Develop, reusing the same proxy for auto inversion.
- Fixed occasional failure of auto inversion on Intel Macs when RAW proxy was not yet ready.
- Fixed thumbnail container distortion in film strip after rotation, and desynchronization between crop mask and zoom/pan.
- Adjusted slider wheel interaction: requires clicking the slider before responding to wheel scroll.

> Nikon Z8 HE/HE* high-efficiency RAW files are currently not supported by LibRaw.`,

  'v1.0.1': `### New Features
- Added mouse wheel adjustment support.
- Added barrel and pincushion distortion correction.
- Added crop aspect ratio presets and preview quality options.
- Deleted files are now moved to system trash instead of permanent deletion.

### Fixes & Improvements
- Fixed import failures for massive files (such as Hasselblad FFF).
- Fixed toast notification dialogs being occluded.
- Improved application state consistency, workflow transitions, and image rotation.
- Fixed pagination and deletion issues on the Rolls page.

> Nikon Z8 HE/HE* high-efficiency RAW files are currently not supported by LibRaw.`,

  'v1.0.0': `### New Features
- Modern film conversion desktop workbench built with Tauri 2 + Rust + WebGL.
- Supports camera RAW digitizations (LibRaw) and scanner TIFF image imports with roll library management.
- Unified transmittance domain and optical density physics algorithms, achieving high-fidelity base subtraction and logarithmic inversion.
- RGB darkroom printer lights, Master and 4-channel point curves, real-time high-precision histogram and waveform scopes.
- Built-in authentic Kodak and Ilford print film and paper 3D LUT simulations.
- Master-grade delivery export in 8/16-bit TIFF, JPEG, PNG, and Contact Sheet layouts.`
};
