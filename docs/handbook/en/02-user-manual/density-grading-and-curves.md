---
id: density-grading-and-curves
track: user-manual
trackOrder: 2
chapterOrder: 4
kicker: "02 / User Manual"
title: "Density Limits, Darkroom Printer Lights, Tone Curves, and Detail Enhancement"
subtitle: "Mastering D-Min/D-Max physical density bounds, logarithmic printer light principles, monotonic spline curves, and high-frequency detail preservation."
readingTime: 14
inApp: true
sinceVersion: "v1.1.0"
aliases: ["develop-workspace"]
---

## Film Modes, Auto Inversion, and Whole-Roll Batch Inversion {#mode-invert-and-batch}

NexFilm's darkroom inversion engine physically decouples the unique chemistry of monochrome emulsions from multi-layer color couplers. Upon entering the Develop workspace, the mode selector and inversion controls at the top of the inspector govern how raw negative transmission data maps into positive print tones:

- **Color Mode**:
  Engineered specifically for chromogenic color negative films featuring integral orange dye couplers. The engine engages a three-channel logarithmic decoupling pipeline: it calculates unexposed film base density, shifts this reference value to zero density, and reconstructs the positive image using spectral dye uncoupling matrices.
- **B&W Mode**:
  Switching to B&W collapses three-channel RGB data into a single optical density channel and turns off color mask subtraction. This mode is made for silver gelatin emulsions such as Ilford HP5 Plus, Kodak Tri-X, and Fomapan, and it avoids the color stains that can appear on black-and-white negatives when the color channels are balanced independently.

The Actions panel provides three primary triggers:
1. **Auto Invert**:
   Targets the active individual frame. The solver analyzes density distributions strictly within the designated Film Area, measures localized film base offsets, calculates optimal density boundaries, and commits all resulting color and curve parameters to the frame history.
2. **Auto Invert Roll**:
   This option remains hidden or disabled by default. **It unlocks exclusively after completing Calibrate Film Base / Leader within the Library grid**. Because the active roll possesses authentic physical anchors for both the unexposed base and the exposed leader, Auto Invert Roll measures the brightest frame across the entire roll as a unified highlight anchor. It then processes all remaining frames under identical physical baselines, skipping calibration references to prevent intra-roll density shifts.
3. **Reset**:
   Clears all color balancing, printer light offsets, tone curves, and detail parameters on the active frame, returning the preview to its raw negative state. Reset alters only color pipeline parameters, leaving your custom Film Area polygon and perspective crop untouched.

## Density Limits (D-Min / D-Max) Physical Meaning and Calibration {#density-limits}

In classical sensitometry, optical density ($D$) measures the logarithmic light-stopping power of an emulsion layer:

$$D = -\log_{10}(T)$$

where $T$ represents optical transmittance. Clear, unexposed negative film base permits maximum light passage, defining minimum optical density ($D_{\min}$). Conversely, heavily exposed and fully developed silver or dye clouds absorb substantial light, establishing maximum optical density ($D_{\max}$). Inverted into a positive image, $D_{\min}$ governs deep shadow foundation density, while $D_{\max}$ defines the highlight ceiling.

The **Density Limits** module provides direct control over these physical endpoints:
- **Master D-Min (Range: -0.50 to +0.50)**:
  Controls the threshold of printable positive black. Advancing Master D-Min toward positive values shifts shadow density thresholds deeper into rich, solid blacks. Moving toward negative values lifts shadow compression, recovering subtle shadow details and analog texture.
- **Master D-Max (Range: -0.50 to +0.50)**:
  Regulates the highlight shoulder compression. Moving positive extends highlight dynamic capacity, safeguarding bright cloud textures from digital clipping. Compressing Master D-Max negative tightens overall print contrast, producing punchier tones.
- **Three-Channel Density Readouts**:
  The digital indicators below the sliders display the actual measured physical densities across Red, Green, and Blue channels. Disparities among channel spans highlight emulsion aging or underexposure, allowing targeted per-channel endpoint corrections.

## Darkroom Printer Lights Logarithmic Color Balance Laws {#printer-lights-laws}

In an optical darkroom, color printing utilizes dichroic filters inside an enlarger head—Cyan, Magenta, and Yellow—to modulate the color quality of light striking photographic paper. The **Printer Lights** module in NexFilm mathematically models this physical enlarger behavior within the digital density domain.

Unlike standard RGB linear gain sliders found in general photo editors, NexFilm's printer lights operate logarithmically on optical density. Adjusting printer lights mimics changing the exposure duration and filtration balance through dichroic glass filters.

