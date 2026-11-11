---
id: troubleshooting-and-appendix
track: user-manual
trackOrder: 2
chapterOrder: 7
kicker: "02 / User Manual"
title: "Troubleshooting Guide, Shortcuts, Backup Strategies, and Open-Source Appendix"
subtitle: "A systematic symptom-cause troubleshooting tree, complete verified shortcut tables, cross-platform data storage, and the open-source appendix."
readingTime: 14
inApp: true
sinceVersion: "v1.1.0"
aliases: ["troubleshooting", "appendix"]
---

## Tone and Color Discrepancy Troubleshooting Tree {#tone-and-color-troubleshooting}

In an optical digital darkroom, inversion anomalies rarely stem from internal algorithmic faults. Instead, they typically arise from reprographic illumination discrepancies, contaminated film base samples, or incorrect framing boundaries. Adopting a structured "Observed Symptom → Root Physical Cause → Step-by-Step Resolution" diagnostic framework allows photographers to pinpoint issues immediately.

The decision matrix below summarizes the most common tone and color discrepancies encountered during inversion:

<table class="doc-table">
    <thead>
        <tr><th>Observed Anomaly</th><th>Root Physical Cause and Diagnostic Focus</th><th>Step-by-Step Resolution Pathway</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Inverted positive appears flat, dark, and severely lacking contrast</strong></td><td>1. Film Area encloses raw 100% transmission backlight from the light table.<br>2. Base sampling sampled an exposed image area instead of unexposed rebate.<br>3. Negative frame was severely underexposed during capture.</td><td>1. Click <strong>Set Film Area</strong> in the toolbar and adjust all edges to exclude raw backlight and perforations.<br>2. Click <strong>Auto Invert</strong> to recompute density limits.<br>3. Adjust Master D-Min and Master D-Max in the Density Limits panel.<br>4. If the scene depicts heavy fog or twilight, natural low contrast is physically correct and should not be forced into harsh digital clipping.</td></tr>
        <tr><td><strong>Image exhibits a stubborn overall color cast resistant to white balance adjustment</strong></td><td>1. Base calibration sampled scratches, dust, or dye fading on the rebate.<br>2. Reprographic light source possesses an inadequate Color Rendering Index (Ra &lt; 95).<br>3. Scanner driver applied irreversible non-linear contrast clipping.</td><td>1. Find a clean, scratch-free unexposed border area and rerun <strong>Calibrate Film Base / Leader</strong>.<br>2. Verify light source spectral continuity in the <strong>Hardware Calibration</strong> module.<br>3. Use the Printer Lights panel to manually balance Red/Green/Blue sliders before clicking the neutral grey dropper.</td></tr>
        <tr><td><strong>Batch processing succeeds across the roll except on isolated frames with severe color jumps</strong></td><td>1. Minor copy stand vibration shifted negative placement, causing boundaries to clip backlight.<br>2. Frame contains a dominant monochromatic subject (e.g., dense forest or ocean), triggering channel-span protection.<br>3. Leader calibration frames were accidentally included in the batch.</td><td>1. Select the affected frame and readjust its <strong>Film Area</strong> individually.<br>2. Run <strong>Auto Invert</strong> on that specific frame.<br>3. Compensate for dominant monochromatic scenes using the Printer Lights panel.<br>4. Confirm that calibration frames carry geometric badges and are excluded from normal batch outputs.</td></tr>
        <tr><td><strong>Deep shadow contrast is lost, displaying a milky, washed-out haze</strong></td><td>1. Film Area enclosed opaque black film holder borders.<br>2. Lens flare or camera internal reflections caused veiling glare during capture.</td><td>1. Tighten the Film Area to eliminate opaque mask borders.<br>2. Advance the <strong>Dehaze</strong> slider in the Detail panel to cut through veiling glare.</td></tr>
    </tbody>
</table>

> [!NOTE] Natural Low Contrast vs Algorithmic Errors
> When a negative records early morning fog, rainy overcast landscapes, or dim interior ambient light, physical scene radiances naturally lack bright specular highlights or deep black shadows. NexFilm's sensitometric pipeline preserves authentic scene dynamics, refusing to stretch the histogram ends to artificial extremes. Photographers should welcome genuine low contrast, utilizing the Tone Curve module when artistic punch is intentionally desired.

## File Decoding, Ingestion, and Rendering Performance {#decoding-and-performance}

When troubleshooting asset ingestion, raw parsing, and rendering performance, apply the following diagnostic guidelines:

### 1. Camera RAW or Flextight .fff Ingestion Failures
- **Verify File Extension Authenticity**: Third-party utilities may alter raw file extensions. Ensure files originate directly from camera media or scanner exports.
- **Nikon Z8 / Z9 High-Efficiency NEF Restrictions**: As documented, embedded LibRaw routines do not currently decode `HE` or `HE*` formats. Switch camera recording formats to standard Lossless Compressed RAW or convert files using the free Adobe DNG Converter.
- **Hasselblad Flextight `.fff` Files**: As of v1.1.0, files decode natively as scanner transmission TIFFs without requiring modified hardware model strings. If parsing fails, verify that FlexColor did not export the file with non-standard proprietary tiling.

