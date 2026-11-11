---
id: overview-and-install
track: getting-started
trackOrder: 1
chapterOrder: 1
kicker: "01.1 / Getting Started"
title: "Understanding NexFilm and Installation Guide"
subtitle: "A physics-grounded negative film inversion workbench and system setup."
readingTime: 7
inApp: true
sinceVersion: "v1.1.0"
aliases: ["quick-start"]
---

## Physics-grounded Inversion vs Empirical Curves {#core-principles}

Color negative inversion in general-purpose photographic software has historically relied on empirical curve manipulation in display gamma color spaces. When photographers import camera scans or flatbed scanner acquisitions into conventional editors, the standard workflow typically involves manually flipping RGB tone curve endpoints, tweaking levels sliders, or applying black-box lookup tables to neutralize the orange-brown film base mask. This approach is fundamentally at odds with photographic physics. Camera sensor RAW files capture photons in a linear transmittance domain, whereas light absorption across the dye layers of negative film emulsion follows the Beer-Lambert law, establishing a logarithmic relationship between optical density and transmittance. Inverting pixel values within a non-linear display gamma space distorts the inter-channel mathematical relationships, leading to clipped highlight color shifts, muddy shadows, and chronic color inconsistencies across frames within the same roll.

NexFilm Engine is built around a unified optical density pipeline. The software converts input sensor transmittance values into optical density ($D = -\log_{10}(T)$) through rigorous mathematical transformations. Density-domain processing enables authentic physical anchoring of both the unexposed film base ($D_{\min}$) and the chemical density ceiling ($D_{\max}$). Within the optical density domain, color balance adjustment corresponds to pure additive shifts across color channels, mirroring the physical behavior of colored enlarger head filtration (Printer Lights) used in traditional photographic darkrooms. NexFilm adheres strictly to verifiable algorithmic principles: the built-in library of over forty film stock names is intended exclusively for archival metadata indexing, EXIF injection, and contact sheet titling. Inversion calculations rely entirely on measured physical base densities rather than non-verifiable black-box film profile presets.

By establishing clear mathematical boundaries between transmittance, optical density, and output color space, NexFilm provides consistent, repeatable, and non-destructive inversion across single frames and entire rolls. Whether working with 135 formats, 120 medium format roll film, large format sheet film, or uncompressed linear TIFF files from drum scanners, the engine delivers predictable tone reproduction anchored directly in emulsion physics.

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
    <div class="doc-callout-body">When authorizing software on macOS, instructions advising users to disable System Integrity Protection (SIP) via <code>csrutil disable</code> in Recovery Mode represent an unnecessary security hazard. NexFilm requires only standard per-app Gatekeeper authorization. Never disable system-wide security protections to run application software.</div>
</div>

### First Launch Authorization on Windows {#first-launch-authorization-on-windows}

When launching the `.exe` installer, Microsoft Defender SmartScreen may display a blue dialog stating that Windows protected your PC from an unrecognized app. Follow these steps:

1. Click the **More info** text link located within the dialog paragraph.
2. The dialog will display the application package name and an unverified publisher status, revealing a button in the bottom right corner labeled **Run anyway**.
3. Click **Run anyway** to proceed with the standard installation wizard.

### First Launch Authorization on macOS {#first-launch-authorization-on-macos}

Mount the downloaded `.dmg` disk image and drag the `NexFilm Engine.app` bundle into your `Applications` folder. When opening the app for the first time, macOS Gatekeeper may block execution, reporting that the developer cannot be verified. Use either of the following standard methods:

1. **Finder Context Menu Method (Recommended)**: Open Finder, navigate to the **Applications** folder, and locate `NexFilm Engine`. Hold the <kbd>Control</kbd> key and click the application icon (or right-click), then choose **Open** from the context menu. A confirmation prompt will appear containing an explicit **Open** button. Click **Open** to clear the quarantine flag and launch the engine.
2. **System Settings Security Panel Method**: If the initial launch was blocked, open **System Settings** from the Apple menu and navigate to **Privacy &amp; Security**. Scroll down to the Security section, where a notice will state that NexFilm Engine was blocked from opening because it is not from an identified developer. Click **Open Anyway**, authenticate with your macOS user password or Touch ID, and the application will start normally.

## Storage Architecture and Read-Only Guarantee {#storage-architecture}

Protecting original photographic data is a core architectural priority. NexFilm enforces a strict **read-only policy for source scans**. Throughout importing, base measurement, interactive WebGL preview rendering, and export processing, the software performs stream-oriented read operations on original image files. NexFilm never overwrites, renames, alters, or relocates your original RAW or TIFF scan files.

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
