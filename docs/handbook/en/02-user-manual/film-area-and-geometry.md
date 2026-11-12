---
id: film-area-and-geometry
track: user-manual
trackOrder: 2
chapterOrder: 3
kicker: "02 / User Manual"
title: "Film Area Definition, Geometric Transformations, and Sprocket Management"
subtitle: "Separating optical analysis bounds from composition crop, correcting reprographic perspective and distortion, and fine-tuning sprocket hole sampling."
readingTime: 8
inApp: true
sinceVersion: "v1.1.0"
---

## Core Differences Between Film Area and Composition Crop {#film-area-vs-crop}

In conventional digital photo editing software, "cropping" is typically viewed merely as a composition and aspect ratio formatting tool. In NexFilm's physical darkroom architecture, however, geometry is split into two functionally distinct operations positioned at opposite ends of the rendering pipeline: **Film Area** and **Composition Crop**. Understanding this boundary is essential for achieving accurate optical inversion.

The core distinctions and pipeline roles are defined as follows:
- **Film Area establishes the optical analysis domain**: It defines the bounding polygon encompassing valid, light-sensitive emulsion on the negative scan. The engine analyzes strictly the pixels bounded by this area to calculate minimum optical density (D-Min / unexposed film base), maximum optical density (D-Max / film leader), and channel dynamic spans. Even if your final artistic composition requires a tighter crop, the Film Area should enclose the entire valid image area so the inversion solver possesses maximum dynamic range samples.
- **Composition Crop determines the final deliverable framing**: It works after physical inversion and tone curve adjustments, defining the exported aspect ratio (for example, 3:2, 1:1, or freeform). Composition cropping does not discard the density statistics collected by the Film Area. Tightening or changing the crop after inversion therefore leaves overall contrast and color balance where they were.

Both tools share the same canvas coordinate system. The Film Area is processed before density calculations, while Composition Crop is applied at the end of rendering.

## Framing Pitfalls and Optical Mechanisms {#framing-pitfalls}

When digitizing negatives with a camera or flatbed, frames are often surrounded by raw backlight, opaque film-holder borders, or 35mm sprocket holes. If the Film Area is drawn carelessly, automatic inversion is working from badly contaminated samples.

The table below outlines the three most frequent framing mistakes and their optical failure modes:

<table class="doc-table">
    <thead>
        <tr><th>Framing Mistake</th><th>Spurious Pixels Included in Analysis</th><th>Internal Algorithm Miscalculation</th><th>Resulting Image Defect</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Enclosing Raw Light Table Backlight</strong></td><td>Unobstructed light table illumination outside the negative strip (pure white light at ~100% transmission).</td><td>The engine interprets the extreme brightness of the light source as the minimum film density (D-Min base), depressing all genuine negative highlight and shadow densities downward.</td><td>The inverted positive appears heavily flattened, dark, and muddy, exhibiting crushed dynamic range with blocked shadows and suppressed highlights.</td></tr>
        <tr><td><strong>Enclosing Opaque Film Holder Borders</strong></td><td>Fully opaque black plastic or metal borders from scanning masks and film carrier edges.</td><td>The engine interprets pure physical occlusion (near-zero transmission) as full film exposure (D-Max leader), artificially pushing the black point baseline upward.</td><td>Shadow contrast is lost; deep blacks lift unnaturally into a milky, washed-out grey haze.</td></tr>
        <tr><td><strong>Enclosing 135 Perforation Sprocket Holes</strong></td><td>Unfiltered backlight shining through 35mm sprocket perforations and associated flare halos.</td><td>Intense light unattenuated by negative dye couplers corrupts three-channel transmission ratios, triggering channel-span protection guards incorrectly.</td><td>Severe, stubborn color casts (frequently cyan, magenta, or yellow) dominate frame edges or whole images, resisting standard white balance dropper adjustments.</td></tr>
    </tbody>
</table>

> [!WARNING] Never Enclose Raw Backlight Inside the Film Area
> Whether working with a high-key daylight landscape or a low-key night exposure, the four corners of your Film Area must align snugly against valid image emulsion, strictly excluding external backlight. If you desire artistic black borders or rebate borders in your final image, use the Sprocket module or dedicated border emulations rather than expanding the Film Area.

## Auto Area Detection and Manual Polygon Adjustment {#auto-area-workflow}

