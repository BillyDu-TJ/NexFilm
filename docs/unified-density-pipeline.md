# 统一密度管线（本轮改动记录）

本轮把「输入类型选择整套密度数学」改为「输入类型只选择输入域」，并给出逐张样本的量化对照。

## 一、统一后的实际调用链

```
文件 → 解码器选择（后缀/容器，只决定解码路径）
     → resolve_input_domain(path, scanner_profile)      ← 唯一允许分叉的一步
        1) 内嵌 ICC → 基色 + 传递曲线（Verified）
        2) 扫描仪 Input Profile / 扫描仪容器记录（Flextight plist 的
           RGBProfile + Gamma，Verified；读不到则用文档默认值 + estimated）
        3) RAW/DNG 元数据（CameraNative + CameraRaw；LinearRaw 扫描仪 DNG 单独识别）
        4) 扫描仪机型识别（Documented default）
        5) 兜底按 sRGB 解释并标 estimated
     → 统一工作域：ProPhoto 线性 f32（不经 u16 线性 sRGB 中转）
     → 统一密度域：逐通道减片基 → D = -log10(T) → 共享窗口原点
       （channel_offsets 恒为 0）
     → 通道跨度响应：三通道跨度比 > 1.15 起按各自测量的跨度分配窗口宽度
       （增益上限 1.35×；跨度比 > 2.0 视为上游截断通道，上限放宽到 3×）
     → 显示映射：ProPhoto→sRGB → gamma/LUT
```

三条业务路径的差别只在**输入端与片基来源**，密度数学完全一致：

| 场景 | 片基来源 | 显示窗口 |
| --- | --- | --- |
| 按卷导入 + 完整标定 | 采样片基 + 采样片头（物理锚点） | `offset + span[i] × highlight`，逐帧 offset 只做整体位移 |
| 按卷导入 + 未标定 | 该帧 Film Area 内的片基估计（片边窄带，其次膜面低密度尾部） | 共享原点 + 逐通道跨度响应 |
| Loose Import | 同上 | 同上（与上一条必须逐像素一致） |

输入域记录随帧持久化在 `PipelineProcessingReport::input_domain`，并出现在技术报告中。

## 二、改动清单

| 文件 | 位置 | 改动与原因 |
| --- | --- | --- |
| `src/app_state.rs` | `InputDomainRecord` 及配套枚举、`PipelineProcessingReport::input_domain` | 新增显式输入域数据结构（primaries / transfer / linear 或 display-referred / 归一 / 来源 / 置信度 / estimated / detail） |
| `src/app_state.rs`、`src/commands.rs`、`ui/main.js` | 显示响应曲线（`DisplayResponse`、`RenderMapping::display_response*`、着色器 `u_display_response*`） | **已实现后又按用户要求整体移除**：它改掉了相机 RAW 原本已认可的观感，且属于「没有任何意义」的额外功能。现在显示映射只有 ProPhoto→sRGB 一步，不存在逐通道响应 |
| `src/commands.rs` | `resolve_input_domain`、`scanner_fff_container_input`、`scanner_fff_plist_value`、`dng_has_uncompressed_linear_raw_rgb_subifd` | 输入域显式解析（内嵌 ICC → 容器记录 → RAW 元数据 → 机型 → 兜底 sRGB） |
| `src/commands.rs` | `linearize_scanner_fff_with_input` | Flextight/Imacon FFF 的 gamma 与基色改为由容器记录决定，读不到才用文档默认值（1.8/sRGB）并标 estimated |
| `src/commands.rs` | 删除 `uses_scan_density_recipe`；`mark_loose_smart_auto_compatibility` → `clear_retired_legacy_domain` | 输入类别不再选择密度数学；`legacy_linear_srgb` 标记只清不写 |
| `src/commands.rs` | `estimate_film_base_f32`、`validate_film_base_candidate`、`film_base_band_agrees`、`collect_film_area_band_rgb32` | 片基采样优先级：片边/色罩窄带 → Film Area 内低密度尾部 → 画面内最亮分位（降权）；质量门限与回退原因 |
| `src/commands.rs` | `analyze_proxy_base_color` | 使用带质量门限的片基估计；不可用时记录 `missing_film_base_reference` 等回退原因 |
| `src/commands.rs` | `share_smart_auto_density_scale` | 内容对齐偏移上限 0.60 → 0.20（**仅在无可用片基时使用**） |
| `src/commands.rs` | `prepare_content_render_limits`、`render_shader_equivalent_core` | 有片基时：共享窗口原点 + **零密度偏移** + 有界通道跨度响应（`measure_content_channel_spans` → `channel_response_from_spans` → `apply_content_channel_response`，死区 1.15×、满额 1.35×、上限 1.35×，上游截断 3×）；无片基时才用内容对齐（上限 0.20） |
| `src/app_state.rs` | `ChannelResponseRecord`、`PipelineProcessingReport::channel_response` | 记录每帧测得的通道跨度、失衡比与实际增益，供技术报告与排查使用（`None`＝该路由不测量内容窗口） |
| `src/commands.rs` | `density_pipeline_sample_report` | 手工对照 harness 现在同时渲染「共享窗口」与「通道响应补偿」两版，并打印逐像素差异数，作为「正常照片零影响」的回归证据 |
| `src/commands.rs` | `compute_content_limits_f32_with_bounds` | 窗口端点排除 ≥0.995 的样本：拼接白边/灯板/高光溢出不再决定端点（§七.7） |
| `src/persistence.rs` | `migrate_retired_density_recipe` | 一次性迁移：清除 `legacy_linear_srgb`，重置 `compatibility_base`，清空 base_color 与已渲染缩略图（LegacyV1 老工程不动），幂等并记录日志 |

