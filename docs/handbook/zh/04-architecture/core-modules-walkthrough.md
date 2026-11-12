---
id: core-modules-walkthrough
track: architecture
trackOrder: 4
chapterOrder: 2
kicker: "Architecture 02 / 模块导览"
title: "核心源码模块导览与数据流分析"
subtitle: "深入剖析 Rust 后端核心 crate、流式分行解码器与 SQLite 数据库工程平滑升级机制。"
readingTime: 12
inApp: false
sinceVersion: "v1.1.0"
---

## Rust 后端核心模块拓扑与职责分工 {#rust-backend-module-topology}

NexFilm 的原生代码组织于 `src/` 目录下，采用高内聚、低耦合的模块化设计。各源文件之间边界清晰，严格区分命令编排、图像解码、数学计算与持久化存储：

```text
src/
├── main.rs & lib.rs             # Tauri 2 运行时装配与应用启动入口
├── commands.rs                  # 前端 IPC 命令编排中心与跨模块调用中枢
├── app_state.rs                 # 核心数据结构、管线配置与状态契约
├── core_math.rs                 # 基础数学库、nalgebra 矩阵与亮度度量
├── pipeline.rs                  # 负片密度反相数学管线
├── color_science.rs             # 线性色彩空间变换、原色色度与 ICC 生成
├── raw_backend.rs               # LibRaw 0.22+ 绑定与 C++ 马赛克垫片
├── rawler_backend.rs            # 纯 Rust RAW 解码后端与机型兼容支持
├── calibration_fit.rs           # SVD 奇异值分解通道分离拟合与条件数守卫
├── dng_writer.rs                # 极简单 IFD 线性与马赛克 DNG 编码器
├── scanner_profile.rs           # 扫描仪机型配置表与容器属性解析
├── io_processor.rs              # 流式分行图像读取与条带化内存管理
├── film_border.rs & geometry.rs # 自动片门轮廓检测与四边形投影算法
├── batch_settings.rs            # 整卷批处理参数广播与合并逻辑
└── persistence.rs               # SQLite 本地工程数据库存储与模式迁移
```

在系统初始化时，`lib.rs` 注册所有供前端调用的 Tauri 命令，并通过全局受控状态托管 `AppState`。`commands.rs` 作为顶层编排中心，负责协调 `raw_backend`、`pipeline` 与 `persistence`，确保耗时的图像运算全部在后台工作线程中完成，避免阻塞系统事件循环。

## 密度数学管线与色彩科学库内核剖析 {#pipeline-and-color-science-internals}

在色彩与密度计算层，NexFilm 严格维护无状态的函数式调用规范，原生兼容 Rayon 跨线程高并发处理：

1. **`src/pipeline.rs`（核心反相引擎）**：  
   `FilmPipeline` 结构体承载了光学密度的核心运算。代码通过 `contract` 字段明确区分旧版与新版管线：
   - **历史路径（`ProcessingContract::LegacyV1`）**：调用 `status_m_crosstalk_matrix()` 应用历史经验串扰矩阵，专供向后兼容旧版工程。
   - **现代统一路径（`ProcessingContract::SmartAutoV11` / `CaptureCorrectedV11`）**：将串扰矩阵置为 $3 \times 3$ 单位矩阵（`Matrix3::identity()`），基于物理实测透射率与明确解析的输入域展开运算，不额外强加未经验证的经验失真。
2. **`src/color_science.rs`（线性色彩变换引擎）**：  
   该模块统一托管了所有色彩空间（`ColorSpaceId`）的色度学定义，包括 sRGB、Display P3、Adobe RGB (1998)、ITU-R BT.2020、Linear ProPhoto RGB（ROMM RGB）、ACES 2065-1 与 ACEScg。所有色度转换矩阵均在双精度浮点数下利用 Bradford 色适应变换严格推导，保证浮点数精度的无损传递。同时，模块内建了标准 ICC Profile 字节级生成器，用于在导出 16-bit 图像时内嵌合规的色彩特性文件。
3. **`src/calibration_fit.rs`（硬件校准拟合）**：  
   在 `Capture Characterized` 模式下，该模块利用 `nalgebra::DMatrix` 与 `SVD` 分解求解带岭回归惩罚项的矩阵方程。模块内置了 `MIN_PATCHES = 4`、`MAX_CONDITION_NUMBER = 1.0e5` 以及 `MAX_VALIDATION_RMSE = 0.08` 三重安全防线，在出现病态条件时自动阻断不良矩阵并记录诊断事件。

> [!NOTE]
> `FilmPipeline` 的设计完全摒弃了全局可变锁。在批量反相或导出时，Rayon 线程池可以同时实例化数十个互不干扰的独立管线实例，实现吞吐量随 CPU 物理核心数呈线性扩展。

## 流式分行解码器与大图内存受控管理 {#streaming-scanline-decoders}

对于专业胶片扫描仪生成的百兆乃至数十吉字节超大 TIFF 或 Flextight `.fff` 图像，直接把整幅未压缩图像读入内存会造成很高的内存峰值。

NexFilm 在 `src/io_processor.rs` 与 `src/commands.rs` 中设计了**流式分行解码器**：

