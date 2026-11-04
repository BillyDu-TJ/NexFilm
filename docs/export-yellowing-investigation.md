# 导出发黄重开调查:五级路径对比诊断报告与修复方案草案

- 日期:2026-11-03
- 基线:main `c8480b5` + 本文新增诊断测试(提交见文末)
- 样张:`test_picture/导出发黄/0218-.tif`(X2D 3FR 经外部工具导出的 16-bit AdobeRGB 负片 TIFF,4300×2831,即 2026-10-19《Loose TIFF 三态色偏排查报告》的同一报修件)
- 上游调查:`docs/loose-tiff-color-divergence-investigation.md`(根因一解码域分叉 + 根因二渲染竞态,声称已修复;c8480b5 提交留言记录 2026-11-02 用户反馈**仍未解决**,调查重开)
- 本对话边界:**只新增诊断测试与文档,不改任何生产代码**

## 0. 结论摘要

对同一帧把画布(Develop 预览)与批量导出两条路径按 解码 → 颜色矩阵来源 → 片基 → 反相范围 → 渲染 → 编码 逐级对比,结果:

| 级 | 状态 | 证据 |
|---|---|---|
| L1 解码 | ✅ 对齐(已修复) | 同入口同矩阵,proxy 2560 与全尺寸 4300 的密度域色度差 <0.001 |
| L2 颜色矩阵来源 | ✅ 无分叉 | 两路共用 `decode_profiled_tiff_prophoto_estimate`,同一内嵌 ICC |
| L3 片基 | ❌ **结构分叉(实测)** | 导出在全分辨率重测,Δ=(-0.020,-0.024,-0.033)D,通道不均衡;继承片基场景还会回写数据库 |
| L4 反相窗口 | ❌ **结构分叉(实测)** | highlight fraction 0.493→0.535(+8.5% 相对);隔离实验证明导出/画布差全部来自重测,与分辨率无关 |
| L5 渲染 | ✅ 数值一致 | WebGL(真实 shader)vs CPU 全帧 max \|Δ\|=0.0029 < 8-bit 量化半步;公式逐字对齐 |
| L6 编码 | ✅ 逐位一致 | sRGB 输出往返编码 mean 不变 |

**三条判定:**

1. **历史偏黄主因(解码域分叉)在 main 上已修复且实测对齐。** 本样张(Smart Auto 散片)当前画布=导出(R/G 差 0.001)。用户 11-02"仍发黄"的实测极可能跑在不含修复的构建上(修复 10-19 实现但 11-02 才随 c8480b5 落库,v1.1.0 发布件不含)——请先用 main 最新构建重测此样张。
2. **仍然活着的分叉是用户怀疑点一,实测成立:** 批量导出在**全分辨率**上重测片基/帧基/高光并重建反相窗口,而画布展示的永远是 **proxy 尺度**测得的状态;导出导出的是"你在画布上从没看到过的状态",且继承片基重测还会**回写数据库**改变工程状态。"偶发"与此吻合:是否分叉取决于帧的 `base_source` 与两次测量的落点差。
3. **怀疑点二(CPU/WebGL 渲染不一致)与怀疑点三(Mac 解码入口/矩阵缺失)实测排除。** 1.1.0 后新增的曲线/细节/色温控件在两条渲染器里公式与顺序逐字一致,数值差小于 8-bit 量化;持久化链路(UI→DB→导出)完整。

偏色方向说明:L3/L4 在本样张上让导出比画布**偏蓝**(R/G 0.723→0.702)。同一个机制在测量落点反向移动的帧上就表现为偏黄/偏红——机制一致,方向逐帧而异,这正是"偶发"的第二重含义。

## 1. 工具与复现

新增三个诊断件(均为 `#[ignore]` 手动诊断或独立脚本,不进 CI):

```bash
# 五级对比(默认样张即 0218-.tif,可用 NEXFILM_DIAG_FRAME 覆盖)
cargo test --lib export_preview_parity_diagnostic -- --ignored --nocapture

# WebGL↔CPU 渲染数值一致性 fixture + 浏览器对比
cargo test --lib cpu_render_fixture_for_webgl_parity -- --ignored --nocapture
node scripts/diagnose-cpu-webgl-parity.cjs
```

输出:`[PARITY]` 标签的结构化数字 + `target/diag-export-parity/*.jpg`(画布状态 vs 导出状态渲染并排)。

## 2. 逐级证据

### L1 解码——对齐

- 画布:`prepare_proxy → decode_profiled_tiff_prophoto_estimate(path, 2560)`(`src/commands.rs:7512`)
- 导出:`batch_export_images → decode_scanner_profiled_estimate_image_buffer(ExportFull) → decode_profiled_tiff_prophoto_estimate(path, u32::MAX)`(`src/commands.rs:11966`)
- 实测(0218-.tif):密度域均值色度

