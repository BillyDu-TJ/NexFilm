---
id: roll-vs-loose-workflows
track: getting-started
trackOrder: 1
chapterOrder: 5
kicker: "01.5 / Getting Started"
title: "Roll Physical Calibration versus Loose Frames Workflow"
subtitle: "Whole-roll calibration, shared white point anti-divergence, and 120 format fallback."
readingTime: 8
inApp: true
sinceVersion: "v1.1.0"
---

## Workflow Selection and Decision Matrix {#workflow-decision-matrix}

In film digitization practice, photographers encounter diverse negative materials and capture configurations: sometimes a complete, unbroken 135 roll developed under uniform chemistry with leader intact; other times, individual proof frames, 120 medium format strips lacking leader tabs, or test shots from disparate sources. To support these distinct engineering inputs, NexFilm provides two primary operating workflows: the **Roll Physical Calibration Workflow** and the **Loose Frames Fast Workflow**.

Evaluating the underlying mechanisms and trade-offs of each workflow ensures consistent darkroom output and optimal processing efficiency.

<table class="doc-table">
    <thead>
        <tr><th>Core Evaluation Dimension</th><th>Roll Physical Calibration Workflow</th><th>Loose Frames Fast Workflow</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Target Emulsion Material</strong></td>
            <td>Complete rolls with consecutive exposures (135 whole rolls, 120 connected strips) sharing uniform processing</td>
            <td>Individual proof frames, large format sheet film, isolated test frames, or mixed laboratory batches</td>
        </tr>
        <tr>
            <td><strong>Mask Physical Calibration</strong></td>
            <td>Dual anchor calibration in Library sampling unexposed base ($D_{\min}$) and exposed leader ($D_{\max}$), shared across the roll</td>
            <td>Per-frame estimation using local unexposed rebate or frame highlight density distribution</td>
        </tr>
        <tr>
            <td><strong>Inter-Frame Consistency</strong></td>
            <td>Much more consistent. Largely removes color casts and tonal drift between frames from the same scene</td>
            <td>Independent per frame. Requires visual fine-tuning or manual propagation using Copy Settings</td>
        </tr>
        <tr>
            <td><strong>Batch Automation</strong></td>
            <td>Enables one-click <strong>Auto Invert Roll</strong>, scanning the roll-wide peak white point to invert all frames sequentially</td>
            <td>Requires frame-by-frame processing or selective broadcasting using <strong>Batch Apply</strong></td>
        </tr>
        <tr>
            <td><strong>Archival Management</strong></td>
            <td>Recorded as structured rolls in the SQLite database, accessible in <strong>Rolls</strong> for contact sheets and batch export</td>
            <td>Maintained in transient session lists, with the option to convert frames into a permanent archival roll</td>
        </tr>
    </tbody>
</table>

For cohesive documentary, portrait, or commercial projects photographed across complete rolls, the Roll Physical Calibration Workflow is the standard recommendation. For quick inspection or ad-hoc adjustments, the Loose Frames Workflow offers immediate setup.

## Dual Anchor Calibration: Measuring D-Min and D-Max {#physical-calibration-anchors}

The inter-frame color consistency delivered by the roll workflow relies on establishing two empirical physical boundary anchors on the chemical emulsion: the **unexposed clear base ($D_{\min}$)** and the **fully exposed, maximum density leader ($D_{\max}$)**.

When you load 135 film, the leader pulled from the cassette is exposed to daylight and develops to maximum silver and dye density. That is the chemical ceiling for this roll ($D_{\max}$). The unexposed emulsion inside the cassette throat, or between frames, receives no light and records only the support plus residual orange dye couplers ($D_{\min}$). Together, these two measurements set the physical dynamic range for the roll under its particular development.

In plain terms, **film base** is the reference white for the roll, and **the leader** is the reference black. Neither is picture content, but both anchor color and contrast across every frame. This is also why frames used for calibration are marked separately and kept out of normal grading.

Calibration is executed directly within the **Library** view:

1. Import the complete roll, ensuring digitized frames include the unexposed rebate and the black leader frame.
2. In the Library toolbar, select **Calibrate Film** to launch the guided calibration sequence.
3. Follow the onscreen prompt to click an area of pure, unexposed film base, confirming red, green, and blue base density vectors. Next, navigate to the black leader frame and sample the darkest area to measure maximum density.
4. Click **Confirm Calibration**. NexFilm locks the global $(D_{\min}, D_{\max})$ density span for the entire roll.

<div class="doc-callout">
    <div class="doc-callout-title">Automated Calibration Frame Exclusion</div>
    <div class="doc-callout-body">Frames designated as leader or base calibration samples serve purely photometric measurement functions and contain no active photographic imagery. NexFilm flags these exposures as <strong>Calibration Frames</strong> in the database. When editing in <strong>Develop</strong>, launching <strong>Auto Invert Roll</strong>, or performing <strong>Export Roll</strong>, the engine automatically bypasses these reference frames, preventing black leader frames from cluttering your final export queue.</div>
