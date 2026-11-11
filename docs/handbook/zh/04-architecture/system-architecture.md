---
id: system-architecture
track: architecture
trackOrder: 4
chapterOrder: 1
kicker: "Architecture 01 / 总体架构"
title: "系统总体架构与前后端渲染等价契约"
subtitle: "Tauri 2 进程模型、异步双阶段流水线与 WebGL2 着色器与 Rust CPU 导出的 WYSIWYG 等价契约。"
readingTime: 12
inApp: false
sinceVersion: "v1.1.0"
---

## 进程模型与系统技术栈拓扑 {#process-model-and-tech-stack}

NexFilm 采用现代桌面混合架构构建，旨在兼顾底层高强度图像计算的极限吞吐与用户交互界面的灵活响应。整个应用由基于 Rust 语言的底层原生内核与基于 Web 技术的高性能暗房前端构成，通过 Tauri 2 框架实现跨平台系统集成与沙箱隔离。

系统整体架构由四大核心支柱协同运作：

1. **Tauri 2 宿主与进程模型**：应用主进程运行于原生 Rust 环境中，负责操作系统窗口管理、原生菜单拦截、跨平台文件系统安全访问以及异步任务调度。前端视图通过平台原生的高性能 WebView（macOS 上的 WebKit、Windows 上的 WebView2、Linux 上的 WebKitGTK 4.1）进行隔离渲染，前端与原生后端之间仅通过经过严格类型校验的 IPC 异步命令通道通信。
2. **Rust 多线程并行计算核心**：图像解码、空间几何投影、透射率数学归一化、片基采样分析以及母带级文件编码等计算密集型任务，全部由 Rust 本地编译代码承担。底层深度依托 Rayon 数据并行框架，充分榨干现代多核 CPU 的 SIMD 矢量计算单元与各物理核心算力。
3. **原生 HTML5 / CSS / WebGL2 前端交互层**：前端杜绝引入重型庞杂的第三方单页框架，坚持使用纯原生 JavaScript、Tailwind CSS 与 WebGL2 硬件加速着色器编写。在用户拖动印片光、调整反差曲线或微调几何片门时，由 GPU 直接在画布上以 60 帧每秒的极低延迟呈现像素级实时预览。
4. **SQLite 本地持久化与状态引擎**：胶卷库元数据、胶片物理范围四角手柄坐标、印片光调色参数、片基测量记录以及硬件校准文件，统一持久化于本地 SQLite 数据库文件 `nexfilm_user.db` 中，提供无破坏、可追溯的工程管理。

| 架构层级 | 技术选型与运行环境 | 核心职能分工 | 性能与安全契约 |
| --- | --- | --- | --- |
| 宿主运行时 | Tauri 2 (Rust) | 操作系统窗口生命周期、安全 IPC 路由、文件拖放 | 最小内存常驻、零捆绑 Chromium 冗余 |
| 原生内核 | Rust 2021 + Rayon | LibRaw / Rawler RAW 解码、流式 TIFF 吞吐、数学流水线 | 多线程数据并行、内存安全、零段错误保证 |
| 图形加速 | WebGL2 (GLSL ES 3.00) | 实时片段着色器、对数密度反相映射、交互曲线渲染 | GPU 硬件加速、60fps 实时预览、零掉帧交互 |
| 数据持久化 | SQLite 3 (rusqlite) | 胶卷库管理、片门几何手柄、印片光参数、撤销重做 | ACID 事务安全、工程级迁移幂等、离线隐私 |

## 异步双阶段性能架构：缩略图秒级入库与按需代理解码 {#two-phase-asynchronous-pipeline}

胶片翻拍工程往往伴随着海量高分辨率 RAW 文件或数百兆的滚筒扫描仪 TIFF 文件。若在用户导入胶卷时立即对每一张照片执行全分辨率解马赛克与反相运算，会导致系统界面长时间假死，内存峰值瞬间突破物理限制。

为了保证数秒内完成整卷胶卷的无卡顿载入，NexFilm 设计了**异步双阶段性能流水线**：

```text
阶段一：图库导入期（秒级入库）
用户拖入胶卷文件夹 -> 扫描文件列表
      │
      ├─> 读取相机 RAW 嵌入式 JPEG 预览（毫秒级提取，零全量解码）
      ├─> 流式提取 TIFF / FFF 第一张缩略图 IFD
      └─> 写入 SQLite 胶卷索引表，立即渲染底片条并恢复交互响应

阶段二：工作台显影期（按需解码）
用户点击选中某张底片切入 Develop 视图
      │
      ├─> 后台线程池按需触发全量 RAW 解码（LibRaw / Rawler）
      ├─> 转换为 16-bit 线性正片代理（有界 LRU 缓存）
      ├─> 上传至 WebGL2 纹理单元
      └─> 开启 60fps GPU 实时着色器交互
```

