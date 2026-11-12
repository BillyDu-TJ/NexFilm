---
id: building-and-contributing
track: architecture
trackOrder: 4
chapterOrder: 3
kicker: "Architecture 03 / Building & Contributing"
title: "Cross-Platform Source Building, Contract Tests & Contributing Guide"
subtitle: "Reproducible builds across macOS, Windows, and Linux, CI contract defenses, and GPL-3.0 PR standards."
readingTime: 14
inApp: false
sinceVersion: "v1.1.0"
---

## Cross-Platform Toolchain Setup & Environment Dependencies {#cross-platform-toolchain-setup}

NexFilm is committed to providing an open, reproducible build environment for software engineers and color scientists. The project uses an unmanaged open-source toolchain, supporting native compilation on macOS (Apple Silicon and Intel architectures), Windows (64-bit MSVC), and modern Linux distributions. Because image processing requires high throughput and deterministic numerical precision, building from source links directly against hardware-optimized system libraries.

Prior to compiling the codebase, ensure your workstation satisfies the following prerequisites across runtimes:

1. **Rust Stable Toolchain**: Install the latest stable Rust distribution (version $\ge 1.78.0$ recommended) using the official `rustup` installer. Confirm that `cargo`, `rustc`, and the standard library toolchains are active in your user environment:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **Node.js LTS Runtime**: Install Node.js LTS (version 20.x or higher). Node.js is required to execute the ten automated regression contracts in `scripts/`, compile documentation artifacts, and serve the local documentation test environment.
3. **Platform-Specific Native Libraries**:
   - **macOS Toolchain Requirements**: NexFilm interfaces with raw camera captures via `libraw`. On macOS systems, install `pkg-config` and `libraw` using Homebrew. Note that LibRaw version 0.22 or higher is strictly required to provide full C++ mosaic shim integration:
     ```bash
     brew update
     brew install pkg-config libraw
     ```
     On Apple Silicon systems, Homebrew installs packages under `/opt/homebrew`, whereas Intel installations utilize `/usr/local`. Verify that your shell exports the corresponding `PKG_CONFIG_PATH` variable.
   - **Windows Toolchain Requirements**: Install Visual Studio 2022 Community or Build Tools. During installation, select the "Desktop development with C++" workload, ensuring that the MSVC v143 toolset, the Windows 10/11 SDK, and C++ CMake tools are installed.
   - **Linux Toolchain Requirements (Debian / Ubuntu)**: Install the WebKitGTK 4.1 development headers alongside fundamental build utilities. Note that modern Tauri 2 applications require WebKitGTK 4.1 rather than legacy 4.0 packages:
     ```bash
     sudo apt update && sudo apt install -y \
       build-essential pkg-config libwebkit2gtk-4.1-dev \
       libraw-dev libssl-dev libgtk-3-dev libsoup-3.0-dev
     ```

Clone the repository to your local development environment:

```bash
git clone https://github.com/BillyDu-TJ/NexFilm.git
cd NexFilm
```

## Compilation Commands, Packaging & Troubleshooting {#compilation-commands-and-packaging}

With the toolchain established, developers can run NexFilm in local development mode or package optimized production binaries for distribution.

### 1. Local Development with Live Hot-Reloading

For interactive engineering and UI adjustments, launch the Tauri development environment. The compiler compiles native Rust crates and loads the front-end within an active webview container supporting live asset reloads:

```bash
cargo tauri dev
```

### 2. Standalone Release Packaging

When generating production binaries, the build system applies full link-time optimization (LTO) and strips debug symbols to yield compact application bundles:

- **macOS (Generates Apple Disk Image .dmg)**:
  ```bash
  # Build native Apple Silicon installer (M1/M2/M3/M4)
  cargo tauri build --target aarch64-apple-darwin --bundles dmg

  # Build Intel x86_64 architecture installer
  cargo tauri build --target x86_64-apple-darwin --bundles dmg
  ```
- **Windows (Generates NSIS Installer executable)**:
  ```bash
  cargo tauri build --bundles nsis
  ```
- **Linux (Generates Debian .deb and standalone AppImage)**:
  ```bash
  cargo tauri build --bundles deb,appimage
  ```

Output binaries and disk images are written to the `target/release/bundle/` directory.

### 3. Compilation Troubleshooting Reference

Building high-performance native desktop applications across diverse platforms occasionally surfaces environment mismatches. The table below lists common build failures alongside authoritative diagnostic resolutions:

| Observed Error Pattern | Root Cause Diagnosis | Recommended Resolution |
| --- | --- | --- |
| `pkg-config: libraw not found` | `PKG_CONFIG_PATH` does not reflect the Homebrew installation prefix | Run `export PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig"` before building |
| `link.exe not found` | Windows environment lacks the MSVC C++ toolchain | Launch Visual Studio Installer, select C++ Build Tools, and update PATH |
| `Package webkit2gtk-4.1 was not found` | Linux distribution has legacy 4.0 headers installed instead of 4.1 | Run `sudo apt install libwebkit2gtk-4.1-dev` (note the 4.1 version designation) |
| `cargo tauri command not found` | Tauri CLI utility is not registered globally | Execute `cargo install tauri-cli --version "^2.0.0"` to register the command |
| `Target architecture mismatch in dylib` | Compiling x86_64 on Apple Silicon without cross toolchains | Explicitly pass `--target aarch64-apple-darwin` during invocation |

