---
id: print-film-emulation
track: user-manual
trackOrder: 2
chapterOrder: 5
kicker: "02 / User Manual"
title: "Print Film and Paper Emulation (3D LUTs) and B&W Silver Halide Workflows"
subtitle: "A two-stage physical print mapping model, detailed analyses of all 14 built-in .cube LUTs, and darkroom variable contrast grade workflows."
readingTime: 9
inApp: true
sinceVersion: "v1.1.0"
---

## Two-Stage Physical Print Mapping Model {#two-stage-physics}

For more than a century, a developed negative was not treated as the finished print. The frame exposed in-camera records scene density in silver halide crystals or dye couplers; it is an intermediate medium. In an optical darkroom, a finished photograph takes two physical steps:

1. **Stage 1: Negative Development and Density Inversion (Scene Reconstruction)**
   The orange film base mask is neutralized, and transmission density ratios are inverted according to the spectral sensitivity of the emulsion layers. This stage focuses on objective, uncolored physical scene reconstruction, preserving complete dynamic range from shadows to highlights.
2. **Stage 2: Secondary Paper and Print Film Exposure (Print Medium Mapping)**
   Inverted positive light from the enlarger lens projects onto photographic paper or motion picture print film. The characteristic sensitometric curve (H&D curve) of the print emulsion, its shoulder highlight rolloff, maximum reflective silver density ($D_{\max}$), and unique spectral dye absorption impart the distinctive organic texture and aesthetic character associated with analog film.

> [!NOTE] Avoiding Destructive One-Step Curves
> Many digital presets attempt to simulate film by forcing harsh S-curves across RGB channels in a single step. This destructive approach crushes delicate shadow textures, clips specular highlights, and causes unnatural hue splits. NexFilm enforces a physical two-stage pipeline: the core engine first accomplishes balanced density inversion, followed by high-precision three-dimensional lookup tables (3D LUTs) that map positive tones into authentic sensitometric print spaces.

## Built-in 14 Official .cube LUT Emulation Matrix {#builtin-luts-matrix}

NexFilm natively distributes 14 production-grade `.cube` 3D LUT profiles. Measured using laboratory spectrophotometers and sensitometric step wedges, these profiles represent authentic physical media rather than subjective commercial filters.

The matrix below provides complete specifications for all 14 built-in LUT profiles:

<table class="doc-table">
    <thead>
        <tr><th>Exact LUT Filename</th><th>Physical Medium Classification</th><th>Sensitometric Curve and Spectral Response</th><th>Recommended Negative Stocks and Creative Applications</th></tr>
    </thead>
    <tbody>
        <tr><td><code>Kodak 2383.cube</code></td><td>35mm color motion picture print stock</td><td>The benchmark of Hollywood theatrical releases. Smooth, luminous highlight rolloff, deep cool-toned shadows, rich midtone density, and excellent skin tone separation.</td><td>Kodak Vision3 500T/250D motion picture negatives, cinematic street photography, and atmospheric night work.</td></tr>
        <tr><td><code>Kodak 2393.cube</code></td><td>High-contrast color motion picture print stock</td><td>Offers deeper, punchier black density and higher overall contrast than 2383, delivering vivid saturation and dramatic separation.</td><td>Low-key night scenes, high-contrast theatrical lighting, neon urban vistas, and bold compositions.</td></tr>
        <tr><td><code>Kodak Portra Endura.cube</code></td><td>Professional portrait silver halide paper</td><td>The traditional standard for portraiture. Ultra-smooth highlight transitions, open shadow detail, and accurate reproduction of delicate skin tones.</td><td>Kodak Portra 160/400/800, natural light portraiture, weddings, and documentary projects.</td></tr>
        <tr><td><code>Kodak Endura Premier.cube</code></td><td>Modern professional silver halide paper</td><td>The premier standard for contemporary commercial display. Features rich saturation, crisp clean whites, and high optical acuity.</td><td>Commercial still life, modern architectural interiors, editorial fashion, and vivid street scenes.</td></tr>
        <tr><td><code>Kodak Supra Endura.cube</code></td><td>All-round professional color paper</td><td>A balanced classic paper stock. Moderate contrast, vibrant color reproduction, and high archival stability.</td><td>Kodak Gold 200, ColorPlus 200, travel documentation, environmental portraits, and general photography.</td></tr>
        <tr><td><code>Kodak Ultra Endura.cube</code></td><td>Ultra-high saturation and contrast paper</td><td>Maximum visual impact. Dramatic separation between shadows and highlights, intense primary colors, and dense blacks.</td><td>Kodak Ektar 100, sweeping natural landscapes, autumn foliage, and bold graphic compositions.</td></tr>
        <tr><td><code>Kodak Ektacolor Edge.cube</code></td><td>Classic consumer snapshot color paper</td><td>Evokes family photo albums of the 1980s and 1990s. Gentle warm-yellow tint, relaxed midtone contrast, and nostalgic character.</td><td>Expired color films, everyday snapshots, candid street photography, and vintage-themed work.</td></tr>
        <tr><td><code>Ilford MGFB 00.cube</code></td><td>Variable contrast fiber-base paper (Grade 00)</td><td>Extremely flat, soft tone curve with maximum latitude. Compresses harsh highlights and deep shadows into a printable scale.</td><td>Extreme direct sunlight, backlit negatives, and overdeveloped contrasty frames.</td></tr>
        <tr><td><code>Ilford MGFB 0.cube</code></td><td>Variable contrast fiber paper (Grade 0)</td><td>Soft contrast delivery preserving delicate gradations in both deep shadows and subtle highlights.</td><td>Diffused indoor portraits, misty mornings, and negatives requiring shadow detail retention.</td></tr>
        <tr><td><code>Ilford MGFB 1.cube</code></td><td>Variable contrast fiber paper (Grade 1)</td><td>Mildly soft paper grade suited for smoothing out negatives developed with excessive gamma.</td><td>Direct sunlight street work, high-contrast architecture, and harsh ambient lighting.</td></tr>
        <tr><td><code>Ilford MGFB 2.cube</code></td><td>Variable contrast fiber paper (Grade 2 Normal)</td><td>The universal darkroom standard. Balanced tonal distribution across blacks, grays, and whites following classical monochrome aesthetics.</td><td>Ilford HP5 Plus, Kodak Tri-X 400, and general black-and-white photography under balanced lighting.</td></tr>
        <tr><td><code>Ilford MGFB 3.cube</code></td><td>Variable contrast fiber paper (Grade 3)</td><td>Moderate contrast boost separating subtle gray tones and adding punch to flat negatives.</td><td>Overcast skies, low-contrast vintage lenses, and flat lighting captures.</td></tr>
        <tr><td><code>Ilford MGFB 4.cube</code></td><td>Variable contrast fiber paper (Grade 4)</td><td>Hard contrast rendering that deepens midtones and accentuates strong geometric silhouettes.</td><td>High-contrast street photography in the Daido Moriyama tradition, gritty night scenes, and graphic abstracts.</td></tr>
        <tr><td><code>Ilford MGFB 5.cube</code></td><td>Variable contrast fiber paper (Grade 5)</td><td>Ultra-hard contrast with heavily compressed midtones, creating stark, graphic black-and-white images.</td><td>Graphic silhouettes, stark chiaroscuro studies, high-impact monochrome compositions, and experimental prints.</td></tr>
    </tbody>
