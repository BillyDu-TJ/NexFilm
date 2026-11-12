---
id: overview-and-install
track: getting-started
trackOrder: 1
chapterOrder: 2
kicker: "01.2 / Getting Started"
title: "Understanding NexFilm: Principles, Installation, and Workspace"
subtitle: "The ideas behind optical density inversion, installer selection, first-launch authorization, and the workspace layout."
readingTime: 8
inApp: true
sinceVersion: "v1.1.0"
aliases: ["nexfilm-overview"]
---

## Physics-grounded Inversion vs Empirical Curves {#core-principles}

Color negative inversion in general-purpose photo software has long relied on pulling curves or applying presets in display gamma color spaces. Photographers importing camera scans or flatbed scans into a conventional editor often flip the RGB curve endpoints, drag the Levels sliders around, or try a black-box lookup table to neutralize the orange-brown film base mask.

That approach runs into a physical problem. Camera sensor RAW files record light in a linear transmittance domain, while light absorption through the dye layers of a negative follows the Beer-Lambert law. Optical density and transmittance therefore have a logarithmic relationship. Inverting pixels directly in a non-linear display space can distort the relationship between channels, which shows up as color shifts in clipped highlights, muddy shadows, and inconsistent color from frame to frame on the same roll.

<div class="doc-callout">
    <div class="doc-callout-title">A Few Terms Before We Go Further</div>
    <div class="doc-callout-body"><p><strong>Optical density</strong> is a logarithmic measure of how much light the film blocks. The darker the negative, the higher the density. <strong>D-Min</strong> is the density of the unexposed film base, where the orange mask lives. <strong>D-Max</strong> is the density of a fully exposed leader or the deepest silver area, representing the darkest end the roll can produce.</p><p><strong>Printer lights</strong> come from the traditional darkroom. Adding more red, green, or blue filtration in an enlarger changes how much of each color reaches the paper. NexFilm uses three logarithmic sliders to do the same job, so the controls are adjusting color balance rather than ordinary RGB brightness.</p></div>
</div>

NexFilm Engine is built around a unified optical density pipeline. The software converts input sensor transmittance values into optical density ($D = -\log_{10}(T)$) through rigorous mathematical transformations. Density-domain processing enables authentic physical anchoring of both the unexposed film base ($D_{\min}$) and the chemical density ceiling ($D_{\max}$). Within the optical density domain, color balance adjustment corresponds to pure additive shifts across color channels, mirroring the physical behavior of colored enlarger head filtration (Printer Lights) used in traditional photographic darkrooms. NexFilm adheres strictly to verifiable algorithmic principles: the built-in library of over forty film stock names is intended exclusively for archival metadata indexing, EXIF injection, and contact sheet titling. Inversion calculations rely entirely on measured physical base densities rather than non-verifiable black-box film profile presets.

By keeping transmittance, optical density, and output color space clearly separated, NexFilm provides consistent, repeatable, non-destructive inversion for single frames and whole rolls. Whether you work with 135, 120 roll film, large-format sheets, or uncompressed linear TIFF files from a drum scanner, the tone reproduction stays tied to the emulsion rather than to a guessed preset.

## Platform Matrix and Installation Packages {#package-selection}

NexFilm is engineered using Rust and modern Web platform standards, combining low-level systems memory safety with GPU fragment shader parallel processing. The application is distributed as self-contained standalone installers for major desktop operating systems. Verify your system architecture and hardware compatibility prior to installation.

<table class="doc-table">
    <thead>
        <tr><th>Platform &amp; Architecture</th><th>Installer Package Pattern</th><th>Target Hardware &amp; OS Minimum</th><th>Backend &amp; Acceleration Profile</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Windows x64</strong></td>
            <td><code>NexFilm.Engine_{version}_x64-setup.exe</code></td>
            <td>Windows 10 / 11 64-bit, GPU supporting WebGL 2.0</td>
            <td>Multi-threaded LibRaw decoding engine, Direct3D 11 WebGL2 canvas rendering</td>
        </tr>
        <tr>
            <td><strong>macOS Apple Silicon</strong></td>
            <td><code>NexFilm.Engine_{version}_aarch64.dmg</code></td>
            <td>Mac computers with Apple M1 / M2 / M3 / M4 processors, macOS 11.0 Big Sur or later</td>
            <td>Native ARM64 architecture instructions, Metal-accelerated WebKit color rendering</td>
        </tr>
        <tr>
            <td><strong>macOS Intel</strong></td>
            <td><code>NexFilm.Engine_{version}_x64.dmg</code></td>
            <td>Mac computers with 64-bit Intel processors, macOS 10.15 Catalina or later</td>
            <td>Native x86_64 compilation, AVX2 vector SIMD instruction acceleration</td>
        </tr>
    </tbody>