| 输入 | R/G | B/G |
|---|---|---|
| preview(2560 proxy 解码) | 0.8283 | 1.5948 |
| export(4300 全尺寸解码) | 0.8280 | 1.5940 |
| export 重采样到 proxy 尺寸 | 0.8285 | 1.5891 |

差 <0.001:10-19 的修复(f32 直转替代 u16 线性 sRGB 传输)在两个分辨率下都成立。同尺度逐位相等的回归测试 `export_estimate_matches_the_canvas_decode_for_profiled_tiffs` 亦在基准(287)内通过。

### L2 颜色矩阵来源——无分叉

两条路径调用同一个函数、读同一个内嵌 ICC(AdobeRGB),编码域 gamma 解码 → 线性 AdobeRGB → 单矩阵 → ProPhoto 估计,逐像素、无逐通道钳制。RAW 路径的矩阵来源差异(LibRaw identity 时借 rawler D65 矩阵)有 `decoder_id()` 一致性检查拒绝混源,不作用于 TIFF。

### L3 片基——结构分叉(实测)

代码分叉:

- 画布:`remeasure_inherited_film_base` 在 **proxy** 上测(`src/commands.rs:8041-8052`)
- 导出:`inherited_film_base_measurement(&state, &geom, &input)`,`input` 是**全分辨率**解码,且结果 `persist_base_and_pipeline` **回写数据库**(`src/commands.rs:11981-12004`,注释自述"this decode is the first chance to measure the film on its own pixels")

实测(同帧同几何,`estimate_film_base_f32`):

| 尺度 | 片基密度 (R,G,B) | 来源 |
|---|---|---|
| proxy 2560 | (0.447, 0.618, 1.031) | content_high_quantile |
| full 4300 | (0.426, 0.594, 0.998) | content_high_quantile |
| **Δ** | **(-0.020, -0.024, -0.033)** | 通道不均衡→作为色罩减除时直接注入色偏 |

触发门槛:`base_source == "inherited_film_base"`——继承卷级片基、尚未在画布打开过的帧才会走到。**画布打开过的帧此块不触发**(画布已用 proxy 重测并持久化,source 不再是 inherited),这正是"偶发"的第一重来源:同一卷里,导出过没打开的帧与打开过的帧状态不同。

### L4 反相窗口——结构分叉(实测,主通道)

代码分叉:

- 画布:`resolve_frame_render_parameters` 在 **proxy** 上测 frame base 与 highlight fraction(`src/commands.rs:3173-3209`),`analyze_proxy_density_limits` 据此定 mapping
- 导出:`batch_export_images` 对**所有** `RollAnchoredProPhotoV11 | CaptureCorrectedV11` 帧在全分辨率上重测 `detect_frame_base_density` + `detect_frame_highlight_fraction` 并重建 `render_mapping`(`src/commands.rs:12008-12031`)

实测:

- `detect_frame_base_density`:proxy (0.447, 0.618, 1.031) vs full (0.426, 0.594, 0.998)
- highlight fraction:**0.493 → 0.535**(+8.5% 相对,白点位置直接移动)
- 合成 Roll 锚点下 `roll_density_mapping_with_frame_base`:density_high (1.036, 0.997, 0.957) vs (1.103, 1.057, 1.005)

**隔离实验(决定性)**——同参数同基色,只动一个变量:

| 渲染 | R/G | B/G |
|---|---|---|
| proxy 输入 + proxy 测量(画布所见) | 0.723 | 1.478 |
| **full 输入 + proxy 测量(仅尺度)** | **0.725** | **1.468** |
| full 输入 + full 重测(导出所用) | 0.702 | 1.494 |

尺度本身贡献 0.002;重测贡献 0.021(十倍)。**导出与画布的差全部来自重测,不是分辨率。** graded 参数(色温 -0.30 等)下三行整体平移、差值不变,参数链路无辜。

### L5 渲染——数值一致(怀疑点二排除)

- 公式静态核对:端点 trim(span/span_luma 中性化)、归一化、ProPhoto→sRGB 矩阵位置、曝光时机(矩阵后、gamma 前)、gamma、曲线位置(打印机光之后美学之前,两处注释互认)、后期调整顺序与系数(tone→contrast→temp(±0.20)→tint(±0.10/0.20)→sat,`core_math.rs:180` vs `main.js:2330`)逐字一致。
- 数值实测:96×64 合成负片,非平凡参数(四通道控制点曲线、temperature -0.25、tint 0.10、gamma 2.2、RGB 通道曝光、饱和、对比、高光/阴影),真实 shader(SwiftShader 无头渲染,含 RGBA16UI 传输纹理解码 + RGBA16F 曲线 LUT 纹理):

  - mean |Δ| = (0.00092, 0.00086, 0.00054)
  - max |Δ| = 0.0026-0.0029,**小于 8-bit 量化一步(1/255≈0.0039)的一半**

