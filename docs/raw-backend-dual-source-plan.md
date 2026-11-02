# RAW 解码双源架构与尼康 HE 支持 —— 工程方案

- 状态：**已实施 M1 / M2 / M-HE（2026-10-18）**；M3（LibRaw 捆绑统一升级）仍押后另立项。实施记录见 §9。
- 日期：2026-10-18
- 关联：`docs/raw-backend-camera-support.md`（X2D/ZR 偏色的根因分析，本文件是它的落地工程方案）
- 事实核实基准：LibRaw master @ 2026-10、rawler 0.8.0（crates.io，2026-08-30 发布）、本仓库捆绑的两个 LibRaw 二进制/源码

---

## 1. 结论摘要

1. **双源方向成立，且当前代码形态下是低风险增量**：LibRaw 解码路径一行不动（存量用户字节级零变化），rawler 只在两条明确触发线上介入。
2. **尼康 HE/HE\* 在开源世界无解，且被专利卡死**：LibRaw 全版本（含 2026-10 master）在相机表里明文标注 "HE/HE\* formats are not supported yet"；rawler 同样没有 TicoRAW/JPEG XS 解压器（dnglab issue #591 仍开放）。HE 压缩基于 intoPIX 的 TicoRAW，**专利持有方未开放开源实现**，darktable 等同样放弃。
3. 因此"支持尼康 HE"的现实边界是：**精准识别 + 明确报错与转换指引**（0.5–1 天），而不是解码。真解码只有商业 SDK 一条路（intoPIX 授权），成本与收益不成比例，建议不做。
4. **M1（矩阵补洞，1–1.5 天）是性价比最高的一步**：用 rawler 解析文件补出颜色矩阵，像素仍由 LibRaw 解。今天就能修 X2D（macOS）与 ZR 无损压缩（全平台）的偏色类问题（A7M5 经实测有家族回退矩阵，不在此列，见 §2.1 注 A）。
5. **M2（像素回退，2–3 天）** 让"LibRaw 完全解不了的机型/压缩"（如未来的新无损格式、0.21.3 上个别容器）也能导入。仓库里已有自研 `demosaic_bayer_fixed` 链路，rawler 的马赛克输出可以直接复用，不需要新写去马赛克。
6. **时机判断：适合现在开工双源，不适合现在动 LibRaw 捆绑升级**。理由见 §3.3——v1.1.0 之后仅 2 个提交、全部落在 UI/色调曲线层，`raw_backend.rs` 零改动；而 LibRaw 升级要同时动 macOS 的源码编译清单和 Win/Linux 的预编译库，属于"大操刀"，且对 HE 毫无收益。

---

## 2. 事实底座：谁支持什么（2026-10 核实）

### 2.1 支持矩阵

| 能力 | LibRaw 0.21.3<br>(macOS 捆绑) | LibRaw 0.22.2<br>(Win/Linux 捆绑) | LibRaw master<br>(2026-10，未捆绑) | rawler 0.8.0<br>(拟引入) |
|---|---|---|---|---|
| Hasselblad X2D 100C 像素 | 可解（用户预览正常佐证） | 可解 | 可解 | 可解 |
| Hasselblad X2D 100C 矩阵 | **缺失 → identity** | 有 | 有（另有 X2D II 100C） | 有（`x2d_100c.toml`） |
| Nikon ZR 像素（无损压缩） | 待样本验证（大概率走通用 NEF 路径可解） | 待样本验证 | **无 ZR 条目** | 可解（`zr.toml`） |
| Nikon ZR 矩阵 | 缺失 | 缺失 | **缺失（master 也没加）** | 有（A + D65 双矩阵，白电平 15892） |
| Sony A7M5 像素 | 可解 | 可解 | 可解 | 可解 |
| Sony A7M5 矩阵 | **有**（实测：非 identity，见注 A） | 未实测；与 0.21.3 同代代码路径，预计同样有 | 有（"Sony ILCE-7M5 (A7 V)"） | 有（`a7m5.toml`） |
| Nikon HE/HE\* 像素 | **不支持**（cameralist 明示） | **不支持** | **不支持**（Z6III/Z8/Z9/Zf 条目均注明） | **不支持**（无 TicoRAW/JPEG XS 解压器） |