在导入阶段，系统仅通过极低开销读取文件头部嵌入的缩略图元数据，以每秒数十张的吞吐速度瞬间完成数十张底片的入库索引，用户可立即在底片条中浏览整体拍摄序列。

当且仅当用户点击特定帧进入 `工作台` 视图时，后端才会调度后台工作线程池，按需启动完整的解马赛克流水线，生成 16-bit 线性工作代理并缓存于受保护的有限内存环中。这种架构将资源消耗精确收敛至用户当前关注的画幅上，彻底消除了内存溢出风险。

## 所见即所得：WebGL2 与 CPU 渲染等价契约 {#wysiwyg-rendering-parity-contract}

在数字暗房软件中，调色师最不能容忍的缺陷是“屏幕预览与最终导出文件颜色不一致”。造成这一问题的根源通常在于：前端预览为了追求帧率采用了简化版近似着色器，而后台全分辨率导出则运行了另一套更加精细的 CPU 运算代码，导致两者在端点截断、曲线插值或色彩空间映射上产生微妙的数学漂移。

NexFilm 在架构层面确立了不可动摇的 **WYSIWYG 数学等价契约**：

1. **核心渲染函数同源对齐**：Rust 后端在导出全分辨率图像时，调用 `commands.rs` 中的核心函数 `render_shader_equivalent_core`；前端 WebGL2 片段着色器则直接将该函数的数学公式逐行翻译为 GLSL 代码。
2. **相同数学公理约束**：
   - **透射率到密度的映射**：两者采用相同的微小正实数门限 $\epsilon = 1.0 \times 10^{-6}$，公式统一为 $D = -\log_{10}(\max(T, \epsilon))$。
   - **片基扣除机制**：当存在有效测量片基时，两者强制将密度偏移固定为零（`channel_offsets = 0`），维持片基在正片输出中绝对中性。
   - **有界跨度响应逻辑**：GLSL 着色器中的动态增益分支与 CPU 中的 `channel_response_from_spans` 完全保持一致的 $1.15\times$ 死区与 $1.35\times$ / $3.0\times$ 约束。
   - **曲线查找表采样**：前端着色器与 CPU 导出器均采用基于 256 阶或 4096 阶三次样条插值的对数曲线 LUT，保证高光过渡平滑无阶跃。
3. **自动化跨端一致性断言**：通过 `scripts/verify-density-contract.cjs` 与着色器语法回归套件，每次代码提交均自动核验 WebGL 着色器变量名、宏定义与后端结构体的一致性，确保预览即所见，导出即所得。

> [!IMPORTANT]
> 任何向渲染管线添加新功能的改动，必须同时在 WebGL2 片段着色器与 Rust CPU 导出核心中以同构数学实现，严禁在任一端单独引入未对齐的启发式算法。

## 系统架构拓扑图与端到端信号流 {#system-architecture-signal-flow}

下图呈现了 NexFilm Engine 的整体系统架构与模块协作关系：

