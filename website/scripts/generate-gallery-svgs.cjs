const fs = require('fs');
const path = require('path');

const galleryDir = path.resolve(__dirname, '../public/gallery');
if (!fs.existsSync(galleryDir)) {
  fs.mkdirSync(galleryDir, { recursive: true });
}

// 1. Kodak Portra 400 (35mm)
// Negative: C-41 orange base, negative portrait scene with sprockets and Kodak 400-2 edge markings
const portraNeg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" width="1600" height="1000">
  <defs>
    <!-- Film Base Orange Mask Gradient -->
    <linearGradient id="c41-base" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#b65727"/>
      <stop offset="50%" stop-color="#bf6330"/>
      <stop offset="100%" stop-color="#ab4d20"/>
    </linearGradient>
    <!-- Negative Scene Gradients (Inverted Densities) -->
    <radialGradient id="neg-sky" cx="50%" cy="30%" r="70%">
      <stop offset="0%" stop-color="#142838"/>
      <stop offset="60%" stop-color="#2a3d4d"/>
      <stop offset="100%" stop-color="#6e553a"/>
    </radialGradient>
    <radialGradient id="neg-portrait" cx="50%" cy="52%" r="35%">
      <stop offset="0%" stop-color="#2b4859"/>
      <stop offset="40%" stop-color="#3c5b6e"/>
      <stop offset="85%" stop-color="#7a5a3a"/>
      <stop offset="100%" stop-color="#a45d2d"/>
    </radialGradient>
    <!-- Grain Filter -->
    <filter id="grain" x="0%" y="0%" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.75" numOctaves="3" result="noise"/>
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 0.12 0" in="noise" result="coloredNoise"/>
      <feComposite operator="in" in2="SourceGraphic"/>
    </filter>
  </defs>

  <!-- Full Film Strip Base with Orange Mask -->
  <rect width="1600" height="1000" fill="url(#c41-base)"/>

  <!-- Top Sprocket Holes Row -->
  <g fill="#0b0b0c">
    ${Array.from({ length: 16 }).map((_, i) => `<rect x="${60 + i * 96}" y="28" width="56" height="74" rx="10"/>`).join('\n    ')}
  </g>

  <!-- Bottom Sprocket Holes Row -->
  <g fill="#0b0b0c">
    ${Array.from({ length: 16 }).map((_, i) => `<rect x="${60 + i * 96}" y="898" width="56" height="74" rx="10"/>`).join('\n    ')}
  </g>

  <!-- Edge Code Markings (Latent Image Barcode & Text) -->
  <g font-family="monospace" font-size="16" font-weight="bold" fill="#69280e" opacity="0.85">
    <text x="140" y="122">KODAK PORTRA 400</text>
    <text x="560" y="122">5036</text>
    <text x="960" y="122">▶ 14A</text>
    <text x="1360" y="122">SAFETY FILM</text>
    <text x="180" y="880">KODAK 400-2</text>
    <text x="640" y="880">14</text>
    <text x="1100" y="880">||| | |||| || | |||</text>
  </g>

  <!-- Negative Active Emulsion Frame (135 Frame) -->
  <g transform="translate(140, 140)">
    <!-- Active Frame Background (Sky/Background Inverted) -->
    <rect width="1320" height="720" rx="4" fill="url(#neg-sky)"/>

    <!-- Negative Mountain / Landscape Silhouette (Dense in Highlight) -->
    <path d="M 0,520 Q 280,380 620,440 T 1320,400 L 1320,720 L 0,720 Z" fill="#203444" opacity="0.85"/>
    <path d="M 0,580 Q 400,480 820,530 T 1320,510 L 1320,720 L 0,720 Z" fill="#314552" opacity="0.9"/>

    <!-- Negative Portrait Subject (Inverted complementary colors) -->
    <!-- Head & Shoulders silhouette with soft inverted tonality -->
    <ellipse cx="660" cy="380" rx="150" ry="190" fill="url(#neg-portrait)"/>
    <!-- Hair Negative (Highlights in hair are dense dark-cyan) -->
    <path d="M 500,380 C 490,230 600,180 660,180 C 720,180 830,230 820,380 C 810,340 760,260 660,260 C 560,260 510,340 500,380 Z" fill="#1b2e3b"/>
    <!-- Face Highlights (Cheek / Forehead highlights inverted to dark cyans) -->
    <ellipse cx="660" cy="370" rx="100" ry="120" fill="#2d4a5c" opacity="0.8"/>
    <ellipse cx="630" cy="360" rx="40" ry="50" fill="#1e3747" opacity="0.9"/>
    <!-- Torso / Garment (Warm clothing inverted to blue/cyan negative tones) -->
    <path d="M 420,720 C 440,540 540,510 660,510 C 780,510 880,540 900,720 Z" fill="#284456"/>

    <!-- Sunflare highlight in negative (dense black circle with halation) -->
    <circle cx="1060" cy="220" r="110" fill="#101c24" opacity="0.88"/>
    <circle cx="1060" cy="220" r="60" fill="#091016"/>

    <!-- Film Grain Overlay -->
    <rect width="1320" height="720" filter="url(#grain)" opacity="0.4" pointer-events="none"/>
  </g>