## 三、量化对照（`cargo test --lib density_pipeline_sample_report -- --ignored --nocapture`）

几何与片基按同一份自动分析（自动片门 + 片基估计）分别跑 v1.0.2 配方与统一管线；数值为渲染后整幅通道均值比。

| 样本 | 输入域（解析结果） | 片基来源 | v1.0.2 R/G · B/G | 统一 R/G · B/G | 说明 |
| --- | --- | --- | --- | --- | --- |
| `哈苏fff\任务 _1233.fff` | CameraNative / CameraRaw（Declared） | Film Area 低密度尾部 | 0.583 · 1.001 | 0.963 · 1.086 | 相机 RAW，已回到「无逐通道响应」的观感；蓝通道满量程 33.3% |
| `哈苏fff\任务 _1343.fff` | CameraNative / CameraRaw | 片边窄带 | 0.980 · 0.970 | 0.991 · 0.969 | 画面本身约 45% 贴顶，两条路径一致 |
| `哈苏fff\无法反相.fff` | AdobeRgb1998 / Gamma2.0（容器记录） | 画面内最亮分位 | 0.894 · 1.037 | 0.806 · 1.046 | 输入域来自 FFF 容器记录 |
| `哈苏fff\1 001-可以反相.fff` | ScannerDevice / Gamma2.0（容器记录） | 片边窄带 | 1.008 · 0.996 | 0.976 · 1.019 | 各通道余量正常 |
| `哈苏fff\任务 _0866.fff` | CameraNative / CameraRaw | 片边窄带 | 0.947 · 1.054 | 0.889 · 0.931 | 蓝通道零值 14.3% |
| `lr合并\_DSC7569-Pano.tif` | AdobeRgb1998 / Gamma2.2（内嵌 ICC） | 片边窄带 | 0.848 · 1.907 | 0.696 · 1.288 | 失衡比 3.35，增益 0.501/1.080/1.678；红通道零值 78.8% → 9.3% |
| `lr合并\_DSC7571-Pano.tif` | 同上 | 画面内最亮分位 | 1.177 · 1.255 | 0.372 · 1.325 | 失衡比 2.12，增益 0.831/1.020/1.231；改善有限，见第五节第 1、2 条 |
| `lr合并\_DSC7583-Pano.tif` | 同上 | 片边窄带 | 0.571 · 2.324 | 0.557 · 1.249 | 失衡比 4.26，增益 0.414/1.097/1.765；红通道零值 81.3% → 5.9% |
| `尼康扫描仪tiff\5.3-1.tif` | AdobeRgb1998 / ICC | 片边窄带 | 1.030 · 0.715 | 0.999 · 0.951 | 无系统偏色；通道比高于 v1.0.2 基准 |
| `精益黑白\raw0002.dng` | Srgb / Linear（LinearRaw 扫描仪 DNG） | 尾部 | 1.001 · 1.018 | 0.944 · 1.139 | 与 v1.0.2 同量级 |
| `尼康nef_raw\_DSC7333.NEF` | CameraNative / CameraRaw | 片边窄带 | 0.975 · 0.972 | 0.826 · 0.944 | 相机 RAW，红通道满量程 23.5% |
| `诺日士jpg\000000070001.jpg` | Srgb / sRGB（estimated） | 片边窄带 | 1.715 · 0.916 | 0.802 · 1.174 | 与 v1.0.2 差异大（老路径 R/G 1.71 明显偏红） |