</table>

<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">Installer Asset Verification</div>
    <div class="doc-callout-body">When retrieving distribution files from the official GitHub Releases page, select the binary installer corresponding to your platform from the assets table. Do not download the automatically packaged <code>Source code (zip)</code> or <code>Source code (tar.gz)</code> files located at the bottom of the release page. Source code archives do not contain compiled runtime binaries and cannot be launched directly.</div>
</div>

For digital image formats, NexFilm natively handles modern digital camera RAW files (including Sony `.ARW`, Canon `.CR2` / `.CR3`, Nikon `.NEF`, and Fujifilm `.RAF`), Imacon / Hasselblad Flextight scanner `.fff` archives, 16-bit linear TIFF files from specialized scanners, and standard 8-bit or 16-bit TIFF and PNG images. Note that due to current decoding limitations in the underlying LibRaw library, Nikon High Efficiency compressed formats (**HE** and **HE\***) generated by bodies such as the Nikon Z8 and Z9 are not supported directly. Users digitizing film with these cameras should record in uncompressed or lossless compressed RAW mode in-camera, or convert files to linear DNG using Adobe DNG Converter prior to ingestion.

## Security Clearances and First Launch {#security-clearance}

NexFilm is open-source software maintained by independent developers. Because the build packages are not currently signed with paid commercial certificates, operating system security frameworks may display warning dialogs during initial launch. These notices are standard operating system behavior for unsigned open-source utilities. NexFilm contains no telemetry tracking, background analytics, or network upload routines. Complete the following standard steps to authorize execution.

<div class="doc-callout">
    <div class="doc-callout-title">System Integrity and Security Policies</div>
    <div class="doc-callout-body">Instructions that tell you to disable System Integrity Protection (SIP) with <code>csrutil disable</code> in Recovery Mode are asking for far more risk than NexFilm needs. The app only needs its own quarantine flag cleared. There is no reason to disable system-wide security to run it.</div>
</div>

### First Launch Authorization on Windows {#first-launch-authorization-on-windows}

When launching the `.exe` installer, Microsoft Defender SmartScreen may display a blue dialog stating that Windows protected your PC from an unrecognized app. Follow these steps:

1. Click the **More info** text link located within the dialog paragraph.
2. The dialog will display the application package name and an unverified publisher status, revealing a button in the bottom right corner labeled **Run anyway**.
3. Click **Run anyway** to proceed with the standard installation wizard.

### First Launch Authorization on macOS {#first-launch-authorization-on-macos}

Mount the downloaded `.dmg` disk image and drag `NexFilm Engine.app` into your **Applications** folder. On first launch, macOS Gatekeeper may report that Apple cannot check the app for malicious software, or that the app is damaged. Use the Terminal command below to clear the app's quarantine attributes:

1. Open **Launchpad** → **Other** → **Terminal** (or **Finder** → **Applications** → **Utilities** → **Terminal**).
2. Copy the complete command below, paste it into Terminal, and press <kbd>Return</kbd>:

```bash
xattr -cr "/Applications/NexFilm Engine.app"
```

3. If macOS asks for a password, enter your Mac login password and press <kbd>Return</kbd>. Terminal does not display characters while you type; this is expected.
4. Return to **Applications** and open `NexFilm Engine` again.

<div class="doc-callout">
    <div class="doc-callout-title">What this command does</div>
    <div class="doc-callout-body">This command recursively clears extended attributes and the download quarantine flag from <code>NexFilm Engine.app</code>. It affects only this app and does not disable Gatekeeper, System Integrity Protection (SIP), or any other system-wide security feature. It assumes the app is installed in <code>/Applications</code>; if you installed it elsewhere, replace the path in the command with the actual location.</div>
</div>

## Storage Architecture and Read-Only Guarantee {#storage-architecture}

Protecting original photographic data is a core architectural priority. NexFilm follows a strict **read-only policy for source scans**. During import, base measurement, interactive WebGL preview rendering, and export, the software only reads the original files. It does not overwrite, rename, alter, or relocate your RAW or TIFF scans.

All archival metadata, frame crop coordinates, photometric base markers, color adjustments, and cached thumbnails are maintained within the standard user application support directory of your operating system.