The lookup table below outlines color cast diagnostics and printer light correction pathways:

<table class="doc-table">
    <thead>
        <tr><th>Observed Positive Color Cast</th><th>Underlying Physical Imbalance</th><th>Recommended Printer Light Action</th><th>Darkroom Enlarger Equivalent</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Overall Cyan Tint</strong></td><td>Insufficient red channel transmission density; red wavelength deficiency.</td><td><strong>Increase Red Printer Light (+)</strong></td><td>Reduce cyan filtration in enlarger head to admit more red light.</td></tr>
        <tr><td><strong>Overall Red Tint</strong></td><td>Excessive red transmission; inadequate cyan absorption.</td><td><strong>Decrease Red Printer Light (-)</strong></td><td>Increase cyan filtration to absorb excess red wavelengths.</td></tr>
        <tr><td><strong>Overall Magenta Tint</strong></td><td>Green transmission obstructed; excess magenta couplers.</td><td><strong>Increase Green Printer Light (+)</strong></td><td>Reduce magenta filtration to inject more penetrating green exposure.</td></tr>
        <tr><td><strong>Overall Green Tint</strong></td><td>Excess green transmission; magenta dye overcompensation.</td><td><strong>Decrease Green Printer Light (-)</strong></td><td>Increase magenta filtration to absorb surplus green light.</td></tr>
        <tr><td><strong>Overall Yellow Tint</strong></td><td>Blue transmission attenuated; excess yellow dye baseline.</td><td><strong>Increase Blue Printer Light (+)</strong></td><td>Reduce yellow filtration to allow more blue exposure onto paper.</td></tr>
        <tr><td><strong>Overall Blue Tint</strong></td><td>Surplus blue transmission; yellow component washed out.</td><td><strong>Decrease Blue Printer Light (-)</strong></td><td>Increase yellow filtration to absorb excess blue, warming the image.</td></tr>
    </tbody>
</table>

The upper right corner houses the **WB Dropper** tool. Activating the dropper and clicking an area representing neutral grey (such as unshaded asphalt, weathered concrete, or neutral fabric) calculates the spectral deviation from neutrality. The engine converts this delta into exact logarithmic exposure offsets across Red, Green, and Blue channels, adjusting the printer light sliders without altering base calibration data.

## Tone Curves and Detail Enhancement {#curve-and-detail}

In the rendering sequence, **Tone Curves** and **Detail** occupy precise positions: curves operate downstream of printer lights and before global look adjustments, while detail enhancement operates at the end of the chain before final export serialization. Neither affects raw density calculations.

### 1. Four-Channel Monotonic Spline Curves
NexFilm features a complete four-channel curve editor consisting of a composite **RGB Master Curve** alongside independent **Red, Green, and Blue Channel Curves**.
- **Monotonic Spline Interpolation**: Curves use monotonic cubic Hermite splines. No matter how close the control points are, the curve stays monotonic, so highlight dips, dark-end wobble, and out-of-range oscillations do not appear.
- **Interaction Guidelines**:
  - Click on empty curve space to insert a control point (up to 16 points per channel);
  - Right-click any control point to delete it;
  - Boundary endpoints are locked horizontally at 0 and 1, moving vertically to lift black points or compress highlight white points;
  - Dragging an endpoint horizontally across a threshold pulls out a new mobile intermediate control point;
  - The "Reset Channel" button restores the active channel to a linear diagonal;
  - A real-time histogram renders behind the curve grid to guide tone adjustments.

### 2. Aesthetic Look Adjustments
Provides standard creative adjustments:
- **Exposure**: Shifts midtone brightness logarithmically across stops (EV);
- **Gamma & Contrast**: Modulates midtone tone contrast while anchoring endpoints;
- **Highlights & Shadows**: Mitigates specular highlight clipping and recovers dense shadow detail;
- **Saturation**: Enhances color purity within a perceptually uniform color space to avoid clipping;
- **Temperature & Tint**: Offers subtle warm-cool Kelvin balancing adjustments.

### 3. Detail and Spatial Frequency Sharpening
The detail module features sharpening and local contrast controls:

<table class="doc-table">
    <thead>
        <tr><th>Control Parameter</th><th>Functional Description and Mathematical Formulation</th><th>Adjustable Value Range</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Amount</strong></td><td>Total sharpening intensity. At 0, the sharpening pass is skipped at the hardware level and pixels pass through unsharpened.</td><td>0 – 150 (Default: 40)</td></tr>
        <tr><td><strong>Radius</strong></td><td>Spatial convolution kernel footprint. Controls edge transition width; smaller values enhance fine textures, while larger values delineate broad contours.</td><td>0.5 – 3.0 px (Default: 1.0)</td></tr>
        <tr><td><strong>Detail</strong></td><td>Frequency weighting between micro-textures and primary edges. Low values confine sharpening to pronounced edges; high values emphasize film grain.</td><td>0 – 100 (Default: 25)</td></tr>
        <tr><td><strong>Masking</strong></td><td>Edge detection threshold. At 0, sharpening applies uniformly. At higher values, smooth skies and out-of-focus backgrounds are excluded to avoid magnifying scanner noise.</td><td>0 – 100 (Default: 0)</td></tr>
        <tr><td><strong>Texture</strong></td><td>Mid-frequency local contrast adjustment, ideal for rendering fabrics, foliage, and skin textures.</td><td>-100 – +100</td></tr>
        <tr><td><strong>Clarity</strong></td><td>Broad-scale local contrast enhancement weighted toward midtones, avoiding shadow crushing.</td><td>-100 – +100</td></tr>
        <tr><td><strong>Dehaze</strong></td><td>Dark-channel prior physical model estimating optical transmission to cut through lens flare and atmospheric haze.</td><td>-100 – +100</td></tr>
    </tbody>
</table>

> [!TIP] Live Grayscale Masking via the Alt Key
> Holding the <kbd>Alt</kbd> key while dragging sharpening sliders transforms the canvas into an analytical diagnostic view:
> - Dragging **Amount**, **Radius**, or **Detail** displays the isolated high-frequency edge signal on a neutral grey field;
> - Dragging **Masking** displays the binary edge detection mask (black areas receive no sharpening; white lines indicate protected sharpened edges).
> Releasing the key returns the canvas to standard color rendering without interrupting scopes or thumbnail caches.

All sharpening, texture, and clarity parameters standardize their physical convolution radii to an image long edge of `2560 px`. Consequently, the sharpening balance viewed on the 2560 preview proxy translates with total pixel parity to multi-megapixel master exports.

## Scopes System and Highlight / Shadow Clipping Alerts {#scopes-and-clipping}

The scopes in the upper right corner provide real-time objective monitoring:
- **Histogram Mode**: Shows pixel tone distribution from pure black (0) to white (255), illustrating channel overlaps;
- **Waveform Mode**: Displays vertical luminance distribution across the horizontal image plane, pinpointing localized specular hotspots;
- **Channel Filters**: Toggles between composite RGB, individual Red/Green/Blue channels, and pure Luminance Y;
- **Clipping Warning Handles**: Exclamation point threshold handles illuminate when pixels exceed dynamic range boundaries, alerting you to highlight burnout or crushed blacks.

## Copy Settings, Paste Settings, and Batch Apply Boundaries {#copy-paste-vs-batch}

Managing color across dozens of roll captures requires clear operational boundaries between parameter replication tools:

<table class="doc-table">
    <thead>
        <tr><th>Dimension</th><th>Batch Apply</th><th>Copy Settings / Paste Settings</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Interface Location</strong></td><td>Top canvas geometry toolbar.</td><td>Top of the right inspector.</td></tr>
        <tr><td><strong>Parameters Transferred</strong></td><td>Film Area polygon coordinates and sprocket hole positions.</td><td>Selected grading parameters: density, printer lights, curves, look, and detail.</td></tr>
        <tr><td><strong>Target Frame Calibration</strong></td><td><strong>Re-measures localized film base and light table transmission independently</strong>.</td><td>Applies values directly <strong>without altering target film boundaries or base readings</strong>.</td></tr>
        <tr><td><strong>Ideal Use Case</strong></td><td>Rigid copy stand capture runs where negative positioning remains fixed across frames.</td><td>Different exposures sharing identical lighting conditions requiring uniform print aesthetics.</td></tr>
    </tbody>
</table>

Clicking **Copy Settings** loads all current parameters into the memory clipboard. When selecting **Paste Settings** on target frames, a selective paste dialog appears:
- Quick presets include:
  1. **All available settings**: Commits all parameters across the board;
  2. **Tone and color (Default)**: Synchronizes density limits, printer lights, look, and tone curves, excluding crop and detail sharpening;
  3. **Geometry only**: Applies rotation, flipping, straighten, and perspective corrections.
- Users can selectively toggle 27 individual settings (including `filmMode`, `printerLights`, `curve`, `sharpen`, and `lut`) for precise control.

Avoid using Batch Apply across frames captured under varying lighting. For consistent lighting conditions, selective Paste Settings provides the most reliable darkroom results.
