# 易过曝与“高光”滑块失效根因分析与 v2 修正方案

> 目标：在**保留正常白点与自然过曝物理规律**（不强行锁死不过曝、不压扁正常高光通透感、极端降 `D-Max` 时不出现霓虹伪色）的前提下，真正修复“上游过早截断导致 `高光` 滑块对过曝区域无效”的问题。

---

## 一、v1 方案的问题复盘（实测 P1 / P2 / P3 诊断）

在 `fix/highlight-headroom-and-recovery` 上实测 `Capture One Catalog1945.tif`（对照 Capture One 正片及极低 `D-Max = -0.50` 压力测试）后，确认 v1 存在三个相互叠加的设计缺陷：

### 问题 1：无限渐近软肩部 `rolloffHighlights(..., 0.80, 1.0)` 强行锁死了白点与正常高光反差
- 在本软件默认工作流中（`u_gamma = 1.00`，见 `ui/main.js:2708-2710` 注释：`already form a display-referred sRGB signal`），`display_linear` 已经是**显示空间亮度**，`0.80` 就是屏幕的 **80% 亮度（约 204/255）**，而不是线性光。
- v1 的双曲渐近线 `knee + (span * excess) / (span + excess)` 从 `0.80` 就开始压缩，且永远无法触及 `ceiling = 1.0`：
  - 默认参数下，`norm = 1.0`（白点）被强行压低到 **`0.900`（约 230/255）**，斜率跌至 `1/4`。在 P1 直方图右上角可以明显看到整个高亮峰被挡在 ~92% 处、右侧留出空档，画面呈现出不自然的“强行 HDR 压高光发灰感”，失去 P2（Capture One）那种顺滑亮白的通透反差。
  - 更违背摄影逻辑的是：哪怕用户主动**降低 `全图 D-Max`**、或把 **`高光` / `曝光` 往右推**，渐近线 `< 1.0` 依然死死拦截，导致高光永远无法正常过曝变白。

### 问题 2：在未收敛的色彩矩阵输出上做等比缩放（`rgb * compressed / max`），造成 `D-Max = -0.50`（P3）诡异霓虹伪色
- 当 `D-Max` 拉到 `-0.50`（P3 中 `D-Max` 降至 `[0.119, 0.139, 0.155]`）时，正常场景密度归一化后达到 `norm ≈ 2.0 ~ 5.0+`：
  1. 若在未限幅时直接进入 `u_positive_to_display`（`ProPhoto -> sRGB` 矩阵，带强负系数 `-0.727`、`-0.307`），诸如 `[1.5, 2.5, 3.5]`（本应三通道全超白点、直接过曝成纯白）会被矩阵相减扭曲成 `[0.159, 2.726, 3.670]`；
  2. 随后 max-RGB 等比缩放 `rgb * (compressed / max)` 把 `[0.159, 2.726, 3.670]` 整体缩成 `[0.04, 0.73, 0.98]`、把 `[3.36, 3.09, 2.97]`（过曝肤色/水泥地）缩成 `[0.99, 0.81, 0.79]`——**严格禁止通道各自向纯白 `1.0` 靠拢**，从而在 P3 中制造出大面积高饱和霓虹绿、粉红与荧光青色块。

### 问题 3：线性高光基准点 `HIGHLIGHT_PIVOT = 0.45` 在 `gamma = 1.0` 下位置偏暗，且负向恢复用等比缩放容易卡死单通道
- 在 `gamma = 1.00` 的默认状态下，`0.45`（约 115/255）低于 50% 中灰，拉动 `高光` 会连带压暗中等亮度的肤色和中间调。将其提高到 **`0.62`**（约 158/255）才能精准作用于受光面高光而不伤及中灰肤色。
- 此外，`高光 < 0`（高光恢复）若在矩阵前做 max-RGB 等比缩放，会把全超限的 `[1.5, 2.5, 3.5]` 整体拉回 `[0.42, 0.70, 0.98]`，经 `ProPhoto -> sRGB` 后反而变暗偏青；而采用**逐通道独立非线性压低（保持灰平衡完全中立）**，则能把 `[1.5, 2.5, 3.5]` 恢复成自然明亮的淡天蓝 `[0.85, 0.98, 0.99]`，也更容易挽回单通道率先偏红的暖色高光。

