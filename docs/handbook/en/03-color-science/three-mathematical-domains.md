---
id: three-mathematical-domains
track: color-science
trackOrder: 3
chapterOrder: 1
kicker: "Color Science 01 / Domain Decoupling"
title: "The Three Mathematical Domains of Negative Inversion"
subtitle: "Rigorous physical derivations and the non-commutative theorem across transmittance, optical density, and print output domains."
readingTime: 14
inApp: true
sinceVersion: "v1.1.0"
aliases: ["color-science"]
---

## Transmittance Domain: Pre-Log Optical Response & Device Basis {#transmittance-domain-pre-log}

In negative film digitization, the physical quantity recorded by an area-array CMOS sensor or a linear CCD scanner is not optical density. Rather, the sensor integrates the radiant photon flux that traverses the developed photographic emulsion and reaches each photosite. The transmittance domain refers explicitly to the linear-light space prior to logarithmic transformation, where measurements remain strictly proportional to incident physical irradiance. The objective of this domain is to obtain a physically meaningful, bounded, positive relative transmittance vector.

Let $I_{\text{open}}(\lambda)$ represent the spectral radiant power incident across the scanning gate without film, let $I_{\text{transmitted}}(\lambda)$ denote the transmitted flux through the developed negative, and let $I_{\text{dark}}(\lambda)$ quantify the combined contributions of sensor thermal dark current and internal optical flare within the scanning chamber. The spectral relative transmittance $T(\lambda)$ is defined as:

$$T(\lambda) = \frac{I_{\text{transmitted}}(\lambda) - I_{\text{dark}}(\lambda)}{I_{\text{open}}(\lambda) - I_{\text{dark}}(\lambda)}$$

For a discrete trichromatic imaging system, channel responses correspond to integrals of the transmitted spectrum weighted by color filter array transmissions and sensor quantum efficiencies. Denoting the raw digitized device vector as $I_{\text{device}} = [R, G, B]^T$, all operations performed within the transmittance domain must preserve radiant linearity:

1. **Dark Level and Stray Light Subtraction**: Removing thermal dark current and internal housing reflections ensures that zero transmitted light yields a true physical zero reading.
2. **Flat-Field Gain Normalization**: Compensating for lens cosine-fourth illumination falloff and backlight spatial chromaticity nonuniformity across the field of view.
3. **Capture Separation Matrix**: Decoupling the overlapping spectral bands of camera Bayer dyes or scanner beam splitters, aligning the channels with the actual optical absorption bands of the cyan, magenta, and yellow film dyes.

If channel separation is omitted at the capture stage, dye absorption signals remain entangled throughout all downstream operations. To prevent mathematical singularities during logarithmic mapping, the evaluated transmittance is clamped to a tiny positive threshold $\epsilon$:

$$T_{\text{valid}} = \max(T, \epsilon), \quad \epsilon = 1.0 \times 10^{-6}$$

> [!IMPORTANT]
> Values within the transmittance domain represent radiometric physical quantities. Any display-referred nonlinearities, monitor gamma curves, or perceptual contrast adjustments are strictly prohibited within this initial stage.

## Optical Density Domain: Post-Log Additive Absorption & Base Subtraction {#optical-density-domain-post-log}

Following logarithmic transformation, the signal transitions from the transmittance domain into the optical density domain. Under the Beer-Lambert absorption law, the attenuation of monochromatic light traversing a homogeneous absorbing medium follows an exponential decay. Evaluating the negative base-10 logarithm transforms multiplicative transmission attenuation into linear additive absorbance, establishing a tractable framework for dye layer decomposition.

The raw acquired optical density vector $D_{\text{raw}}$ is defined as the common logarithm of the reciprocal of valid relative transmittance:

$$D_{\text{raw}} = -\log_{10}(T_{\text{valid}}) = \begin{bmatrix} -\log_{10}(T_R) \\ -\log_{10}(T_G) \\ -\log_{10}(T_B) \end{bmatrix}$$

In chromogenic color negatives processed under standard C-41 chemistry, the unexposed film rebates contain residual unreacted color couplers forming an orange cast. This unexposed substrate introduces a substantial constant additive baseline. Designating the measured unexposed film base density under identical scanning conditions as $D_{\text{base}}$, the net optical density $D_{\text{net}}$ is obtained through channel-wise subtraction:

$$D_{\text{net}} = D_{\text{raw}} - D_{\text{base}} = \begin{bmatrix} D_{\text{raw}, R} - D_{\text{base}, R} \\ D_{\text{raw}, G} - D_{\text{base}, G} \\ D_{\text{raw}, B} - D_{\text{base}, B} \end{bmatrix}$$

