---
id: building-and-contributing
track: architecture
trackOrder: 4
chapterOrder: 3
kicker: "Architecture 03 / 编译与贡献"
title: "跨平台源码构建、自动化契约测试与开源贡献指南"
subtitle: "macOS、Windows 与 Linux 编译链路全复现、CI 自动化防御契约与 GPL-3.0 贡献规范。"
readingTime: 12
inApp: false
sinceVersion: "v1.1.0"
---

## 跨平台工具链准备与依赖配置 {#cross-platform-toolchain-setup}

NexFilm 致力于为全球开发者提供完全透明、确定性可复现的本地编译环境。项目采用纯正的开源技术栈，支持在 macOS（Apple Silicon 与 Intel 架构）、Windows（64 位 MSVC）与各大主流 Linux 发行版上独立构建。

在从源码构建之前，开发主机需安装基础工具链：

1. **Rust 稳定版工具链**：通过官方安装器 `rustup` 安装最新稳定版 Rust（建议版本 $\ge 1.78.0$），确保包含 `cargo` 与标准库：
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **Node.js LTS 运行时**：安装 Node.js LTS 版本（建议 Node.js 20.x 或更高版本），用于驱动自动化回归契约脚本与官网构建套件。
3. **平台专属原生依赖项**：
   - **macOS 环境**：通过 Homebrew 安装 `pkg-config` 与 `libraw` 动态库（必须为 LibRaw 0.22+ 版本）：
     ```bash
     brew update
     brew install pkg-config libraw
     ```
   - **Windows 环境**：安装 Visual Studio 2022 生成工具，勾选“使用 C++ 的桌面开发”工作负载，确保包含 MSVC v143 编译器与 Windows 10/11 SDK。
   - **Linux 环境（Ubuntu / Debian 系）**：安装 WebKitGTK 4.1 开发包与系统基础构建工具：
     ```bash
     sudo apt update && sudo apt install -y \
       build-essential pkg-config libwebkit2gtk-4.1-dev \
       libraw-dev libssl-dev libgtk-3-dev
     ```

克隆代码仓库至本地工作目录：

```bash
git clone https://github.com/BillyDu-TJ/NexFilm.git
cd NexFilm
```

## 编译命令、排错指南与安装包打包 {#compilation-commands-and-packaging}

完成工具链配置后，开发者可在本地以开发热重载模式或全优化生产模式启动 NexFilm。

### 1. 本地开发与实时热重载

运行以下命令，Tauri 将自动编译 Rust 本地依赖并启动原生窗口，同时前端文件支持修改即时生效：

```bash
cargo tauri dev
```

### 2. 生产环境分发包构建

当准备打包各平台独立安装包时，执行对应的构建命令：

- **macOS（打包为标准 DMG 镜像）**：
  ```bash
  # 构建 Apple Silicon 原生包 (M1/M2/M3/M4)
  cargo tauri build --target aarch64-apple-darwin --bundles dmg

  # 构建 Intel x86_64 架构包
  cargo tauri build --target x86_64-apple-darwin --bundles dmg
  ```
- **Windows（打包为 NSIS 引导安装程序）**：
  ```bash
  cargo tauri build --bundles nsis
  ```
- **Linux（打包为 Debian 包与 AppImage）**：
  ```bash
  cargo tauri build --bundles deb,appimage
  ```

构建生成的发布包将输出至 `target/release/bundle/` 对应子目录下。

### 3. 常见编译故障排查速查表

| 常见错误日志现象 | 触发根本原因 | 权威推荐修复方案 |
| --- | --- | --- |
| `pkg-config: libraw not found` | macOS 上 `PKG_CONFIG_PATH` 未正确指向 Homebrew 安装路径 | 执行 `export PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig"` 后重试 |
| `link.exe not found` | Windows 环境未安装 MSVC C++ 构建工具链 | 启动 Visual Studio Installer，勾选安装 C++ 生成工具并配置环境变量 |
| `Package webkit2gtk-4.1 was not found` | Linux 环境仅安装了旧版 4.0 包，缺少 4.1 接口 | 执行 `sudo apt install libwebkit2gtk-4.1-dev`，注意区别于旧版 4.0 |
| `cargo tauri command not found` | 全局或本地未注册 Tauri CLI | 执行 `cargo install tauri-cli --version "^2.0.0"` 注册原生命令 |

## 自动化契约测试套件全景解读 {#automated-contract-test-suites}