</svg>`;

// Positive: Kodak Portra 400 + Portra Endura paper tone
const portraPos = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" width="1600" height="1000">
  <defs>
    <!-- Golden Hour Sky Gradient (Inverted Positive) -->
    <linearGradient id="pos-sky" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#e8a87c"/>
      <stop offset="35%" stop-color="#f3c99f"/>
      <stop offset="70%" stop-color="#b8d5e5"/>
      <stop offset="100%" stop-color="#8bb4ce"/>
    </linearGradient>
    <!-- Natural Skin Tone Gradient (Portra Endura warmth) -->
    <radialGradient id="pos-skin" cx="48%" cy="46%" r="50%">
      <stop offset="0%" stop-color="#fae1d0"/>
      <stop offset="45%" stop-color="#f2cbb4"/>
      <stop offset="85%" stop-color="#db9c7a"/>
      <stop offset="100%" stop-color="#b6714e"/>
    </radialGradient>
    <linearGradient id="pos-hair" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#4d2f1d"/>
      <stop offset="60%" stop-color="#2a160c"/>
      <stop offset="100%" stop-color="#140a05"/>
    </linearGradient>
    <filter id="grain-pos" x="0%" y="0%" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.75" numOctaves="3" result="noise"/>
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 0.08 0" in="noise" result="coloredNoise"/>
      <feComposite operator="in" in2="SourceGraphic"/>
    </filter>
  </defs>

  <!-- Clean Darkroom Border Framing -->
  <rect width="1600" height="1000" fill="#121214"/>

  <!-- Inner Active Photographic Print Area -->
  <g transform="translate(140, 140)">
    <!-- Sky & Background -->
    <rect width="1320" height="720" rx="4" fill="url(#pos-sky)"/>

    <!-- Gentle Mountain Hills / Warm Sunset Horizon -->
    <path d="M 0,520 Q 280,380 620,440 T 1320,400 L 1320,720 L 0,720 Z" fill="#6a7d66" opacity="0.7"/>
    <path d="M 0,580 Q 400,480 820,530 T 1320,510 L 1320,720 L 0,720 Z" fill="#46543f" opacity="0.85"/>

    <!-- Soft Golden Sun / Backlight Rim -->
    <circle cx="1060" cy="220" r="140" fill="#fff5e6" opacity="0.95"/>
    <circle cx="1060" cy="220" r="80" fill="#ffffff"/>

    <!-- Hair Silhouette -->
    <ellipse cx="660" cy="380" rx="160" ry="195" fill="url(#pos-hair)"/>

    <!-- Face & Neck -->
    <ellipse cx="660" cy="380" rx="115" ry="135" fill="url(#pos-skin)"/>
    <!-- Delicate Cheek Warmth -->
    <ellipse cx="630" cy="380" rx="35" ry="30" fill="#e88975" opacity="0.35"/>
    <ellipse cx="695" cy="380" rx="35" ry="30" fill="#e88975" opacity="0.35"/>

    <!-- Soft Hair Details overlapping face -->
    <path d="M 500,380 C 490,230 600,180 660,180 C 720,180 830,230 820,380 C 790,300 740,240 660,240 C 580,240 530,300 500,380 Z" fill="url(#pos-hair)"/>

    <!-- Linen Coat / Warm Garment (Portra warm earth tones) -->
    <path d="M 420,720 C 440,540 540,510 660,510 C 780,510 880,540 900,720 Z" fill="#9c7a59"/>
    <!-- Fold shadows -->
    <path d="M 640,530 L 630,720 L 680,720 L 670,530 Z" fill="#6d5137" opacity="0.6"/>

    <!-- Warm Golden Light Wrap on Shoulders -->
    <path d="M 780,540 C 840,580 880,640 890,720 L 900,720 C 880,640 840,570 780,530 Z" fill="#fed69e" opacity="0.8"/>

    <!-- Film Grain Overlay -->
    <rect width="1320" height="720" filter="url(#grain-pos)" opacity="0.5" pointer-events="none"/>
  </g>

  <!-- Subtle Minimal Film Margin Branding Badge -->
  <g font-family="monospace" font-size="13" fill="#aaaab0" opacity="0.7">
    <text x="140" y="890">KODAK PORTRA 400 · 35mm</text>
    <text x="140" y="910" fill="#c4814f">PRINTED ON KODAK PORTRA ENDURA (COLOR EMULATION)</text>
    <text x="1460" y="890" text-anchor="end">D_min: 0.24 / 0.59 / 0.96</text>
    <text x="1460" y="910" text-anchor="end">UNIFIED DENSITY ENGINE</text>
  </g>
</svg>`;