### 3.1 散片默认几何（无 Film Area）

目标文档引用的 5.3-1 基准（R/G 1.028、B/G 0.774）是散片默认几何口径。上一版文档里的同几何表是**在已被移除的显示曲线下**测得的，不再代表当前代码；需要时用 `NEXFILM_REPORT_NO_AREA=1` 重跑，不要沿用旧数字。

### 3.2 几何稳定性（新发现，需后续处理）

自动片门检测对解码尺寸不稳定：同一个 `_DSC7571-Pano.tif` 在 1024px 得到覆盖大半画面的四边形、1200px 得到 fallback（无片门）、1600px 得到一个仅覆盖右下角的退化四边形。片基与窗口随几何变化，渲染结果随之变化。生产流程用的是固定导入缩略图（几何稳定），但用户确认的 Film Area 质量仍是散片结果的主要不确定性来源。

## 四、迁移与失效

- `migrate_retired_density_recipe` 在 `init_db` 中执行：凡带 `legacy_linear_srgb` 且不是 LegacyV1 的记录，一律改到 `linear_prophoto_estimate`，`compatibility_base` 重置为 `unresolved`，`base_color` 与 `rendered_thumb_base64` 清空，并记录 `retired_density_recipe_migrated` 回退原因；重复执行不再命中。
- LegacyV1 老工程（`contract = legacy_v1`）完全不动。
- `is_smart_auto_compatibility` 保留为只读支持：新代码不再写这个标记，迁移后不再有帧命中它。

## 五、仍未解决的问题