<svg viewBox="0 0 860 420" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg" style="background:#161618; border-radius:12px; border:1px solid #27272a; font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;">
<rect x="25" y="25" width="245" height="370" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="40" y="55" fill="#e4e4e7" font-size="12" font-weight="bold">原生前端 (WebView2 / WebKit)</text>
<text x="40" y="73" fill="#a1a1aa" font-size="10">UI 交互层 · WebGL2 实时渲染</text>
<rect x="40" y="95" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="52" y="116" fill="#fafafa" font-size="11">五大视图控制器 (main.js)</text>
<text x="52" y="132" fill="#a1a1aa" font-size="9">Library / Develop / Rolls / Calib</text>
<rect x="40" y="155" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="52" y="176" fill="#fafafa" font-size="11">暗房微调交互 (tone-curve.js)</text>
<text x="52" y="192" fill="#a1a1aa" font-size="9">印片光对数滑块 / 四通道三次样条</text>
<rect x="40" y="215" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="52" y="236" fill="#fafafa" font-size="11">几何手柄 (geometry.js)</text>
<text x="52" y="252" fill="#a1a1aa" font-size="9">自动片门检测 / 四点透视单应变换</text>
<rect x="40" y="275" width="215" height="52" rx="6" fill="#3f3f46" stroke="#fbbf24" stroke-width="1.5"/>
<text x="52" y="297" fill="#fbbf24" font-size="11" font-weight="bold">WebGL2 片段着色器</text>
<text x="52" y="313" fill="#fef08a" font-size="9">60fps 实时预览 · GLSL ES 3.00</text>
<rect x="40" y="338" width="215" height="38" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="52" y="358" fill="#fafafa" font-size="10">IPC Client (invoke/listen)</text>
<line x1="270" y1="357" x2="305" y2="357" stroke="#fbbf24" stroke-width="2"/>
<rect x="305" y="25" width="250" height="370" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="320" y="55" fill="#e4e4e7" font-size="12" font-weight="bold">Tauri 2 宿主与命令编排</text>
<text x="320" y="73" fill="#a1a1aa" font-size="10">系统服务 · 异步 IPC 编排层</text>
<rect x="320" y="95" width="220" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="116" fill="#fafafa" font-size="11">IPC 命令中心 (commands.rs)</text>
<text x="332" y="132" fill="#a1a1aa" font-size="9">resolve_input_domain / analyze_base</text>
<rect x="320" y="155" width="220" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="176" fill="#fafafa" font-size="11">应用共享状态 (app_state.rs)</text>
<text x="332" y="192" fill="#a1a1aa" font-size="9">PipelineState / Roll / TuningParams</text>
<rect x="320" y="215" width="220" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="236" fill="#fafafa" font-size="11">两阶段解码任务池</text>
<text x="332" y="252" fill="#a1a1aa" font-size="9">后台异步线程池 · 16-bit 线性代理</text>
<rect x="320" y="275" width="220" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="296" fill="#fafafa" font-size="11">SQLite 持久化引擎 (persistence.rs)</text>
<text x="332" y="312" fill="#a1a1aa" font-size="9">nexfilm_user.db · 增量配方平滑迁移</text>
<rect x="320" y="335" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="355" fill="#fafafa" font-size="10">文件系统与安全沙箱门禁</text>
<line x1="555" y1="179" x2="590" y2="179" stroke="#34d399" stroke-width="2"/>
<rect x="590" y="25" width="245" height="370" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="605" y="55" fill="#e4e4e7" font-size="12" font-weight="bold">Rust 图像计算与母带引擎</text>
<text x="605" y="73" fill="#a1a1aa" font-size="10">高性能 Native 内核 · 数学流水线</text>
<rect x="605" y="95" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="116" fill="#fafafa" font-size="11">双解码后端 (raw_backend.rs)</text>
<text x="617" y="132" fill="#a1a1aa" font-size="9">LibRaw 0.22+ 与 Rawler 纯 Rust</text>
<rect x="605" y="155" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="176" fill="#fafafa" font-size="11">流式分行读取器 (io_processor.rs)</text>
<text x="617" y="192" fill="#a1a1aa" font-size="9">百兆/GB 级 TIFF 与 FFF 恒定内存</text>
<rect x="605" y="215" width="215" height="48" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="236" fill="#fafafa" font-size="11">色彩科学库 (color_science.rs)</text>
<text x="617" y="252" fill="#a1a1aa" font-size="9">Linear ProPhoto / ACEScg / ICC 生成</text>
<rect x="605" y="275" width="215" height="52" rx="6" fill="#3f3f46" stroke="#34d399" stroke-width="1.5"/>
<text x="617" y="297" fill="#34d399" font-size="11" font-weight="bold">CPU 导出器 (render_shader...)</text>
<text x="617" y="313" fill="#a7f3d0" font-size="9">全分辨率逐行计算 · 与着色器完全等价</text>
<rect x="605" y="338" width="215" height="38" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="358" fill="#fafafa" font-size="10">DNG 封装器 (dng_writer.rs)</text>
</svg>

## 本地持久化与无损编辑安全准则 {#local-persistence-and-safety}

在长期的胶卷数字化管理中，用户对数千张底片的裁剪范围、印片光与曲线调色记录是极其宝贵的智力资产。NexFilm 坚持纯粹的无损编辑原则：**软件绝不修改用户硬盘上的原始 RAW、TIFF 或扫描件文件内容**。

所有的编辑操作均被抽象为轻量级配方状态，存储于 SQLite 数据库中。数据库管理模块（`src/persistence.rs`）遵循以下安全准则：

1. **版本化模式迁移**：数据库表结构维护明确的 `MATH_VERSION` 与 `RAW_DECODE_VERSION`。当底层数学模型升级时（如 v1.1.0 废弃旧版 `legacy_linear_srgb` 经验模式），系统通过 `migrate_retired_density_recipe` 函数执行无感幂等迁移，将旧记录安全平滑地提升至新标准，同时为不可逆改动保留完整的审计日志。
2. **ACID 事务与状态原子性**：整卷批处理、参数广播或批量导出标记均被包裹在单次 SQLite 事务中。如果在操作过程中发生断电或程序异常退出，事务机制保证数据库状态要么完全回滚、要么完全提交，绝不会产生破坏性的半写入脏数据。
3. **平台安全存储路径隔离**：在调试模式下，数据库存放在工程目录以便追溯；在正式发布版中，数据库自动迁移至操作系统规范的标准应用支持目录（macOS 上的 `~/Library/Application Support/NexFilm Engine/`、Windows 上的 `%APPDATA%/NexFilm Engine/`），确保普通系统清理工具不会意外误删用户的调色库。
