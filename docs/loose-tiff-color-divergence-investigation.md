# Loose TIFF 三态色偏排查报告 —— 0218-.tif 报修件（P1 惨白 / 画布 P2 / 导出偏黄）

- 日期：2026-10-19
- 状态：**已修复**（2026-10-19 实施两处修复，见 §5 修复记录；验证全绿，未提交）
- 排查材料：`/Users/apple/Downloads/0218-.tif`（X2D 3FR 经外部工具导出的 16-bit AdobeRGB 负片 TIFF，大端 MM + 标准 ADBE ICC + 3000 DPI，单页 4300×2831）；用户实测截图三张（P1/P2/导出视图）与用户导出 JPG `loose_1792310662253_498_001.jpg`
- 关联：`docs/raw-backend-dual-source-plan.md` §9.4（报修样本验证记录，其中"成片无法事后修正"的旧结论已被本报告取代）

---

## 0. 结论摘要

报修件在当前构建上呈现三个状态，背后是**两个独立缺陷**：

| 现象 | 测量（画布区均值） | 归因 |
|---|---|---|
| P1 导入初始"惨白"中间态 | (240, 240, 232)，R/G=1.000，B/G=0.964，鬼影为**负像**结构 | **根因二**：画布渲染全局状态与纹理内容非原子绑定，ProPhoto transport 纹理被按 legacy 域直显（跳过 `decodeProPhotoTransport`） |
| P2 反相后画布 | (137, 139, 154)，B/G=1.108 ≈ 诊断 smart-auto 基准 1.047–1.125 | 正常输出（Smart Auto 路径当前特征，可作修复基准） |
| 导出 JPG 偏黄 | (87, 77, 63)，R/G=1.133，B/G=0.816 ≈ 诊断 legacy 基准 0.757–0.878 | **根因一**：导出与画布对带 ICC 的 TIFF 走了**不同的解码输入域** |

用户判断正确：两者都是本应用的 bug，与报修文件本身无关（该 TIFF 颜色正确，外部写入）。

---

## 1. 根因一：导出与画布的 TIFF 输入域分叉（高置信，已实锚）

### 1.1 两条路径

同一帧、同一份代码里，"画布 proxy"与"导出全分辨率"走不同解码器：

**画布**（`prepare_proxy` → SmartAuto 非 compatibility 分支，`src/commands.rs:7508-7519`）：

```
带 ICC 的 TIFF → decode_profiled_tiff_prophoto_estimate（f32 直转）
    encoded AdobeRGB --gamma 解码--> 线性 AdobeRGB --单矩阵--> ProPhoto 估计
    全程 f32，无逐通道钳制
```

**导出**（`batch_export_images`，帧为 SmartAuto 非 compatibility → `src/commands.rs:11955` → `decode_scanner_profiled_estimate_image_buffer` 的 TIFF 分支 `src/commands.rs:4847-4852`）：

```
带 ICC 的 TIFF → decode_reduced_tiff_for_working_space（u16 线性 sRGB 传输）
    encoded AdobeRGB --gamma 解码--> 线性 AdobeRGB --矩阵--> 线性 sRGB
    ★ 每通道 clamp(0,1) + u16 量化 → 再压回 ProPhoto
```

`decode_profiled_tiff_prophoto_estimate` 的文档注释（`src/commands.rs:4447-4456`）自己就写明了后者的破坏性：**负片橙罩有意落在 sRGB 色域之外，经 u16 线性 sRGB 传输会逐通道钳制——在真实合并扫描上红通道 84.7% 被钳（原文件仅 0.95%），产生青色高光/品红阴影**。该修复当时只落进了 `prepare_proxy` 的 SmartAuto TIFF 分支，**没有落到 `decode_scanner_profiled_estimate_image_buffer`**，而导出、Capture 回退等路径都在用后者。

### 1.2 数值证据（诊断工具输出，`target/diag-loose-tiff/`）

同一输入的各渲染分支全图 B/G：

| 渲染分支 | B/G | 说明 |
|---|---|---|
| `0218--negative.jpg` | 0.368 | 解码负片本体（强橙，正确） |
| `0218--legacy.jpg`（u16 传输链渲染） | 0.878 | **导出走的就是这条链** |
| `0218--smart-auto.jpg`（f32 直转链渲染） | 1.052 | **画布 P2 走的是这条链** |
| 用户导出 JPG 实测 | 0.816 | 落在 legacy 链一侧（叠加用户白平衡微调） |
| 用户 P2 截图实测 | 1.108 | 落在 smart-auto 链一侧（含场景内容） |

同输入两链 B/G 跨度约 0.76–1.13，画布/导出一致性契约被输入域分叉直接破坏。