Within the net optical density space, the developed dye mass exhibits a predictable inverse relationship with original scene exposure. Provided the photographic emulsion operates between the toe underexposure threshold and the shoulder saturation plateau, net density adheres closely to the classic Hurter-Driffield sensitometric formulation:

$$D_{\text{net}, c} \approx \gamma_c \cdot \log_{10}(E_c) + C_c, \quad c \in \{R, G, B\}$$

where $\gamma_c$ denotes the photographic development contrast factor for dye layer $c$. Within this optical density domain, essential darkroom operations reduce to elementary linear algebra:

| Density Operation | Physical Analog & Darkroom Practice | Governing Mathematical Formulation |
| --- | --- | --- |
| Film Base Subtraction | Eliminating polyester support and unexposed orange coupler absorption | $D_{\text{net}} = D_{\text{raw}} - D_{\text{base}}$ |
| Printer Lights Grading | Emulating red, green, and blue additive/subtractive optical enlarger head filtration | $D_{\text{graded}} = D_{\text{net}} + \Delta P_{\text{light}}$ |
| Density Window Normalization | Mapping scene negative density span to target display luminance bounds | $D_{\text{norm}} = (D_{\text{high}} - D) / (D_{\text{high}} - D_{\text{low}})$ |
| Spectral Crosstalk Decoupling | Correcting magenta secondary absorption in high-density regions | $D_{\text{calibrated}} = M_{\text{crosstalk}} \cdot D_{\text{net}}$ |

## Positive Print & Output Domain: Linear ProPhoto to Display Rendering {#positive-print-and-output-domain}

Even after film base subtraction and printer lights balance, the net density vector represents an analytical measurement of physical absorption rather than a displayable positive image. The positive print and output domain is responsible for mapping inverted relative exposures into a standardized, colorimetrically defined linear workspace, followed by aesthetic tone reproduction and display-referred encoding.

The NexFilm Engine utilizes Linear ProPhoto RGB (ROMM RGB with an identity linear transfer function) as its universal internal positive working space. Once inverted relative exposures are placed within this wide-gamut linear framework, subsequent grading and mastering proceed systematically:

1. **Working Space Linearization**: Converting normalized logarithmic density into linear positive exposure $E_{\text{pos}} = 10^{-\alpha \cdot D_{\text{graded}}}$, restoring radiant proportionality to the positive scene estimate.
2. **Per-Channel and Master Tone Curves**: Applying discrete red, green, and blue tone curves to shape shadow and highlight color balance, followed by a composite luminance curve modeling traditional print paper D-log-H characteristics.
3. **Print Film Emulation 3D LUTs**: Evaluating verified three-dimensional lookup tables calibrated against motion picture print stocks (such as Kodak 2383 or Vision Premier 2393), conferring characteristic deep shadow d-max and rich dye separation.
4. **Target Color Space Transform & OETF Encoding**: Projecting positive color coordinates into target output profiles (sRGB, Display P3, Adobe RGB, or ACEScg) and applying corresponding optical-electro transfer functions.

> [!NOTE]
> Contrast adjustments, shadow recovery, and saturation balancing in the positive print domain are perceptual choices. Those display-referred transforms should not feed back into the optical density measurements.

## Non-Commutative Matrix Proof: The Invalidation of Density Color Space Transforms {#non-commutative-matrix-proof}

A widespread architectural defect in legacy film inversion software is the confusion between transmittance-domain matrices and density-domain matrices. Several heuristic implementations attempt to apply standard monitor color space matrices directly to logarithmic density vectors, asserting that this approximates optical calibration. This section presents a formal proof demonstrating why such operations are mathematically invalid.

Let $x = [T_1, T_2, T_3]^T \in \mathbb{R}_{>0}^3$ represent a strictly positive trichromatic transmittance vector. Let $A \in \mathbb{R}^{3 \times 3}$ denote an optical sensor channel separation matrix with coefficients $a_{ij}$.

In the physical transmittance domain, matrix $A$ acts as a linear operator over radiant flux, decoupling optical channel crosstalk:

$$y_{\text{trans}} = A x, \quad y_i = \sum_{j=1}^3 a_{ij} x_j$$

Mapping this physically corrected transmittance vector into the optical density domain yields the true density vector:

$$D_{\text{correct}} = -\log_{10}(A x) = \begin{bmatrix} -\log_{10}\left(\sum_{j=1}^3 a_{1j} x_j\right) \\ -\log_{10}\left(\sum_{j=1}^3 a_{2j} x_j\right) \\ -\log_{10}\left(\sum_{j=1}^3 a_{3j} x_j\right) \end{bmatrix}$$

