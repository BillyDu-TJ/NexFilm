---
id: roll-vs-loose-workflows
track: getting-started
trackOrder: 1
chapterOrder: 4
kicker: "01.4 / Getting Started"
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
            <td>Maximum consistency. Completely eliminates color casts and tonal divergence across frames within the same scene</td>
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

When loading 135 film, the leader tongue pulled from the cassette is exposed to daylight and achieves maximum silver and dye density upon development, representing the chemical saturation ceiling ($D_{\max}$). In contrast, the unexposed emulsion inside the cassette throat or the spacing between frames receives zero light exposure, recording only the optical density of the polyester or acetate support and the residual orange dye couplers ($D_{\min}$). Together, these two measurements define the physical dynamic range of the emulsion under specific development chemistry.

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

When batch processing film in conventional photo editing software, photographers encounter color jumping across consecutive shots. In a backlit sequence of three shots—one wide angle showing sky, one close-up of dark shadows, and one portrait—independent automatic levels tools stretch each histogram separately based on local pixel energy. This causes the wide shot to appear cyan, the shadow shot to appear magenta and over-exposed, and skin tones in the portrait to drift yellow.

NexFilm solves this problem with its **Auto Invert Roll** batch architecture:

1. **Unified Base Mask Subtraction**: Every frame across the roll is stripped of its orange mask using the calibrated global $D_{\min}$ density vector, eliminating base-induced frame variances.
2. **Roll-Wide Photometric Analysis**: Prior to inverting individual frames, the batch routine scans the optical density of all saved film areas across the roll, identifying the single brightest genuine highlight across the entire sequence.
3. **Global Shared White Point Synchronization**: The software establishes this roll-wide highlight density as the shared white point anchor for every exposure in the roll.

Because all frames reference identical black and white density mapping constants, bright sky areas do not cause midtones to collapse, and dark shadows do not trigger spurious brightening. Contrast and color balance reflect the actual scene lighting observed during exposure, preserving color coherence across consecutive frames.

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
            <td>Completely eliminates gray-world clipping, color banding, and blown highlight artifacts in extreme contrast exposures</td>
        </tr>
    </tbody>
</table>

When working with headless negatives, users do not need to sample a leader in the Library. Open the frame in Develop and select **Auto Invert**. The bounded span model evaluates the local rebate base density and dynamically anchors highlight limits within the $1.15\times$ to $1.35\times$ safety window, delivering crisp, natural contrast without breaking emulsion physics.

## Managing Loose Imports and Archival Roll Association {#loose-import-to-archive}

The Loose Import workflow provides instant access for single-frame adjustments. Dragging RAW or TIFF files directly into the window allows immediate grading without catalog registration.

However, leaving assets in loose status long-term complicates retrieval and precludes contact sheet compilation. NexFilm bridges transient sessions with permanent roll archives in the **Rolls** view:

1. **Reviewing Loose Sessions**: Open the **Rolls** view in the top header. The left sidebar displays permanent catalog rolls alongside a dedicated **Loose Imports Archive** container.
2. **Multi-Frame Selection**: In the loose frames list, hold the <kbd>Shift</kbd> key to select frames belonging to a common photographic set.
3. **Consolidation into Permanent Rolls**: Click **Convert to New Roll** at the bottom of the list. A roll metadata dialog prompts for name, film stock, and camera details. Confirming the conversion updates the foreign key records in the SQLite database, grouping the selected frames into a permanent archival roll.

<div class="doc-callout">
    <div class="doc-callout-title">Relocating Source Scans for Loose Imports</div>
    <div class="doc-callout-body">Loose frames strictly observe the same read-only storage commitments as structured rolls. If source scan files are moved to another drive or directory, click <strong>Locate File</strong> in the toolbar to reconnect the missing paths. All adjustment parameters, crop data, and snapshots are preserved in the local database.</div>
</div>

Combining structured roll calibration with rapid loose frame grading allows photographers to adhere to rigorous scientific standards while maintaining flexibility across diverse digitization projects.