证据落点：

- 0.21.3（rsraw-sys 0.1.1 捆绑源码）`src/tables/cameralist.cpp:713`：`"Nikon Z 9 (HE/HE* formats are not supported yet)"`；cameralist 中无 X2D 100C、无 ILCE-7M5。
- 0.22.2（rawlib 0.7.1 捆绑 `libraw.a`）字符串表：有 `"Hasselblad X2D 100C"`、`"NIKON Z 8"`、`"NIKON Z 9"`；无 Z 6_3 / ZR / X2D II / 7M5 / A1M2，无任何 "efficiency" 字样。
- master（2026-10 拉取 cameralist.cpp）：Z 6 III / Z 8 / Z 9 / Z f 四条均带 "(HE/HE\* formats are not supported yet)" 注记；无任何含 "ZR" 的条目。
- rawler 0.8.0 docs.rs `decompressors` 模块清单：`arw6, crx, deflate, jpeg, jpegxl, ljpeg, packed, radc`——无 tico/jxs/jpeg_xs。
- A7M5 实测（macOS 0.21.3，用户样本 ARW）：见 §2.1 注 A。

> **注 A（探针实测，2026-10-18）**：在 macOS / LibRaw 0.21.3 上用用户提供的 A7M5 样本 ARW 实测 `extract_camera_rgb_with_policy`，返回 `camera_to_srgb = [2.01237, -0.90486, -0.10751, -0.11960, 1.56399, -0.44439, 0.04016, -0.46075, 1.42059]`，`camera_matrix_missing = false`。cameralist 中并无 ILCE-7M5 条目，该矩阵来自 LibRaw 的 Sony 家族回退路径——**A7M5 现状不构成偏色问题，本轮无需为它做什么**。对照组：Hasselblad X2D 与 Nikon ZR 没有这类家族回退（X2D 在 0.21.3 无 cameralist 条目，代码层结论为 identity 矩阵，待样本实证；ZR 全版本无条目）——这正是症状集中在 X2D/ZR 而非 Sony 新机的原因。

### 2.2 尼康 HE/HE\*：专利阻塞，不是工程问题

- HE/HE\* 自 Z6III（2024-06）引入，ZR（2025-09 上市）的三种 NEF 压缩（无损 / HE\* / HE）之一；HE\* 基于 intoPIX **TicoRAW**（JPEG XS 系），文件体积约为无损压缩的 40–60%。
- darktable 社区结论："High Efficiency RAW is a patented format. Therefore, no FOSS raw processor will support it."；dnglab issue #591（2025-08 开，仍开放）请求 HE\* 支持未果；LibRaw 从 0.21.x 起在相机表里持续明示不支持。
- **对 NexFilm 的含义**：用户拍 HE 的文件，现在与可预见的将来都无法在开源解码器下解出像素。行业通行做法是引导用户改拍无损压缩，或用 Adobe DNG Converter / 尼康 NX Studio 转成 DNG 后再进流程——DNG 是 LibRaw/rawler 都吃得很好的格式。

### 2.3 rawler 是什么

- dnglab 项目的纯 Rust RAW 解码库：`decode_file(path) -> RawImage`，输出单通道 u16 马赛克 + 完整元数据（CFA、黑白电平、`xyz_to_cam`、按光源索引的 `color_matrix`、`wb_coeffs`、blackareas、orientation）。
- crates.io：v0.8.0（2026-08-30），仓库持续活跃（2026-10 仍有提交）；机型配置以 TOML 维护，新增相机远快于 LibRaw 的 C++ 表（ZR 已收录，LibRaw master 至今没有）。
- 许可证 **LGPL-2.1**；NexFilm 本体是 **GPL-3.0-only**（`Cargo.toml:6`），LGPL-2.1 代码可单向并入 GPL-3.0 程序，**无新增合规义务**。
- 定位：**只做辅源**。旧机型覆盖面和多年打磨不如 LibRaw，所以任何"两边都能解"的文件永远走 LibRaw。