</table>

## Opacity Blending and Custom 3D LUT Specifications {#opacity-and-custom-luts}

The **Print Emulation** module provides flexible controls for fine-tuning medium characteristics:

- **Opacity Blending (0% – 100%)**:
  Defaults to 100% for full paper emulation. The slider blends continuously. At 60%–80%, the image keeps the neutral density of the inversion while picking up some of the highlight rolloff and tonal weight of photographic paper. Choosing "No Built-in LUT" bypasses the shader pass.
- **Custom Third-Party `.cube` Loading**:
  Clicking "Load Custom LUT" opens the native file picker to import custom lookup tables. NexFilm supports standard Adobe `.cube` files across grid resolutions of 17×17×17, 33×33×33, and 65×65×65, evaluated via hardware trilinear interpolation.

> [!WARNING] Never Apply Digital Cinema Camera Log LUTs
> The 3D LUT engine operates downstream of negative inversion on positive display-referred data. Never apply Log-to-Rec709 conversion LUTs designed for digital cinema cameras (such as ARRI LogC or Sony S-Log3), as this will introduce severe saturation clipping and color distortions.

## Pure Silver Halide Monochrome Darkroom Workflow {#bw-darkroom-workflow}

For black-and-white photographers, NexFilm offers an authentic digital equivalent of working with traditional Multigrade Variable Contrast paper under an enlarger:

1. **Switch to B&W Mode**:
   Engage **B&W** in the inspector mode switcher. The pipeline combines the three color channels into a single optical density channel and removes the color dye masks; the histogram switches to monochrome luminance.
2. **Define Boundaries and Run Auto Invert**:
   Frame the active negative area and trigger **Auto Invert**. The engine measures the minimum and maximum silver densities to construct a balanced positive.
3. **Select an Ilford MGFB Paper Grade**:
   In the Print Emulation dropdown, simulate changing dichroic enlarger filters:
   - For flat, overcast scenes, apply `Ilford MGFB 3` or `4` to tighten midtones and clean up highlights;
   - For harsh direct sunlight negatives with dense highlights, apply `Ilford MGFB 0` or `00` to expand shadow separation.
4. **Preserve Authentic Silver Grain**:
   In the Detail panel, advance Masking to 40 or higher. This protects the grain structure of the emulsion while focusing contrast adjustments strictly along primary image edges.

## Emulation Boundaries and Downstream Workflow Parity {#downstream-interop}

Understanding how print emulation interfaces with downstream post-production tools ensures seamless integration into professional pipelines:

- **Standalone Delivery within NexFilm**:
  When preparing images for digital publication, social sharing, client handoff, or fine art giclée printing, apply the desired paper or film print LUT directly within the Develop workspace. Export as 16-bit TIFF or sRGB JPEG to complete your workflow in one pass.
- **DaVinci Resolve Cinema Workflows**:
  If negative captures are destined for DaVinci Resolve alongside digital cinema footage, **set Print Emulation to No Built-in LUT and export as 16-bit Linear TIFF or Linear DNG**. Delivering uncompressed linear positives allows the colorist to apply Kodak 2383 or proprietary print profiles within Resolve's ACES or DaVinci Wide Gamut environment, preserving pipeline integrity.
