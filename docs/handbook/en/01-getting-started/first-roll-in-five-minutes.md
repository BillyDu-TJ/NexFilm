---
id: first-roll-in-five-minutes
track: getting-started
trackOrder: 1
chapterOrder: 4
kicker: "01.4 / Getting Started"
title: "Walk Through Your First Roll"
subtitle: "A complete first pass from import and film area selection to printer lights, proofing, and export."
readingTime: 10
inApp: true
sinceVersion: "v1.1.0"
---

## Step 1: Selecting Import Mode and Registering Metadata {#import-workflow}

When NexFilm opens, it displays the darkroom-inspired **Library** workspace by default. The first step in processing a roll is bringing your digitized scans into the local workspace. Click **Import Roll** in the top-right header to open the **Import Method** dialog. It offers three entry points: Import by Roll, Loose Import, and Continue Editing Roll.

<table class="doc-table">
    <thead>
        <tr><th>Entry Point</th><th>When to Use It</th><th>What Happens Next</th><th>Recommended Use</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Import by Roll</strong></td>
            <td>Complete rolls photographed consecutively (for example, 36 exposures of 135 or 12 frames of 120)</td>
            <td>Opens Roll Metadata for the format, camera, film stock, date, and notes. Choose <strong>Select Images</strong> afterward to pick the scans in the system file chooser. The result is a separate archived roll.</td>
            <td>Standard recommendation. Enables whole-roll batch calibration, shared white-point synchronization, and structured contact sheet rendering</td>
        </tr>
        <tr>
            <td><strong>Loose Import</strong></td>
            <td>Individual proof captures, loose historic test files, or mixed-aspect frame collections</td>
            <td>Opens the system file chooser directly, or lets you drop RAW and TIFF files into the main window. No roll metadata is required.</td>
            <td>Quick inspection. Loose frames stay outside the roll archive and are better suited to temporary, frame-by-frame work</td>
        </tr>
        <tr>
            <td><strong>Continue Editing Roll</strong></td>
            <td>You previously archived a roll and want to return to it for more grading</td>
            <td>Pick an existing roll from the <strong>Roll Archive</strong> list and choose <strong>Open &amp; Edit</strong>.</td>
            <td>Returning to older work. This entry only opens the archived roll; it does not add new files to it</td>
        </tr>
    </tbody>
</table>

When choosing **Import by Roll**, fill in the format (for example, `135`), camera, film stock (for example, `Kodak Portra 400`), and date in the **Roll Metadata** window. Click **Select Images** in the lower-right corner, choose the scans for this roll in the system file chooser, and confirm the selection. NexFilm then catalogs the files in the local SQLite database (`nexfilm_user.db`). Import only creates lightweight previews and metadata records. Your original files stay read-only and are not moved, renamed, or duplicated.

## Step 2: Entering Develop and On-Demand Proxy Generation {#proxy-generation}

In the **Library** thumbnail grid, double-click any representative frame, or select a frame and click the **Develop** tab in the centered view switcher. The application transitions smoothly into the GPU-accelerated grading workbench.

During your initial entry into Develop, a brief loading sequence appears on the central canvas. This indicates that NexFilm is initializing its **on-demand 16-bit linear proxy generation pipeline**. Traditional photo editing software attempts full debayering and decoding of every high-resolution RAW file across an entire roll upon import, consuming gigabytes of system memory and stalling disk I/O. NexFilm uses an asynchronous, demand-driven architecture:

1. **Zero Full-Resolution Decoding on Ingestion**: The Library reads embedded container previews to achieve immediate thumbnail rendering across dozens of rolls.
2. **On-Demand 16-bit Floating-Point Proxy Processing**: Only when an exposure is opened in Develop does the background Rust engine invoke LibRaw or the high-throughput TIFF parser, transforming Bayer sensor data into an unclipped 16-bit linear transmittance texture uploaded to WebGL 2.0 texture units.
3. **Real-Time GPU Shader Pipeline**: The main viewport relies on custom WebGL2 fragment shaders to calculate logarithmic optical density conversions, printer light color additions, and tone curves in parallel, updating the display at a sustained 60 frames per second during parameter interaction.