1. **宽色域拼接 TIFF：已由有界通道跨度响应解决（`_DSC7583` / `_DSC7569` 达标）**。这三个 `lr合并\*Pano.tif` 的红通道在文件里只剩绿通道 1/4–2/3 的密度跨度，一个三通道共用的窗口必然把红压在半程以下（R/G 0.018–0.093）。现在保留共用窗口原点（片基仍是中性参考），只让每个通道按自己测得的跨度分配窗口宽度：`g_i = clamp(span_i / luma(span), 1/上限, 上限)`（上限通常 1.35，跨度比超过 2.0 判为上游截断通道时放宽到 3），死区 1.15×、1.35× 处取满，等价于把窗口按响应做 `density / gain`，片基（密度 0）在三个通道仍落在同一显示值。跨度用**逐通道 2%–98%**（`measure_content_channel_spans`）测量：窗口端点仍保持同像素，而响应测量必须看到"某通道的极值不与画面亮度对齐"的情况，否则 `_DSC7583` 这种红通道塌陷会被低估（同像素端点测得 3.70×，逐通道测得 4.26×）。`_DSC7583` 现为天空浅蓝带白云、混凝土近中性、草地暖褐，与 LR+NLP 参考一致。**与已被移除的「逐通道显示响应曲线」的区别**：那一版是显示阶段额外加的一条逐通道曲线，对所有帧无条件生效，改掉了相机 RAW 已认可的观感；本版按死区斜坡生效，且原点共享、片基仍中性。正常素材（哈苏 FFF ×5、尼康扫描仪 TIFF ×4、Epson／精益 DNG ×6、尼康 NEF ×3、诺日士 JPEG ×3）实测失衡比 1.03–1.49，增益上限 1.35×，属于有界修正而非自由缩放。
2. `_DSC7571-Pano.tif` 只改善了一部分（R/G 0.093 → 0.372）：它在工作域里的失衡比只有 2.12，红通道的缺陷更多体现在**电平**而不是跨度（扣片基后的中位密度 R 0.196 / G 0.337 / B 0.580），而电平对齐正是被否决的灰世界操作。它同时也是这一批里唯一在离线 harness 里**没有检出片基**的一帧（几何 fallback），片基退化为「画面内最亮分位」（置信度 0.499）。要彻底解决它需要更稳健的片基/几何（第三节 3.2 已记录自动片门对该文件不稳定），或对"电平型"失衡另立判据。
3. 通道贴顶：`任务 _1233.fff` 蓝 33.3%、`_DSC7333.NEF` 红 23.5%、`任务 _1343.fff` 约 45%（该帧在 v1.0.2 下同样是 45%）。这些帧的失衡比都在 1.4 以下，通道响应补偿按设计不介入；端点与画面分布的固有问题仍需 H-D 反演/胶片响应模型（§九列为后续版本）。
4. 扫描仪 TIFF 的 B/G 略低于同几何的 v1.0.2（5.3-1：0.685 对 0.715，约 −4%），R/G 略高（1.123 对 1.030）。文档基准 0.774 是在「散片、无 Film Area」几何下测得的，与本表的自动片门几何不可直接比较；两套口径的数值都在上表可复现。
5. 扫描仪 FFF 的容器记录只解析 `RGBProfile` / `Gamma`；FlexColor `ImageCorrection` 的阴影/高光端点未使用，记录读不到时仍是文档默认 1.8 + sRGB（标 estimated）。
6. 卷级 Roll Anchored 路由不受本轮影响：端点来自采样锚点，显示窗口宽度本就 ∝ 每通道物理跨度（`density_high[i] = offset[i] + span[i] × highlight`），等价于已经按物理响应做过逐通道斜率归一，因此不再叠加内容增益。同理，`prepare_content_render_limits` 的 `calibrated_span` 分支保持原样。Loose Import 与未标定 Roll 走的是同一条内容窗口分支，两者都在本轮覆盖范围内。
7. 内容对齐偏移上限收紧到 0.20 后，无可用片基的帧（`无法反相.fff`、`5.3-4.tif`、`_DSC7571-Pano.tif`）走画面内容对齐，仍可能有轻微残留偏色。

## 六、验证方式

```
cargo test
cargo check --all-targets
cargo fmt -- --check
cargo test --lib density_pipeline_sample_report -- --ignored --nocapture
node scripts/verify-density-contract.cjs  （以及 scripts 下其余契约脚本、ui/settings-copy.test.js）
```

新增回归测试：整幅偏色响应（§七.1）、主导色场景不反向（§七.2）、同卷通道比一致（§七.3）、输入类别/后缀/provenance 不改变密度数学（§七.4、§七.5）、拼接白边不影响片基与窗口（§七.7）、片基质量门限与回退（§七.8）、输入类别不再写入 legacy 标记、迁移幂等。原「LegacyV1 不受显示曲线影响」测试随该功能一起删除。
通道响应补偿的回归测试：`balanced_channel_response_keeps_the_shared_window`（通道均衡时窗口逐值不变）、`compressed_channel_recovers_its_display_span`（压缩通道重获显示范围，且片基在三个通道上仍落在同一显示值）、`channel_response_compensation_stays_bounded`（增益上限与最小跨度生效）。
`density_pipeline_sample_report`（`--ignored`）现在为每个样本同时渲染「共享窗口」与「通道响应补偿」两版并打印差异像素数：正常素材必须为 `0 of N`，三个拼接 TIFF 会打印实际增益与失衡比。
`merged_panorama_wide_gamut_diagnostic`（§七.6，`--ignored`）现在**只打印**三个拼接 TIFF 的蓝通道零值/满量程占比，因为该验收项当前未达标，测试不再假装通过。
§七.10（前端标志位 / WebGL / CPU / 缩略图 / 导出 / 批处理一致）由 `scripts/verify-webgl-shaders.cjs` 的着色器编译契约、`scripts/verify-density-contract.cjs` 的「必须不含逐通道响应」断言、以及 CPU/导出共用 `render_shader_equivalent_core` 三项保证；仍无跨端逐像素断言。

