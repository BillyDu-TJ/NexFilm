---
id: hardware-calibration-guide
track: color-science
trackOrder: 3
chapterOrder: 3
kicker: "Color Science 03 / Hardware Calibration"
title: "Hardware Calibration Principles & Experimental Workflow Guide"
subtitle: "Dark frame subtraction, flat-field normalization, and target fitting across three operational modes."
readingTime: 14
inApp: true
sinceVersion: "v1.1.0"
aliases: ["hardware-calibration"]
---

## Three Operational Modes & Calibration Hierarchy {#three-operational-modes}

In film scanning and camera digitization, hardware environments and precision requirements vary significantly across users. To balance operational convenience with radiometric rigor, NexFilm categorizes its digitization workflows into three distinct operational tiers. Each mode carries explicit physical meaning and defined operational boundaries within the processing pipeline:

| Operating Tier | Calibration Prerequisites | Algorithmic Basis & Mathematics | Intended Application & Accuracy |
| --- | --- | --- | --- |
| `Smart Auto (ProPhoto Estimate)` | Single film capture; zero reference frames required | Heuristic film base sampling, global density window analysis, and Linear ProPhoto projection | Rapid daily inversion, uncalibrated loose frames, and commercial scanner files |
| `Capture Corrected` | Dark Frame and Open Gate Flat Field | Per-pixel thermal dark bias subtraction and open-gate radiant transmittance normalization | Advanced camera scanning; eliminates vignetting and backlight chromaticity drift |
| `Capture Characterized` | Dark Frame, Flat Field, and Transmission Target | SVD pseudo-inverse fitting of a regularized $3 \times 3$ capture separation matrix | Laboratory scanning rigs requiring formal decoupling of sensor dyes and illumination |

Under `Smart Auto`, the pipeline assumes an ideal linear sensor response and derives the clear base density and dynamic range window directly from statistical cues within the frame. This path requires minimal setup and satisfies general photography needs.

When switched to `Capture Corrected`, the pipeline operates strictly in the pre-log transmittance domain. By measuring dark current and the bare illumination field, it executes per-pixel radiometric normalization. In the highest tier, `Capture Characterized`, the system builds upon flat-field normalization to calculate a capture separation matrix tailored to the specific sensor color filter array and backlight spectral power distribution.

## Physics of Dark and Flat-Field Correction & Quality Gates {#dark-and-flat-field-correction}

In camera scanning systems, raw sensor readings rarely represent pure optical transmittance through the film emulsion. Solid-state sensors contribute fixed pattern noise, thermal dark current, and read bias. Internal lens barrels and scanning hoods reflect stray flare. Furthermore, macro optics suffer from cosine-fourth cos4 illumination falloff and optical vignetting, while diffused acrylic light panels often display measurable spatial non-uniformity in luminance and correlated color temperature.

The `Capture Corrected` pathway removes these systematic capture artifacts through two empirical reference acquisitions:

1. **Dark Frame**: Recorded with all incident illumination blocked, isolating sensor dark current, thermal noise, readout bias, and potential light leaks within the camera chamber.
2. **Open Gate Flat Field**: Recorded with the film carrier completely empty under identical scanning exposure, mapping the two-dimensional spatial irradiance of the backlight and the geometric falloff of the lens.

Denoting raw sample readings as $I_{\text{sample}}(x, y)$, dark frame values as $I_{\text{dark}}(x, y)$, and open-gate readings as $I_{\text{open}}(x, y)$, the normalized relative transmittance is evaluated per pixel:

$$T(x, y) = \frac{I_{\text{sample}}(x, y) - I_{\text{dark}}(x, y)}{I_{\text{open}}(x, y) - I_{\text{dark}}(x, y)}$$

In the low-level processing core (`src/raw_backend.rs`), NexFilm implements rigorous numerical safety inspections. The engine rejects arbitrary epsilon offsets that conceal corrupt data, diagnosing samples via explicit `QualityFlags` bitmasks:

- **Invalid Denominator (`InvalidDenominator`)**: Flagged when open-gate irradiance minus dark bias falls below the numerical safety threshold, avoiding divide-by-zero errors.
- **Negative Sample (`NegativeSample`)**: Triggered when recorded sample irradiance falls below the dark baseline, indicating lighting fluctuations or camera movement.
- **Sensor Saturation (`SaturatedSample` / `SaturatedReference`)**: Identified when sample or reference values reach the analog-to-digital converter saturation threshold (such as 16383 for 14-bit converters).
- **Metadata Mismatch (`GeometryMismatch` / `ExposureMismatch` / `IsoMismatch`)**: Detected when calibration frames differ in dimensions, ISO sensitivity, exposure time, or white balance from the active negative capture.

> [!NOTE]
> The open-gate flat field must be acquired at the identical lens aperture, focal length, and reproduction ratio as the negative scans. Changing the aperture diaphragm shifts the entrance pupil and alters vignetting profiles, compromising field correction.

## Transmission Target Sampling & CaptureSeparation3x3 Fitting {#capture-characterized-fitting}

In `Capture Characterized` mode, the objective is solving the `CaptureSeparation3x3` matrix to align digitized RGB transmittance with the physical absorption bands of the cyan, magenta, and yellow dye couplers. This mathematical fitting is conducted within the `src/calibration_fit.rs` module.

