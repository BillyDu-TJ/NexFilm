# RAW 后端相机支持：X2D / ZR 偏色的根因与升级方案

> 状态：2026-10 审查结论。涉及用户报告："哈苏 X2D 和尼康 ZR 预览正常，导出偏黄"。

## 现象

使用 Hasselblad X2D（3FR）与 Nikon ZR（NEF）的用户报告：库缩略图/预览颜色正常，
Develop/导出的正片整体偏黄。

## 根因

**两个平台捆绑的 LibRaw 版本不同。**

| 平台 | 依赖 | 捆绑 LibRaw | 相机表覆盖（2026-10 实测） |
| --- | --- | --- | --- |
| macOS | `rsraw-sys 0.1.1`（crates.io，源码编译） | 0.21.3（2024-01） | 最新哈苏 X1D II；最新尼康 Z 9/Z fc；**无 X2D**，无 A7M5 |
| 其他 | `rawlib 0.7.1` | 0.22.2（2025-12-25 发布线的预编译库） | **含 X2D / X2D II**；含 Z 8/Z 6III（HE 除外）；**无 Nikon ZR**，无 A7M5 |
| LibRaw master（2026-10） | — | — | 含 X2D/X2D II、Sony ILCE-7M5 (A7 V)、A1 II；**仍无 Nikon ZR**（cameralist 与 normalize_model 均无条目） |

LibRaw 打开相机表里没有的机型时，`rgb_cam` 保持初始值（单位阵），`cam_mul`
不可信。NexFilm 在 `decode_image_buffer` 的 RAW 分支里把 `libraw_get_rgb_cam`
的结果作为 camera→linear-sRGB 矩阵在 f32 中应用，因此这类文件会：

1. 以**传感器原生原色**被当作 sRGB 处理（单位阵 = 无色彩表征）；
2. 经密度域反相后表现为整体色偏（报告中的"偏黄"）；
3. **Develop 预览与导出走同一解码**（仅 half_size 差异，实测同一文件两路均值
   差 < 0.25%，即解马赛克噪声级），因此色偏在预览与导出中同时存在——用户是在
   导出后与相机 JPEG（内嵌预览，由相机渲染）对比时才注意到。

对不受支持机型，LibRaw 的像索解码本身通常仍可完成（NEF/3FR 容器是自描述
TIFF），但颜色元数据缺失不可在应用层弥补：项目内没有该传感器的实测矩阵，任何
"借用同厂旧机型矩阵"的做法都必须显式标注为估计值。

**分机型结论（2026-10）：**

- **X2D**：升级到 ≥ 0.22（rawlib 现有 0.22.2 或 vendor 0.22.x）即解决。
- **Nikon ZR**：**当前任何 LibRaw 版本（含 master）都没有 ZR 条目**，也没有把
  ZR 归一化到 Z 6III 家族的逻辑。升级 LibRaw 不能直接解决 ZR；需要拿一个 ZR
  NEF 样片实测其回退解码质量，并向 LibRaw 上游报障/等收录。
- **Sony A7M5**（本次复现用机）：master 已支持（2026-05-21 提交），0.22.2
  稳定版尚未包含。

## 已落地的缓解

- `raw_backend::camera_matrix_missing()`：检测解码返回单位阵（= LibRaw 未能
  表征该相机）的情况，`decode_image_buffer` 在 RAW 分支输出明确诊断日志
  （含 LibRaw 版本与处置建议）。
- `scripts/verify-webgl-shaders.cjs` 增加 macOS 浏览器路径（此前只查 Windows）。

## 根治方案（待排期）

目标：全平台统一为**仓库自带（vendored）的 LibRaw 源码 + 自有构建**，摆脱对
`rsraw-sys` / `rawlib` 两个第三方打包的版本依赖——它们正是版本滞后的根源。

1. **首选**：vendor LibRaw 0.22.x 源码（上游 tag 或 rawlib 打包对应的源码版本）
   到 `vendor/`，仿照 `src/raw_mosaic_shim.cpp` 的既有模式：build.rs 用 cc 统一
   编译 LibRaw + shim，Rust 侧只经过稳定的 C API / shim 访问器（不直接引用
   C++ 结构体布局）。`rsraw-sys` 的 build.rs（79 个源文件清单 + bindgen）是
   现成模板，0.22 的文件清单需按上游调整。
   - 收益：全平台同一解码版本；新机型支持只需换 vendor 目录。
   - 风险：解码版本变化会轻微改变所有 RAW 的输出（新的黑电平/颜色表），需要
     用 `raw_decode_version` 触发已有项目的重解码提示，并跑校准回归
     （`data_process_pipeline_doc.md` §10.2）。