---

## 3. 代码审查：缝在哪里，改动面有多大

### 3.1 解码入口的形状

平台分叉收敛在 `src/raw_backend.rs`，对外只有 4 个入口（两平台同名同签名，1726–1731 行统一 re-export）：

| 入口 | 位置 | 职责 | 调用点（commands.rs） |
|---|---|---|---|
| `extract_camera_rgb_with_policy` | non_macos 1180 / macos 1541 | 全幅/半幅去马赛克 RGB + 矩阵 | 4683（主解码）、4799（Smart Auto） |
| `decode_raw_mosaic` | non_macos 1012 / macos 1596 | 裸马赛克（扫描仪标定/暗场） | 2350–2352、12361–12363 |
| `read_white_balance` | non_macos 1122 / macos 1461 | as-shot 白平衡 | — |
| `read_raw_container_source` | non_macos 1082 / macos 1492 | RAW DNG 导出的再封装源 | 6743 |

另有缩略图走 `RawProcessor::unpack_thumb`（macos 1360）。

### 3.2 已经就绪、可直接复用的基础设施

这是"为什么改动面小"的实质：

1. **自研去马赛克与暗场链**：共享层已有 `demosaic_bayer_fixed`（raw_backend.rs:361）、`correct_cfa_capture`（:439）、`decode_capture_corrected_input`（:602）。rawler 只出马赛克也没关系——回退路径直接复用这条链，不用写一行去马赛克。
2. **identity 矩阵探针已落地**：`camera_matrix_missing`（:712）+ `decode_image_buffer` 里的诊断日志（commands.rs:4689）。M1 的触发信号是现成的。
3. **矩阵换算工具已有**：`color_matrix_from_cam_xyz`（:800）做的正是 rawler `xyz_to_cam` → 3×3 的同族换算。
4. **解码行为版本化**：`persistence::RAW_DECODE_VERSION` 已贯穿锚定与标定校验（commands.rs:2321、2898、5475 等）。换源/补矩阵后 bump 一次，既有"重新解析/重新锚定"流程自动接管，不需要新的迁移机制。
5. **回归护栏现成**：270 个 lib 测试、19 个 UI 测试、13 个 verify 脚本（本仓库 scripts/），外加上一轮建立的真实样本渲染对比（A7M5 ARW）。

### 3.3 v1.1.0 以来的变动与开工时机

- v1.1.0 → HEAD（fbd7113）只有 **2 个提交**：`700205b`（点曲线 + Detail 模块，+4472 行，几乎全在 ui/main.js、tone-curve.js、core_math.rs 曲线部分、commands.rs 渲染参数）和 `fbd7113`（实验脚本）。**`raw_backend.rs` 零改动**。
- 结论：解码层与最近的特性（点曲线/Detail）完全正交，互不踩脚。**双源是纯增量（新增模块 + 4 个入口外面套一层分派），现在开工不会与任何在途工作冲突。**
- 反面是 LibRaw 捆绑升级（"大操刀"）：macOS 侧要换 rsraw-sys 的 LibRaw 源码树和 cc 编译清单，Win/Linux 侧要替换 rawlib 的预编译库并处理其静态依赖（jpeg/lcms2/z/gomp），两套构建系统、三平台验证，且**对 HE 零收益、对 ZR 零收益**（master 也没支持）。建议作为独立工程（M3）押后，与双源解耦。
- **前置条件 M0**：当前工作树里有上一轮已验证未提交的修复（film_border.rs、commands.rs、raw_backend.rs、ui/main.js、两个 verify 脚本、docs/raw-backend-camera-support.md）。开工前先落盘这批修复，保持双源分支干净。（注意 `docs/website-plan.md` 与 `.gitignore` 的 `website` 条目属于另一条官网工作线，勿混入。）

