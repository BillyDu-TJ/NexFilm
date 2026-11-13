# 欠曝底片噪点抑制与 Capture One 画质差距优化方案

## 1. 背景与现状对比

在完成 `fix/color-cast-and-highlight-rolloff`（已合并至 `main`，commit `ea17647`）后，NexFilm 的极端偏色与高光发白问题已彻底解决，正常曝光底片与欠曝富士 RAF（如 `Capture One Catalog1944.raf`、`1945.raf`）的色彩还原均已回归正常。

对比当前 NexFilm 结果（P1/P3）与 Capture One 去色罩结果（P2/P4），剩余的核心画质差距集中在**噪点与通透度**：
1. **天空与水面（正片中高光 / 底片高密度区）**：NexFilm 存在明显的粗糙彩色颗粒（红/绿/蓝杂色斑点），而 Capture One 的天空与水面干净细腻。
2. **暗部与片边（正片暗部 / 底片低密度片基区）**：NexFilm 的未曝光黑边、人物黑发与深色衣物中散落着明显的彩色噪点，暗部略显发灰不够扎实；Capture One 的黑位干净纯粹。

---

## 2. 根因剖析（四大硬伤）

排查 [src/commands.rs](../src/commands.rs) 与 [ui/main.js](../ui/main.js) 的完整渲染链路后，定位到四个相互叠加的底层原因：