// 2. Fujifilm Superia X-TRA 400 (35mm)
// Negative: C-41 orange mask with distinctive Fuji magenta/green dye negative response
const superiaNeg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" width="1600" height="1000">
  <defs>
    <linearGradient id="fuji-base" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#b85a22"/>
      <stop offset="50%" stop-color="#c6682b"/>
      <stop offset="100%" stop-color="#aa4b19"/>
    </linearGradient>
    <linearGradient id="fuji-neg-scene" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#182c3c"/>
      <stop offset="50%" stop-color="#5a2e46"/>
      <stop offset="100%" stop-color="#2d4033"/>
    </linearGradient>
    <filter id="fuji-grain" x="0%" y="0%" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.8" numOctaves="3" result="noise"/>
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 0.14 0" in="noise" result="coloredNoise"/>
      <feComposite operator="in" in2="SourceGraphic"/>
    </filter>
  </defs>

  <rect width="1600" height="1000" fill="url(#fuji-base)"/>

  <!-- Sprocket Holes -->
  <g fill="#0b0b0c">
    ${Array.from({ length: 16 }).map((_, i) => `<rect x="${60 + i * 96}" y="28" width="56" height="74" rx="10"/>`).join('\n    ')}
    ${Array.from({ length: 16 }).map((_, i) => `<rect x="${60 + i * 96}" y="898" width="56" height="74" rx="10"/>`).join('\n    ')}
  </g>

  <!-- Fuji Edge Code -->
  <g font-family="monospace" font-size="16" font-weight="bold" fill="#5c2006" opacity="0.85">
    <text x="140" y="122">FUJIFILM SUPERIA 400</text>
    <text x="620" y="122">X-TRA</text>
    <text x="1000" y="122">▶ 28</text>
    <text x="1360" y="122">SAFETY FILM</text>
    <text x="180" y="880">FUJI CH-7</text>
    <text x="640" y="880">28A</text>
    <text x="1100" y="880">|||| | | || ||| || |</text>
  </g>

  <!-- Negative Frame: Tokyo Street & Lush Green Foliage (Green inverts to deep magenta/purple negative) -->
  <g transform="translate(140, 140)">
    <rect width="1320" height="720" rx="4" fill="url(#fuji-neg-scene)"/>

    <!-- Architecture Silhouettes -->
    <rect x="180" y="180" width="340" height="540" fill="#203444"/>
    <rect x="580" y="100" width="420" height="620" fill="#182733"/>
    <rect x="1060" y="240" width="220" height="480" fill="#2b3e4d"/>

    <!-- Foliage / Trees in front of buildings (Negative of vibrant greens = Magenta/Violet density) -->
    <path d="M 80,720 Q 220,440 380,720 Z" fill="#66204c" opacity="0.95"/>
    <path d="M 320,720 Q 480,480 640,720 Z" fill="#752258" opacity="0.9"/>
    <path d="M 880,720 Q 1040,420 1200,720 Z" fill="#611d47" opacity="0.95"/>

    <!-- Neon sign highlights (dense negative black) -->
    <rect x="630" y="220" width="40" height="180" fill="#080e14"/>
    <rect x="710" y="300" width="120" height="30" fill="#0b1219"/>

    <rect width="1320" height="720" filter="url(#fuji-grain)" opacity="0.4" pointer-events="none"/>
  </g>
