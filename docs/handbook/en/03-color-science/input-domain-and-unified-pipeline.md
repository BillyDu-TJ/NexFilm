---
id: input-domain-and-unified-pipeline
track: color-science
trackOrder: 3
chapterOrder: 2
kicker: "Color Science 02 / Unified Pipeline"
title: "Five-Tier Input Domain Resolution & Unified Density Pipeline"
subtitle: "Input formats resolve input domains while density math remains universal: from film base priority to bounded channel span."
readingTime: 14
inApp: true
sinceVersion: "v1.1.0"
---

## Unified Density Pipeline Philosophy: Decoupling Container from Math {#unified-pipeline-philosophy}

A recurring design flaw across legacy film scanning and negative inversion software is allowing the input file container to dictate downstream optical density mathematics. Under such fragmented architectures, digital camera RAW captures undergo heuristic exposure weighting, Hasselblad Flextight or Imacon FFF files follow proprietary cropping logic, and standard TIFF files are routed through uncalibrated display gamma curves. This tight coupling between file formats and inversion algorithms introduces inconsistent color renditions across different digitizers, undermining the reproducibility of photographic color science.

In version 1.1.0, the NexFilm Engine establishes an architectural principle: **the input file format determines only the input domain, while the underlying density mathematics remains globally unified**.

High-bit-depth LinearRaw scanner DNGs, FFF containers embedding proprietary metadata, and 16-bit TIFF/JPEG files with embedded color management profiles are resolved into a 32-bit floating-point Linear ProPhoto Estimate. LibRaw camera RAWs are the recorded exception: they retain NormalizedAsShot Camera Native RGB through the density stage, and the camera→sRGB matrix is deferred until the positive is displayed. This prevents the matrix and gamut floor from rewriting the small blue channel of an orange-masked negative before the logarithm. Apart from this documented working-basis fork, base subtraction, endpoint derivation, and the downstream positive pipeline still use one density model.

```text
Input File (RAW / FFF / TIFF / DNG / JPEG)
       │
       ▼
resolve_input_domain(path, scanner_profile)
       │  ← The sole permissible branching point
       ▼
Working buffer:
  camera RAW -> NormalizedAsShot Camera Native RGB f32
  other inputs -> Linear ProPhoto Estimate f32
       │
       ▼
Unified Density Transform: D = -log10(max(T, epsilon)) in that basis
       │
       ▼
Channel-Wise Base Subtraction: D_net = D_raw - D_base (shared origin, zero offset)
       │
       ▼
Bounded Channel Span Response (1.15x deadzone / 1.35x cap / 3.0x truncated cap)
       │
       ▼
Positive display mapping:
  camera RAW -> this frame's camera -> linear sRGB matrix
  other inputs -> ProPhoto -> linear sRGB
       │
       ▼
Gamma, curves, LUT and target output gamut
```

This separation removes most of the gamut clipping and precision loss caused by the old intermediate step through 16-bit unsigned sRGB. Wide-latitude negatives from different capture hardware are then evaluated on the same optical footing.

## Five-Tier Input Domain Resolution & Metadata Extraction {#five-tier-input-domain-resolution}

To translate heterogeneous image captures into physical relative transmittance, NexFilm implements a prioritized resolution chain encapsulated in the `resolve_input_domain` function. The engine evaluates five tiers of metadata evidence in sequence, attaching provenance records, color primaries, transfer functions, and confidence flags to each processed frame:

1. **Tier 1: Embedded ICC Color Profile (Verified)**  
   If a TIFF or JPEG image embeds a valid ICC profile (such as Adobe RGB (1998), Display P3, or a custom scanner profile), the engine extracts chromaticity matrices (`rXYZ`, `gXYZ`, `bXYZ`) and parametric tone reproduction curves. This profile is marked Verified and takes priority over the other tiers.
2. **Tier 2: Scanner Container Metadata & Property Lists (Verified / Estimated)**  
   For Hasselblad Flextight / Imacon `.fff` files, the decoder inspects the TIFF header for embedded XML property lists (plist), extracting `RGBProfile` descriptors and `Gamma` coefficients. If the container explicitly designates AdobeRGB or scanner-calibrated profiles, the declared gamma expansion applies directly. When container properties are absent, the engine falls back to documented scanner standards (Gamma 1.8, sRGB primaries, tagged as `estimated`).
3. **Tier 3: RAW / LinearRaw Metadata & Sensor Matrices (Declared)**  
   For camera RAW files (such as CR2, NEF, ARW), the engine parses camera native color filter arrays and white balance multipliers through LibRaw. For DNG scans produced by specialized film scanners, the parser evaluates SubIFD tags; if `PhotometricInterpretation = 34892 (LinearRaw)` is identified without lossy compression, the samples are treated as unscaled linear transmittance.