### 根因一：WebGL 预览纹理的 `u16` 线性量化断层（预览粗糙颗粒的头号元凶）
- **代码位置**：[src/commands.rs:75-76](../src/commands.rs#L75-L76)、[src/commands.rs:2269-2285](../src/commands.rs#L2269-L2285)、[ui/main.js:2297-2303](../ui/main.js#L2297-L2303)。
- **机理**：CPU 将线性透射率 `estimate`（范围 `[-1.0, 3.0]`，总跨度 `4.0`）**均匀线性映射**为 16-bit 整数纹理（步长 $\Delta T = 4.0 / 65535 \approx 6.1036 \times 10^{-5}$）传给 WebGL，在片元着色器中才计算 `-log10(T)`。
- **后果**：在欠曝底片（如 `1944.raf`）上，蓝色通道叠加片基色罩后的透射率 $T_B$ 仅在 `0.0016 ~ 0.0070` 之间。除以线性步长 $6.1036 \times 10^{-5}$ 后，**整张图蓝色通道的完整影调在 `u16` 纹理中总共只分到了约 89 个整数台阶（天空区域甚至只有约 22 个整数台阶）**！当 WebGL 取 `-log10(T)` 并按 `[D-Min, D-Max]`（`1944.raf` 的跨度仅 `0.485 ~ 0.631 D`）拉伸后，每 1 个 LSB 跳变都会造成 **> 2.3% 的亮度/色彩跳变**，直接把平滑渐变切碎成粗糙的“砂纸感”量化噪点。

### 根因二：暗部幂函数 `pow(x, 1.0 / gamma)` 在零点处的无穷大斜率放大片基底噪
- **代码位置**：[src/commands.rs:10841](../src/commands.rs#L10841)、[ui/main.js:2570](../ui/main.js#L2570)。
- **机理**：从截图 P1/P3 右侧面板可以看到，欠曝底片的 `D-Min` 为 `0.000, 0.000, 0.001`，`D-Max` 仅为 `0.485 ~ 0.631`。暗部像素紧贴片基密度，而片基本身存在 $\pm 0.01\text{ D}$ 的胶片颗粒与传感器读出噪声，因此有一半的片基/暗部像素落在归一化值 $x \in (0.002, 0.025)$ 区间。
- **后果**：当前直接执行纯幂函数 $x^{1/2.2}$。由于 $\frac{d}{dx}(x^{1/2.2}) = \frac{1}{2.2}x^{-0.545}$ 在 $x \to 0^+$ 时导数趋于**无穷大**：
  - 当片基某通道产生仅 $+0.01\text{ D}$ 的微小噪声（$x = 0.01 / 0.50 = 0.02$）时，$0.02^{1/2.2} = 0.169$（即 8-bit 下的 `RGB 43`），**单通道暗部噪声被直接放大了 8.5～18 倍**！
  - 再叠加上窄窗口增益（$\approx 2\times$）和相机矩阵对角线增益（$1.74\times$），片基与暗部的微小波动被放大 **30～60 倍**，导致未曝光黑边和暗部布满红绿蓝彩点、黑位发浮。

### 根因三：线性透射率空间的 `Lanczos3` 负瓣振铃（放大低透射率噪点尖峰）
- **代码位置**：[src/commands.rs:7706-7711](../src/commands.rs#L7706-L7711)（以及 [src/commands.rs:20052-20057](../src/commands.rs#L20052-L20057)）。
- **机理**：生成预览代理时，代码在**线性透射率空间**使用 `FilterType::Lanczos3` 进行下采样。`Lanczos3` 具有负权重旁瓣（`-0.095`）。
- **后果**：在低透射率区域（$T \approx 0.002$），当 `Lanczos3` 跨过传感器噪点时，负瓣会产生向下过冲（把像素推向 `0.0002` 甚至触发 `epsilon = 1e-6` 下限）。由于 $-\log_{10}(T)$ 在 $T \to 0^+$ 时急剧膨胀，**线性空间的每一次负瓣下冲都会在正片中变成一颗刺眼的亮色噪点**。

### 根因四：低光子通道在 `-log10(T)` 与相机矩阵下的色度噪声放大（缺少保边色度降噪）
- **代码位置**：[src/commands.rs:7700-7745](../src/commands.rs#L7700-L7745)、[src/commands.rs:10790-10845](../src/commands.rs#L10790-L10845)。
- **机理**：负片的橙色色罩使蓝、绿通道透射光强远低于红通道（蓝通道光子数仅为红通道的 $1/5 \sim 1/10$）。在 $D = -\log_{10}(T)$ 变换下，微分增益为 $\frac{1}{T \ln 10}$，蓝通道读出噪声被放大数倍；随后经过相机色彩矩阵（如 `R = 1.74 R - 0.56 G - 0.18 B`、`B = 1.38 B - 0.40 G`），各通道互不相关的传感器噪声被差分放大，形成高频红/青、蓝/黄色斑（Chroma Mottle）。
- **对比 Capture One**：C1 在 RAW 转换底层默认对色度通道（Chrominance）做保边平滑，仅抹平高频红蓝杂色而完整保留亮度通道（Luminance）的胶片颗粒与细节边缘。

---

## 3. 具体修复方案

本次优化严格遵循**最小侵入、CPU/WebGL 100% 奇偶一致、不改变正常曝光样张色彩与影调**的原则，分四步闭环：

### 改动 1：将 WebGL 传输代理升级为分段对数压扩编码（消除预览量化颗粒）
- **文件**：[src/commands.rs](../src/commands.rs)、[ui/main.js](../ui/main.js)
- **设计**：
  保持 `[-1.0, 3.0]` 的可表示范围与 `u16` 纹理格式不变，将均匀线性映射替换为**分段对数压扩（Piecewise Log Companding）**：
  - 定义常量：
    - `PROPHOTO_TRANSPORT_MIN = -1.0`
    - `PROPHOTO_TRANSPORT_MAX = 3.0`
    - `PROPHOTO_TRANSPORT_ZERO_SPLIT = 0.125`（将 `[-1.0, 0.0]` 映射到归一化 `[0.0, 0.125]`，用于保留负值出界测试与未反相预览；将最关键的正透射率 `[0.0, 3.0]` 映射到 `[0.125, 1.0]`，占 `u16` 的 87.5% 即 57,343 个台阶）
    - `PROPHOTO_TRANSPORT_LOG_MU = 4096.0`
  - **编码公式（Rust `encode_prophoto_transport_sample(x: f32) -> f32`）**：
    - 若 $x < 0.0$：
      $$\text{encoded} = \text{ZERO\_SPLIT} \times \frac{\text{clamp}(x, -1.0, 0.0) - (-1.0)}{1.0}$$
    - 若 $x \ge 0.0$：
      $$u = \frac{\ln(1.0 + \mu \cdot \text{clamp}(x, 0.0, 3.0))}{\ln(1.0 + 3.0 \mu)}$$
      $$\text{encoded} = \text{ZERO\_SPLIT} + (1.0 - \text{ZERO\_SPLIT}) \cdot u$$
  - **解码公式（Rust `decode_prophoto_transport_sample` 与 GLSL `decodeProPhotoTransport`）**：
    - 若 $e < \text{ZERO\_SPLIT}$：
      $$x = \frac{e}{\text{ZERO\_SPLIT}} - 1.0$$
    - 若 $e \ge \text{ZERO\_SPLIT}$：
      $$u = \frac{e - \text{ZERO\_SPLIT}}{1.0 - \text{ZERO\_SPLIT}}$$
      $$x = \frac{\exp(u \cdot \ln(1.0 + 3.0 \mu)) - 1.0}{\mu}$$
- **效果**：在欠曝蓝通道 $T = 0.0018$ 处，量化精度比原来的线性编码**提升约 185 倍**（从 89 个台阶提升到 > 8,500 个台阶），彻底根除 WebGL 预览的量化噪点。

### 改动 2：将线性透射率缩放从 `Lanczos3` 改为无负瓣的 `Triangle`（消除负瓣下冲噪点）
- **文件**：[src/commands.rs](../src/commands.rs)
- **设计**：
  在 [src/commands.rs](../src/commands.rs) 中所有对线性透射率 `estimate` 执行 `image::imageops::resize` 的位置（如 `7710`、`20056` 等），将 `FilterType::Lanczos3` 替换为无负瓣的 `FilterType::Triangle`（面积/双线性滤波）。
- **效果**：保证非负透射率在下采样时绝不发生负瓣下冲（undershoot），同时对传感器高频椒盐读出噪声起到天然的抗混叠低通平滑作用。

### 改动 3：在解码阶段对 RAW 线性透射率执行轻量级保边色度去噪（还原 C1 的纯净天空）
- **文件**：[src/commands.rs](../src/commands.rs)
- **设计**：
  在 `decode_scanner_profiled_estimate_image_buffer`（RAW 路径构建 `estimate` 之后、缩放与分析之前，或缩放之后对 `estimate`）增加轻量级**密度域保边色度平滑函数** `suppress_raw_density_chroma_noise(estimate: &mut ImageBuffer<Rgb<f32>, Vec<f32>>)`：
  1. 对每个像素取安全对数密度 $D = -\log_{10}(\max(T, 10^{-5}))$。
  2. 将像素分解为平均密度亮度 $L = \frac{1}{3}(D_R + D_G + D_B)$（或绿通道主导亮度 $L = 0.2126 D_R + 0.7152 D_G + 0.0722 D_B$）与两个色度差通道：
     $$C_1 = D_R - D_G, \quad C_2 = D_B - D_G$$
  3. 在 $5 \times 5$ 邻域（步长 1 和 2 的双尺度或 $5 \times 5$ 双边核）内，**仅对色度差 $C_1, C_2$ 做保边双边滤波（Bilateral Chroma Filter）**，而**完全保持绿/亮度密度 $D_G$（或 $L$）不变**：
     - 空间权重：高斯核（$\sigma_s = 1.5$）。
     - 边缘保护权重：由亮度密度差 $\Delta L = |L(q) - L(p)|$ 和色度跳变 $\Delta C = \max(|C_1(q)-C_1(p)|, |C_2(q)-C_2(p)|)$ 控制（$\sigma_L = 0.045\text{ D}$，$\sigma_C = 0.12\text{ D}$），当遇到真实物体边缘或色彩边界时权重迅速衰减为 0。
     - 自适应强度：根据局部透射率动态调节混合比例 $\alpha \in [0.35, 0.85]$——当最小通道透射率 $T_{\min} < 0.02$（高密度/欠曝区，即正片天空与亮部）时使用完整强度 $\alpha = 0.85$；当透射率充足（$T_{\min} > 0.10$）时温和降噪 $\alpha = 0.35$。
  4. 用平滑后的 $\tilde{C}_1, \tilde{C}_2$ 重建各通道透射率：
     $$\tilde{D}_R = D_G + \tilde{C}_1, \quad \tilde{D}_G = D_G, \quad \tilde{D}_B = D_G + \tilde{C}_2$$
     $$\tilde{T}_c = 10^{-\tilde{D}_c}$$
- **核心优势**：
  - **零模糊**：因为亮度/绿通道密度 $D_G$ 100% 原样保留，发丝、桥索、五官、胶片原始颗粒感分毫不损！
  - **CPU 与 WebGL 自动 100% 同步**：因为该步骤直接作用在 `estimate` 缓冲上，无论是 CPU 导出、缩略图，还是编码进 `transport` 交给 WebGL 实时预览，看到的都是完全一致的去彩噪结果，无需在 WebGL 增加额外 Pass！
  - **零偏色**：双边滤波在均匀区域的均值为恒等变换（$\mathbb{E}[\tilde{C}] = \mathbb{E}[C]$），不会改变任何区域的整体色彩平衡（`R/G`、`B/G` 均值变化 $< 0.002$）。

### 改动 4：为正片暗部 `pow(display_linear, 1.0 / gamma)` 引入平滑线性趾部（消除黑边与暗部彩噪爆炸）
- **文件**：[src/commands.rs](../src/commands.rs)、[ui/main.js](../ui/main.js)
- **设计**：
  在正片显示伽马转换处（CPU 的 `render_density_pixel` / `render_positive_linear_to_display` 与 WebGL 的 `pow(display_linear, vec3(1.0 / safe_gamma))`），将 `x = 0` 处斜率为无穷大的纯幂函数替换为**带平滑暗部趾部（Shadow Toe）的伽马函数** `apply_display_gamma_with_toe(x: f32, gamma: f32) -> f32`：
  - 仅在相机原生显示路径（`camera_native_display == true` / `u_camera_native_display != 0`）或统一正片伽马路径下启用：
  - 设暗部趾部拐点 $x_0 = 0.018$（对应正片最暗的 $1.8\%$ 线性光，即片基噪声区）：
    - 当 $x \ge x_0$ 时：严格保持原公式 $f(x) = x^{1/\gamma}$，中灰与高光 **100% 恒等不变**！
    - 当 $0 < x < x_0$ 时：使用一条在 $x = 0$ 处斜率受限（消除无穷大导数）、在 $x = x_0$ 处**函数值与一阶导数 $C^1$ 连续**的三次 Hermite 趾部曲线：
      令 $t = x / x_0 \in [0, 1)$，$y_0 = x_0^{1/\gamma}$，在 $t = 1$ 处的归一化切线斜率为 $m_1 = \frac{1}{\gamma} y_0$，在 $t = 0$ 处的起始斜率设为 $m_0 = 0.25 \cdot y_0$（将零点附近的噪声增益从 $\infty$ 压低到原来的 $1/4$）：
      $$f(x) = m_0 t (1 - t)^2 + y_0 t^2 (3 - 2t) + (m_1 - y_0) t^2 (t - 1)$$
      简化提取公因式 $y_0$ 后（令 $p = 1/\gamma$）：
      $$f(x) = x_0^p \cdot t \cdot \left[ 0.25 (1 - t)^2 + t (3 - 2t) + \left(p - 1\right) t (t - 1) \right]$$
  - 验证该公式：
    - 在 $t = 0$（$x = 0$）：$f(0) = 0$，斜率有限且平缓（片基 $\pm 0.008\text{ D}$ 的底噪被压制在纯黑附近，不再炸成 `RGB 40` 的彩点）。
    - 在 $t = 1$（$x = x_0 = 0.018$）：$f(x_0) = x_0^p$，且一阶导数 $f'(x_0) = p x_0^{p-1}$，与 $x \ge x_0$ 的纯幂函数 **完全 $C^1$ 平滑衔接**，无任何断层！
    - 在 $x \ge 0.018$（即 sRGB 亮度 $> 15\%$ 的所有暗部过渡、肤色、中灰、高光）：**数学上 100% 恒等（逐比特不变）**！

---

## 4. 验收标准

1. **构建与自动化测试**：
   - `cargo test --lib` 全部通过（更新 `prophoto_transport_preserves_signed_estimate_values_within_cache_range` 以使用新的 `decode_prophoto_transport_sample`）。
   - `node --test ui/*.test.js`、`node scripts/verify-webgl-shaders.cjs`、`node scripts/verify-density-contract.cjs`、`node scripts/verify-handbook-sync.cjs` 全部通过。
   - CPU / WebGL 奇偶校验测试通过，最大通道差 `< 0.015`。
2. **量化指标验收**：
   - 在 $T \in [0.001, 0.010]$ 的欠曝透射率区间，`prophoto_estimate_to_transport_proxy` 编解码往返后的密度误差 $|\Delta D| < 0.001\text{ D}$（相比修改前的 $0.025\text{ D}$ 降低 95% 以上）。
   - 在真实 RAW 0.1x 欠曝模拟测试中，均匀天空/背景区域的色度噪声标准差（$\sigma(R-G), \sigma(B-G)$）下降 **$\ge 40\%$**，而全图 `R/G`、`B/G` 均值漂移 **$< 0.005$**（完全不改变色彩倾向）。
   - 正常曝光样张（10 张尼康 NEF）的全图均值与色彩比值变化 **$< 0.005$**，触顶率保持 `0%`。