</svg>`;

// Positive: Fujifilm Superia X-TRA 400 Inverted Positive
const superiaPos = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" width="1600" height="1000">
  <defs>
    <linearGradient id="fuji-pos-sky" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#387cb4"/>
      <stop offset="60%" stop-color="#72a8d1"/>
      <stop offset="100%" stop-color="#b6d5e8"/>
    </linearGradient>
    <filter id="fuji-grain-pos" x="0%" y="0%" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.8" numOctaves="3" result="noise"/>
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 0.09 0" in="noise" result="coloredNoise"/>
      <feComposite operator="in" in2="SourceGraphic"/>
    </filter>
  </defs>

  <rect width="1600" height="1000" fill="#121214"/>

  <g transform="translate(140, 140)">
    <!-- Crisp Tokyo Daytime Sky -->
    <rect width="1320" height="720" rx="4" fill="url(#fuji-pos-sky)"/>

    <!-- Modern Architecture in Daylight -->
    <rect x="180" y="180" width="340" height="540" fill="#d4d7dc"/>
    <rect x="580" y="100" width="420" height="620" fill="#e9ecf0"/>
    <rect x="1060" y="240" width="220" height="480" fill="#ccd0d6"/>

    <!-- Architectural Window Patterns -->
    <g fill="#435263" opacity="0.8">
      ${[220, 280, 340, 400, 460, 520].map(y => `
        <rect x="210" y="${y}" width="40" height="35" rx="2"/>
        <rect x="270" y="${y}" width="40" height="35" rx="2"/>
        <rect x="330" y="${y}" width="40" height="35" rx="2"/>
        <rect x="390" y="${y}" width="40" height="35" rx="2"/>
      `).join('\n')}
      ${[140, 200, 260, 320, 380, 440, 500].map(y => `
        <rect x="620" y="${y}" width="50" height="40" rx="2"/>
        <rect x="700" y="${y}" width="50" height="40" rx="2"/>
        <rect x="780" y="${y}" width="50" height="40" rx="2"/>
        <rect x="860" y="${y}" width="50" height="40" rx="2"/>
      `).join('\n')}
    </g>

    <!-- Superia Iconic Crisp Greens (4th Color Layer richness) -->
    <path d="M 80,720 Q 220,440 380,720 Z" fill="#2d6e3c" opacity="0.95"/>
    <path d="M 120,720 Q 240,480 340,720 Z" fill="#3ea357" opacity="0.85"/>
    <path d="M 320,720 Q 480,480 640,720 Z" fill="#265a32" opacity="0.95"/>
    <path d="M 360,720 Q 500,520 600,720 Z" fill="#358b49" opacity="0.8"/>
    <path d="M 880,720 Q 1040,420 1200,720 Z" fill="#286938" opacity="0.95"/>
    <path d="M 940,720 Q 1060,460 1160,720 Z" fill="#46aa5f" opacity="0.85"/>

    <!-- Vibrant Tokyo Street Signage (Red/Cyan pop) -->
    <rect x="630" y="220" width="40" height="180" fill="#e03e36" rx="4"/>
    <rect x="710" y="300" width="120" height="30" fill="#f59e0b" rx="3"/>

    <rect width="1320" height="720" filter="url(#fuji-grain-pos)" opacity="0.5" pointer-events="none"/>
  </g>

  <g font-family="monospace" font-size="13" fill="#aaaab0" opacity="0.7">
    <text x="140" y="890">FUJIFILM SUPERIA X-TRA 400 · 35mm</text>
    <text x="140" y="910" fill="#c4814f">KODAK ENDURA PREMIER EMULATION (4TH COLOR LAYER BALANCE)</text>
    <text x="1460" y="890" text-anchor="end">D_min: 0.28 / 0.64 / 1.02</text>
    <text x="1460" y="910" text-anchor="end">UNIFIED DENSITY ENGINE</text>
  </g>
</svg>`;

