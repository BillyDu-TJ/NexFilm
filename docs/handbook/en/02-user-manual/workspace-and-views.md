---
id: workspace-and-views
track: user-manual
trackOrder: 2
chapterOrder: 1
kicker: "02 / User Manual"
title: "Interface Overview, Five Core Views, and Interaction Guidelines"
subtitle: "A detailed breakdown of menu bars, view routing, the filmstrip, inspector groups, and non-destructive interaction paradigms."
readingTime: 8
inApp: true
sinceVersion: "v1.1.0"
aliases: ["interface-basics"]
---

## Menu Bar and Global Asynchronous Operations {#menu-and-global-actions}

NexFilm Engine is built around a unified, darkroom-centric single-window interface designed to balance high-throughput batch roll processing with pixel-level precision on individual frames. The top of the window integrates the application menu bar, a central view navigation switcher, and persistent background action triggers.

The application menu bar handles system-wide environment preferences and diagnostic tools:
- **Preferences**: Allows instant, on-the-fly switching between interface languages (Simplified Chinese `zh-CN` and English `en`) as well as between Darkroom Dark and Light UI themes. Theme and language switches take effect immediately without requiring an engine restart, preserving all active slider positions, curve control points, and canvas viewport coordinates.
- **View**: Offers direct routing commands across the five core application views alongside native full-screen toggles for immersive darkroom grading sessions.
- **Help**: Provides direct access to the built-in offline technical handbook, open-source licensing disclosures, and runtime diagnostic system details.

Anchored to the right side of the primary header, three global asynchronous task triggers remain permanently accessible across all application screens:
1. **Import Roll**: Opens the multi-format ingestion modal to import new camera RAW capture folders or scanner directories into the project catalog.
2. **Export**: Displays a real-time badge count of currently selected frames in the active working roll. Clicking this button opens the batch export dialogue to process the selected frames into high-resolution deliverables.
3. **Export Roll**: Triggers a full-roll background export of all frames in the current active roll without requiring manual selection inside the library grid.

> [!NOTE] Asynchronous Threading and Non-Blocking UI
> NexFilm Engine leverages a high-concurrency Rust core. All demanding computing tasks—including Bayer debayering, bounded proxy computation, streaming full-resolution color transformations, and uncompressed TIFF serialization—execute entirely within detached background thread pools. Whether ingesting hundreds of 50-megapixel RAW frames or exporting 16-bit linear masters, the user interface remains at a consistent 60 frames per second. Users can navigate across views or grade other frames without latency.

## Five Core Views and State Transitions {#five-core-views}

The core capabilities of the software are segmented into five orthogonal views. Users transition between these views via the central tab selector in the header. The architectural responsibilities and data flow between views are summarized below:

<table class="doc-table">
    <thead>
        <tr><th>View Name</th><th>Identifier</th><th>Primary Responsibilities and Data Flow</th><th>Typical Darkroom Usage</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Library</strong></td><td>Library</td><td>Displays the thumbnail grid of the active working roll. Supports multi-selection, frame deletion, and launching <strong>Calibrate Film Base / Leader</strong>.</td><td>Initial inspection after import, pruning unexposed frames, selecting calibration frames for roll-wide physical anchoring.</td></tr>
        <tr><td><strong>Develop</strong></td><td>Develop</td><td>The primary high-precision grading workspace. Houses the central canvas, geometry toolbar, interactive filmstrip, and ten inspector modules.</td><td>Fine-tuning film bounds, correcting camera perspective, balancing printer lights, drawing tone curves, and applying detail sharpening.</td></tr>
        <tr><td><strong>Rolls</strong></td><td>Rolls</td><td>The persistent local film catalog. Features multi-dimensional faceted filtering across formats, cameras, and dates, with roll info editing and contact sheet generation.</td><td>Searching historical projects, appending lab notes, re-linking offline source scans, and generating printable contact sheets.</td></tr>
        <tr><td><strong>Hardware Calibration</strong></td><td>Hardware Calibration</td><td>Reprographic capture rig profiling. Calibrates sensor spectral separation, backlight spectral power distribution (SPD), and lens vignetting falloff.</td><td>Setting up new camera bodies, upgrading high-CRI light sources, or switching reprographic macro reproduction lenses.</td></tr>
        <tr><td><strong>Documentation</strong></td><td>Documentation</td><td>The built-in offline technical manual and color science reference, covering all optical models, printer light color tables, and troubleshooting decision trees.</td><td>Offline field reference during scanning sessions, verifying parameter definitions, or reviewing color math formulations.</td></tr>
    </tbody>