The system utilizes an optically certified transmission calibration target (such as an IT8.7/1 transmission target or Kodak Q-60 film step wedge). Suppose the target contains $N$ measured transmission patches, where $y_k \in \mathbb{R}^3$ denotes the calibrated spectrophotometric transmittance vector of the $k$-th patch, and $x_k \in \mathbb{R}^3$ represents the digitized relative transmittance recorded by the scanning system.

We seek an optimal $3 \times 3$ channel separation operator $M$ that minimizes the sum of squared residuals while incorporating a ridge regression penalty to maintain well-conditioned matrix behavior:

$$\min_M \sum_{k=1}^N \| y_k - M x_k \|_2^2 + \lambda \| M - I \|_F^2$$

where $I$ denotes the $3 \times 3$ identity matrix and $\lambda$ represents the Tikhonov regularization scalar. Setting up the data matrices $X \in \mathbb{R}^{N \times 3}$ and $Y \in \mathbb{R}^{N \times 3}$, the regularized normal equations are solved via Singular Value Decomposition (SVD):

$$M^T = (X^T X + \lambda I)^{-1} X^T Y$$

To maintain stability across varied hardware setups, `calibration_fit.rs` enforces three defensive gates:

1. **Minimum Patch Count**: A valid measurement set requires at least 4 independent patches (`MIN_PATCHES = 4`) spanning low, medium, and high optical densities.
2. **Condition Number Guard**: The spectral condition number $\kappa(X) = \sigma_{\text{max}} / \sigma_{\text{min}}$ must remain below $1.0 \times 10^5$ (`MAX_CONDITION_NUMBER`). If narrow-band illumination or collinear patches induce an ill-conditioned system, the optimizer aborts and reverts safely to the identity transform.
3. **Validation Residual Ceiling**: The root-mean-square error (RMSE) evaluated across withheld validation patches must not exceed 0.08 (`MAX_VALIDATION_RMSE`). Discrepancies above this limit indicate external stray light contamination or patch misalignment, rendering the calibration untrustworthy.

## Darkroom Digitization Standard Operating Procedure (SOP) {#darkroom-calibration-sop}

To ensure that calibration assets deliver repeatable accuracy across entire scanning sessions, technicians should observe the following standard darkroom procedure:

1. **Phase 1: Illumination Thermal Stabilization**  
   Energize the high-CRI backlight panel (recommended CRI ≥ 95, TLCI ≥ 98) at least 20 minutes prior to calibration. This warmup period allows the LED emitter junctions to achieve thermal equilibrium, preventing correlated color temperature and luminance drift during scanning. Darken the room completely to eliminate ambient flare.
2. **Phase 2: Mechanical Alignment & Optical Lock**  
   Secure the digitizing camera to a rigid copy stand. Using a two-axis spirit level or mirror reflection target, confirm that the camera sensor plane is parallel to the film carrier stage. Set the macro lens to its optimum aperture range (typically f/5.6 to f/8). Lock the focus barrel and disable optical and sensor stabilization systems.
3. **Phase 3: Dark Frame Acquisition**  
   Fasten the opaque lens cap and shield the camera optical viewfinder to prevent rear light ingress. Set the camera to base sensitivity (ISO 64 or ISO 100) and manual exposure mode (M), matching the shutter speed anticipated for the negatives (such as 1/15 s). Record three consecutive frames, average them to reduce random noise, and store the result in raw format.
4. **Phase 4: Open Gate Flat-Field Acquisition**  
   Remove the lens cap and clean the carrier glass or mask. With the film holder empty, preserve identical optical focus and aperture settings. Adjust shutter speed or backlight intensity so that the illumination histogram sits comfortably between 70% and 85% of maximum scale (ETTR without clipping any channel). Capture and archive the raw file.
5. **Phase 5: Transmission Target Acquisition**  
   Position the transmission calibration target (such as a 35mm or 120 format IT8.7/1 slide) within the carrier. Verify that all target patches lie within the uniform field of view. Mask off all exposed light panel areas surrounding the target frame with black cardstock to eliminate veiling glare. Record the capture in raw format.

Within the NexFilm interface, navigate to the `Hardware Calibration` workspace. Import the three raw acquisitions in sequence. The engine processes the flat-field map and calculates the separation matrix within seconds, generating a verified profile stamped with an internal SHA-256 integrity hash.

## Experimental Boundaries & Technical Disclosures {#experimental-boundary-disclosure}

Scientific integrity requires distinguishing between verified physical implementations and exploratory models. The NexFilm engineering team maintains full transparency regarding current capabilities:

1. **Radiometric Transmittance Normalization is Verified**: Dark subtraction and open-gate field normalization under `Capture Corrected` rest upon classical radiometric optics. These steps have undergone end-to-end regression testing and eliminate illumination non-uniformity.
2. **Target Fitting Separates Capture Hardware, Not Film Couplers**: The regularized $3 \times 3$ matrix calculated under `Capture Characterized` isolates sensor filter overlaps and backlight spectral features. It does not invert the chemical sensitometric response of photographic emulsions.
3. **No Embedded Status M Filter Integration Library**: Because commercial camera filter arrays differ substantially from physical Status M densitometer response curves, the internal `status_m_crosstalk_matrix()` is retained solely as an empirical reference for legacy sessions. NexFilm does not claim to function as a certified laboratory densitometer.
4. **Hardware Profiles are Setup-Specific**: Generated calibration profiles remain valid only for the exact camera body, lens, working aperture, and illumination panel used during acquisition. Modifying the reproduction ratio or changing backlight color temperature requires recalculating the profile.
