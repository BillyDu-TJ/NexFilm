<p align="center">
  <img src="icons/icon.png" width="112" alt="NexFilm Engine icon">
</p>

<h1 align="center">NexFilm Engine</h1>

<p align="center">
  Film-negative conversion, color grading, and roll management in one local desktop workspace.<br>
  集负片转换、校色、胶卷管理与高质量输出于一体的本地桌面应用。
</p>

<p align="center">
  <a href="https://github.com/BillyDu-TJ/NexFilm/releases/latest"><img alt="Download for Windows x64" src="https://img.shields.io/badge/Windows-x64%20installer-1677ff?style=for-the-badge&logo=windows11&logoColor=white"></a>
  <a href="https://github.com/BillyDu-TJ/NexFilm/releases/latest"><img alt="Download for macOS Apple Silicon" src="https://img.shields.io/badge/macOS-Apple%20Silicon-111111?style=for-the-badge&logo=apple&logoColor=white"></a>
  <a href="https://github.com/BillyDu-TJ/NexFilm/releases/latest"><img alt="Download for macOS Intel" src="https://img.shields.io/badge/macOS-Intel-555555?style=for-the-badge&logo=apple&logoColor=white"></a>
</p>

<p align="center">
  <a href="https://github.com/BillyDu-TJ/NexFilm/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/BillyDu-TJ/NexFilm?display_name=tag&sort=semver"></a>
  <a href="https://github.com/BillyDu-TJ/NexFilm/actions/workflows/release.yml"><img alt="Release build status" src="https://github.com/BillyDu-TJ/NexFilm/actions/workflows/release.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="GNU GPL v3" src="https://img.shields.io/badge/license-GPL--3.0--only-2ea44f.svg"></a>
</p>

<p align="center">
  <a href="#中文说明">中文</a> · <a href="#english">English</a>
</p>

<p align="center">
  <img src="docs/assets/nexfilm-v1.0-develop.png" width="100%" alt="NexFilm Engine Develop workspace">
</p>

<p align="center"><sub>NexFilm Engine · Develop 工作区 / Develop workspace · 浅色主题 / Light theme</sub></p>

---

<a id="中文说明"></a>

## 中文说明

> [!IMPORTANT]
> NexFilm Engine v1.2.1 是当前稳定版本。Windows 与 macOS 安装包目前未进行代码签名，macOS DMG 也未经过 Apple 公证。请只从本仓库的 Releases 下载，并始终保留原始扫描文件；正式批量输出前，建议先导出少量画面检查结果。

### 下载与安装