### 1.3 修复建议 A

在 `decode_scanner_profiled_estimate_image_buffer` 的 TIFF 分支（`src/commands.rs:4847`）补上与画布相同的直转路径：

1. `is_tiff_extension(path) && !is_scanner_fff_tiff(path)` 且 `embedded_input_profile(path).is_some()` 且 `scanner_profile.is_none()` 时，改走 `decode_profiled_tiff_prophoto_estimate`（或把其中单像素转换提为可复用函数），保持 target_long_edge 缩放语义。
2. 绑定了 Scanner Profile 的 TIFF：profile 目前作用在线性 sRGB f32 上（`apply_linear_rgb_image`）；直转路径需要在"线性 AdobeRGB → ProPhoto"之前先把 profile 应用到线性源域，或维持现状（带 profile 的 TIFF 罕见，可暂留旧链并注释）。
3. 无内嵌 ICC 的 TIFF：维持 u16 回退（`tiff_smart_auto_input_is_estimated` 语义不变）。
4. 调用点影响面：`batch_export_images`（11945/11955）、`prepare_proxy` Capture 回退（7430）、其余非 RAW 杂项（7521/7548）——只有 TIFF 分支行为改变。

**验证方案**：
- 回归测试：对带 ICC 的测试 TIFF，断言 `decode_scanner_profiled_estimate_image_buffer` 输出与 `decode_profiled_tiff_prophoto_estimate`（同边长）逐像素一致（容差 ≤1 LSB）。
- 跑 `NEXFILM_DIAG_FRAME=<tif> cargo test --lib loose_tiff_render_diagnostic -- --ignored --nocapture`，修复后导出链与 smart-auto 链的 B/G 应收敛（|Δ| < 0.03）。
- 端到端：用 0218-.tif 重新导出，导出 JPG 与画布 P2 的 B/G 差应 < 0.03（当前 0.29）。

---

## 2. 根因二：P1 惨白中间态 —— 渲染全局状态与纹理非原子绑定（机理已定位，亮度残差待查）

### 2.1 排除过程（全部为实测/实算，非推测）

在**当前代码**上以真实报修件复刻"全新导入 → prepare_proxy → proxy 缓冲 → shader 两条分支"（临时探针，已删除）：

```
[PROBE] contract=SmartAutoProPhotoV11 domain=linear_prophoto_estimate base_source=unresolved
[PROBE] buffer 2560x1685 base_density=(0,0,0) flags=2  → bit0=0(未分析) bit1=1(prophoto)
[PROBE] staging view (invert OFF):        (126.4, 80.3, 26.4)  R/G=1.574 B/G=0.329  ← 正确的橙色色罩
[PROBE] density path (invert ON):         ( 55.3,135.0,217.7)  R/G=0.409 B/G=1.612  ← 蓝调，也不是白
[PROBE] staging transport-as-legacy:      (146.1,143.1,138.7)  R/G=1.021 B/G=0.969  ← ★ 与 P1 签名吻合
```

- 面板取证：P1 截图时帧处于未显影、未分析状态（D-Min/Max = 占位 0.100/2.000，全图锚 0.00，所有滑杆中性，彩色模式）→ 对应 `u_invert_enabled=0` 的 staging 分支，正常应为橙色。
- 结构取证：P1 鬼影与负片本体/导入预览**正相关**（corr +0.19/+0.19），与一切正片渲染**负相关**（smart-auto −0.34、legacy −0.34、导出 JPG −0.20）→ P1 显示的是**负片数据的结构**，不是正片。
- 签名取证：P1 的 R/G=1.000（精确中性）、B/G=0.964，与"transport 编码值被当作线性值直接显示"的模拟值 R/G=1.021、**B/G=0.969** 吻合（两个独立比值误差 <2%/<1%）；正常 staging 的 B/G=0.329 相差一个数量级。橙罩被 transport 编码（(e+1)/4，值域 [0.25, 0.5]）压平成中性灰，这正是"惨白无色彩"的来源。

### 2.2 机理

WebGL 画布的解码方式由**全局变量** `currentProxyDomain` 决定（`ui/main.js:3923-3926`）：为 `legacy_linear_srgb` 时，shader 跳过 `decodeProPhotoTransport`（`ui/main.js:2435-2437`），把纹理原始码值当线性 sRGB 显示。当 ProPhoto transport 纹理（flags bit1=1 的帧）遇上被污染成 legacy 的域标志，即得 P1。域标志与纹理不是原子绑定的，具体竞态入口（`ui/main.js`）：