---

## 4. 方案设计

### 4.0 总原则

- **LibRaw 永远优先**：能解且矩阵非 identity → 一律 LibRaw，存量路径字节级不变。
- **rawler 只在两条触发线上介入**：① LibRaw 解出像素但矩阵是 identity（M1，只借矩阵）；② LibRaw 连像素都解不出（M2，全量回退）。
- 所有回退都**可观测**：实际使用的源与矩阵写入 `RawMetadata`（泛化现有 `libraw_version` 字段为 `decoder_source + version`），诊断日志与关于页可见。

### 4.1 M1：颜色矩阵补洞（rawler 作元数据源）

**修什么**：X2D（macOS 0.21.3）与 ZR 无损压缩（全平台）——即"预览能出、颜色不对/导出偏黄"这一整类。（A7M5 实测有 Sony 家族回退矩阵，不需要 M1，见 §2.1 注 A。）

**怎么做**（改动集中在一个共享包装层，两平台实现不动）：

```
extract_camera_rgb_with_policy(path, ...)   // 现有平台入口，不动
        │
        ▼ LibRaw 解码（像素 + 矩阵）
camera_matrix_missing(camera_to_srgb)? ── 否 ──► 直接返回（99% 存量文件走这里，零开销）
        │ 是
        ▼ rawler::decode_file(path)（只取元数据，像素丢弃）
xyz_to_cam / color_matrix[D65] ──► color_matrix_from_cam_xyz ──► XYZ→sRGB（D65）
        │
        ▼ 替换 LibRaw 结果里的 camera_to_srgb，像素原封不动
返回（元数据里记录 matrix_source = "rawler 0.8.0"）
```

- 触发即冷路径：只有 identity 矩阵的文件多付一次 rawler 解析（且可以只解析元数据头，实现时验证 rawler 能否跳过解压）。
- WB 不动：M1 只换矩阵，`NormalizedAsShot` 增益仍来自 LibRaw 的 as-shot。
- **矩阵语义必须核对**（见 §5 风险表第 1 条）：rawler 的 `xyz_to_cam` 是 XYZ(D65/按光源)→camera 还是反方向、白点 chromatic adaptation 归属，实现时用一台两源都认识的相机（如 A7M4）交叉验证，矩阵差超过阈值即拒绝替换并保持诊断。
- LibRaw 原始（identity）矩阵与新矩阵都记录，便于回滚与工单排查。

**工作量：1–1.5 天**（含交叉验证与单测：合成 identity 场景 + 真实样本）。

### 4.2 M2：像素级回退（rawler 作像素源）

**修什么**：LibRaw `unpack` 直接失败的文件——典型是"太新的机型走了新容器/新无损变体"，以及任何 LibRaw 崩掉的场景。这是未来新相机的保险丝，也是把"导入失败"变成"能用"的那一步。

**怎么做**：

1. `decode_raw_mosaic` 外套分派：LibRaw 失败 → `rawler::decode_file` → 适配成 `RawMosaic`：
   - `samples` ← `pixels_u16()`（单通道马赛克，语义一致）
   - `cfa` ← `cropped_cfa()` → `CfaPattern::Bayer{filters}`（**边界声明：Bayer-only**；XTrans 继续只走 LibRaw，rawler 的 XTrans 支持留待样本再评估）
   - `black_level/white_level` ← `blacklevel/whitelevel`，`masked_areas` ← `blackareas`，orientation ← `Orientation`