1. **条带化按需流式提取**：利用底层 TIFF 库的 Strip / Tile 读取接口，解码器每次仅从磁盘拉取数十行的连续像素条带，在内存中就地执行色彩空间转换、片基扣除与印片光运算。
2. **零拷贝导出管道**：在生成 16-bit 线性 TIFF 或 JPEG 导出文件时，CPU 导出器通过分行回调直接将处理完成的扫描线推入输出文件编码器，整图处理过程中的物理内存常驻空间始终被严格压制在数十兆字节以内。
3. **双 RAW 解码后端智能编排**：系统默认采用经过多年工业界检验的 `LibRaw 0.22+` 动态库，结合定制的 C++ 垫片（`raw_mosaic_shim.cpp`）直接截取解马赛克前的纯净拜尔矩阵数据。当运行于某些精简环境时，系统支持平滑回退至纯 Rust 编写的 `Rawler` 解码后端，保证极高的跨平台可移植性。

## SQLite 数据库模式演进与历史配方平滑迁移 {#sqlite-schema-and-recipe-migration}

工程状态持久化模块 `src/persistence.rs` 负责维系用户编辑会话的一致性。针对软件迭代过程中可能出现的底层算法重构，系统制定了完善的迁移升级契约：

```text
应用启动 -> 打开 nexfilm_user.db
              │
              ├─> 检查 user_version PRAGMA 与常量 MATH_VERSION (当前为 5)
              │
              ├─> 执行 migrate_retired_density_recipe()
              │     ├─> 检索带有 legacy_linear_srgb 标记的历史帧
              │     ├─> 若属于 LegacyV1 契约则予以安全保留
              │     └─> 其余旧帧提升为 linear_prophoto_estimate
              │           ├─> compatibility_base 重置为 unresolved
              │           ├─> 清空历史 base_color 与陈旧缩略图
              │           └─> 记录 retired_density_recipe_migrated 日志
              │
              └─> 提交事务，完成幂等初始化
```

在 v1.1.0 统一密度管线落地过程中，历史遗留的 `legacy_linear_srgb` 经验模式被正式废弃。为了防止老工程中陈旧的密度数据干扰新算法，`migrate_retired_density_recipe` 函数在数据库初始化时自动扫描所有非 LegacyV1 的存量记录，平滑清除已失效的派生状态，并促使前端在下次载入时通过统一管线自动触发一次高质量的重新反相。整个迁移过程完全幂等，重复执行零副作用。

## 前端 UI 交互模块与核心文件职责矩阵 {#frontend-rendering-and-ui-modules}

NexFilm 前端摒弃了复杂的第三方状态库，通过清晰的功能模块划分管理界面生命周期与视图响应：

- **`ui/main.js`（暗房主控与应用调度）**：管理图库、工作台、历史胶卷、硬件校准与文档五大视图的切换；负责底片条渲染、快捷键派发以及与 Tauri IPC 的异步交互。
- **`ui/tone-curve.js`（暗房印片光与曲线系统）**：负责四通道（Master/R/G/B）色调曲线交互与实时绘制，基于三次样条插值计算 256 级 LUT；处理印片光加减色数值联动。
- **`ui/geometry.js`（片门几何手柄与透视矫正）**：实现四角手柄拖拽微调，结合后端自动检测算法标注有效底片成像区域，绘制透视网格线与旋转参考标尺。
- **`ui/contact-sheet.js`（经典接触印样排版系统）**：管理多帧胶卷按行网格排版、底片边框渲染、黑白/彩色印样纸基模拟以及 EXIF 元数据排版打印。
- **`ui/density-math.js`（前端密度辅助计算）**：提供轻量级的前端密度坐标转换工具，辅助直方图波形图与高光阴影警告的计算。

下表总结了 NexFilm 核心模块的职责分布与交互契约：

| 模块文件路径 | 开发语言与运行时 | 模块核心职能 | 上游依赖与下游协作 |
| --- | --- | --- | --- |
| `src/commands.rs` | Rust (Tauri Commands) | 异步命令接入、管线编排、全分辨率导出 | 调度 pipeline, persistence, raw_backend |
| `src/pipeline.rs` | Rust (Rayon 並行) | 负片反相核心数学、片基扣除、有界通道响应 | 接收 app_state 输入，向导出器输出像素 |
| `src/color_science.rs` | Rust | 线性色彩空间变换、ProPhoto/ACES 矩阵、ICC 编码 | 被 pipeline 与 dng_writer 依赖 |
| `src/raw_backend.rs` | Rust + C++ Shim | LibRaw 绑定、CFA 拜尔读取、坏点与暗场校正 | 为 commands 提供解码缓冲数据 |
| `src/persistence.rs` | Rust (rusqlite) | SQLite 数据库读写、模式演进、配方平滑迁移 | 为全局提供 Roll 与 Frame 数据持久化 |
| `ui/main.js` | 原生 JavaScript | 视图导航、底片条虚拟列表、全局事件总线 | 调用 Tauri invoke 与前端各功能模块 |
| `ui/tone-curve.js` | 原生 JS + Canvas | 四通道三次样条曲线绘制、印片光滑块交互 | 向 WebGL 着色器与后端同步曲线 LUT |
| `ui/geometry.js` | 原生 JS + SVG | 四点透视手柄交互、自动片门选区显示 | 向 commands 同步几何坐标四元组 |
| `ui/contact-sheet.js` | 原生 JS + Canvas | 接触印样排版排布、边框镂空、高分辨率导出 | 从 SQLite 检索整卷帧，生成拼版画布 |