Now consider the flawed implementation that applies matrix $A$ after the logarithmic transformation, multiplying raw densities $D_{\text{raw}} = -\log_{10}(x)$ by $A$:

$$D_{\text{wrong}} = A [-\log_{10}(x)] = \begin{bmatrix} \sum_{j=1}^3 a_{1j} [-\log_{10}(x_j)] \\ \sum_{j=1}^3 a_{2j} [-\log_{10}(x_j)] \\ \sum_{j=1}^3 a_{3j} [-\log_{10}(x_j)] \end{bmatrix} = \begin{bmatrix} -\log_{10}\left(\prod_{j=1}^3 x_j^{a_{1j}}\right) \\ -\log_{10}\left(\prod_{j=1}^3 x_j^{a_{2j}}\right) \\ -\log_{10}\left(\prod_{j=1}^3 x_j^{a_{3j}}\right) \end{bmatrix}$$

For $D_{\text{correct}} \equiv D_{\text{wrong}}$ to hold identically across all valid inputs $x$, the arguments of the logarithms must satisfy:

$$\sum_{j=1}^3 a_{ij} x_j \equiv \prod_{j=1}^3 x_j^{a_{ij}}, \quad \forall i \in \{1, 2, 3\}$$

Taking the partial derivative of both sides with respect to arbitrary component $x_k$:

The partial derivative of the left-hand summation evaluates to a constant:

$$\frac{\partial}{\partial x_k} \left(\sum_{j=1}^3 a_{ij} x_j\right) = a_{ik}$$

The partial derivative of the right-hand product yields:

$$\frac{\partial}{\partial x_k} \left(\prod_{j=1}^3 x_j^{a_{ij}}\right) = a_{ik} x_k^{a_{ik}-1} \prod_{j \neq k} x_j^{a_{ij}}$$

Equating these partial derivatives across all $x \in \mathbb{R}_{>0}^3$ requires that matrix $A$ degrade strictly to the identity matrix $I$ or a purely diagonal scaling matrix. Whenever matrix $A$ incorporates off-diagonal cross-coupling terms (where $a_{ij} \neq 0$ for $j \neq k$), this identity collapses:

$$-\log_{10}(A x) \neq A [-\log_{10}(x)]$$

**Conclusion**: Physical radiant flux sums linearly before the logarithm, whereas dye optical absorbance sums algebraically after the logarithm. Attempting to apply a standard chromaticity matrix in the density domain fails to decouple capture hardware variations and introduces severe chromatic distortion across highlight roll-offs and deep shadow transitions.

## Signal Flow Diagram & Concrete Technical Boundaries {#signal-flow-and-physical-boundaries}

To assist engineers and technical colorists in tracing data flow throughout the codebase, the diagram below outlines the NexFilm v1.1.0 signal pipeline across all three mathematical domains:

<svg viewBox="0 0 860 380" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg" style="background:#161618; border-radius:12px; border:1px solid #27272a; font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;">
<rect x="20" y="20" width="250" height="340" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="35" y="48" fill="#e4e4e7" font-size="12" font-weight="bold">Transmittance Domain (Pre-Log)</text>
<text x="35" y="66" fill="#a1a1aa" font-size="10">Radiant Photon Flux · Linear Light</text>
<rect x="35" y="85" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="47" y="104" fill="#fafafa" font-size="11">RAW / Scanner RGB</text>
<text x="47" y="118" fill="#a1a1aa" font-size="9">Black level, demosaic, bad pixels</text>
<rect x="35" y="142" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="47" y="161" fill="#fafafa" font-size="11">Dark Subtraction &amp; Flat Field</text>
<text x="47" y="175" fill="#a1a1aa" font-size="9">T = (I - dark) / (open - dark)</text>
<rect x="35" y="199" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="47" y="218" fill="#fafafa" font-size="11">Capture Separation 3x3</text>
<text x="47" y="232" fill="#a1a1aa" font-size="9">Sensor x backlight decoupling</text>
<rect x="35" y="256" width="220" height="42" rx="6" fill="#3f3f46" stroke="#fbbf24" stroke-width="1.5"/>
<text x="47" y="275" fill="#fbbf24" font-size="11" font-weight="bold">Log Transform with Epsilon</text>
<text x="47" y="289" fill="#fef08a" font-size="9">D_raw = -log10(max(T, eps))</text>
<line x1="145" y1="127" x2="145" y2="142" stroke="#71717a" stroke-width="1.5"/>
<line x1="145" y1="184" x2="145" y2="199" stroke="#71717a" stroke-width="1.5"/>
<line x1="145" y1="241" x2="145" y2="256" stroke="#71717a" stroke-width="1.5"/>
<line x1="270" y1="277" x2="305" y2="277" stroke="#fbbf24" stroke-width="2"/>
<rect x="305" y="20" width="250" height="340" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="320" y="48" fill="#e4e4e7" font-size="12" font-weight="bold">Density Domain (Post-Log)</text>
<text x="320" y="66" fill="#a1a1aa" font-size="10">Beer-Lambert Absorbance Space</text>
<rect x="320" y="85" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="104" fill="#fafafa" font-size="11">Base Sampling &amp; Subtraction</text>
<text x="332" y="118" fill="#a1a1aa" font-size="9">D_net = D_raw - D_base</text>
<rect x="320" y="142" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="161" fill="#fafafa" font-size="11">Printer Lights Grading</text>
<text x="332" y="175" fill="#a1a1aa" font-size="9">Logarithmic additive color shift</text>
<rect x="320" y="199" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="218" fill="#fafafa" font-size="11">Bounded Channel Span Response</text>
<text x="332" y="232" fill="#a1a1aa" font-size="9">1.15x deadzone, 1.35x / 3x caps</text>
<rect x="320" y="256" width="220" height="42" rx="6" fill="#3f3f46" stroke="#34d399" stroke-width="1.5"/>
<text x="332" y="275" fill="#34d399" font-size="11" font-weight="bold">Inversion &amp; Positive Exposure</text>
<text x="332" y="289" fill="#a7f3d0" font-size="9">E_pos = 10^(-alpha * D)</text>
<line x1="430" y1="127" x2="430" y2="142" stroke="#71717a" stroke-width="1.5"/>
<line x1="430" y1="184" x2="430" y2="199" stroke="#71717a" stroke-width="1.5"/>
<line x1="430" y1="241" x2="430" y2="256" stroke="#71717a" stroke-width="1.5"/>
<line x1="555" y1="277" x2="590" y2="277" stroke="#34d399" stroke-width="2"/>
<rect x="590" y="20" width="250" height="340" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="605" y="48" fill="#e4e4e7" font-size="12" font-weight="bold">Positive &amp; Output Domain</text>
<text x="605" y="66" fill="#a1a1aa" font-size="10">Linear ProPhoto · Color Transform</text>
<rect x="605" y="85" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="104" fill="#fafafa" font-size="11">Linear ProPhoto Working Space</text>
<text x="617" y="118" fill="#a1a1aa" font-size="9">Universal linear-light positive buffer</text>
<rect x="605" y="142" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="161" fill="#fafafa" font-size="11">Four-Channel Curves &amp; LUTs</text>
<text x="617" y="175" fill="#a1a1aa" font-size="9">RGB curves and Kodak 2383</text>
<rect x="605" y="199" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="218" fill="#fafafa" font-size="11">Spatial Spatial Enhancements</text>
<text x="617" y="232" fill="#a1a1aa" font-size="9">High-pass mask sharpening, shadows</text>
<rect x="605" y="256" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="275" fill="#fafafa" font-size="11">Display &amp; Master Delivery</text>
<text x="617" y="289" fill="#a1a1aa" font-size="9">sRGB / P3 / ACEScg / 16-bit TIFF</text>
<line x1="700" y1="127" x2="700" y2="142" stroke="#71717a" stroke-width="1.5"/>
<line x1="700" y1="184" x2="700" y2="199" stroke="#71717a" stroke-width="1.5"/>
<line x1="700" y1="241" x2="700" y2="256" stroke="#71717a" stroke-width="1.5"/>
</svg>

NexFilm v1.1.0 maintains an explicit, verifiable boundary regarding implemented capabilities versus ongoing research:

1. **Implemented Capabilities**: Strict mathematical isolation of the three operational domains; dark frame subtraction and open-gate flat-field relative transmittance normalization; transmission target patch fitting with SVD condition number safety guards; bounded channel span response for cross-platform color stability; and 16-bit linear mastering alongside CFA camera RAW DNG encapsulation.
2. **Current Technical Boundaries**: The v1.1.0 release does not incorporate an optical Status M spectral integration library, and historical matrix estimates are retained solely for legacy backward compatibility; furthermore, batch-specific nonlinear photographic H-D emulsion curves are not inverted. Inversion relies entirely on verifiable relative transmittance and rigorous logarithmic density algebra.