### 2. Large Scanner TIFFs (Multi-Gigabyte Files) and System RAM
- Digitizing 8×10 sheet film at high optical resolutions produces 16-bit TIFFs exceeding several gigabytes. NexFilm utilizes scanline-streaming routines, loading bounded 2560 px proxies into active display memory. If imports lag on older machines, verify that your system drive maintains at least 16 GB of available virtual memory swap space.

### 3. Visual Parity Between Develop Preview and Full-Scale Master Exports
- **Bounded Proxy versus Full Master Rendering**: The Develop canvas operates in real time on a dedicated long-edge `2560 px` bounded proxy, whereas exports process uncompressed native capture rasters from disk.
- **Micro-Grain Texture and Acuity**: While sharpening and clarity convolution kernels scale proportionally to a 2560 px baseline, ultra-high-resolution files viewed at 100% display finer analog silver halide grain structures due to anti-aliasing downsampling filters. Generating a single test proof before batch processing full rolls is standard darkroom practice.

### 4. Blank or Dark Thumbnails When Upgrading from v1.0.2 to v1.1.0
- Version 1.1.0 introduced an overhauled optical density inversion engine. To prevent legacy thumbnails rendered with older formulas from misrepresenting active projects, the database migration clears outdated thumbnail caches on initial launch. Open the affected roll and trigger **Auto Invert** or **Auto Invert Roll** once to regenerate all previews with current color physics.

## Verified Global Shortcuts and Canvas Gestures {#shortcuts-and-gestures}

To facilitate rapid, fatigue-free darkroom grading, NexFilm standardizes keyboard shortcuts across the application. The system strictly avoids fabricating unimplemented hotkeys. The complete list of verified shortcuts and mouse gestures is detailed below:

<table class="doc-table">
    <thead>
        <tr><th>Shortcut or Mouse Gesture</th><th>Operational Function</th><th>Context and Execution Boundaries</th></tr>
    </thead>
    <tbody>
        <tr><td><kbd>⌘</kbd>+<kbd>Z</kbd> (<kbd>Ctrl</kbd>+<kbd>Z</kbd> on Windows)</td><td><strong>Undo previous action</strong></td><td>Global scope. When an input text box is focused (such as entering metadata or numeric slider values), native text undo takes priority.</td></tr>
        <tr><td><kbd>Shift</kbd>+<kbd>⌘</kbd>+<kbd>Z</kbd> (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> or <kbd>Ctrl</kbd>+<kbd>Y</kbd>)</td><td><strong>Redo previous action</strong></td><td>Global scope outside active text fields. Reapplies reverted color corrections or geometric adjustments.</td></tr>
        <tr><td><kbd>←</kbd> / <kbd>→</kbd> Arrow Keys</td><td><strong>Navigate to previous / next frame</strong></td><td>Operates in the Develop view when the filmstrip is visible, maintaining current canvas zoom and pan positions.</td></tr>
        <tr><td><kbd>Enter</kbd></td><td><strong>Confirm crop or numeric entry</strong></td><td>Commits on-screen composition crop boundaries or validates direct numeric entry in slider text fields.</td></tr>
        <tr><td><kbd>Esc</kbd></td><td><strong>Dismiss modal, menu, or cancel input</strong></td><td>Closes active dialog windows, exits popover menus, or cancels active numeric editing to revert previous values.</td></tr>
        <tr><td>Hold <kbd>Space</kbd> + Mouse Drag</td><td><strong>Pan canvas viewport</strong></td><td>Engaged across the Develop canvas whenever an image is loaded, enabling smooth navigation at high zoom levels.</td></tr>
        <tr><td>Mouse Wheel (Canvas / Filmstrip)</td><td><strong>Continuous zoom / Horizontal scroll</strong></td><td>Zooms continuously centered on the mouse cursor over the canvas; scrolls the frame sequence horizontally over the filmstrip.</td></tr>
        <tr><td>Mouse Wheel (Inspector Sliders)</td><td><strong>Precision fine-step slider adjustment</strong></td><td><strong>Safety Lock Mechanism</strong>: Sliders must first be clicked directly with the left mouse button before the wheel adjusts values.</td></tr>
        <tr><td>Hold <kbd>Alt</kbd> + Drag Sharpening Slider</td><td><strong>Engage live grayscale diagnostic mask</strong></td><td>Dragging Amount, Radius, or Detail reveals isolated edge signals; dragging Masking displays the binary edge detection mask.</td></tr>
    </tbody>
</table>

## Cross-Platform Data Storage and Backup Protocols {#data-storage-and-backup}