2. 之后的链路完全复用：`correct_cfa_capture`（暗场/漏光校正、质量旗标）→ `demosaic_bayer_fixed` → 既有 WB/矩阵应用。
3. `extract_camera_rgb_with_policy` 同理分派（回退路径的 WB 用 rawler `wb_coeffs` 喂 `normalized_as_shot_gains`，语义与 LibRaw `cam_mul` 对齐）。
4. RAW DNG 导出（`read_raw_container_source` + `dng_writer.rs`）在 M2 顺带覆盖：rawler 的马赛克 + `color_matrix` 正好是 DNG 再封装需要的全套。
5. 缩略图不回退：容器内嵌 JPEG 缩略图由 LibRaw 的 identify 层即可取得（identify 成功、unpack 失败是常见组合），浏览体验不受影响。
6. 触发 `RAW_DECODE_VERSION` bump，走既有重解析/重锚定流程。

**工作量：2–3 天**（含 RawMosaic 适配层、分派、样本验收、回归）。

### 4.3 M-HE：HE 文件的识别、报错与指引

不解码，但把体验做对：

1. **识别**：LibRaw unpack 失败 + Nikon 文件 → 读取 NEF 的压缩标记判定是否 HE/HE\*（判定字段需逆向确认：TIFF Compression tag 与 makernote 的 NEFCompression 值域，参照 lclevy/nikon_nef 项目与真实样本；**判定只在失败路径上做，不影响任何成功解码**）。
2. **报错文案**（替换现在的通用解码错误）：明确告知"此文件为尼康 HE/HE\* 压缩，暂无法解码"，并给出两条出路：相机端改用"无损压缩"；或用 Adobe DNG Converter / 尼康 NX Studio 转 DNG 后导入（DNG 路径本App已完全支持）。
3. 文档与 FAQ 同步一句：HE/HE\* 是专利格式（intoPIX TicoRAW），开源解码器（LibRaw/dnglab/darktable）均不支持，非本应用独有限制。

**工作量：0.5–1 天**（主要是判定字段的逆向确认与样本验证）。

### 4.4 备选架构（记录，不作首选）

**dnglab 侧车进程**：不链接 rawler，捆绑 dnglab CLI，把 LibRaw 解不掉的文件转成 DNG 再喂给 LibRaw。

- 优点：rawler 的 0.x API 不稳定问题被进程边界隔离；升级 = 换二进制。
- 缺点：包体积（Rust 全功能 CLI）、进程管理与临时文件、错误信息跨进程丢失、每文件一次全量转写延迟。
- **保留为依赖足迹超预算时的退路**：若 `cargo tree` 评估 in-process rawler 的编译/体积代价不可接受（Tauri 包体敏感），切换到此方案，M1/M2 的分派逻辑不变。

### 4.5 明确不做的事

- **不引入商业 TicoRAW SDK**（intoPIX 授权 + C SDK 集成 + 专利合规，成本与用户量不成比例）。
- **不自己逆向实现 HE 解压**（专利风险直接落在项目上）。
- **不把 rawler 提为主源**（旧机型覆盖与成熟度不及 LibRaw）。
- **不在本轮做 LibRaw 捆绑统一升级**（M3 另立项：对 HE/ZR 零收益，纯粹为旧机型生态，收益不足以对冲三平台构建系统改造的风险）。

---

## 5. 风险与对策

| # | 风险 | 对策 |
|---|---|---|
| 1 | **矩阵语义弄反/白点不一致**（rawler `xyz_to_cam` 方向、D50/D65、chromatic adaptation） | 用两源都支持的机型（A7M4 等）交叉验证，换算结果与 LibRaw 矩阵差超阈值即拒绝替换、维持诊断日志；单测固化阈值 |
| 2 | rawler 自身 bug / 解码错位 | 永远只做回退源；回退输出过 `correct_cfa_capture` 的质量旗标（`QualitySummary`），异常即显式标记 |
| 3 | rawler 0.x API 不稳定 | Cargo 锁定精确版本；适配层收窄在一个模块（`mod rawler_backend`），升级只改一个文件 |
| 4 | 黑电平/掩蔽区语义差异导致暗场校正偏差 | M2 验收用含暗场标定的样本走全链路，观察 `QualityFlag::OutOfRange` 等旗标 |
| 5 | HE 误判（把无损压缩判成 HE 打击正常用户） | 判定只发生在 LibRaw unpack **失败之后**；文案同时提示"若确认是无损压缩请反馈样本" |
| 6 | 换源后显色变化引发"颜色变了"工单 | `RAW_DECODE_VERSION` bump 触发既有重解析/重锚定；元数据记录实际解码源，工单可诊断 |
| 7 | 依赖足迹/编译时间上涨（rawler 拉入的 crate 树） | M0 阶段先 `cargo tree` 评估；超预算转 §4.4 侧车方案 |
| 8 | rawler LGPL-2.1 合规 | 无风险：NexFilm 本体 GPL-3.0-only，单向兼容，无新增义务（§2.3） |