## Automated Regression Contract Test Suites {#automated-contract-test-suites}

NexFilm enforces an engineering culture centered on machine-verified regression gates. The `scripts/` directory maintains comprehensive contract scripts designed to prevent functional regressions across all core features:

1. **Rust Numerical Core Tests**:  
   Execute `cargo test --locked --lib`. The suite of 224 unit tests verifies linear algebra transforms in `core_math`, chromatic adaptation matrices in `color_science`, bounded span algorithms in `pipeline`, and schema migrations in `persistence`.
2. **Desktop Automated Contract Defenses**:  
   Prior to merging code, developers run the automated verification suite:
   - `node scripts/verify-density-contract.cjs`: Asserts that channel offsets remain zero with a valid film base and verifies that unaligned per-channel display curves do not exist in shaders or Rust cores.
   - `node scripts/verify-webgl-shaders.cjs`: Checks GLSL shader syntax validity and uniform variable parity across CPU and GPU cores.
   - `node scripts/verify-contact-sheet.cjs`: Validates contact sheet pagination math, grid alignments, and export filename schemes.
   - `node scripts/verify-develop-detail-ui.cjs`: Validates the Develop viewport, curve controls, and WebGL rendering passes.
   - `node scripts/verify-i18n-contract.cjs`: Validates symmetry across all 278 localization keys in English and Chinese.
   - `node scripts/verify-runtime-assets.cjs`: Confirms the presence of runtime LUTs, icons, and shader assets.
   - `node scripts/verify-ui-startup-order.cjs`: Verifies script tag declaration order inside `index.html`.
   - `node scripts/verify-slider-contract.cjs`: Verifies UI slider ranges, steps, and default values.
3. **Dual-End Handbook Consistency**:  
   Running `node scripts/compile-handbook.mjs && node scripts/verify-handbook-sync.cjs` verifies that the compiled desktop data (`ui/documentation-data.js`) matches the Markdown sources byte for byte.

> [!IMPORTANT]
> All automated contract suites must pass cleanly with zero failures before opening a pull request. Altering contract tests to mask algorithmic defects or regressions is strictly prohibited.

## GitHub Actions CI/CD Pipeline Architecture {#ci-cd-release-workflow}

The continuous integration and release pipeline is defined in `.github/workflows/release.yml`. It uses a two-stage verification strategy to keep runner cost down and protect the integrity of release artifacts:

```text
Trigger: Version tag push (e.g. v1.1.0) or workflow dispatch
  │
  ├─> Stage 1: Verification & Audit Gate (Verify Job)
  │     - Environment: Clean macos-14 runner
  │     - Actions: Checkout repository, install pkg-config and libraw 0.22+
  │     - Audit: cargo fmt --check (Code formatting enforcement)
  │     - Audit: cargo test --locked --lib (Full numerical unit tests)
  │     - Audit: node --check ui/*.js & node scripts/verify-*.cjs
  │     - Gate: Stage 2 proceeds only if Stage 1 passes with zero errors
  │
  └─> Stage 2: Concurrent Multi-Platform Packaging (Matrix Build)
        ├─> Windows x64 Build (windows-latest -> NSIS bundle)
        ├─> macOS Apple Silicon Build (macos-15 -> DMG bundle)
        ├─> macOS Intel Build (macos-15-intel -> DMG bundle)
        └─> Publish packaged release assets directly to GitHub Releases
```

In Stage 1, the pipeline executes within a clean macOS environment, installing native build tools and verifying syntax formatting via `cargo fmt --check`. All 224 Rust unit tests and 10 Node.js contract scripts must pass with zero warnings. If any regression occurs, the pipeline terminates immediately, notifying maintainers before release binaries are generated.

In Stage 2, GitHub Actions launches a three-way concurrent runner matrix targeting Windows, macOS Apple Silicon, and macOS Intel. Each runner compiles the platform bundle and uploads release assets directly to GitHub Releases using `tauri-apps/tauri-action`. This ensures that distributed binary artifacts are generated through an audited cloud build process.

## Licensing Governance & GPL-3.0 Contribution Guidelines {#gpl-contribution-guidelines}

NexFilm is distributed under the **GNU General Public License version 3 (GPL-3.0)**. Contributors are welcomed into the development community under the following governance principles:

1. **Copyleft Governance**: GPL-3.0 requires that any distributed modification or derivative work incorporating NexFilm code must remain open source under the GPL-3.0 license. Packaging core optical algorithms into closed-source commercial software violates this license.
2. **Single Source of Truth (SSOT) Documentation**: All documentation originates from the Markdown files within `docs/handbook/{zh,en}/`. Never modify `ui/documentation-data.js` manually. Documentation updates must be compiled via `node scripts/compile-handbook.mjs`.
3. **Commit Standards & Pull Requests**: Contributors should use Conventional Commits formatting (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`). Pull requests should outline the engineering motivation, affected modules, and automated test output confirming zero regressions.
4. **Code Quality and Architectural Alignment**: New contributions must preserve existing comments and documentation integrity. Avoid introducing external dependencies when native implementations suffice. Every pull request undergoes peer review and automated continuous integration before merging into main.