Understanding the persistent file architecture across operating systems is essential for safeguarding film archives over years of production:

<table class="doc-table">
    <thead>
        <tr><th>Operating System Platform</th><th>Absolute Physical Application Data Path</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Windows Production Release</strong></td><td><code>%APPDATA%\NexFilm Engine\</code> (typically resolves to <code>C:\Users\&lt;Username&gt;\AppData\Roaming\NexFilm Engine\</code>)</td></tr>
        <tr><td><strong>macOS Production Release</strong></td><td><code>~/Library/Application Support/NexFilm Engine/</code></td></tr>
        <tr><td><strong>Development / Debug Builds</strong></td><td>Root directory of the local cloned repository workspace</td></tr>
    </tbody>
</table>

### Database Structure and Integrity Files
- `nexfilm_user.db`: The core SQLite database. Stores all catalog metadata, camera profiles, emulsion dictionaries, frame index trees, complete grading history stacks, and bounded 2560 px thumbnail caches. It **does not store copies of your raw image files**, recording only absolute filesystem path references.
- `rolls.json`: An auxiliary mirror file maintained for backwards compatibility and rapid external inspection, committed in sync with database transactions.

### Backup and Migration Rules
> [!IMPORTANT] Dual-Track Backup Policy
> Because NexFilm operates as a reference-based system, **backing up only `nexfilm_user.db` will not recover your project if raw capture files are lost**. Conversely, backing up raw files without the database requires manually re-entering all grading parameters upon re-import.
> Follow this two-track backup routine:
> 1. Regularly back up external storage drives containing your original camera RAW or scanner TIFF files;
> 2. Simultaneously back up the application data directory containing `nexfilm_user.db`.
> When migrating to a new workstation, copy your raw captures and data directory to the new machine to resume work seamlessly.

## Known Limitations and Technical Boundaries {#known-limitations}

In accordance with scientific and open-source principles, NexFilm transparently discloses known operational boundaries:

1. **Unsigned Package Certificates**: Current Windows packages and macOS `.dmg` disk images are built via open-source automated CI workflows without commercial enterprise code signing certificates. The macOS disk image has not undergone Apple notarization, requiring manual first-launch authorization via system settings.
2. **Nikon Z8 / Z9 High-Efficiency NEF Restrictions**: Due to decoder limitations, high-efficiency compressed NEF formats are not natively decoded. Convert captures to DNG prior to ingestion.
3. **Legacy Proprietary Scanner Formats**: Given the variety of legacy flatbed and drum scanning software, rare proprietary scanner formats continue to be tested for ongoing compatibility.
4. **No Precompiled Linux GUI Releases**: While the underlying Rust core compiles cleanly across Unix environments, official distributed installers focus on Windows and macOS.
5. **Path Dependency in Non-Destructive Storage**: Moving folders or unmounting external drives transitions frames into an offline state, requiring the Locate File tool to reconnect paths.
6. **Experimental Hardware Calibration**: The Hardware Calibration view involves complex spectral fitting routines and is classified as experimental, subject to future development.

## Open-Source Licensing, Issue Reporting, and Acknowledgments {#license-feedback-acknowledgments}

### Open-Source Licensing
NexFilm Engine is distributed under the **GNU General Public License v3.0 only (GPL-3.0-only)**. All frontend application code, Rust engine components, darkroom shaders, and compilation scripts are available publicly on GitHub. Users are free to run, study, modify, and redistribute the software under the condition that all derivative works remain fully open-source under identical terms.

### Structured Issue Reporting Guidelines
If you encounter crashes, decoding anomalies, or UI rendering glitches, open an issue on the official GitHub repository. To facilitate rapid diagnosis and patch deployment, please provide the following technical details:
- **Environment Diagnostics**: Operating system version (e.g., Windows 11 23H2 or macOS Sonoma 14.5 Apple Silicon) and the exact installed NexFilm Engine version (e.g., v1.1.0);
- **Hardware and Capture Details**: Camera body, reprographic lens, light source manufacturer, file extension, and pixel dimensions;
- **Step-by-Step Reproduction Log**: Detailed steps leading to the anomaly, accompanied by relevant console error logs and stack traces;
- **Privacy Guidelines**: Never upload images containing private personal information to public issues. Test charts or non-sensitive photographic crops are preferred for diagnostic investigations.

### Theoretical Foundations and Community Acknowledgments
NexFilm's physically interpretable, math-driven darkroom models have been shaped by valuable open-source contributions. The development team extends sincere gratitude:
- To Zhihu researchers **@黄昊Haosky** and **@V777** for their foundational theoretical investigations into lossless negative mask uncoupling, spectral dye overlap physics, and logarithmic optical density inversion, which provided critical inspiration for NexFilm's core architecture;
- To all analog photographers, darkroom technicians, and beta testers who have contributed defect reports, translation reviews, and reprographic test samples to the project.
