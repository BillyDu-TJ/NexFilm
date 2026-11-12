# 极端偏色与高光褪色发白修复方案（基于 Phase 4 基线）

- 日期：2026-11-12
- 目标基线：Phase 4 完成态（`3f59dbc`，即 `integrate/camera-native-rgb` 合入 `main` 后的状态）
- 计划分支：`fix/color-cast-and-highlight-rolloff`
- 核心原则：**绝不牺牲正常照片的色彩表现（好图零退化）；用最小、最精准的外科手术式修改，彻底解决“欠曝极端偏色”与“高光褪色发白/触顶”两大死穴。**

---

## 一、背景与前车之鉴

### 1.1 为什么当前未提交的尝试（线性共享黑白点）必须回滚？
当前工作区未提交的改动试图把相机 RAW 的三通道逐通道归一化取消，改为用共享亮度端点 `black = luma(d_min)`、`white = luma(d_max)` 在指数域转换后再乘相机矩阵。
- **致命错误**：彩色负片 R/G/B 三层感光乳剂的物理对比度（密度跨度）天生不同（例如 `_DSC7741.NEF` 的 `d_max` 为 `0.665 / 0.853 / 1.020`）。用同一个平均标量去除三个通道，导致在画面高光处正片值变成 `0.638 / 1.078 / 1.665`（蓝通道被人为放大到红通道的 2.6 倍），进入相机矩阵后整张好图严重偏青绿。
- **第二个隐藏错误**：当前尝试编写的 `fit_linear_display_rgb` 在 `maximum > 1.0` 时执行 `luma + (value - luma) * (1.0 - luma) / (maximum - luma)`。代数上这不仅把最亮通道永远钉死在 `1.0`（依然触顶），而且是**向中性灰（`luma`）做去饱和收缩**——越亮越把彩色高光洗成灰白色，根本不是保持色彩的高光软肩部。

### 1.2 Phase 4 基线（`3f59dbc`）做对了什么、还差哪两步？
Phase 4 把相机 RAW 的密度计算从“先乘矩阵+色域压缩再取对数”改成了“在相机原生 RGB 下取对数并做逐通道同像素端点归一化，最后在正片显示端乘 `camera_to_display` 矩阵”。
- **做对的部分**：保留了每个通道各自的胶片响应跨度（`d_min[c] .. d_max[c]`），因此正常曝光的负片（如 `_DSC7732`、`_DSC7735`、`_DSC7741`）白平衡准确、肤色与草地自然。
- **遗留的两个病灶**：
  1. **病灶 A（欠曝极端偏蓝，代表样张 `Capture One Catalog1949.raf`）**：
     `validate_film_base_candidate` 硬编码了 `FILM_BASE_MAX_DENSITY = 1.60` 和 `FILM_BASE_MAX_CHANNEL_SPREAD = 0.75`。翻拍欠曝 3～4 档时，整张底片透射率降低，算出的片基密度整体上移到 `1.68 ~ 2.45 D`，跨度达 `0.77 D`。质量门直接把真实片基拒之门外（置零），迫使管线退回 `share_smart_auto_density_scale`（三通道共享跨度 + `±0.20 D` 偏移上限），无法抵消 `0.50 ~ 0.78 D` 的橙色色罩差，整张图变成深蓝色。
  2. **病灶 B（高光褪色发白与大面积触顶，代表样张 `4000363.ARW`）**：
     逐通道归一化把高光送到 `[0, 1]` 顶端后，直接乘以对角线高达 `1.38 ~ 1.74` 的相机矩阵 `u_positive_to_display`。在蓝天或亮部区域，主导通道被矩阵放大到 `1.1 ~ 1.4`，随后代码直接按通道独立执行 `.clamp(0.0, 1.0)`。主导通道被硬生生削平在 `1.0`，次要通道继续上升，导致高光色相向纯白漂移（褪色发白），天空 ROI 触顶率高达 `28.46%`。