</table>

State transitions between views operate with instantaneous reactive synchronisation. Double-clicking any thumbnail in the Library opens the Develop view focused directly on that frame. Conversely, any printer light tweak, curve modification, or crop adjustment made in the Develop view is committed immediately to the local database and reflected in the thumbnail displays across the Library and Filmstrip.

## Filmstrip Architecture and Multi-Frame Comparison {#filmstrip-navigation}

Located directly beneath the main canvas in the Develop view, the Filmstrip displays every continuous frame belonging to the active roll. It functions as the critical navigation and visual parity hub during grading:

- **Sequential Frame Ribbon**: Every frame in the filmstrip features its chronological frame sequence number and source file stem. The active editing target is distinctly highlighted with an illuminated accent border.
- **Smooth Horizontal Navigation**: Hovering over the filmstrip allows horizontal scrolling via the mouse wheel. Pressing the keyboard directional keys <kbd>←</kbd> and <kbd>→</kbd> instantly navigates to the preceding or succeeding frame. During frame transitions, the canvas maintains the exact current zoom ratio and pan position, facilitating rapid comparative checks between adjacent frames.
- **Live Synchronised Thumbnails**: Filmstrip previews are dynamic rendering targets rather than static file icons. When you adjust printer lights or apply a film print 3D LUT in the right inspector, the corresponding filmstrip thumbnail regenerates within milliseconds via GPU shader passes. Examining the entire filmstrip allows you to detect subtle density inconsistencies or tint shifts across a series of exposures immediately.
- **Physical Calibration Frame Badges**: Frames flagged as unexposed film base or fully saturated film leaders display dedicated geometric corner badges, preventing them from being mistakenly exported as standard photographic frames.

## Inspector Architecture and Precision Input Controls {#inspector-controls-and-gestures}

The inspector situated on the right side of the Develop view serves as the control hub for all density transformations and darkroom print parameters. The panels are organized top-to-bottom following the physical image reconstruction pipeline across ten collapsible modules:

1. **Scopes**: Real-time signal analysis toggling between Histogram and Waveform modes, displaying composite RGB, individual Red/Green/Blue channels, or Luminance Y, equipped with clipping warning overlays.
2. **Actions**: Houses the single-frame Auto Invert routine, the physically anchored Auto Invert Roll batch processor, and the color Reset trigger.
3. **Mode**: Switches seamlessly between Color (three-channel subtractive dye coupling) and B&W (pure single-channel optical density conversion).
4. **Density Limits**: Direct controls for Master D-Min (shadow density anchor) and Master D-Max (highlight density anchor) to govern contrast span.
5. **Printer Lights**: Logarithmic exposure controls for Red (Cyan/Red), Green (Magenta/Green), and Blue (Yellow/Blue) channels alongside an optical neutral grey balance dropper.
6. **Tone Curve**: Four-channel control point spline editor (composite RGB plus independent R, G, and B curves) backed by live histogram displays.
7. **Look**: Broad aesthetic grading controls including Exposure, Gamma, Contrast, Highlights, Shadows, Saturation, Temperature, and Tint.
8. **Detail**: Spatial frequency sharpening (Amount, Radius, Detail, Masking) and local contrast adjustments (Texture, Clarity, Dehaze).
9. **Sprocket**: Interactive sprocket hole color sampling, detection tolerance bounds, and transition edge feathering.
10. **Print Emulation**: 14 built-in physical silver halide paper and motion picture print film 3D LUTs with variable opacity blending.

To guarantee surgical precision and eliminate accidental parameter changes, the inspector incorporates four core interaction principles:
- **Section Navigation Dots**: A vertical rail of miniature indicator dots sits on the left edge of the inspector. Clicking any dot instantly scrolls the inspector to that specific module.
- **Persistent Panel Collapsing**: Clicking the chevron toggle on any panel header collapses that module to maximize screen space. Collapse states persist across application sessions in your local profile.
- **Activation Lock on Sliders**: Scrolling the mouse wheel over empty inspector space scrolls the panel list vertically. To adjust a numeric slider using the mouse wheel, the slider handle must first be clicked directly. This explicit activation lock prevents inadvertent modifications while browsing long panels.
- **Direct Numeric Editing**: Every slider value readout can be clicked to engage keyboard entry. Users can type precise floating-point values, pressing <kbd>Enter</kbd> to confirm or <kbd>Esc</kbd> to dismiss and restore the prior setting.