<table class="doc-table">
    <thead>
        <tr><th>Operating System</th><th>Local Database &amp; Configuration Directory</th><th>File Names and Storage Roles</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Windows</strong></td>
            <td><code>%APPDATA%\NexFilm Engine\</code></td>
            <td><code>nexfilm_user.db</code> (Primary SQLite database storing library indices and adjustments)<br /><code>rolls.json</code> (Backward-compatible roll catalog snapshot)<br /><code>profiles/</code> (Hardware calibration and flat-field profile repository)</td>
        </tr>
        <tr>
            <td><strong>macOS</strong></td>
            <td><code>~/Library/Application Support/NexFilm Engine/</code></td>
            <td><code>nexfilm_user.db</code> (Primary SQLite database containing edits and crop data)<br /><code>rolls.json</code> (Roll synchronization mirror)<br /><code>profiles/</code> (Flat-field and calibration target data)</td>
        </tr>
    </tbody>
</table>

Because the application references original files via absolute disk paths, moving source image folders, altering parent volume names, or switching external storage drives will cause file paths to become invalid, marked by offline warnings in the library. In this event, your adjustments remain safe in the database. Select **Locate File** in the toolbar, navigate to the updated directory, and NexFilm will automatically reconnect the entire roll using matching filenames.

<div class="doc-callout">
    <div class="doc-callout-title">Thumbnail Refresh Recommendations After Version Upgrades</div>
    <div class="doc-callout-body">When upgrading from earlier versions such as <code>v1.0.2</code> to <code>v1.1.0</code> or later, changes to the density pipeline weighting and channel span algorithms may cause older cached thumbnails to appear darker than expected. Open existing rolls in the Develop workspace and trigger <strong>Auto Invert</strong> once to re-render the thumbnail and refresh stored recipe parameters. Before processing large archival collections, verify your complete workflow by running a small test batch of two or three frames from import to export.</div>
</div>

## Four Core Workspace Regions at a Glance {#interface-layout}

The NexFilm user interface is structured around darkroom ergonomics, prioritizing canvas visibility and responsive parameter control. The main application window is partitioned into four functional regions:

<table class="doc-table">
    <thead>
        <tr><th>Interface Region</th><th>Physical Location &amp; Components</th><th>Core Functional Responsibilities</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Menu Bar</strong></td>
            <td>Top-left corner displaying the <code>NEXFILM</code> branding, <strong>setting</strong>, <strong>View</strong>, and <strong>Help</strong> menus</td>
            <td>Manages application preferences. Access <strong>setting</strong> to toggle between English and Chinese (zh-CN) or switch between dark and light interface themes. Use <strong>View</strong> for rapid module navigation, and <strong>Help</strong> to review version details.</td>
        </tr>
        <tr>
            <td><strong>Top Header &amp; Action Bar</strong></td>
            <td>Centered view navigation tabs flanked by global action buttons on the right</td>
            <td>Provides access to five primary views: <strong>Library</strong>, <strong>Develop</strong>, <strong>Rolls</strong>, <strong>Hardware Calibration</strong>, and <strong>Documentation</strong>. The permanent global action buttons—<strong>Export Roll</strong>, <strong>Import Roll</strong>, and <strong>Export</strong>—remain accessible across all views.</td>
        </tr>
        <tr>
            <td><strong>Main Workspace</strong></td>
            <td>Central viewport dynamically adapting to the active view selection</td>
            <td>In <strong>Library</strong>, presents thumbnail grids with roll metadata headers. In <strong>Develop</strong>, hosts the WebGL 2.0 preview canvas on the left with responsive zoom and pan, paired with the inspector controls on the right (Histogram, Waveform, Film Area, Calibration, Printer Lights, and Curves). In <strong>Rolls</strong>, displays archived contact histories.</td>
        </tr>
        <tr>
            <td><strong>Filmstrip</strong></td>
            <td>Horizontal scrolling filmstrip docked at the bottom of the Develop view</td>
            <td>Displays chronological thumbnails of all exposures in the current roll. When visible, scroll horizontally with the mouse wheel or navigate frames using the <kbd>←</kbd> and <kbd>→</kbd> keyboard arrows.</td>
        </tr>
    </tbody>
</table>

Understanding the physical separation of these workspaces allows for swift darkroom throughput. In the next chapter, we examine the optical requirements for film digitization to ensure your hardware setup provides optimal image data.