// 3. Ilford HP5 Plus 400 (120 Medium Format 6x6)
// Negative: Silver Halide negative with rebate numbers, silver grains, high dynamic range
const hp5Neg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" width="1600" height="1000">
  <defs>
    <linearGradient id="hp5-sky-neg" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#141416"/>
      <stop offset="60%" stop-color="#323238"/>
      <stop offset="100%" stop-color="#64646e"/>
    </linearGradient>
    <filter id="hp5-grain" x="0%" y="0%" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.65" numOctaves="4" result="noise"/>
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 0.18 0" in="noise" result="coloredNoise"/>
      <feComposite operator="in" in2="SourceGraphic"/>
    </filter>
  </defs>

  <!-- Clear Acetate / Polyester Film Rebate (B&W clear base with subtle fog) -->
  <rect width="1600" height="1000" fill="#242426"/>

  <!-- 120 Rebate Edge Markings -->
  <g font-family="sans-serif" font-size="18" font-weight="bold" fill="#787882" opacity="0.75" letter-spacing="4">
    <text x="160" y="80">ILFORD HP5 PLUS</text>
    <text x="760" y="80">SAFETY FILM</text>
    <text x="1360" y="80">8</text>
    <text x="160" y="940">400</text>
    <text x="760" y="940">▶ 8A</text>
    <text x="1360" y="940">ILFORD</text>
  </g>

  <!-- Medium Format 6x6 Active Frame (Square Aspect within 1600x1000) -->
  <g transform="translate(440, 140)">
    <!-- Active 720x720 Square Frame -->
    <rect width="720" height="720" fill="url(#hp5-sky-neg)"/>

    <!-- Dramatic Architectural Arch in Negative (Highlights are dense black/charcoal) -->
    <path d="M 120,720 L 120,380 C 120,180 600,180 600,380 L 600,720 L 460,720 L 460,420 C 460,280 260,280 260,420 L 260,720 Z" fill="#18181c"/>
    
    <!-- Center light beam (bright sunlight through arch = dense silver deposit) -->
    <ellipse cx="360" cy="540" rx="140" ry="180" fill="#0f0f12" opacity="0.95"/>
    <ellipse cx="360" cy="540" rx="80" ry="100" fill="#050506"/>

    <!-- Deep shadow details (negative is clear base / light gray) -->
    <rect x="0" y="640" width="720" height="80" fill="#54545e" opacity="0.8"/>

    <rect width="720" height="720" filter="url(#hp5-grain)" opacity="0.6" pointer-events="none"/>
  </g>
</svg>`;

// Positive: Ilford HP5 Plus 400 + Ilford Multigrade FB Grade 2 Silver Gelatin Print
const hp5Pos = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" width="1600" height="1000">
  <defs>
    <linearGradient id="hp5-sky-pos" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#fafafc"/>
      <stop offset="50%" stop-color="#dadade"/>
      <stop offset="100%" stop-color="#a4a4ac"/>
    </linearGradient>
    <filter id="hp5-grain-pos" x="0%" y="0%" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.65" numOctaves="4" result="noise"/>
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 0.12 0" in="noise" result="coloredNoise"/>
      <feComposite operator="in" in2="SourceGraphic"/>
    </filter>
  </defs>

  <rect width="1600" height="1000" fill="#121214"/>

  <!-- 6x6 Medium Format Inverted Gelatin Silver Print -->
  <g transform="translate(440, 140)">
    <rect width="720" height="720" fill="url(#hp5-sky-pos)"/>

    <!-- Classical Stone Arch (Warm deep blacks to luminous silver midtones) -->
    <path d="M 120,720 L 120,380 C 120,180 600,180 600,380 L 600,720 L 460,720 L 460,420 C 460,280 260,280 260,420 L 260,720 Z" fill="#28282c"/>

    <!-- Stone texture & midtone bevels -->
    <path d="M 140,720 L 140,400 C 140,220 580,220 580,400 L 580,720 L 540,720 L 540,420 C 540,260 180,260 180,420 L 180,720 Z" fill="#505058" opacity="0.6"/>

    <!-- Piercing Sunlight through Arch (Pure white paper base) -->
    <ellipse cx="360" cy="540" rx="140" ry="180" fill="#f8f8fa" opacity="0.95"/>
    <ellipse cx="360" cy="540" rx="80" ry="100" fill="#ffffff"/>

    <!-- Floor Flagstones with rich shadow gradients -->
    <rect x="0" y="640" width="720" height="80" fill="#1c1c1f"/>
    <line x1="0" y1="680" x2="720" y2="680" stroke="#383840" stroke-width="2"/>
    <line x1="200" y1="640" x2="160" y2="720" stroke="#383840" stroke-width="2"/>
    <line x1="420" y1="640" x2="390" y2="720" stroke="#383840" stroke-width="2"/>

    <rect width="720" height="720" filter="url(#hp5-grain-pos)" opacity="0.55" pointer-events="none"/>
  </g>

  <g font-family="monospace" font-size="13" fill="#aaaab0" opacity="0.7">
    <text x="440" y="890">ILFORD HP5 PLUS · 120 (6x6)</text>
    <text x="440" y="910" fill="#c4814f">PRINTED ON ILFORD MGFB 2 (VARIABLE CONTRAST SILVER GELATIN)</text>
    <text x="1160" y="890" text-anchor="end">D_min: 0.08 (CLEAR BASE)</text>
    <text x="1160" y="910" text-anchor="end">UNIFIED DENSITY ENGINE</text>
  </g>
</svg>`;