---

## 二、修复方案设计（精准外科手术）

### 2.1 分支与基线清理
1. 保留 `AGENTS.md`（`test_picture/` 绝对只读约束）以及 `docs/color-cast-failure-analysis.md`、`docs/color-cast-root-cause-and-fix.md` 复盘记录。
2. 彻底丢弃当前工作区在 `src/`、`ui/`、`scripts/`、`docs/` 上的未提交“共享亮度黑白点”试验代码，回到干净的 Phase 4 基线 `3f59dbc`。
3. 将 `integrate/camera-native-rgb`（`3f59dbc`）快进合并到 `main`，并从 `main` 切出新分支 `fix/color-cast-and-highlight-rolloff`。

### 2.2 修复一：解除片基质量门与曝光绑定的绝对上限（解决欠曝极端偏色）
- **位置**：`src/commands.rs` 中的 `validate_film_base_candidate`（约第 2035–2075 行）。
- **改动**：
  - 删除 `FILM_BASE_MAX_DENSITY = 1.60` 和 `FILM_BASE_MAX_CHANNEL_SPREAD = 0.75` 检查。
  - 保留物理上真正恒成立的校验：
    1. 样本数量充足（`samples >= minimum_samples`）；
    2. 密度值有限（`value.is_finite()`）；
    3. 最小可见透射密度下限（`value >= FILM_BASE_MIN_DENSITY = 0.02`，排除纯白截断/灯板溢出像素）；
    4. 片边窄带与画面内低密度尾部的一致性校验 `film_base_band_agrees` 保持不变。
- **效果与安全性**：
  - 对所有正常曝光的底片（原本片基密度 `< 1.60`、跨度 `< 0.75`），判定结果 **100% 不变（逐字节零影响）**。
  - 对欠曝底片（如 `1949.raf` 及人工降曝 10 倍的测试样本），有效片基不再被误杀，能够正常执行逐通道减片基与逐通道同像素端点窗口，彻底消除因回退到共享窗口导致的满屏深蓝。

### 2.3 修复二：保持逐通道归一化不变，在相机原生显示矩阵后引入保色相高光软肩部（解决褪色发白与天空触顶）
- **位置**：
  - CPU 渲染核心：`src/commands.rs` 的 `render_shader_equivalent_core`
  - WebGL 渲染着色器：`ui/main.js` 的主渲染 Fragment Shader 与 Detail Pass（通过 `u_camera_native_display` 标识启用）
  - 奇偶校验脚本：`scripts/diagnose-cpu-webgl-parity.cjs`
  - 状态版本：`src/app_state.rs` 的 `DENSITY_WINDOW_RULE_VERSION`（从 `4` 升到 `5`，让旧的未标定帧重新检验片基门限）
- **作用域隔离**：
  - 仅在 `camera_native_display`（即帧携带有效 `camera_to_display` 矩阵、且为彩色模式新管线）时启用高光软肩部；
  - 非相机 RAW 路径（ProPhoto Estimate：JPEG、TIFF、DNG、扫描仪 FFF）保持原有路径 **100% 逐字节不变**，确保 74 个非相机 fixture 与全部既有 ProPhoto 白点单测零改动通过。