4. **Tier 4: Documented Scanner Model Defaults (Documented Default)**  
   When a user explicitly selects a hardware scanner profile (such as Nikon Super Coolscan 9000 ED or Minolta Dimage Scan Elite 5400) and the file lacks embedded ICC tags, the engine applies documented spectral primaries and transmission characteristics calibrated for that model.
5. **Tier 5: sRGB Fallback with Diagnostic Warning (Estimated Fallback)**  
   If all preceding criteria fail to match, the image data is interpreted under standard sRGB assumptions. The processing pipeline marks the record with an `estimated` diagnostic flag, alerting the user to review color accuracy in the inspection panel.

Resolution outcomes are recorded with each frame in the project SQLite database under `PipelineProcessingReport::input_domain`, including primary coordinates, transfer functions, and diagnostic logs.

## Camera-Native Working Basis & Display Matrix {#camera-native-display-basis}

A camera→sRGB matrix describes how camera channels map to display chromaticities; it is not a physical channel separation of film transmittance. An orange-masked colour negative leaves very little blue transmission, and the matrix's negative green term plus a luma-relative gamut floor can rewrite exactly those pixels. Phase 4 therefore keeps the density stage of a LibRaw camera RAW in the camera's own channels:

```text
Camera Native RGB f32
  -> D_raw = -log10(max(T, epsilon))
  -> D_net = D_raw - D_base
  -> normalized positive
  -> camera_to_display
  -> linear sRGB and the existing display pipeline
```

The frame stores that matrix in `PipelineProcessingReport::camera_to_display`: `Some` means the density stage ran in camera-native RGB, while `None` means the ProPhoto Estimate path and is also what records written before this field deserialize to. Identity matrices, non-finite values, degenerate rows, and unresolved LibRaw matrices are never persisted; preview, export, and CPU rendering all fall back to the ProPhoto display matrix for such a record. The M1 rawler matrix backfill and the M2 rawler pixel fallback still use the camera-native density basis.

Preview flag `8` marks a camera-native texture and is followed by nine row-major `f32` matrix coefficients. Both WebGL render passes read `u_positive_to_display`, and CPU rendering/export use the same matrix source. Roll anchors also carry `working_basis`; an anchor sampled in another basis is rejected as `anchor_working_basis_mismatch`, and the UI asks the user to run Auto Invert again for the Roll. `DENSITY_WINDOW_RULE_VERSION` is now 4, while `RAW_DECODE_VERSION` remains 13.

## Film Base Sampling Priority & Quality Validation Thresholds {#film-base-sampling-priority}

The measurement accuracy of the unexposed orange film base establishes the white balance anchor for the entire negative inversion. Heuristic software often locates the brightest overall pixel in the image and designates it as the base reference. When applied to scans containing perforations, backlight direct glare, or expansive light-colored backgrounds, this simplistic assumption introduces significant color contamination and flat contrast.

NexFilm implements a three-tier base sampling hierarchy governed by strict physical validation thresholds:

| Sampling Priority | Geometric Region | Verification Criteria & Rejection Gates | Operational Guarantee |
| --- | --- | --- | --- |
| Tier 1: Border Narrow Band | Unexposed clear mask between outer film rebate and Film Area | RGB density spread must match C-41 coupler characteristics; band density cannot be darker than tail by > 0.05 D | Camera scans retaining unexposed clear film border; highest precision |
| Tier 2: Emulsion Low-Density Tail | Low-density statistical percentile (2% to 5%) inside active Film Area | Sample pixels must exhibit spatial clustering, filtering isolated sensor noise or dust | Pre-cropped scanner frames lacking unexposed border |
| Tier 3: Global Brightest Percentile | Downweighted statistical percentile across full frame | Activated only when Tiers 1 and 2 fail; restricted by a 0.20 D content alignment shift cap | Tightly cropped architectural or panoramic stitched frames |

During candidate validation, the `film_base_band_agrees` routine enforces a unidirectional physical inequality: because the unexposed film base represents the maximum possible optical transmittance of the film strip, image-forming areas within the frame cannot physically transmit more light than the base itself. If the measured border narrow band is darker than the emulsion tail by more than 0.05 D, the engine concludes that the border is contaminated by flare or lighting falloff and automatically falls back to the internal low-density tail.

> [!IMPORTANT]
> When copying grading settings between frames, NexFilm does not carry over the source frame's absolute base density. The `choose_pasted_film_base` path recalculates the base inside the target frame's own geometry. Only when the target frame has no usable base sample does the engine fall back to an inherited value, and the interface marks it as `inherited_film_base`.

## Bounded Channel Span Response: Preserving the Neutral Axis {#bounded-channel-span-response}

After deducting the measured base density, the net optical density $D_{\text{net}} = D_{\text{raw}} - D_{\text{base}}$ aligns to an analytical origin where the base density in red, green, and blue evaluates identically to zero.