// 4. Kodak Vision3 500T (5219 Motion Picture Film)
// Negative: Tungsten motion picture stock with BH-1866 cinema perfs and halation density
const visionNeg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" width="1600" height="1000">
  <defs>
    <linearGradient id="vision-base" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#b85926"/>
      <stop offset="50%" stop-color="#c46630"/>
      <stop offset="100%" stop-color="#a84b1d"/>
    </linearGradient>
    <radialGradient id="vision-neg-light" cx="65%" cy="40%" r="55%">
      <stop offset="0%" stop-color="#0a141e"/>
      <stop offset="35%" stop-color="#182c3c"/>
      <stop offset="70%" stop-color="#4e352b"/>
      <stop offset="100%" stop-color="#8c4e24"/>
    </radialGradient>
    <filter id="vision-grain" x="0%" y="0%" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.7" numOctaves="3" result="noise"/>
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 0.13 0" in="noise" result="coloredNoise"/>
      <feComposite operator="in" in2="SourceGraphic"/>
    </filter>
  </defs>

  <rect width="1600" height="1000" fill="url(#vision-base)"/>

  <!-- Cinema BH-1866 Perforations (Slightly narrower rounded curve perfs) -->
  <g fill="#0b0b0c">
    ${Array.from({ length: 16 }).map((_, i) => `<rect x="${60 + i * 96}" y="28" width="52" height="74" rx="8"/>`).join('\n    ')}
    ${Array.from({ length: 16 }).map((_, i) => `<rect x="${60 + i * 96}" y="898" width="52" height="74" rx="8"/>`).join('\n    ')}
  </g>

  <!-- Eastman Kodak 5219 Keykode / Edge Marking -->
  <g font-family="monospace" font-size="16" font-weight="bold" fill="#581e05" opacity="0.85">
    <text x="140" y="122">EASTMAN 5219</text>
    <text x="560" y="122">KJ 42 1892 0412</text>
    <text x="1120" y="122">▶ +08</text>
    <text x="1380" y="122">VISION3</text>
    <text x="180" y="880">500T</text>
    <text x="640" y="880">KODAK ECN-2</text>
    <text x="1100" y="880">|| | |||| | ||| || ||</text>
  </g>

  <!-- Negative Active Frame (Night cinematic street / tungsten interior) -->
  <g transform="translate(140, 140)">
    <rect width="1320" height="720" rx="4" fill="url(#vision-neg-light)"/>

    <!-- Neon Bar / Diner Frame (Negative deep cyan & halation rings) -->
    <path d="M 220,160 L 1100,160 L 1100,640 L 220,640 Z" fill="none" stroke="#162c3e" stroke-width="18"/>
    
    <!-- Tungsten Lamp (In negative, extremely dense dark spot surrounded by slight orange halation fringe) -->
    <!-- Halation fringe in negative -->
    <circle cx="860" cy="320" r="105" fill="#8f3214" opacity="0.6"/>
    <!-- Intense filament density -->
    <circle cx="860" cy="320" r="60" fill="#081018"/>
    <circle cx="860" cy="320" r="28" fill="#020406"/>

    <!-- Actor in shadow / warm backlight (Negative complement) -->
    <path d="M 460,720 C 470,520 540,460 620,440 C 690,440 760,490 780,720 Z" fill="#2a4254"/>
    <ellipse cx="620" cy="370" rx="65" ry="85" fill="#38566b"/>
    <!-- Rim light on profile (Dense negative line) -->
    <path d="M 670,300 C 690,340 690,400 660,440" stroke="#081018" stroke-width="12" fill="none"/>

    <rect width="1320" height="720" filter="url(#vision-grain)" opacity="0.45" pointer-events="none"/>
  </g>