- 持久化链路:曲线四通道/细节六参/色温色相均进 `captureEditState → updateBackendParams`(`ui/main.js:1444-1486`),`TuningParams` serde 兼容旧数据(缺字段落默认并有测试)。
- 未覆盖:Detail 多通道链(本 harness 参数为中性;两实现各自有缩放适配设计)、`mediump` 精度(桌面 GPU 实际为 fp32,风险低)。

### L6 编码——逐位一致(怀疑点三之编码侧排除)

`encode_export_buffer`(sRGB 输出)= 显示域值→线性→sRGB 再编码,round-trip 后 mean 逐位不变;导出链的 Lanczos3 缩放在显示域且不改变均值色度;USM 只影响细节。AdobeRGB/ProPhoto 输出走同一矩阵管线,无双重变换。

## 3. 修复方案草案(留待实施对话,按推荐排序)

**方向 A(推荐,改动最小):导出即所见。**
`batch_export_images` 删除 11986-12031 的重测与重建(或收紧为"仅当帧从未分析、无任何可用窗口时"的兜底),导出直接消费与画布相同的持久化 `base_color`/`render_mapping`。继承片基的重测统一移到 `prepare_proxy`(画布首次见到该帧时测一次、持久化,此后两路只读)。同时移除导出里的 `persist_base_and_pipeline` 回写——**导出必须是只读操作**。
代价:未打开过的帧若分析缺失,导出需要先触发一次与画布相同的分析(见方向 B 的机制)。

**方向 B:测量输入统一到 proxy。**
若产品上要求"导出前未分析帧也能直接出片":导出路径在渲染前对**proxy 尺度**的解码跑与画布完全相同的分析函数(同函数、同分辨率、同输入),用其结果渲染。全分辨率解码只供像素,不参与测量。两路数字因此完全相同,不需要"冻结状态"的时序假设。

**方向 C(治本,配合 1.2 Density 层):测量分辨率无关化。**
把 frame base / highlight 测量改为在固定分析尺度(proxy 等价降采样)上进行并持久化为帧状态的一部分(与 D-min/D-max/Content Range 分离的既有语义一致),画布与导出一律读同一份持久化测量。这同时消灭"导出回写"的必要性,并为 1.2 的 Density 校准提供稳定的测量合同。

**验收标准(无论哪个方向):**

1. `export_preview_parity_diagnostic` 的 roll-anchored 三行收敛:画布状态 vs 导出状态 |ΔR/G| < 0.005 且 |ΔB/G| < 0.005;
2. 新增契约测试:导出使用的 `base_color` 与 `render_mapping` 与数据库持久化值逐字段相同(即导出不改状态);
3. 保留本诊断测试作为长期回归(样张在库:`test_picture/导出发黄/0218-.tif`)。

**待用户确认的一点:** 请用 main 最新构建(c8480b5 及以后)重新导出一次 0218-.tif。若已不黄,则历史报修全部归于已修复的 L1,重开的工单只需处理 L3/L4;若仍黄,把导出件放进 `test_picture/导出发黄/`,本诊断的 `[PARITY]` 数字可以直接定位残余分叉在哪一级。

## 4. 本次新增的诊断资产

- `src/commands.rs`(import_contract_tests 测试模块):`export_preview_parity_diagnostic`(五级对比)、`cpu_render_fixture_for_webgl_parity`(渲染对齐 fixture)及三个打印辅助——纯测试代码,生产路径零改动
- `scripts/diagnose-cpu-webgl-parity.cjs`:真实 WebGL shader 与 CPU 渲染器数值对比 harness
- 本报告

## 5. 修复记录(2026-11-03,用户实测确认后实施)

用户在应用内实测确认:导出发黄已消除(解码域修复生效)。同时报告新问题并下达修复指令,两件修复同日落地:

**修复一:L3/L4 导出重测分叉(方案 A"导出即所见")。** `batch_export_images` 删除了继承片基重测(+数据库回写)与全分辨率反相窗口重建两块代码;导出现在严格只读,渲染数据库快照里持久化的 `base_color` 与 `render_mapping`——与画布所见完全一致。继承片基的换测只发生在画布侧(prepare_proxy 缓存命中/全新解码、批量应用三处,均已有)。验收:静态契约(scripts/verify-density-contract.cjs 断言 batch export 体内不得出现任何重测/回写调用)+ 更新了钉住旧行为的断言。