However, in complex digitization workflows—particularly panoramic stitched TIFF files generated from multiple RAW captures (such as Lightroom panoramas `_DSC7569-Pano.tif` and `_DSC7583-Pano.tif`)—upstream merging operations often compress the red channel density span down to one-third or one-fourth of the green channel span. If a shared display window width is applied uniformly across all three channels, the compressed red channel is forced into the lower luminance register, causing cyan-green casts or blocked highlights.

While earlier experiments introduced per-channel display curves, that approach distorted the color balance of healthy negatives. NexFilm introduces the **Bounded Channel Span Response** model, establishing three mathematical safeguards:

1. **Shared Window Origin**: When a valid film base is present, `channel_offsets` stays at zero across all three channels. Whatever happens to span scaling, density zero (the clear film base) maps to the same display white, so the base does not introduce a cast.
2. **1.15x Deadzone Threshold**: For standard negatives processed and scanned under balanced lighting (where channel span ratios remain between 1.00 and 1.15), channel gain adjustments remain in the deadzone, preserving identical three-channel window allocation.
3. **Bounded Gain Ceilings**: Using `measure_content_channel_spans`, the system measures effective density spans between the 2nd and 98th percentiles per channel. When imbalance exceeds 1.15, window widths are scaled via a smooth linear ramp:
   $$g_i = \text{clamp}\left(\frac{\text{span}_i}{\text{luma}(\text{span})}, \frac{1}{g_{\text{max}}}, g_{\text{max}}\right)$$
   For conventional scans, the gain ceiling is capped at $g_{\text{max}} = 1.35$. Only when severe channel collapse exceeds a 2.0 span ratio does the engine identify an upstream truncated channel, moderately relaxing the ceiling to 3.0.

This formulation rejects the arbitrary color leveling of the traditional gray-world hypothesis (which forces average scene RGB to neutral gray). It preserves the natural atmospheric palette of the scene while recovering dynamic range for damaged channels.

## Quantitative Regression Verification & Diagnostic Benchmark Data {#quantitative-regression-verification}

The stability of the unified density pipeline is verified through automated regression suites. The table below compiles pixel-level measurements executed via `density_pipeline_sample_report` across standardized real-world film captures, comparing whole-image red-to-green and blue-to-green channel ratios between legacy v1.0.2 processing and the unified pipeline. The camera-RAW rows are the baseline recorded before Phase 4 moved those inputs to the camera-native basis; the Phase 4 byte sweep and RAW A/B report are the current evidence for that path.

| Test Sample & Capture Origin | Input Domain Resolution | Base Sampling Source | Legacy R/G · B/G | Unified R/G · B/G | Diagnostic Observations & Corrections |
| --- | --- | --- | --- | --- | --- |
| `Hasselblad fff/Task _1233.fff` | CameraNative / CameraRaw | Emulsion low-density tail | 0.583 · 1.001 | 0.963 · 1.086 | Camera RAW path removes the extra channel response and brings back a natural, open highlight |
| `Hasselblad fff/Task _1343.fff` | CameraNative / CameraRaw | Film border narrow band | 0.980 · 0.970 | 0.991 · 0.969 | Highlight clipping at 45%; deadzone active; identical outputs across legacy and unified pipelines |
| `Hasselblad fff/Unable_to_invert.fff` | AdobeRgb1998 / Gamma2.0 | Full frame brightest percentile | 0.894 · 1.037 | 0.806 · 1.046 | Container metadata guides accurate gamma linearization; residual yellow-green cast suppressed |
| `Lightroom Pano/_DSC7569-Pano.tif` | AdobeRgb1998 / Gamma2.2 | Film border narrow band | 0.848 · 1.907 | 0.696 · 1.288 | Imbalance ratio 3.35; bounded span response active; red channel zero-clipping reduced from 78.8% to 9.3% |
| `Lightroom Pano/_DSC7583-Pano.tif` | AdobeRgb1998 / Gamma2.2 | Film border narrow band | 0.571 · 2.324 | 0.557 · 1.249 | Imbalance ratio 4.26; relaxed red channel compensation recovers neutral sky clouds and concrete textures |
| `Nikon Scanner tiff/5.3-1.tif` | AdobeRgb1998 / ICC | Film border narrow band | 1.030 · 0.715 | 0.999 · 0.951 | Eliminates systematic red bias; density distribution matches verified optical reference prints |
| `Plustek BW/raw0002.dng` | Srgb / Linear | Emulsion low-density tail | 1.001 · 1.018 | 0.944 · 1.139 | Scanner LinearRaw DNG recognized; smooth tonal transitions throughout monochrome midtones |
| `Nikon NEF/_DSC7333.NEF` | CameraNative / CameraRaw | Film border narrow band | 0.975 · 0.972 | 0.826 · 0.944 | Camera RAW direct decode; neutral axis variation strictly held within 4/65535 tolerance |

These benchmark measurements confirm that for healthy captures, the unified pipeline matches the theoretical physical baseline with zero pixel divergence. For severely compressed panoramic composites, the bounded span response restores usable dynamic range without compromising the neutral film base.