</svg>`;

// Positive: Kodak Vision3 500T + Kodak 2383 Print Stock Emulation
const visionPos = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000" width="1600" height="1000">
  <defs>
    <!-- Cinematic Night Palette (Kodak 2383 rich blacks and teal/orange contrast) -->
    <radialGradient id="vision-pos-light" cx="65%" cy="40%" r="65%">
      <stop offset="0%" stop-color="#fff1d6"/>
      <stop offset="25%" stop-color="#f8a245"/>
      <stop offset="65%" stop-color="#1b303a"/>
      <stop offset="100%" stop-color="#0a1217"/>
    </radialGradient>
    <filter id="vision-grain-pos" x="0%" y="0%" width="100%" height="100%">
      <feTurbulence type="fractalNoise" baseFrequency="0.7" numOctaves="3" result="noise"/>
      <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 0.1" in="noise" result="coloredNoise"/>
      <feComposite operator="in" in2="SourceGraphic"/>
    </filter>
  </defs>

  <rect width="1600" height="1000" fill="#121214"/>

  <!-- Inverted Positive: Cinematic 35mm Frame with Kodak 2383 Look -->
  <g transform="translate(140, 140)">
    <rect width="1320" height="720" rx="4" fill="url(#vision-pos-light)"/>

    <!-- Neon Bar Window Structure -->
    <path d="M 220,160 L 1100,160 L 1100,640 L 220,640 Z" fill="none" stroke="#254b5e" stroke-width="18"/>

    <!-- Iconic Tungsten Halation (Signature Kodak Vision3 Red Rim Glow) -->
    <circle cx="860" cy="320" r="115" fill="#f43f1e" opacity="0.45"/>
    <circle cx="860" cy="320" r="75" fill="#ff7a29" opacity="0.8"/>
    <circle cx="860" cy="320" r="45" fill="#ffdf7a"/>
    <circle cx="860" cy="320" r="22" fill="#ffffff"/>

    <!-- Character in Dramatic Chiaroscuro Profile -->
    <path d="M 460,720 C 470,520 540,460 620,440 C 690,440 760,490 780,720 Z" fill="#141a20"/>
    <ellipse cx="620" cy="370" rx="65" ry="85" fill="#18222a"/>

    <!-- Warm Amber Tungsten Rim Light on Profile -->
    <path d="M 670,300 C 690,340 690,400 660,440" stroke="#f6993f" stroke-width="10" stroke-linecap="round" fill="none"/>
    <path d="M 670,300 C 690,340 690,400 660,440" stroke="#fff4cf" stroke-width="4" stroke-linecap="round" fill="none"/>

    <!-- Subtle Red Halation Glow along edge of character silhouette -->
    <path d="M 676,295 C 700,340 700,405 666,448" stroke="#e11d48" stroke-width="16" opacity="0.5" stroke-linecap="round" fill="none"/>

    <rect width="1320" height="720" filter="url(#vision-grain-pos)" opacity="0.45" pointer-events="none"/>
  </g>

  <g font-family="monospace" font-size="13" fill="#aaaab0" opacity="0.7">
    <text x="140" y="890">KODAK VISION3 500T (5219) · 35mm</text>
    <text x="140" y="910" fill="#c4814f">PRINTED WITH KODAK 2383 PRINT FILM EMULATION LUT</text>
    <text x="1460" y="890" text-anchor="end">D_min: 0.22 / 0.54 / 0.91</text>
    <text x="1460" y="910" text-anchor="end">UNIFIED DENSITY ENGINE</text>
  </g>
</svg>`;

const files = [
  { name: 'portra-400-neg.svg', content: portraNeg },
  { name: 'portra-400-pos.svg', content: portraPos },
  { name: 'superia-400-neg.svg', content: superiaNeg },
  { name: 'superia-400-pos.svg', content: superiaPos },
  { name: 'hp5-plus-400-neg.svg', content: hp5Neg },
  { name: 'hp5-plus-400-pos.svg', content: hp5Pos },
  { name: 'vision3-500t-neg.svg', content: visionNeg },
  { name: 'vision3-500t-pos.svg', content: visionPos }
];

for (const file of files) {
  const filePath = path.join(galleryDir, file.name);
  fs.writeFileSync(filePath, file.content.trim(), 'utf8');
  console.log('✓ Generated:', file.name, `(${file.content.length} bytes)`);
}