**修复二:胶片范围选区导致选区外白化(用户澄清产品语义后的整功能移除)。** 用户明确定义:选区只改变校色(分析)输入,选区内外受同一渲染,不能因未选中而变白。据此整体移除 c8480b5 引入的"片门外纸白"(rebate whitening):

- WebGL shader:白化仅在 sprocket 工具配置时经其颜色相似度遮罩生效;仅有片门选区时不再有任何白化路径。
- CPU `render_shader_equivalent_core`:删除 `film_area_confirmed` 硬白分支。
- sprocket 工具(齿孔白化)行为不变。

验收:`film_area_points_do_not_apply_perspective_correction` 改为断言"带选区渲染与无选区渲染逐位一致";CPU/WebGL 数值 harness 各加"小选区重渲染必须逐位不变"不变式;真实样张 AB 面板四角从纯白 (255,255,255) 恢复为图像内容,画布/导出仍一致(B/G 1.147 vs 1.144)。全量回归:cargo 287 / node 19 / verify 脚本全过(含新静态护栏),rustfmt 无新增差异。

## 6. 后续报告:重开后画面发白/消失(2026-11-04,同日修复)

用户复现序列:开图先橙(暂存负片)→ 1-2 秒后变白;保存范围后图片消失;自动反相恢复正常;重置再次消失;再反相又正常。导出已确认正常。

**诊断方法**:直接读应用数据库 `nexfilm_user.db` 中该帧的持久化状态(参数/基色/管线状态/几何),用真实管线代码重建代理缓冲(`washed_open_state_diagnostic`,#[ignore]),再以真实 shader 在无头浏览器按应用的 uniform 规则渲染(`scripts/diagnose-washed-open-state.cjs`),与截图数值对拍。

**变体定位**(真实 shader,同一缓冲):

| 渲染变体 | mean 8-bit RGB | 对应现象 |
|---|---|---|
| 持久化状态(真实基色+真实窗口) | (115, 115, 130) | 重开后的"发灰" |
| 占位窗口 [0.1,2.0] | (35, 46, 77) | 深蓝暗 |
| **零基色 + 反相开** | **(200, 242, 253)** | **"跳成白色"** |
| 暂存分支(反相关) | (127, 80, 27) | 正常的橙色负片 |

**根因(所见≠所存的状态解耦)**:

1. `doAutoColor` 反相后设置了新窗口,但**从不更新画布的 `currentBaseDensity`**——画布继续用上一次代理头里的旧基色渲染,而数据库已被 `analyze_proxy_base_color` 写入新基色(改过片门后两者差异显著)。反相后的"正常"其实是旧基色的偶然效果;任何后来的代理重载(重开/分辨率升级/切帧回来)提交新基色后就是另一种观感。
2. 反相流程先打开 `proxyHasAnalyzedBase` 旗标(8795),基色要等代理重载(8802)才随头提交——旗标与基色解耦的窗口内,反转分支可能以零/旧基色渲染,即零基色变体的"白色"。
3. `selectImage` 不复位 `proxyHasAnalyzedBase`/`currentBaseDensity`,跨帧残留扩大上述窗口。
4. 保存范围/重置后没有主动重显代理,重准备期间画布可能停留在空帧(工作区灰 #dadada,即截图的"图片消失")。

**修复**(47d790a 之后同日落地):

- Rust:`AutoColorLimits` 新增 `base_density` 字段,`analyze_proxy_density_limits` 返回分析实际使用的基色(与持久化状态、下一个代理头一致)。
- JS:`doAutoColor` 立即 `currentBaseDensity = limits.base_density`——反相完成瞬间画布即等于持久化状态,结构性消灭"反相后 ≠ 重开"。
- JS:`selectImage` 复位 `proxyHasAnalyzedBase` 与 `currentBaseDensity`(由新代理头重新提交,杜绝跨帧泄漏)。
- JS:保存范围与重置完成后主动 `ensureProxyDisplayed`(非阻塞,prepare 优先),画布在过渡期保持有图。

**护栏**:verify-density-contract 新增四条静态断言(AutoColorLimits 必须带 base_density、doAutoColor 必须立即提交、selectImage 必须复位旗标与基色)。全量回归 cargo 287 / node 19 / verify 全过。

**待用户复测**:①重开已反相帧应与反相完成时观感一致;②保存范围/重置后画布不应消失;③若仍有短暂异常,运行 `cargo test --lib washed_open_state_diagnostic -- --ignored && node scripts/diagnose-washed-open-state.cjs`,用最新数据库状态对拍变体表即可定位残余输入。注意:反相后的观感本身(=持久化状态渲染)如仍不满意,那是分析(片门下基色/窗口)的调校问题,不再是显示一致性缺陷。