NexFilm 奉行“契约先行、机器守门”的软件工程文化。在 `scripts/` 目录下部署了 10 余项独立的自动化契约断言脚本，全方位拦截功能回归：

1. **底层 Rust 数学单元测试**：  
   执行 `cargo test --locked --lib`。全套 224 项单元测试覆盖了 `core_math` 矩阵运算、`color_science` 色适应变换、`pipeline` 跨度响应以及 `persistence` 数据库模式升级。
2. **桌面端自动化契约防御网**：  
   每次代码变动均可通过以下命令实现秒级全量自检：
   - `node scripts/verify-density-contract.cjs`：断言有片基时通道原点为零，并且不包含未对齐的逐通道显示曲线。
   - `node scripts/verify-webgl-shaders.cjs`：核验 GLSL 着色器语法有效性与前后端 Uniform 变量映射契约。
   - `node scripts/verify-contact-sheet.cjs`：断言接触印样排版网格计算与文件名导出规则。
   - `node scripts/verify-develop-detail-ui.cjs`：验证工作台视图、色调曲线面板与 WebGL 渲染通道链。
   - `node scripts/verify-i18n-contract.cjs`：校验 278 个多语言词条在中英双语配置中的对称性。
   - `node scripts/verify-runtime-assets.cjs`：核对运行时所需的相纸 LUT、图标与着色器资源完整性。
   - `node scripts/verify-ui-startup-order.cjs`：断言 `index.html` 中前端脚本标签的声明加载依赖顺序。
   - `node scripts/verify-slider-contract.cjs`：断言滑块范围、步长与默认值的一致性。
3. **双端同源文档一致性验证**：  
   `node scripts/compile-handbook.mjs && node scripts/verify-handbook-sync.cjs` 确保手册编译产物 `ui/documentation-data.js` 与 Markdown 源文件保持字节级一致。

> [!IMPORTANT]
> 任何 Pull Request 在提交前都应保证上述本地测试套件全部通过。不要修改契约脚本来迁就存在缺陷的代码实现。

## GitHub Actions CI/CD 流水线工作流解剖 {#ci-cd-release-workflow}

项目的持续集成与发布工作流维护于 `.github/workflows/release.yml` 文件中。流水线分为严谨的两阶段门禁：

```text
触发：发布标签推送 (如 v1.1.0) 或手动派发
  │
  ├─> 阶段一：源码统一验证
  │     - 运行环境：macos-14 纯净云端镜像
  │     - 步骤：检出代码 -> 安装 pkg-config & libraw 0.22+
  │     - 检验：cargo fmt --check（格式严格对齐）
  │     - 检验：cargo test --locked --lib（全量数学单元测试）
  │     - 检验：node --check ui/*.js & node scripts/verify-*.cjs
  │     - 门禁判定：仅当上述验证全部 100% 通过，才放行第二阶段
  │
  └─> 阶段二：矩阵并发跨平台构建
        ├─> 构建 Windows (x64) 安装包 (windows-latest -> NSIS)
        ├─> 构建 macOS Apple Silicon 安装包 (macos-15 -> DMG)
        ├─> 构建 macOS Intel 安装包 (macos-15-intel -> DMG)
        └─> 自动汇聚产物并发布至 GitHub Releases 资产库
```

这种两阶段设计避免了在代码存在基础语法错误或契约破坏时浪费宝贵的跨平台云端编译时长，确保了对外发布的每一个二进制资产均经过严密检验。

## 开源协议约束与 GPL-3.0 贡献准则 {#gpl-contribution-guidelines}

NexFilm 采用 **GNU 通用公共许可证第 3 版（GPL-3.0）** 授权分发。每一位参与贡献的开发者应当尊重并遵守以下准则：

1. **代码开源与衍生作品传染性**：GPL-3.0 要求任何包含或派生自 NexFilm 源码的分发版本，必须以相同的 GPL-3.0 许可证完整开源全部源代码。任何试图将核心色彩算法包装为闭源商业产品的行为均属侵权。
2. **SSOT 单一事实源原则**：使用文档的唯一权威源头位于 `docs/handbook/{zh,en}/` 目录下的 Markdown 文件。严禁直接手改 `ui/documentation-data.js` 编译产物。修改文档必须同步运行 `node scripts/compile-handbook.mjs`。
3. **严谨的提交历史与规范**：推荐使用遵循 Conventional Commits 规范的 Git 提交消息（如 `feat:`, `fix:`, `docs:`, `refactor:`, `test:`）。在发起 Pull Request 时，需在描述中清晰列出修改动机、影响模块与本地回归测试证据。