Upon entering the Develop workspace, the toolbar above the main canvas presents the complete suite of boundary adjustment controls:

1. **Entering Boundary Edit Mode**: Click the **Set Film Area** button on the toolbar. The canvas switches into boundary mode: regions outside the active polygon dim with a semi-transparent dark mask, and an interactive polygon with four corner pins and edge handles appears.
2. **Auto Area Detection**: Click **Auto Area**. The background engine executes an edge gradient detection pass over the cached thumbnail, locating the contrast boundary between the film frame and the rebate border. This routine typically resolves the boundary within 200 milliseconds.
3. **Manual Fine-Tuning**: Because images with low-contrast skies or dark shadow edges may confuse edge detection, inspect the bounding polygon. Drag any corner pin independently or pull an edge center handle to align the bounding box with the emulsion edge. If you wish to utilize unexposed film borders for base color estimation, leave approximately 1 millimeter of clean unexposed film rebate along one edge, ensuring no raw backlight is exposed.
4. **Saving Boundary Changes**: Once adjusted, click the green **Save Area** button on the toolbar. Navigating away via the filmstrip without saving will discard boundary edits.
5. **Batch Application on Copy Stands (Batch Apply)**: When shooting a full roll on a rigid copy stand where negative positioning remains identical across frames, calibrate the Film Area on your initial frame and save it. Then click **Batch Apply** in the toolbar to broadcast the boundary to all remaining frames in the roll. Target frames inherit the geometry while independently re-measuring localized film base values to compensate for minor light table falloff.

## Geometric Transformations: Rotation, Perspective, and Distortion Correction {#geometry-and-perspective}

Even with bubble levels on reprographic copy stands, subtle tilt or camera pitch misalignments can occur between the camera sensor plane and the film plane. Furthermore, macro lenses at 1:1 reproduction ratios can introduce minor geometric distortion. NexFilm provides comprehensive geometric alignment tools:

- **Orthogonal Rotation and Mirroring**:
  - Rotate 90° clockwise or counter-clockwise via the canvas toolbar to quickly orient vertical frames.
  - **Flip Horizontal (Flip H)** and **Flip Vertical (Flip V)**: If a negative strip is inadvertently loaded upside down with emulsion facing backward, click Flip H to restore proper reading orientation without physical rescanning.
- **Straighten**: An angle rotation slider calibrated to 0.1° increments (spanning -45° to +45°) paired with an on-canvas overlay grid enables rapid leveling of tilted horizons.
- **Perspective Correction**: Found in the inspector Geometry panel, providing **Vertical Perspective** and **Horizontal Perspective** sliders. Dual-axis adjustments counteract keystone distortion caused by sensor-to-film tilt, restoring rectangular geometry to skewed negatives.
- **Aspect and Scale Governance**: **Perspective Aspect** fine-tunes horizontal-to-vertical dimensional proportions following keystone correction, while **Perspective Scale** and **Constrain Crop** prevent dark canvas borders from intruding into the active composition.
- **Lens Distortion Compensation**: Compensates for barrel or pincushion distortion inherent to close-focus macro reproduction optics, straightening curved film frame lines.
- **Aspect Ratio Locking**: Supports standard aspect ratio constraints including Free, Original, 1:1 (square medium format), 2:3 (135 full frame), 3:4, 4:5 (large format sheets), and 16:9 widescreen crops.

## Sprocket Hole Sampling and Edge Feathering Control {#sprocket-control}

For 35mm sprocket-hole photography, or whenever you want to keep an uncropped film-rebate look, the inspector has a dedicated **Sprocket** module:

- **Sample Sprocket Hole**: Activates a precision eyedropper tool. Clicking within an exposed perforation hole samples the unattenuated backlight color and intensity directly from that frame.
- **Tolerance**: Adjusts the detection window around the sampled perforation brightness. Increasing tolerance ensures that flare halos and optical dispersion along perforation edges are effectively neutralized, preventing color contamination across adjacent emulsion.
- **Feather**: Regulates the transition radius between raw perforations and active negative emulsion. Proper feathering yields smooth, organic transitions, preventing digital stair-stepping or artificial edge fringing.

By pairing rigorous optical boundary definition with comprehensive perspective correction, NexFilm ensures every negative enters the density inversion stage on an authentic, calibrated physical foundation.