For viewport interaction, use the mouse wheel over the canvas to zoom smoothly in and out centered on the cursor position. Hold down the keyboard spacebar <kbd>Space</kbd> and drag the mouse to pan across the negative, examining emulsion silver grain structure and optical focus. Double-clicking any empty canvas area resets the view to fit the full frame.

## Step 3: Defining Valid Film Area and Excluding Border Highlights {#film-area-definition}

Within the Develop workspace, the most influential step governing inversion quality is establishing the **Film Area**. In the right-hand inspector panel, expand the "Geometry and Area" panel and select the **Set Film Area** button. Four corner handles and bounding border lines will appear over the negative preview.

Click **Auto Area** in the toolbar. The edge-detection algorithm scans the frame boundaries and fits an initial rectangular crop against the dark rebate. Human visual confirmation is essential at this juncture.

<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">The Optical Consequence of Including Light Panels and Sprockets</div>
    <div class="doc-callout-body">A backlight panel transmits raw, unobstructed light that is often many times brighter than the clear film base. If unmasked light panel areas or transparent 135 sprocket hole cutouts are accidentally enclosed inside the film area, the inversion algorithm will identify the pure light source as the brightest pixel in the exposure. In subsequent logarithmic normalization, the genuine photographic exposure range is forced downward, causing the final positive image to look muddy, dark, and flat across all midtones.</div>
</div>

<table class="doc-table">
    <thead>
        <tr><th>Inspection Checkpoint</th><th>Acceptable Crop Criteria</th><th>Defective Crop Behavior</th><th>Corrective Adjustment Procedure</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Surrounding Light Panel</strong></td>
            <td>The interior of the box contains only film emulsion, with no raw backlight leaking in</td>
            <td>Crop box is positioned too wide, showing illuminated light panel along the edges</td>
            <td>Drag the border handles inward until external white light is outside the active area</td>
        </tr>
        <tr>
            <td><strong>135 Sprocket Holes</strong></td>
            <td>Sprocket cutouts stay outside the crop line</td>
            <td>The crop rectangle includes the interior of sprocket holes</td>
            <td>Pull the upper and lower crop borders inward. If the film is tilted, level the frame first using the angle slider</td>
        </tr>
        <tr>
            <td><strong>Clear Film Base Rebate</strong></td>
            <td>Allows a hairline gap of unexposed rebate along the edges, with active imagery occupying 98% of the box</td>
            <td>Thick swaths of orange unexposed rebate occupy prominent portions of the active frame</td>
            <td>Tighten crop boundaries against the exposed scene content so histogram calculations reflect actual imagery</td>
        </tr>
    </tbody>
</table>

Once the boundary rectangle is confirmed to hug the exposed image area, click **Save Area** in the inspector. NexFilm stores the crop coordinates in the database and hides the interactive handles.

## Step 4: Executing Auto Invert and Batch Applying Area {#auto-invert-and-batch}

With the film area confirmed, click the **Auto Invert** button in the inspector. NexFilm processes the frame in milliseconds:

1. The engine samples the unexposed orange base rebate located outside the crop line (or from pre-calibrated base markers) to calculate the negative base density vector $(D_{\min}^R, D_{\min}^G, D_{\min}^B)$.
2. It executes logarithmic transformation and matrix density subtraction across all pixels enclosed within the crop boundary, mapping the image into calibrated optical density space.
3. The bounded channel span response model calculates initial highlight-shadow contrast and color balance, presenting an authentic, natural positive image on the canvas.

If the roll was captured on a fixed copy stand and the negative sits in the same place each time, you do not need to redraw the crop for every frame. Calibrate the first frame, click **Batch Apply** in the inspector, choose **Film Area &amp; Geometry**, and click **Apply to Roll**.