---

## 6. 里程碑、工作量与验收

| 阶段 | 内容 | 工作量 | 验收标准 |
|---|---|---|---|
| **M0 前置** | 提交当前工作树修复；`cargo tree` 评估 rawler 足迹；建样本基线 | 0.5 天 | 基线样本集渲染哈希入库；现有 270+19 测试与 13 脚本全绿 |
| **M1 矩阵补洞** | §4.1 | 1–1.5 天 | X2D 样本 develop/export 显色与 0.22.2 平台渲染一致；ZR 样本矩阵非 identity；A7M5 现状样本渲染哈希不变（回归护栏）；交叉验证阈值单测通过 |
| **M2 像素回退** | §4.2 | 2–3 天 | ZR 无损样本在 macOS 全链路可用（含 RAW DNG 导出）；人为构造 LibRaw 失败场景能回落 rawler；RAW_DECODE_VERSION bump 后既有锚定流程正确触发 |
| **M-HE 识别指引** | §4.3 | 0.5–1 天 | HE/HE\* 样本报专用文案；无损压缩样本零误判 |
| M3（押后，另立项） | LibRaw 捆绑统一升级 | 3–4 天 | 独立评审 |

合计（M0–M-HE）：**约 4–6 个纯开发日** + 样本收集等待。建议 M1 单独先发版（小步快跑，直接修在报的 X2D/ZR 工单）。

---

## 7. 开放问题与所需样本

**需要收集的样本**（决定验收进度，不阻塞开发）：

1. Hasselblad X2D 100C 的 3FR（报修用户处可取）
2. Nikon ZR 的 NEF 三份：无损压缩 / HE / HE\* 各一
3. 一台两源都认识的老机型样本（如 A7M4 / Z7II）做矩阵交叉验证
4. （可选）Z6III 样本，覆盖 HE 家族第二成员

**待实现期确认的技术点**：

- rawler 能否只解析元数据不全量解压（M1 冷路径开销）
- NEF 压缩标记的 HE 判定字段（§4.3）
- rawler 对 X2D 的 3FR 与 X2D II 支持深度（`x2d_100c.toml` 已确认，II 代待样本）
- `decode_raw_mosaic` 回退路径与扫描仪标定（暗场/漏光帧）交互的质量旗标行为

---

## 8. 参考资料

- LibRaw master cameralist（HE 注记原文）：https://github.com/LibRaw/LibRaw/blob/master/src/tables/cameralist.cpp
- rawler（dnglab 解码库）：https://github.com/dnglab/rawler ／ docs.rs：https://docs.rs/rawler/0.8.0/
- dnglab HE\* 支持请求（开放中）：https://github.com/dnglab/dnglab/issues/591
- dnglab Releases（Z6III/Z50II 等机型收录节奏）：https://github.com/dnglab/dnglab/releases
- TicoRAW 专利背景与社区结论：https://discuss.pixls.us/t/nikon-z-series-high-efficiency-raw-support/50428
- 尼康 NEF 格式笔记（HE/TinyRAW）：https://github.com/lclevy/nikon_nef
- Nikon ZR 规格页（NEF 压缩选项）：https://onlinemanual.nikonimglib.com/zr/en/15-08.html
- 根因分析：`docs/raw-backend-camera-support.md`