从 [GitHub Releases](https://github.com/BillyDu-TJ/NexFilm/releases/latest) 下载与电脑架构匹配的 v1.2.1 安装包：

| 平台 | 下载文件 | 安装方式 |
| --- | --- | --- |
| Windows 10/11 x64 | x64 <code>.exe</code> 安装程序 | 双击并按提示安装；未签名版本可能触发 SmartScreen |
| macOS Apple Silicon | 文件名含 <code>aarch64</code> 的 <code>.dmg</code> | 打开 DMG，将 NexFilm Engine 拖入 Applications |
| macOS Intel | 文件名含 <code>x86_64</code> 的 <code>.dmg</code> | 打开 DMG，将 NexFilm Engine 拖入 Applications |

不要下载 Releases 中自动生成的 <code>Source code</code> 压缩包，它们不是可直接运行的安装包。若 macOS 阻止首次启动，请先确认文件来自本仓库，再在 Finder 中右键应用并选择 **打开**，或前往 **系统设置 → 隐私与安全性** 允许启动。

### v1.2.1 更新内容

从功能形态上看，`v1.2.1` 没有新增复杂的调节面板，重点提升**解码稳定性、源文件兼容性、片基反相一致性与异常报错清晰度**；但在工程底层，这一版对直接 RGB 线性化、宽色域/合并 DNG 解码与片基/密度管线进行了深入重构：

- **扫描 JPEG / PNG / TIFF 直通 ProPhoto f32**：直接 RGB 图像不再经过 16 位线性 sRGB 中转，也不在密度计算前压缩色度；结合 `moxcms` 真实 ICC 解析、浮点 TIFF 保留与 EXIF 方向校正，**正常扫描仪导出的 JPEG 现已可在 v1.2.1 中稳定反相**，无片基扫描件也能获得可靠色彩表现。
- **LinearRaw 与 Lightroom 拼接合并 DNG 完整支持**：同时兼容 IFD0 与 SubIFD 结构，支持通过 `rawler` 读取压缩 LinearRaw、在裁切降采样前执行 DNG Stage-2 `OpcodeList2`（`MapTable` / `MapPolynomial`）逆映射，并按内嵌 ICC 或双光源 `ColorMatrix1/2` + `AsShotNeutral` 还原线性色彩。
- **统一现代解码与片基锚定**：统一预览、Roll 锚点、Contact Sheet 与批量导出所用的现代解码入口（`RAW_DECODE_VERSION = 15` / `DENSITY_WINDOW_RULE_VERSION = 6`），分离可信片基（`d_min = 0`）与内容兜底，升级后自动失效旧缓存并按新管线刷新。
- **Canon `.CR2` 预览与 135 齿孔片门修复**：严格校验嵌入 JPEG 预览并跳过 RAW 切片 IFD，防止高像素 `.CR2` 误读数十 MB 的无损传感器数据作缩略图；为横向 `3:2` 135 扫描件增加上/下边界内缩恢复，减少片门误锁定至胶片最外侧边沿的问题。
- **清晰报错与一键复制诊断**：内嵌 ICC 损坏、相机矩阵/元数据退化、导入、自动反相、自动框选、Develop 与批量导出异常均可在 Toast 中提示明确阶段原因，并支持一键复制结构化诊断信息以便反馈排查。

### NexFilm 是什么

NexFilm Engine 用于将相机翻拍或扫描仪输出的胶片负片转换为正片，并把导入、反相、校色、胶卷归档和输出组织在同一个工作区。应用采用 Rust、Tauri、SQLite 与 WebGL 构建；图库、编辑状态和胶卷资料均保存在本地，核心工作流不依赖云端服务。

### 主要功能

- **胶卷与散张工作流**：按画幅、相机、胶片型号、日期和备注导入整卷，也可直接拖入散张；备注用于区分同一款胶片的多卷素材，并显示在胶卷卡片与胶卷资料中；支持继续编辑、追加画面、修改胶卷信息、筛选归档、重定位缺失文件和删除记录。
- **自动胶片范围识别**：自动检测有效成像区域，也可拖动四角或边缘手动修正；范围仅用于内容分析与边缘排除，不会自动改变画面透视。固定机位扫描时可将范围批量应用到同卷其他画面。
- **密度域反相与自动校色**：基于线性透射率进行密度转换、片基扣除与统一的通道响应补偿，输入域逐帧记录；提供 Color / B&W、D-Min / D-Max、Printer Lights、曝光、Gamma、对比度、高光、阴影、饱和度、色温和色调控制。算法细节见 [统一密度管线说明](docs/unified-density-pipeline.md) 与 [数据处理管线说明](data_process_pipeline_doc.md)。
- **曲线与细节**：印片光之后是四条控制点曲线（RGB 主曲线加红绿蓝分通道，带直方图背景，可拖动、加点、删点；首尾端点可自由横移并显示实时坐标），观感调整之后是细节分组：锐化的数量 / 半径 / 细节 / 蒙版，以及纹理、清晰度、去朦胧。按住 <kbd>Alt</kbd> 拖动锐化参数可在预览中查看锐化信号或蒙版的灰度视图。半径按长边 2560 像素换算，预览与导出观感一致；每个分组标题右侧的三角可收起参数，状态全局保留。
- **整卷标定与实验性硬件校正**：可按卷标定共享白点并整卷自动反相；硬件校正视图可管理采集校正配置（dark / open gate / flat / 透射目标）与扫描仪输入配置，用于固定的翻拍或扫描设备。该部分仍标记为实验性。
- **实时 Develop 工作区**：WebGL 交互预览，支持裁切、拉直、透视、90 度旋转、水平/垂直翻转、缩放平移，以及直方图和波形图。
- **设置复制与批处理**：可按类别选择要复制的校色、曲线、细节、LUT、齿孔和几何参数，再粘贴到其他相似画面。
- **打印胶片模拟**：内置 Kodak 与 Ilford 打印胶片/相纸风格 LUT，支持载入自定义 <code>.cube</code> LUT 并调节强度。
- **完整输出工具**：可导出选中画面或整卷，支持 16/8 位 TIFF、16 位 PNG、JPEG，以及线性 16 位 TIFF、线性 DNG 和相机 RAW DNG；提供输出色彩空间、尺寸、放大策略、锐化、JPEG 质量、命名模板、重名策略和可选 EXIF 胶卷信息。
- **胶卷接触印样**：导出前预览整卷排版，支持包含或省略片边的严格排布，并以逐帧高清渲染合成带胶片型号、拍摄日期、相机和画面编号的 JPEG / PNG Contact Sheet。
- **本地化与主题**：提供中文/英文界面和浅色/深色主题；前端样式随安装包本地打包，可离线使用，完整使用手册维护在 [docs/handbook](docs/handbook) 与官网文档中心。

### 快速开始

#### 1. 导入

1. 点击右上角 **Import Roll**。
2. 整卷扫描选择 **Import by Roll**，填写画幅、相机、胶片和日期后选择图片；临时处理散张可选择 **Loose Import**，也可直接拖入窗口。
3. 导入过程会建立本地预览与编辑记录，不会复制或修改原始扫描文件。

Library 与 Develop 显示当前工作卷；历史胶卷保存在 **Rolls**。从 Rolls 使用 **Promote to Library**，或在导入菜单中选择 **Continue Editing Roll**，即可继续以前的工作。

#### 2. 确认范围并反相

1. 在 **Library** 选择画面并进入 **Develop**。
2. 首次编辑先选择 **Set Film Area → Auto Area**。检查检测结果，必要时拖动四角、边缘或整框调整范围，也可切换画幅比例锁，再选择 **Save Area**。
3. 同卷画面位置一致时使用 **Batch Apply**；位置不一致时应逐张确认。
4. 选择 **Auto Invert** 生成正片，再使用密度、印片光、曲线、观感调整、细节、色彩平衡和 LUT 完成调整。
5. 齿孔或边框干扰校色时，可使用 **Sample Sprocket Hole** 采样；相似画面可通过 **Copy Settings / Paste Settings** 复用参数。

#### 3. 导出与归档

1. 在 Library 选择画面，或进入 Rolls 后使用 **Export Roll** 输出整卷。
2. 在导出窗口设置格式、色彩空间、尺寸、锐化、命名模板与重名策略，然后选择目标文件夹。
3. 导出在后台执行，可继续浏览和编辑。正式批量导出前请先检查一张样片。
4. 在 **Rolls → Roll Contents** 中选择 **Export Contact Sheet** 可先预览排版与片边开关再生成接触印样；源文件移动后使用 **Locate File** 重新关联。

16 位 TIFF 适合存档或继续精修；JPEG 适合日常分享。需要把结果交给其他软件继续调色时，可以选择线性 16 位 TIFF 或线性 DNG（线性光，不带显示曲线，并写入对应色彩空间）；需要原样归档扫描的 RAW 数据时，可以选择相机 RAW DNG，它不参与任何调整，仅适用于相机 RAW 素材，其余画面请改用线性 DNG。命名模板支持 <code>{Roll}</code>、<code>{Camera}</code>、<code>{Film}</code>、<code>{Date}</code>、<code>{Original}</code> 和 <code>{Seq}</code>。

### 文件格式与色彩输出

输入选择器支持常见相机 RAW，包括 DNG、NEF/NRW、CR2/CR3、ARW/SRF/SR2、RAF、RW2、ORF/ORI、SRW、PEF、3FR、ERF、KDC/DCR、IIQ、MOS、MRW、X3F、RWL、FFF 和 RAW，以及 TIFF、JPEG、PNG。RAW 与 FFF 兼容性取决于内置解码器和具体设备；遇到问题时请在 [Issues](https://github.com/BillyDu-TJ/NexFilm/issues) 中注明相机或扫描仪型号、文件格式与错误信息。

> [!WARNING]
> 使用 **HE/HE*** 高效率压缩格式拍摄的 Nikon NEF 文件目前无法导入（开源解码库 LibRaw 与 <code>rawler</code> 均暂不支持该专利压缩格式）。此限制当前无法在 NexFilm 内修复；如需导入，请在相机中改用无损压缩/标准 NEF 格式，或先转换为 TIFF/DNG 后再导入。

自 **v1.2.1** 起，扫描得到的 JPEG 与 PNG 会按内嵌 ICC（或 sRGB）直接线性化进入 32 位浮点 ProPhoto 管线，并结合改进后的无片基内容兜底策略计算反相，正常扫描仪导出的 JPEG 也能直接获得稳定可靠的色彩还原；若需更充沛的高光恢复空间或使用硬件标定，仍推荐选用相机 RAW 或 16 位 TIFF。遇到异常源文件触发降级或报错时，可直接点击界面 Toast 上的 **“复制诊断”** 附上阶段日志提交 Issue。

输出格式为 16 位 TIFF、8 位 TIFF、16 位 PNG 和 8 位 JPEG。可嵌入与像素编码匹配的 sRGB、Display P3、Adobe RGB (1998)、Rec.2020、ProPhoto RGB、ACEScg 或 ACES2065-1 ICC 配置文件。尺寸调整保持宽高比，默认不会放大小图，也不会在重名时静默覆盖文件。

### 数据、隐私与备份

- Windows 正式版数据目录：<code>%APPDATA%\NexFilm Engine\\</code>
- macOS 正式版数据目录：<code>~/Library/Application Support/NexFilm Engine/</code>
- Debug 构建默认在仓库工作目录保存数据
- 主要数据库文件：<code>nexfilm_user.db</code>；<code>rolls.json</code> 是兼容镜像

数据库保存源文件路径、胶卷资料、编辑参数和预览图，但不会保存一套完整原图。NexFilm 不会覆盖源扫描文件；请同时备份原图和数据目录。移动原图后需在应用内使用 **Locate File** 重新定位。

### 本地开发

Windows 10/11 x64 是主要开发平台；GitHub Actions 同时构建 macOS Apple Silicon 和 Intel 版本。

前置条件：

- Rust stable 与 Tauri CLI 2
- Windows：Microsoft C++ Build Tools、Windows SDK、WebView2 Runtime
- macOS：Xcode Command Line Tools
- Node.js LTS，用于运行前端校验脚本

~~~powershell
git clone https://github.com/BillyDu-TJ/NexFilm.git
cd NexFilm
cargo install tauri-cli --version "^2" --locked
cargo tauri dev
~~~

首次编译会下载 Rust 依赖并编译随仓库提供的 LibRaw 源码，因此明显慢于后续增量编译。

常用检查：

~~~powershell
cargo fmt --check
cargo check --locked
cargo test --locked --lib
node --check ui/main.js
node --check ui/geometry.js
node --check ui/contact-sheet.js
node --check ui/density-math.js
node --check ui/tone-curve.js
node --test ui/tone-curve.test.js
node scripts/verify-contact-sheet.cjs
node scripts/verify-develop-detail-ui.cjs
node scripts/verify-density-contract.cjs
node scripts/verify-histogram-contract.cjs
node scripts/verify-geometry-contract.cjs
node scripts/verify-i18n-contract.cjs
node scripts/verify-library-management-contract.cjs
node scripts/verify-ui-startup-order.cjs
node scripts/verify-webgl-shaders.cjs
~~~

本地打包：

~~~powershell
# Windows
cargo tauri build --bundles nsis
~~~

~~~bash
# macOS
cargo tauri build --target aarch64-apple-darwin --bundles dmg
cargo tauri build --target x86_64-apple-darwin --bundles dmg
~~~

### 维护者：发布 v1.2.1

仓库使用单一的手动工作流 [Build and Release NexFilm 1.2.1](https://github.com/BillyDu-TJ/NexFilm/actions/workflows/release.yml)。运行时输入标签 <code>v1.2.1</code>；工作流会先执行 Rust 与前端校验，随后并行构建 Windows x64、macOS Apple Silicon 和 macOS Intel 安装包，并发布为正式 GitHub Release。

发布前请确认 <code>Cargo.toml</code>、<code>Cargo.lock</code> 与 <code>tauri.conf.json</code> 均为 <code>1.2.1</code>，README 与发布说明已同步，提交并推送全部发布文件，再从 Actions 手动运行一次工作流。当前配置为 <code>releaseDraft: false</code>、<code>prerelease: false</code>。

### 已知限制

- Windows 和 macOS 安装包尚未进行代码签名；macOS DMG 尚未经过 Apple 公证。
- 不同 RAW 相机、扫描仪与 FFF 文件的兼容性仍需持续验证。
- Nikon HE/HE* 高效率 RAW 压缩格式暂无法由开源解码库（LibRaw / `rawler`）解码。
- 硬件校正相关能力（Capture Corrected、Capture Characterized、扫描仪输入配置）仍为实验性，需要在真实设备素材上继续验证。
- 超大 JPEG/PNG 受解码器内存上限约束，可能提示解码失败，建议改用 TIFF 或 DNG。
- 全分辨率导出大型扫描件需要较大的可用内存。
- Linux 暂无经过验证的正式安装包。
- 数据库引用原始文件路径，不会复制原图；移动或删除源文件会导致画面离线。
- WebGL 预览与全尺寸导出仍需在更多真实 RAW 文件上持续进行像素级一致性验证。

### 赞赏

如果 NexFilm 对你的胶片工作流有所帮助，欢迎分别支持软件作者与 UI 设计师。赞赏完全自愿，不影响软件功能、开源许可或问题处理优先级。

<table align="center">
  <tr>
    <td align="center"><strong>软件作者 Billy_Du</strong><br><img src="ui/assets/author-sponsor-qr.jpg" width="240" alt="Billy_Du 收款码"></td>
    <td align="center"><strong>UI 设计师 lonely-xmw</strong><br><img src="ui/assets/uidesigner-sponsor-qr.jpg" width="240" alt="lonely-xmw 收款码"></td>
  </tr>
</table>

### 反馈、鸣谢与许可

- 问题反馈：[GitHub Issues](https://github.com/BillyDu-TJ/NexFilm/issues)
- 软件作者：Billy_Du，GitHub [@BillyDu-TJ](https://github.com/BillyDu-TJ)，邮箱 [790704944@qq.com](mailto:790704944@qq.com) / [duzhongtao188@gmail.com](mailto:duzhongtao188@gmail.com)
- UI 设计：lonely-xmw，GitHub [@lxmw44426-tech](https://github.com/lxmw44426-tech)，邮箱 [lxmw44426@gmail.com](mailto:lxmw44426@gmail.com)
- 代码贡献：感谢 `jacky_tan`（[PR #10](https://github.com/BillyDu-TJ/NexFilm/pull/10) / [PR #12](https://github.com/BillyDu-TJ/NexFilm/pull/12)）贡献相机原生 RGB 反相主路径、显示矩阵传递与 RAW 预览方向修正
- 理论参考：感谢知乎用户 <code>@黄昊Haosky</code> 与 <code>@V777</code> 对科学去色罩理论的分享
- 开源许可：[GNU General Public License v3.0 only](LICENSE)

提交 Bug 时请包含系统版本、设备型号、输入格式、复现步骤和错误信息，并避免上传含隐私内容的原片。代码贡献请保持改动聚焦，并在 Pull Request 中写明验证方式。

---

<a id="english"></a>

## English

> [!IMPORTANT]
> NexFilm Engine v1.2.1 is the current stable release. Windows and macOS packages are currently unsigned, and macOS DMGs are not notarized by Apple. Download only from this repository's Releases page, keep the original scans, and test a small export before processing a full roll.

### Download and install

Download the v1.2.1 package for your system from [GitHub Releases](https://github.com/BillyDu-TJ/NexFilm/releases/latest):

| Platform | Package | Installation |
| --- | --- | --- |
| Windows 10/11 x64 | x64 <code>.exe</code> installer | Run the installer; an unsigned build may trigger SmartScreen |
| macOS Apple Silicon | <code>.dmg</code> containing <code>aarch64</code> | Open the DMG and drag NexFilm Engine to Applications |
| macOS Intel | <code>.dmg</code> containing <code>x86_64</code> | Open the DMG and drag NexFilm Engine to Applications |

The automatically generated <code>Source code</code> archives are not application installers. If macOS blocks the first launch, verify that the DMG came from this repository, then right-click the app in Finder and choose **Open**, or allow it under **System Settings → Privacy & Security**.

### What's new in v1.2.1

From a user-facing feature standpoint, `v1.2.1` introduces no new UI panels and focuses on **decoding stability, broader source compatibility, film-base consistency, and clear error diagnostics**. Beneath the surface, however, it represents a substantial internal overhaul of direct RGB linearization, LinearRaw/merged DNG decoding, and film-base anchoring:

- **Direct RGB & scanned JPEG / PNG / TIFF straight to ProPhoto f32.** Scanned TIFF, JPEG, and PNG inputs bypass the legacy 16-bit linear-sRGB transport and luminance chroma compression, linearizing directly into `f32` ProPhoto RGB with `moxcms` ICC parsing, `f32`/`f64` float TIFF support, and EXIF orientation handling—so **standard scanner JPEGs now invert cleanly out of the box**.
- **Complete LinearRaw & Lightroom stitched / merged DNG support.** Handles both IFD0 and SubIFD LinearRaw layouts, compressed LinearRaw decoding via `rawler`, DNG Stage-2 `OpcodeList2` (`MapTable` / `MapPolynomial`) inverse mapping prior to crop or downsampling, and linear color resolution via embedded ICC or dual-illuminant `ColorMatrix1/2` + `AsShotNeutral`.
- **Unified modern decode entry & film-base anchoring.** Unifies preview, Roll anchor sampling, Contact Sheets, and batch export on one modern decode contract (`RAW_DECODE_VERSION = 15` / `DENSITY_WINDOW_RULE_VERSION = 6`), separating trusted bases (`d_min = 0`) from borderless content fallbacks and refreshing affected caches cleanly on upgrade.
- **Canon `.CR2` preview & 135 sprocket border fixes.** Tightens embedded JPEG preview detection and skips raw slice IFDs (`0xC640`) so high-res `.CR2` files never read tens of megabytes of lossless sensor slices as thumbnails; recovers inward top/bottom bounds on horizontal `3:2` 135 scans when outer sprocket edges dominate gradient projections.
- **Pipeline degradation warnings & copyable diagnostics.** Surface actionable stage errors in Toast notifications with a one-click **Copy details** button for ICC parse errors, DNG/scanner/camera-matrix fallbacks, and import, auto-invert, film-area, Develop, or batch-export failures.

### What is NexFilm?

NexFilm Engine converts camera-scanned or scanner-produced film negatives into positives while keeping import, inversion, grading, roll archiving, and output in one workspace. It is built with Rust, Tauri, SQLite, and WebGL. The library, edit state, and roll metadata remain local; the core workflow does not require a cloud service.

### Features

- **Roll and loose-frame workflows** with format, camera, film stock, date, and note metadata. The note tells several rolls of the same film stock apart and appears on the roll card and in the roll details; drag-and-drop import, archived-roll editing, append, metadata editing, missing-file relocation, and deletion are all supported.
- **Automatic film-area detection**, with manual corner/edge adjustment and batch reuse for consistently positioned scans. The detected area guides analysis and edge exclusion without automatically changing image perspective.
- **Density-domain inversion and automatic grading** with per-channel film-base subtraction and one shared channel-response stage, with the input domain recorded per frame; Color / B&W modes, D-Min / D-Max, Printer Lights, exposure, gamma, contrast, highlights, shadows, saturation, temperature, and tint. See the [unified density pipeline](docs/unified-density-pipeline.md) and the [data-processing pipeline](data_process_pipeline_doc.md) for implementation details.
- **Tone curve and Detail**: four point curves after Printer Lights (composite RGB plus red, green, and blue channels, drawn over the frame's histogram, with dragging, adding, removing, and freely movable endpoints with a live coordinate readout), and a Detail group after Aesthetics covering sharpening (amount, radius, detail, masking) plus texture, clarity, and dehaze. Holding <kbd>Alt</kbd> while dragging a Sharpening control shows the sharpening signal or the mask as a grayscale view. Every radius is quoted at a 2560 px long edge, so the preview and the export show the same work; a triangle at the right of each section title folds the controls away and the state is remembered.
- **Roll calibration and experimental hardware calibration**: measure a shared roll white point and run Roll auto invert; the calibration view manages capture profiles (dark / open gate / flat / transmission target) and scanner input profiles for a fixed copy stand or scanner. This part is still marked experimental.
- **Real-time Develop workspace** powered by WebGL, with crop, straighten, perspective, quarter-turn rotation, horizontal/vertical flip, zoom/pan, histogram, and waveform.
- **Selective Copy Settings / Paste Settings** for grade, curve, detail, LUT, sprocket, and geometry groups.
- **Print-film emulation** with bundled Kodak and Ilford print-film/paper LUTs, custom <code>.cube</code> LUT loading, and opacity control.
- **Complete output tools** for selected frames or a full roll: 16/8-bit TIFF, 16-bit PNG, JPEG, linear 16-bit TIFF, linear DNG, and camera RAW DNG; output color space; resizing; optional enlargement; sharpening; JPEG quality; filename templates; conflict policies; and optional roll metadata in EXIF.
- **Contact sheet preview and export**, with strict layouts that include or omit film borders and per-frame high-quality rendering into JPEG or PNG, labeled with film stock, date, camera, and frame number.
- **English/Chinese UI and light/dark themes**, with frontend styles bundled for offline use; full user manuals are maintained on the web docs hub and in [docs/handbook](docs/handbook).

### Quick start

#### 1. Import

1. Select **Import Roll** in the upper-right corner.
2. Use **Import by Roll** for a complete roll and enter its format, camera, film stock, and date. Use **Loose Import** or drag files into the window for individual scans.
3. Import creates local previews and edit records without copying or modifying the source scans.

Library and Develop show the current working roll; previous rolls remain under **Rolls**. Use **Promote to Library** or **Continue Editing Roll** to resume earlier work.

#### 2. Confirm the film area and invert

1. Select a frame in **Library** and open **Develop**.
2. On first edit, choose **Set Film Area → Auto Area**. Review the result, adjust corners, edges, the whole frame box, or aspect locks if needed, then choose **Save Area**.
3. Use **Batch Apply** only when the scans share the same placement.
4. Choose **Auto Invert**, then refine Density Limits, Printer Lights, the Tone Curve, Aesthetics, Detail, Color Balance, and LUT settings.
5. Use **Sample Sprocket Hole** when perforations or borders affect the grade, and **Copy Settings / Paste Settings** for similar frames.

#### 3. Export and archive

1. Select frames in Library, or use **Export Roll** from a roll view.
2. Choose the format, color space, dimensions, sharpening, filename template, conflict policy, and destination.
3. Export runs in the background. Check one test image before committing to a full batch.
4. Use **Export Contact Sheet** under **Rolls → Roll Contents** to preview the sheet layout and border toggle before exporting; use **Locate File** if source scans have moved.

16-bit TIFF is recommended for archival output or further editing; JPEG is convenient for sharing. Filename tokens are <code>{Roll}</code>, <code>{Camera}</code>, <code>{Film}</code>, <code>{Date}</code>, <code>{Original}</code>, and <code>{Seq}</code>.

### File formats and color output

The input picker supports common camera RAW formats, including DNG, NEF/NRW, CR2/CR3, ARW/SRF/SR2, RAF, RW2, ORF/ORI, SRW, PEF, 3FR, ERF, KDC/DCR, IIQ, MOS, MRW, X3F, RWL, FFF, and RAW, plus TIFF, JPEG, and PNG. RAW and FFF compatibility depends on the bundled decoder and the specific device. Report failures through [Issues](https://github.com/BillyDu-TJ/NexFilm/issues) with the camera or scanner model, file format, and error.

> [!WARNING]
> Nikon NEF files recorded with **HE/HE*** high-efficiency RAW compression cannot currently be imported because open-source RAW decoders (LibRaw and `rawler`) do not support this proprietary format yet. NexFilm cannot work around this limitation at present; record in Lossless Compressed / standard NEF in-camera or convert to TIFF/DNG before importing.

Starting in **v1.2.1**, scanned JPEGs and PNGs linearize directly via embedded ICC (or sRGB) into 32-bit float ProPhoto RGB alongside improved borderless content-fallback handling, so normal scanner JPEGs invert cleanly out of the box. For extra highlight headroom or hardware calibration workflows, 16-bit TIFF or camera RAW remains preferable. If an unusual source file triggers a pipeline fallback or error, click **Copy details** on the Toast notification to share the stage diagnostic in [Issues](https://github.com/BillyDu-TJ/NexFilm/issues).

Exports are 16-bit TIFF, 8-bit TIFF, 16-bit PNG, 8-bit JPEG, linear 16-bit TIFF, linear DNG, or camera RAW DNG. NexFilm can embed a matching sRGB, Display P3, Adobe RGB (1998), Rec.2020, ProPhoto RGB, ACEScg, or ACES2065-1 ICC profile, and linear TIFF files carry the matching linear profile. Resizing preserves aspect ratio, does not enlarge by default, and does not silently overwrite name conflicts.

The linear and RAW formats are hand-off outputs. Linear TIFF and linear DNG hold the developed positive with no display curve, so another application can grade it further; linear DNG declares its colour space in the file itself. Camera RAW DNG re-wraps each frame's own RAW mosaic for archival, so grading, resizing, sharpening, and output color do not apply to it, and frames whose source is not a camera RAW file (TIFF, JPEG, or a scanner's linear DNG) are reported as failures instead — export those as linear DNG.

### Data, privacy, and backups

- Windows release data: <code>%APPDATA%\NexFilm Engine\\</code>
- macOS release data: <code>~/Library/Application Support/NexFilm Engine/</code>
- Debug builds: repository working directory
- Main database: <code>nexfilm_user.db</code>; <code>rolls.json</code> is a compatibility mirror

The database stores source paths, roll metadata, edit parameters, and previews, but not a complete copy of the originals. NexFilm never overwrites source scans. Back up both the originals and the application data directory, and use **Locate File** after moving source files.

### Local development

Windows 10/11 x64 is the primary development platform. GitHub Actions also packages Apple Silicon and Intel macOS builds.

Prerequisites:

- Rust stable and Tauri CLI 2
- Windows: Microsoft C++ Build Tools, Windows SDK, and WebView2 Runtime
- macOS: Xcode Command Line Tools
- Node.js LTS for frontend verification

~~~powershell
git clone https://github.com/BillyDu-TJ/NexFilm.git
cd NexFilm
cargo install tauri-cli --version "^2" --locked
cargo tauri dev
~~~

The first build downloads Rust dependencies and compiles the vendored LibRaw sources, so it is substantially slower than later incremental builds.

Run the verification commands listed in the [Chinese development section](#本地开发), or use the same checks run by the release workflow.

Local packaging:

~~~powershell
# Windows
cargo tauri build --bundles nsis
~~~

~~~bash
# macOS
cargo tauri build --target aarch64-apple-darwin --bundles dmg
cargo tauri build --target x86_64-apple-darwin --bundles dmg
~~~

### Maintainers: publish v1.2.1

The repository uses one manual workflow: [Build and Release NexFilm 1.2.1](https://github.com/BillyDu-TJ/NexFilm/actions/workflows/release.yml). Run it with tag <code>v1.2.1</code>. It verifies the Rust and frontend sources, then builds Windows x64, macOS Apple Silicon, and macOS Intel packages in parallel and publishes a stable GitHub Release.

Before running it, confirm that <code>Cargo.toml</code>, <code>Cargo.lock</code>, and <code>tauri.conf.json</code> contain version <code>1.2.1</code> and that the README and release notes are synchronized, then commit and push every release file. The current workflow uses <code>releaseDraft: false</code> and <code>prerelease: false</code>.

### Known limitations

- Windows and macOS packages are unsigned; macOS DMGs are not Apple-notarized.
- Compatibility still needs broader validation across RAW cameras, scanners, and FFF variants.
- Nikon HE/HE* high-efficiency RAW compression is not currently decodable by open-source decoders (LibRaw / `rawler`).
- Hardware calibration (Capture Corrected, Capture Characterized, scanner input profiles) is still experimental and needs validation on real hardware.
- Very large JPEG/PNG files are limited by the decoder's memory ceiling and may report a decode failure; prefer TIFF or DNG for those.
- Exporting a large scan at full resolution needs a substantial amount of free memory.
- No verified Linux release package is currently provided.
- The database references source paths and does not copy originals; moved or deleted files become unavailable until relocated.
- Pixel-level consistency between WebGL preview and full-resolution export continues to be tested against more real RAW files.

### Sponsorship

If NexFilm helps your film workflow, you may support the author and UI designer separately. Sponsorship is optional and does not affect features, licensing, or issue priority.

<table align="center">
  <tr>
    <td align="center"><strong>Author: Billy_Du</strong><br><img src="ui/assets/author-sponsor-qr.jpg" width="240" alt="Billy_Du payment QR code"></td>
    <td align="center"><strong>UI Designer: lonely-xmw</strong><br><img src="ui/assets/uidesigner-sponsor-qr.jpg" width="240" alt="lonely-xmw payment QR code"></td>
  </tr>
</table>

### Feedback, credits, and license

- Support: [GitHub Issues](https://github.com/BillyDu-TJ/NexFilm/issues)
- Author: Billy_Du, GitHub [@BillyDu-TJ](https://github.com/BillyDu-TJ), email [790704944@qq.com](mailto:790704944@qq.com) / [duzhongtao188@gmail.com](mailto:duzhongtao188@gmail.com)
- UI design: lonely-xmw, GitHub [@lxmw44426-tech](https://github.com/lxmw44426-tech), email [lxmw44426@gmail.com](mailto:lxmw44426@gmail.com)
- Code contributors: thanks to `jacky_tan` ([PR #10](https://github.com/BillyDu-TJ/NexFilm/pull/10) / [PR #12](https://github.com/BillyDu-TJ/NexFilm/pull/12)) for the camera-native RGB inversion path, display-matrix transport, and RAW preview upright fix
- Theory references: thanks to Zhihu users <code>@黄昊Haosky</code> and <code>@V777</code> for sharing work on scientific film-mask removal
- License: [GNU General Public License v3.0 only](LICENSE)

Bug reports should include the operating-system version, device model, input format, reproduction steps, and error message. Do not upload private scans. Keep code contributions focused and include verification details in the pull request.