## 八、本轮修复（三条业务路径的偏色）

问题现象：场景 2（未标定卷）与场景 3（散片）在这一批真实素材上都有肉眼可见的偏色，而且**同一卷不同帧的颜色各不相同**；场景 1（完整标定卷）表现正常。三处根因与修复：

1. **窗口原点被逐通道平移**（`prepare_content_render_limits_with_spans`）：有片基的分支一度改用了带内容对齐偏移的 `share_smart_auto_density_scale`（偏移上限 0.20 D），把「密度 0 = 片基」的显示值按通道拆开——片基本身被染色，而偏移量来自画面自己，所以偏色随画面内容变化。现在恢复 `share_smart_auto_density_scale_without_offsets`：有片基时 `channel_offsets` 恒为 0。
2. **逐通道跨度响应的死区过宽**（`CHANNEL_RESPONSE_BALANCED_RATIO`）：真实扫描的三层响应差一般在 1.1~1.8×，旧死区 1.6× 使绝大多数帧从不补偿，三通道只能共用一个跨度，短通道永远到不了白点。死区降为 1.15×、满额 1.35×、上限 1.35×（上游截断通道 3×）。该补偿按 `density / gain` 缩放且原点是共享的，片基仍落在同一显示值。
3. **片基判据太松**（`FILM_BASE_BAND_AGREEMENT`）：片边窄带与膜面低密度尾部原本允许相差 0.25 D 就算互相印证，而 0.2 D 的片基误差本身就是一次可见偏色。现在只允许窄带**不得比尾部更暗超过 0.05 D**（片基是底片密度最低处，画面内不可能比它更透），方向单向收紧；一旦越界就退回膜面尾部候选。
4. **老工程的一次性重算**：帧的显示端点随帧持久化，所以用旧规则分析过的场景 2/3 帧不会自己更新。`PipelineProcessingReport::analysis_window_rule` 记录端点是用哪一版窗口规则推导的（旧记录反序列化为 0），`frame_needs_window_reanalysis` 让这类帧在下次自动反相时重新推导一次；完整标定卷（`RollAnchoredDirectInvert`）与采集校正（`CaptureCorrectedV11`）不受影响，端点仍只来自采样锚点。用户对这一类帧执行一次「自动反相」即可，之后可用复制/粘贴把参数广播给整卷。

回归证据（`cargo test --lib` 224 项、`scripts/verify-*.cjs` 11 项、`cargo fmt --check` 全过）之外，新增断言把「曝光/高光/阴影/全局 D-Min/D-Max 不得引入偏色」这条钉住：

- `tone_controls_never_tint_a_neutral_print`：主曝光、高光、阴影、全局 D-Min/D-Max 微调单独生效时，中性画面的三通道最大差 ≤ 4/65535，且每个控制与基线完全一致；
- `span_corrected_frame_keeps_its_neutral_axis`、`compressed_channel_recovers_its_display_span`：跨度补偿生效时片基与中性轴保持中性（合成样本 R/G 从 0.17 回到 1.00，片基显示值三通道完全相等）；
- `balanced_channel_response_keeps_the_shared_window`：三通道跨度一致的健康帧，窗口逐值不变。

三路径对照工具：`cargo test --lib three_pipeline_parity_report -- --ignored --nocapture`。它读取本机 `nexfilm_user.db` 中用户已确认的 Film Area 与 `rolls.json` 中已采样的片基/片头，对同一张照片分别渲染 P1（完整标定）、P2（未标定卷）、P3（散片）与 v1.0.2，断言 P2/P3 逐像素一致、片基显示值三通道相等，并把图片写到 `target/three-pipeline/`。