---

## 9. 实施记录（2026-10-18）

### 9.1 落地结构

- `src/rawler_backend.rs`（新增，唯一触碰 rawler 的模块）：M1 元数据探针（`decode_dummy` 跳过解压）、M2 马赛克/相机RGB/容器源回退、M-HE 判定（自带最小 TIFF/makernote 读取器）。
- `src/raw_backend.rs`：三个平台入口改名 `libraw_*` 后，由共享分派层（同名包装函数）接管；`RawMetadata.libraw_version` 泛化为 `decoder_source + decoder_version`（`decoder_id()` 参与标定一致性校验）；`CameraRgbData` 上提共享层并携带 `camera_to_srgb_source`（`LibRaw` / `RawlerFallback(光源)` / `Unresolved`）。
- `RAW_DECODE_VERSION` 11 → 12：旧载荷经既有重解析/重锚定流程自动接管；带 `#[serde(default)]`，旧 JSON 反序列化不炸。
- 依赖：`rawler = "=0.8.0"`，`default-features = false`（关闭 clap/rawdb/inspector CLI 依赖）；`cargo tree` 增量约 57 个纯 Rust crate（最大为 jxl-oxide 解压栈），无需转 §4.4 侧车方案。

### 9.2 关键实现事实（与 §4 的差异与确认）

1. **矩阵换算（§4.1 的"语义核对"已完成并有实证）**：LibRaw 的 `rgb_cam` = 行归一化 `cam_rgb = cam_xyz · xyz_rgb` 的**普通逆**（dcraw 的 `pseudoinverse` 末尾返回转置逆、`cam_xyz_coeff` 再转置一次，两者相消）。Rust 实现 `camera_to_srgb_from_xyz_to_cam` 在 D810 真机样本上与 LibRaw 实测矩阵**逐元素一致（最大偏差 3.6e-7）**。交叉验证假警报注：A7M5 上 LibRaw 用的是 Sony 家族回退表（非本相机的 Adobe 表），与 rawler 的逐机型 D65 表本来就不该相同——这类文件正是 M1 要修的对象。
2. **CFA 相位**：LibRaw 对部分机型（如 D810）用四色编码 `0xB4B4B4B4`（第二绿 = 码 3），rawler 用普通 `0x94949494`；两者的逐位 RGB 相位相同。M2 一致性断言比较相位映射而非字面 filters 值。
3. **像素一致性**：D810 无损 NEF 上 rawler 与 LibRaw 全幅逐像素差 ≤1 LSB（14 位量化级）。
4. **白电平语义**：rawler 给机型标称白点（D810 = 15520），LibRaw 给逐文件实测 `linear_max`（15311）；M2 元数据记录 rawler 值，暗场校正走既有 `QualityFlag::SaturatedSample` 纪律。
5. **朝向**：rawler 0.8 的 `RawImage.orientation` 硬编码 Normal（其源码标注 TODO fixme），不可用；M2 直接读 TIFF IFD0 0x0112，经 LibRaw 同款映射表 `"50132467"[o & 7]` 转 flip，保证与主源逐位一致。
6. **HE 判定字段（§4.3 的逆向确认）**：`NEFCompression` = makernote **0x0093**（老机型，SHORT 内联）或 **0x0051**（新 Z 系，数据偏移 10 处 u16）；**13 = HE，14 = HE\***（ExifTool Nikon.pm 与 rawler `NefCompression` 双源一致）。判定只发生在 LibRaw 解码失败之后，非尼康文件一律返回 None。
7. **rawler `color_matrix` 布局**：行 = 相机通道、列 = XYZ，与 LibRaw `cam_xyz` 同约定；优先取 D65，其次 D50/Daylight/A，最终回退任意表并记录光源名。
8. **iso/exposure**：rawler 的 `RawImage` 不暴露 EXIF 曝光元数据，M2 马赛克记录 `None`（由既有 `MissingMetadata` 旗标纪律兜底）。