NexFilm passes the crop coordinates to the rest of the roll. Each target frame still measures its own film base inside that boundary, which helps with small differences in light falloff.

## Step 5: Refining Density with Printer Lights and Non-Destructive Edits {#printer-lights-tuning}

Auto Invert provides a physically sound photographic baseline, but individual lighting situations and artistic intentions require fine-tuning. In classical darkroom printing, color technicians adjust exposure by inserting red, green, and blue optical filters into the enlarger head, known as Printer Lights.

NexFilm reproduces this darkroom methodology in the **Printer Lights** inspector panel. Operating in the logarithmic density domain, adjustments follow additive and subtractive color filtration rules:

1. **Red Light**: Moving the slider right increases red light, warming the positive and removing cyan coldness from shadow areas. Moving left decreases red, shifting the image toward Cyan.
2. **Green Light**: Moving the slider right warms the tone toward green. Moving left decreases green, reduces yellow-green casts, and steers the palette toward magenta.
3. **Blue Light**: Moving right warms the palette toward yellow. Moving left cools the image toward Blue, ideal for neutralizing warm tungsten indoor lighting.

To prevent accidental parameter shifts, slider interaction includes intentional focus requirements: scrolling the mouse wheel over a slider produces no change until you **click the slider once with the left mouse button to give it focus**. Once focused, the mouse wheel steps values with precision. When evaluating sharpening, holding down the keyboard <kbd>Alt</kbd> key while adjusting **Amount**, **Radius**, or **Detail** temporarily renders a high-contrast grayscale edge mask on the canvas, reverting to full-color preview when released.

<div class="doc-callout">
    <div class="doc-callout-title">Non-Destructive History and Universal Shortcuts</div>
    <div class="doc-callout-body">Every adjustment in NexFilm is recorded non-destructively in local memory. Press <kbd>Ctrl</kbd>+<kbd>Z</kbd> (<kbd>⌘</kbd>+<kbd>Z</kbd> on macOS) to undo the last change. Press <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> or <kbd>Ctrl</kbd>+<kbd>Y</kbd> to redo it. With the filmstrip visible, use <kbd>←</kbd> and <kbd>→</kbd> to move between frames. Adjustments are saved to the local database in the background, so there is no save dialog to worry about.</div>
</div>

## Step 6: Single-Frame Proof Export and Full-Roll Output {#export-and-verification}

After refining your adjustments, standard darkroom procedure requires exporting a single high-resolution proof before committing to a full-roll batch export.

Select the frame you want to inspect in the **Library**, then click **Export** in the top-right header to open the **Batch export** dialog:

1. **Export Format Selection**: For online publishing and client previews, select high-quality **JPEG** (quality factor 95 to 100). For downstream grading in Adobe Photoshop, Lightroom, or DaVinci Resolve, select uncompressed **16-bit TIFF**.
2. **Color Space Assignment**: For standard displays and web deployment, choose **sRGB**. For wide-gamut monitors such as the Apple Studio Display or hardware-calibrated printing monitors, select **Display P3** or **Adobe RGB**.
3. **Choose an Output Folder**: Set the save location in the **Destination** section. The export button stays inactive until a destination is selected.
4. **Color Management**: Choose the desired **Output color**. NexFilm writes the matching color-space description into the exported file, and color-managed viewers and browsers use it to display the image.

Click **Export frames**. The background renderer reads the source at full resolution and writes the finished file. How long it takes depends on dimensions, format, and your machine; a normal JPEG is usually quick, while a large TIFF or DNG takes longer. Open the result in your system image viewer, zoom to 100%, and check shadow noise, skin tones, and highlight texture.

When the proof matches your quality expectations, click **Export Roll** in the header. The roll export dialog allows configuring output filename tokens (such as `{Roll}_{Seq}_{Film}`) and setting worker thread concurrency. Click export to process the entire roll sequentially, delivering a complete set of color-consistent digital positives in minutes.