The main canvas supports panning by holding <kbd>Space</kbd> while dragging with the mouse. Scrolling the wheel directly on the canvas performs smooth, continuous zooming centered precisely at the cursor location. The application maintains an exhaustive non-destructive undo stack: outside of text fields, pressing <kbd>⌘</kbd>+<kbd>Z</kbd> (<kbd>Ctrl</kbd>+<kbd>Z</kbd> on Windows) reverts actions, while <kbd>Shift</kbd>+<kbd>⌘</kbd>+<kbd>Z</kbd> (<kbd>Ctrl</kbd>+<kbd>Y</kbd> on Windows) reapplies them.

## Complete Darkroom and Engine Terminology {#terminology-glossary}

To foster precise communication and consistency with classical darkroom physics, the table below documents the core concepts and operational terminology utilized across NexFilm Engine:

<table class="doc-table">
    <thead>
        <tr><th>Standard Term</th><th>English Nomenclature</th><th>Physical Meaning and Engine Definition</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Roll</strong></td><td>Roll</td><td>A cataloged collection of frames captured or scanned under a common production run, possessing shared camera, emulsion, capture date, and processing notes.</td></tr>
        <tr><td><strong>Frame</strong></td><td>Frame</td><td>A single photographic negative image entity within a roll, acting as the smallest discrete unit for grading adjustments, history tracking, and high-resolution export.</td></tr>
        <tr><td><strong>Film Area</strong></td><td>Film Area</td><td>The bounding polygon delineating the active image area on the negative scan. Excludes perforations, film borders, and raw light table backlighting to focus optical density analysis strictly on valid emulsion.</td></tr>
        <tr><td><strong>Film Base</strong></td><td>Film Base</td><td>The unexposed clear polyester or triacetate carrier material. On color negative film, this area contains the orange masking dyes, establishing the physical zero point for mask subtraction.</td></tr>
        <tr><td><strong>Film Leader</strong></td><td>Film Leader</td><td>The heavily exposed and fully developed head of the film strip, possessing the absolute maximum optical silver/dye density ($D_{\max}$) achievable by that specific roll emulsion.</td></tr>
        <tr><td><strong>Optical Density</strong></td><td>Optical Density ($D$)</td><td>The logarithmic measure of light obstruction through a photographic medium, defined mathematically as $D = -\log_{10}(T)$, where $T$ is optical transmittance. All NexFilm inversions occur in this linear density domain.</td></tr>
        <tr><td><strong>D-Min / D-Max</strong></td><td>Density Limits</td><td>The physical endpoints calibrated in the inspector. D-Min sets the deepest printable black foundation, whereas D-Max sets the shoulder highlight boundary, defining overall image contrast.</td></tr>
        <tr><td><strong>Printer Lights</strong></td><td>Printer Lights</td><td>Emulation of darkroom optical enlarger dichroic color heads, introducing logarithmic exposure shifts across Red, Green, and Blue spectral bands to neutralize color casts.</td></tr>
        <tr><td><strong>Orange Mask</strong></td><td>Film Mask</td><td>The inherent orange coloration generated by color coupler byproducts in color negative emulsions. Mask neutralization deducts this baseline optical density across all channels.</td></tr>
        <tr><td><strong>Contact Sheet</strong></td><td>Contact Sheet</td><td>A scaled, sequential darkroom index print generated by exposing negative strips directly against photographic paper. NexFilm reproduces authentic perforations and frame edge codes.</td></tr>
        <tr><td><strong>Input Domain</strong></td><td>Input Domain</td><td>The mathematical and physical representation of raw pixel values upon decoding (such as camera linear sensor RAW, scanner transmittance space, or display gamma spaces).</td></tr>
    </tbody>
</table>

> [!TIP] Non-Destructive Source File Guarantee
> NexFilm Engine operates strictly under a read-only architecture for all raw capture assets. Adjusting film boundaries, rotating geometry, applying logarithmic inversions, or applying custom 3D LUTs will never alter or overwrite source camera RAW files or scanner TIFFs. All grading formulas and metadata entries persist exclusively inside the local application database, allowing full reversibility at any time.