### 9.3 验证

- `cargo test --lib`：285 通过 / 0 失败（含 13 个新增单测：矩阵契约、flip 映射、合成 TIFF 的 HE 判定双端序×双标签、CFA→filters 往返、归一化旗标纪律、旧版元数据反序列化）。
- 真实样本交叉验证（环境变量门控，`NEXFILM_RAW_SAMPLE_ARW` / `NEXFILM_RAW_SAMPLE_NEF`）：A7M5 ARW（矩阵对照 + 结构不变式）、D810 无损 NEF（马赛克元数据/像素/朝向/矩阵全对照 + HE 阴性）。默认测试不依赖样本，保持封闭。
- `node --test ui/*.test.js` 19 通过；13 个 `scripts/verify-*.cjs` 全部通过。

### 9.4 报修样本（0218-.tif）的验证记录（2026-10-18 补）

> **2026-10-19 修订**：本节早期把 0218-.tif 当作"导出成片"的推断有误（该文件实为外部工具
> 写入的 AdobeRGB 负片捕获，颜色正确），其中"成片颜色已烙进像素、无法事后修正"的结论
> **作废**。报修复现的偏色（导出偏黄 + P1 惨白中间态）已查明为应用管线内部的两个独立
> bug，与矩阵缺失无关。根因、证据与修复建议见
> `docs/loose-tiff-color-divergence-investigation.md`。以下矩阵层面的分析仍然成立。

报修用户只提供了导出成片（`0218-.tif`，4300×2831 16-bit RGB），3FR 原片尚未拿到。
用该文件完成了症状定量与根因源码级闭环：

- **症状定量**：全图 R/G = 1.42、B/G = 0.44，四个象限比值一致（±0.02）——系统性
  解码级色偏，非场景内容；画面中应为中性色White的船身呈黄褐色。与 identity 矩阵
  诊断一致。
- **根因源码级确认**：X2D 的 3FR 以传感器型号 "100-20-Coated6" 进 LibRaw 的
  colordata 表。0.21.3（macOS 捆绑）的 Hasselblad 表止步于 "100-17-Coated5"，无此
  条目 → `adobe_coeff` 落空 → `rgb_cam` = identity。0.22.2（Win/Linux 捆绑）新增
  "100-20-Coated6"：`{6468,-1899,-545,-4526,12267,2542,-388,1276,6096}`（×1e-4）。
  **该矩阵与 rawler `x2d_100c.toml` 的 D65 矩阵逐位相同**（dnglab 与 LibRaw 同出
  Adobe 数据）。因此 M1 在 macOS 上回填的矩阵与 0.22.2 平台自带的完全一致——
  "X2D 显色与 0.22.2 平台渲染一致" 这一验收标准在矩阵层面成立（两端 LibRaw 解出的
  像素相同 + 相同矩阵 = 相同显色）。
- **成片 TIF 无法事后修正**：缺失的矩阵作用在胶片反相之前的线性相机域；导出成片
  已经过了反相与色调链，事后线性修正没有正确数学。真正的修复 = 升级后的版本重新
  导入 3FR 重新导出。验收测试已就绪：`NEXFILM_RAW_SAMPLE_3FR=<3FR路径> cargo test
  --lib x2d_capture`（触发条件、回填矩阵、来源记录、非恒等断言）。

### 9.5 遗留与等待样本

- X2D（3FR）与 ZR（无损/HE/HE*）真实样本仍按 §7 收集；到位后跑同一组门控测试即可出具验收证据（§6 M1/M2 验收标准的现场部分）。
- rawler 元数据探针对 3FR 是否真的跳过解压未实测（NEF 已实测走 dummy 路径）；如未跳过，M1 冷路径退化为一次全量 rawler 解码，功能不受影响。