---

## 二、v2 修正方案：符合摄影直觉的信号流

### 核心思路
1. **默认态不压灰、允许真过曝**：去掉非相机现代管线的 `0.80 -> ∞` 无限渐近压缩。改用**紧贴白点（`0.88 ~ 1.12 -> 0.88 ~ 1.00`）的逐通道 $C^1$ 有限软切**——`<= 0.88` 零压缩保持原图通透，**`>= 1.12` 彻底封顶 `1.000` 纯白**。只要推高曝光、推高 `高光` 或调低 `D-Max`（如 P3），过曝区域就能完全按照自然摄影规律干净退饱和并变成纯白。
2. **余量留在 `矩阵前 + 高光恢复后` 收口**：依然去掉最上游 `norm <= 1.0` 的截断，让 `exp2(u_master_exposure)` 和 `applyLinearHighlights` **在矩阵前**率先消费真实高光余量；随后经过 `finiteSoftClip(0.88)` 把仍然超限的信号规整回 `[0.0, 1.0]` 再进 `u_positive_to_display` 色彩矩阵，从数学根源彻底消灭 P3 的矩阵负值炸裂与霓虹伪色。
3. **相机原生路径回归 Phase 4 膝点 `0.85`**：`camera_native_display` 在矩阵后保留 Phase 4 验证过的 `rolloff(0.85, 0.990)`，确保 Nikon NEF 等相机矩阵天空保护与 A/B 诊断完全不退化；非相机现代扫描路径在矩阵后直接 `clamp(positive_linear, 0.0, 1.0)`。

### 信号流对比

```text
density
  └─> normalize_density_channel_unclamped_high  [保留 >= 0.0，不截断 1.0 上限]
        └─> * exp2(u_master_exposure)           [非 legacy]
        └─> applyLinearHighlights(x, h)         [工作空间高光恢复/提亮，pivot = 0.62]
        └─> finiteSoftClipPerChannel(x, 0.88)   [<= 0.88 恒等；1.00 -> 0.970；>= 1.12 -> 1.000 纯白]
        └─> u_positive_to_display * x           [仅 u_mode == 0]
        └─> display_linear:
              - 相机原生路径 (cameraNativeDisplay): rolloffHighlights(x, 0.85, 0.990)
              - 非相机扫描/黑白路径:                 clamp(x, 0.0, 1.0)
        └─> gamma / display_gamma_toe -> curves -> applyPostGammaAdjustments(..., highlights = 0.0)
```

---

## 三、精确公式与数值校验（CPU + WebGL 严格一致）

### 1. 线性/工作空间 `高光` 调整 (`apply_linear_highlight_adjustment_rgb` / `applyLinearHighlights`)
- **常量**：
  - `LINEAR_HIGHLIGHT_PIVOT = 0.62`
  - `LINEAR_HIGHLIGHT_SLIDER_MAX = 0.75`
  - `LINEAR_HIGHLIGHT_RECOVERY_GAIN = 1.45`
  - `LINEAR_HIGHLIGHT_BOOST_GAIN = 1.25`
- **公式**：
  - 若 `!isFinite`（Rust）则返回 `[0.0; 3]`；负分量钳至 `0.0`；`abs(highlights) <= 1e-6` 原样返回。
  - **当 `highlights < 0`（高光恢复 —— 逐通道独立恢复）**：
    - `amount = min(-highlights / 0.75, 1.0)`，`gain = 1.45 * amount`
    - 对每个通道 $x_i$：若 $x_i \le 0.62$ 原样保留；若 $x_i > 0.62$，令 $d = \ln(x_i / 0.62)$，$d_{\text{new}} = \frac{d}{1.0 + \text{gain} \cdot d}$，输出 $0.62 \cdot \exp(d_{\text{new}})$。
    - 性质：中灰/纯灰 `R=G=B` 严格不偏色；`<= 0.62` 像素 0 改动；避免 `P3` 超限像素在矩阵后变青或暖白单通道锁死。
  - **当 `highlights > 0`（高光提亮 —— 峰值驱动、允许冲白）**：
    - `maximum = max(r, g, b)`；若 `maximum <= 0.62` 原样返回。
    - `amount = min(highlights / 0.75, 1.0)`，`gain = 1.25 * amount`
    - $d = \ln(\text{maximum} / 0.62)$，$d_{\text{new}} = d \cdot \left(1.0 + \text{gain} \cdot \frac{d}{1.0 + d}\right)$，$\text{adjustedMax} = 0.62 \cdot \exp(d_{\text{new}})$，输出 `rgb * (adjustedMax / maximum)`。