- **核心数学规则（`rolloff_camera_display_rgb` / `rolloffCameraDisplay`）**：
  保持 Phase 4 的**逐通道密度归一化 `normalized_working`** 完全不变！在 `normalized_working` 乘完 `positive_to_display` 矩阵并乘完 `exp2(master_exposure)` 得到 `positive_linear` 之后、进入 `pow(..., 1.0 / gamma)` 之前，执行保色相高光软肩部：

  1. **暗部截断**：
     将负通道钳制到 `0.0`：`rgb = max(rgb, 0.0)`。
  2. **中暗调恒等区（`0.0 ~ 0.85`，好图 100% 零影响区）**：
     设软肩部起点 `KNEE = 0.85`，渐近上限 `CEIL = 0.990`（严格低于 `0.995` 触顶统计线）。
     当峰值通道 `m = max(r, g, b) <= KNEE` 时：
     **直接原样返回 `rgb`（数学恒等变换）！** 画面 `0.0 ~ 0.85` 范围内的所有暗部、中间调、肤色、草地、牛毛与 Phase 4 基线逐像素完全一致。
  3. **保色相高光软肩部（当 `m = max(r, g, b) > KNEE` 时）**：
     对峰值通道 `m` 做平滑有理函数压缩（在 `KNEE` 处函数值与一阶导数均连续，无硬折角）：
     ```text
     span = CEIL - KNEE          // 0.140
     excess = m - KNEE
     compressed_m = KNEE + (span * excess) / (span + excess)
     rgb = rgb * (compressed_m / m)
     ```
     **为什么按比例缩放 `rgb * (compressed_m / m)` 而不是向 `luma` 收缩？**
     - **保持色相与饱和度（不褪色发白）**：非负三通道同乘一个正标量 `compressed_m / m`，通道比值 `R : G : B`（例如蓝天的 `B/G = 1.13`）保持 **100% 不变**，蓝天永远不会因为高光压缩而褪色变成灰白色！
     - **保持中性白绝对中性**：若像素本来是中性白 `(m, m, m)`，缩放后依然是 `(compressed_m, compressed_m, compressed_m)`，绝不偏色。
     - **消除高光死白与触顶**：相机矩阵放大到 `1.10 ~ 1.50` 的高光（如蓝天、白云、白教堂外墙）被平滑映射到 `[0.94, 0.98]` 区间内，既保留了高光内部的层次过渡，又保证峰值严格 `< 0.995`，不再触发大面积死白截断。

---

## 三、验收标准（硬性门槛）

1. **安全红线**：`test_picture/` 严禁任何写入、删除、移动或软硬链接操作，所有诊断图输出到 `target/`。
2. **正常负片零退化（A/B 视觉与数值验收）**：
   - 使用 `test_picture\尼康NEF 翻拍`（`_DSC7732.NEF`、`_DSC7735.NEF`、`_DSC7741.NEF`）生成与 Phase 4 基线（`3f59dbc`）的左右 A/B 对比图。
   - 正常样张不得出现任何青绿/蓝紫偏色，全图 `R/G`、`B/G` 与 Phase 4 基线的偏差必须 `<= 0.015`，肉眼观感与左图（Phase 4）在暗部和中间调一致，仅高光过曝边缘更平滑。
3. **曝光不变性与欠曝片基恢复**：
   - 在人为压低 10 倍曝光（模拟欠曝 3.3 档，片基密度 `> 1.60 D`）以及高密度色罩候选（如 `[2.3, 2.6, 2.9]`）下，`validate_film_base_candidate` 必须通过（`usable = true`），且 10× 欠曝渲染结果与正常曝光渲染结果完全一致。
4. **高光不发白、不触顶**：
   - 经相机矩阵放大后的高光像素（例如 `positive_linear = [0.75, 0.90, 1.25]`）经过软肩部后，峰值必须 `< 0.995`（零触顶），且通道比例 `B/G`、`R/G` 与压缩前完全一致（误差 `< 1e-4`，绝不向灰白褪色）。
   - 若环境中存在 `4000363.ARW` 或同类蓝天/高光 RAW 样张，天空 ROI 触顶比例（任一通道 `>= 0.995`）必须从 `28.46%` 降至 `< 1%`，且 `B/G > 1.05`。
5. **CPU / WebGL 严格一致与全套回归测试**：
   - `cargo test --lib` 全部通过；
   - `node --test ui/*.test.js` 与 `node scripts/verify-*.cjs` 全部通过；
   - `cpu_render_fixture_for_webgl_parity` + `node scripts/diagnose-cpu-webgl-parity.cjs` 最大通道差 `< 0.02`。