</div>

## Whole Roll Processing and Shared White Point Anti-Divergence {#whole-roll-batch-processing}

When batch processing film in conventional photo software, photographers often run into color jumping across consecutive shots. Imagine a backlit sequence of three frames: a wide shot with plenty of sky, a close-up dominated by dark shadows, and a portrait. If automatic levels stretch each histogram on its own, the wide shot may go cyan, the shadow shot may go magenta and look overexposed, and the portrait skin may drift yellow.

NexFilm solves this problem with its **Auto Invert Roll** batch architecture:

<div class="doc-callout">
    <div class="doc-callout-title">What Is a Shared White Point?</div>
    <div class="doc-callout-body">It is not the brightest pixel from each individual frame. NexFilm first finds the real highlight limit across the roll and then uses that same white point for every frame. A sky-heavy frame and a shadow-heavy frame can then share the same scene color instead of being pulled in different directions by local auto levels.</div>
</div>

1. **Unified Base Mask Subtraction**: Every frame is stripped of its orange mask with the calibrated global $D_{\min}$ density vector, reducing variation caused by the base.
2. **Roll-Wide Photometric Analysis**: Prior to inverting individual frames, the batch routine scans the optical density of all saved film areas across the roll, identifying the single brightest genuine highlight across the entire sequence.
3. **Global Shared White Point Synchronization**: The software establishes this roll-wide highlight density as the shared white point anchor for every exposure in the roll.

Because all frames share the same black and white density mapping constants, bright sky does not force the midtones down, and dark shadows do not trigger a sudden lift. Contrast and color balance stay close to the lighting recorded at capture time, and consecutive frames hold together much better.

## 120 Medium Format and Headless Negative Fallback Pipeline {#120-and-headless-fallback}

Photographers frequently digitize 120 medium format roll film (which features paper backing and produces no fogged leader upon processing) or 135 strips whose leader tabs were discarded by the processing laboratory. In such situations, physical $D_{\max}$ leader sampling is impossible.

To address this condition, NexFilm incorporates a **Bounded Channel Span Response** fallback model:

<table class="doc-table">
    <thead>
        <tr><th>Response Interval</th><th>Maximum Channel Span Ratio</th><th>Mathematical Control Function</th><th>Tonal Preservation Objective</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Dead Zone</strong></td>
            <td>Ratio $\le 1.15\times$</td>
            <td>Zero channel span stretching; retains natural logarithmic density ratios</td>
            <td>Prevents monochromatic scenes (such as clear blue skies or green foliage) from being forcibly neutralized by balance algorithms</td>
        </tr>
        <tr>
            <td><strong>Active Range</strong></td>
            <td>$1.15\times < \text{Ratio} \le 1.35\times$</td>
            <td>Smooth non-linear bounded convergence weighting across active channels</td>
            <td>In the absence of a physical leader anchor, dynamically extends highlight contrast within safe bounds, preserving organic tonal rolloff</td>
        </tr>
        <tr>
            <td><strong>Hard Cap Ceiling</strong></td>
            <td>Ratio $\ge 3.0\times$</td>
            <td>Strict clipping limit forbidding any channel from stretching beyond 3x the master channel</td>
            <td>Avoids the usual gray-world clipping, color banding, and blown highlights that show up in extreme-contrast frames</td>
        </tr>
    </tbody>
</table>

When working with a roll that has no leader, there is no need to sample one in the Library. Open the frame in Develop and choose **Auto Invert**. The bounded span model reads the local base density and places the highlight limit inside the $1.15\times$ to $1.35\times$ safety window. The result usually stays crisp and natural without pushing the emulsion past what it can hold.

## What Loose Import Can and Cannot Do {#loose-import-to-archive}

The Loose Import workflow is handy for one-off adjustments. Drop RAW or TIFF files into the window and start grading without creating a roll entry.

It is not, however, the same kind of archive as Import by Roll. Loose frames do not appear as roll cards in **Rolls**, and they do not take part in roll calibration, shared white points, contact sheets, or roll-wide batch processing. They can be graded and exported one at a time, which makes them useful for test frames, quick checks, and scattered material, but they are not a good primary archive for a real roll.

If a set of scans clearly belongs to one roll, use Import by Roll from the beginning. Frames already handled as Loose Imports cannot currently be merged into a roll from the interface. To put them into the roll archive, import them again as a roll. Keep any previous grading notes or proof exports before doing so, and avoid repeating work unnecessarily.

<div class="doc-callout">
    <div class="doc-callout-title">Relocating Source Scans for Loose Imports</div>
    <div class="doc-callout-body">Loose frames strictly observe the same read-only storage commitments as structured rolls. If source scan files are moved to another drive or directory, click <strong>Locate File</strong> in the toolbar to reconnect the missing paths. All adjustment parameters, crop data, and snapshots are preserved in the local database.</div>
</div>

Combining structured roll calibration with rapid loose frame grading allows photographers to adhere to rigorous scientific standards while maintaining flexibility across diverse digitization projects.