### 2. 有限白点软切 (`finite_soft_clip_highlights_rgb` / `finiteSoftClipHighlights`)
- 参数：`knee = 0.88`，上限 `clip = 2.0 - knee = 1.12`。
- 对每个通道 $x$（已先保护非有限值为 `0.0`、`x = max(x, 0.0)`）：
  $$y(x) = \begin{cases} x, & x \le 0.88 \\ 1.0 - \dfrac{(1.12 - x)^2}{4(1.0 - 0.88)}, & 0.88 < x < 1.12 \\ 1.0, & x \ge 1.12 \end{cases}$$
- 关键输出点位：
  - `x <= 0.88`：严格 1:1 恒等（不压灰 P1）
  - `x = 0.95`：`0.940`
  - `x = 1.00`：`0.970`（sRGB 约 `247/255`）
  - `x = 1.05`：`0.990`
  - `x >= 1.12`：**严格 `1.000`（纯白 255，允许 P3 与高光+ 正常过曝）**
  - `h = -0.75` 时：`x = 1.00 -> 0.822`、`x = 1.30 -> 0.886`、`x = 2.00 -> 0.949`（真正把 `1944` 白裙等过曝纹理拽回 `[0, 1]`）。

---

## 四、测试调整与验证清单

1. **更新单测断言**：
   - 将 `CAMERA_DISPLAY_ROLLOFF_KNEE` 恢复为 `0.85`，并把 `camera_display_rolloff_is_identity_below_the_knee` 恢复测试 `[0.75, 0.85, 0.1]`。
   - 更新 `modern_highlight_headroom_survives_camera_and_scanner_routes`：在默认 `highlights = 0.0` 下验证 **`norm = 0.90 < 1.05 <= 1.30`，且扫描路径在高光大过曝 `norm = 1.80` 时确实能干净触顶 `u16::MAX`**（不再强行锁死在 `< u16::MAX`）。
   - 更新 `modern_linear_highlights_recover_only_above_the_pivot`：验证 `<= 0.62` 像素（如 `0.35, 0.45, 0.62`）在 `highlights = -0.75` 下严格不变，`norm = 1.05, 1.30, 1.80` 全部成功拉回 `< u16::MAX` 且保持严格单调与灰平衡中立。
   - 更新 `modern_linear_highlight_boost_is_safe_and_neutral`：验证 `highlights = +0.60` 时 `<= 0.62` 不变、`norm = 0.90` 明显提亮、非相机扫描路径在高值（如 `1.30`）允许达到 `u16::MAX` 正常过曝，且全程单调、灰平衡中立（`max - min <= 2`）。
   - 新增 **极端 `D-Max`（P3 等价场景 `norm = [1.5, 2.5, 3.5]` / `[3.3, 3.0, 2.9]`）回归测试**：断言在默认 `highlights = 0.0` 时经 `ProPhoto -> sRGB` 矩阵后输出为纯白 `[u16::MAX; 3]` 而非霓虹青/红伪色；在 `highlights = -0.75` 时能拉回明亮柔和且无极端偏色的淡色结果。
2. **全量验证命令**：
   - `cargo test --lib`
   - `node --test ui/*.test.js`
   - `node scripts/verify-webgl-shaders.cjs`
   - `node scripts/verify-density-contract.cjs`
   - `node scripts/verify-handbook-sync.cjs`
   - `cargo test --lib cpu_render_fixture_for_webgl_parity -- --ignored --nocapture` + `node scripts/diagnose-cpu-webgl-parity.cjs`
   - `cargo test --lib camera_rolloff_ab_diagnostic -- --ignored --nocapture`
   - `cargo fmt --all -- --check`