2. **备选**：等 `rsraw-sys` 发布捆绑 0.22.x 的新版本后仅升级 macOS 依赖。
   截至本审查（2026-10）crate 上只有 0.1.1（0.21.3）。
3. **不采用**：为 X2D/ZR 硬编码"借来的"相机矩阵——违反项目科学边界
   （无参考测量、无误差报告）；Capture Corrected 路径不受此问题影响
   （不走 LibRaw 颜色矩阵）。

### 升级机制（"一次性改造 + 低成本跟进"）

LibRaw 每 3-6 个月才收录新相机，"永久不用管"不存在；能把每次跟进压到
约半个工作日即可：

- 每次升级 = 替换 `vendor/` 里的 LibRaw 源码 → bump `RAW_DECODE_VERSION`
  → 跑解码 A/B 对比脚本（同一批样片、新旧 LibRaw 各渲染一遍）→ 跑校准回归
  → 提交。
- 建议把 A/B 对比脚本固化（输入一组样片路径，输出每文件两版解码的均值/色差
  报告），本次 A7M5 ARW 已可作第一批样片。

## 验证清单（升级后）

- [ ] X2D 3FR：Develop 预览/导出无色偏，`camera_matrix_missing` 不再触发。
- [ ] A7M5 ARW：0.21.3 vs 0.22.x 解码 A/B 通过。
- [ ] ZR NEF（需先向用户收集样片）：确认上游支持后的解码质量；
      若仍无官方支持，确认 `camera_matrix_missing` 诊断正确触发并给出提示。
- [ ] 校准回归：对既有校准 Profile 重跑验证误差（§10.2），记录
      `raw_decode_version` 变更。
- [ ] 导出与 Develop 预览逐像素一致性抽检。

---

后续落地路径见 `docs/raw-backend-dual-source-plan.md`（双源架构工程方案，
含尼康 HE/HE\* 支持边界：受 TicoRAW 专利限制，LibRaw 与 rawler 均不支持，
方案为识别 + 转换指引）。另：2026-10-18 用用户 A7M5 样本实测，
0.21.3 对 ILCE-7M5 会给出 Sony 家族回退矩阵（非 identity），
A7M5 不属于偏色问题机型；偏色面收敛为 X2D（macOS）与 ZR（全平台）。

## 双源落地补充（2026-10-18）

`docs/raw-backend-dual-source-plan.md` 的 M1/M2/M-HE 已实施（实施记录见该文档 §9），
本文件的"根治方案"按如下方式部分兑现：

- **X2D（macOS 0.21.3）/ ZR 无损（全平台）的矩阵缺失**：已修。LibRaw 解出像素但
  矩阵为 identity 时，自动从 rawler 0.8.0 的机型库借取 D65 矩阵（仅元数据，不解像素），
  换算与 LibRaw `cam_xyz_coeff` 逐元素一致。诊断日志与 `CameraRgbData.camera_to_srgb_source`
  可观测回退。
- **LibRaw 完全解不开的机型/压缩**：已修。`decode_raw_mosaic`、`extract_camera_rgb_with_policy`、
  `read_raw_container_source` 三个入口在 LibRaw 失败后自动回落 rawler 像素（仅 Bayer，
  X-Trans 仍只走 LibRaw），复用自研 `demosaic_bayer_fixed` 链。
- **尼康 HE/HE\*：仍然无解，这是格式限制不是本应用缺陷**。HE/HE\* 基于 intoPIX TicoRAW
  （JPEG XS 系），专利持有方未开放实现，开源解码器（LibRaw 全版本、rawler/dnglab、darktable）
  均不支持。现在导入 HE/HE\* 文件会得到明确报错与两条出路：相机端改用"无损压缩"重拍；
  或用 Adobe DNG Converter / 尼康 NX Studio 转成 DNG 后导入（DNG 路径完全支持）。
  判定字段：NEF makernote 0x0093（新 Z 系为 0x0051）值 13/14，仅在 LibRaw 解码失败后判定，
  无损压缩文件零误判。
- **验证清单更新**：原清单中"升级 LibRaw 后 camera_matrix_missing 不再触发"一项，在
  X2D/ZR 上由 M1 提前达成（0.21.3 不升级也消除色偏）；LibRaw 捆绑统一升级（M3）仍押后。