1. **初始值即 legacy**：`let currentProxyDomain = 'legacy_linear_srgb'`（2087 行）——任何 proxy 解析完成前发生的渲染都按 legacy 解释纹理。
2. **短头兜底为 0**：`const flags = dataView.byteLength >= 28 ? … : 0`（4280 行）——缓冲不足 28 字节时 flags=0 → 域落回 legacy。
3. **解析中途无条件写全局且不回滚**：`currentProxyDomain = …`（4284 行）先于 token/id 守卫（4309 行）执行；一次被丢弃的加载（快速切帧、后台预热）也会把域标志改掉，而纹理与 `currentDMin/currentDMax/currentBaseDensity` 同样是分离的全局，可能拼出"新纹理 + 旧帧域/旧窗口"的组合。

亮度残差：签名匹配的模拟值为 146 灰，P1 实测 240 白，差约 +0.7 档；截图时滑杆全中性，这 0.7 档的确切来源未定（候选：截图前瞬时的滑杆状态、双通道加载拼出的旧窄窗口与密度路径组合、缩略图回灌）。**通道签名已足以定性**；残差列为修复前的待查项。

### 2.3 修复建议 B

1. **提交后置**：`loadProxyImage` 的解析段（4262-4342）先把 flags/domain/base/尺寸读进局部变量，全部守卫（token、id、isFullProxy）通过后**一次性**提交到全局；缓存命中路径同样适用。
2. **初始值不可用化**：`currentProxyDomain` 初始改为 `null`；`renderWebGL` 在域未知时拒绝渲染（清屏或保持隐藏），不得默认按 legacy 解释。
3. **元数据随纹理走**：将 `{texture, domain, baseDensity, frameId}` 封装为单一对象整体换入，渲染只从当前对象取值，彻底消除"一个帧的纹理配另一个帧的状态"。
4. 回归：`node --test ui/*.test.js` 增加对"提交函数在过期 token 下不落任何全局"的单测；`verify-webgl-shaders.cjs` 可加域/纹理一致性静态检查；手动复现序列——导入 0218-.tif 后在胶片条快速来回切帧，画布任何时刻不得出现中性惨白图。

---

## 3. 修复顺序建议

1. **先修根因一**（导出输入域分叉）：用户可直接感知（导出偏黄=报修主诉），改动面小、验证手段完备。
2. **再修根因二**（渲染状态竞态）：影响所有帧类型的显示稳定性，改动在 UI 层，需配合手动复现序列验证。
3. P2 无需单独处理：它就是 Smart Auto 路径的基准输出；根因一修复后导出向它收敛。

## 4. 报修件归因（修订）

0218-.tif 本身颜色正确（外部工具写入的 AdobeRGB 负片）。其复现的"导出偏黄"即根因一；"P1 惨白"即根因二。此前 §9.4 中"成片颜色已烙进像素、无法事后修正"的结论作废——该 TIFF 未经过任何本应用处理，偏色完全产生于本应用管线内部。

---

## 5. 修复记录（2026-10-19）

**根因一**（`src/commands.rs`）：`decode_scanner_profiled_estimate_image_buffer` 在"无绑定 Scanner Profile 的普通 TIFF"上先尝试 `decode_profiled_tiff_prophoto_estimate`（与画布相同的直转），失败（压缩 TIFF 等流式读取器不认的情况）才回落 u16 传输链。原 `profile.filter(...)` 上移到分支前，行为不变。

**根因二**（`ui/main.js`）：
1. `currentProxyDomain` 初始值 `'legacy_linear_srgb'` → `null`；
2. `loadProxyImage` 解析段改为**局部变量先行**（headerBaseDensity/headerDomain/baseAnalyzed），在 token/id 守卫通过后与纹理**一次性提交**到全局（原实现中 base/domain 在守卫前就写全局，过期加载会污染下一帧）；
3. `renderWebGL` 增加守卫：域未提交时拒绝渲染（不猜测解码方式）。

**验证**：
- 新增回归测试 `export_estimate_matches_the_canvas_decode_for_profiled_tiffs`：合成 AdobeRGB ICC TIFF 夹具，断言导出解码 == 画布解码（逐位），并证明夹具确实分离两条输入域（旧路径分歧 > 0.01）；`NEXFILM_DIAG_FRAME` 指向真实报修件时附加双路逐位相等断言（已跑通过）。
- `cargo test --lib` 287 通过（286+1）、`node --test ui/*.test.js` 19 通过、13 个 verify 脚本全过；顺手清掉了测试模块一处陈旧未用导入与一处多余 `mut`（上一会话遗留警告）。
- 待用户在应用内实测：重新导出 0218-.tif 应不再偏黄；导入后快速切帧不应再出现惨白中间态。
