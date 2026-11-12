---
id: import-formats-and-library
track: user-manual
trackOrder: 2
chapterOrder: 2
kicker: "02 / 工作台手册"
title: "格式支持、导入入口与胶卷档案管理"
subtitle: "全画幅 RAW、Flextight .fff 与扫描 TIFF 支持，按卷与散张导入的实际流程，以及丢失文件的重关联"
readingTime: 9
inApp: true
sinceVersion: "v1.1.0"
aliases: ["import-and-rolls"]
---

## 输入格式支持矩阵与解码规格 {#format-matrix}

NexFilm Engine 面向胶片数字化全流程构建，旨在打破相机数码翻拍与传统专业扫描仪之间的文件格式壁垒。软件底层集成了基于原生 Rust 优化的高性能色彩与图像解码管线，能够直接读取从主流数码相机原始 RAW 到工业级鼓式扫描仪的无压缩图像文件。

以下为当前版本完整支持的输入文件格式规格矩阵：

<table class="doc-table">
    <thead>
        <tr><th>格式类别</th><th>文件扩展名</th><th>色彩编码与位深</th><th>解码特性与暗房建议</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>相机原始 RAW</strong></td><td><code>.dng</code>, <code>.nef</code>, <code>.cr2</code>, <code>.cr3</code>, <code>.arw</code>, <code>.raf</code>, <code>.rw2</code>, <code>.orf</code>, <code>.pef</code>, <code>.3fr</code>, <code>.iiq</code> 等主流单反/无反格式</td><td>12-bit / 14-bit / 16-bit 线性传感器原始拜耳（Bayer）马赛克数据</td><td>相机翻拍的最佳输入源。绕过机内白平衡与色调曲线，保留完整的传感器动态范围与线性透射率数据，为高精度去色罩提供充沛的曝光宽容度。</td></tr>
        <tr><td><strong>哈苏 Flextight / Imacon</strong></td><td><code>.fff</code></td><td>16-bit 线性扫描仪原始传感器数据</td><td>自 <strong>v1.1.0</strong> 起，NexFilm 重构了解码判定逻辑，直接按扫描仪流式线性光学密度解算，不再强依赖 FlexColor 的专有设备识别字符串，解决了将扫描仪文件误判为相机 RAW 的历史问题。</td></tr>
        <tr><td><strong>工业扫描仪 TIFF</strong></td><td><code>.tif</code>, <code>.tiff</code></td><td>16-bit / 8-bit 未压缩或 LZW 压缩格式</td><td>支持 Epson、Noritsu、Frontier、Plustek 等扫描仪输出的单通道黑白与三通道彩色正片 TIFF。可以按行流式解码百兆乃至 GB 级高分辨率大画幅图像，内存占用保持平稳。</td></tr>
        <tr><td><strong>标准位图格式</strong></td><td><code>.png</code>, <code>.jpg</code>, <code>.jpeg</code></td><td>8-bit / 16-bit 标准 sRGB 或 Display P3 色彩空间</td><td>支持快速读取预览与测试。受限于 8-bit JPEG 严重的动态范围压缩与片基色罩截断，无法达到 RAW 级别的自动物理校色精度，仅建议用于快速验证构图。</td></tr>
    </tbody>
</table>

> [!WARNING] Nikon Z8 / Z9 高效率压缩 NEF 格式限制
> 当前内置解码库暂不支持 Nikon Z8 与 Z9 机身中的 `HE`（高效率）以及 `HE*`（高效率无损压缩）NEF 编码文件。若使用上述机型翻拍，请在相机机内设置中将 RAW 记录格式调整为标准的“无损压缩 RAW”，或在导入前使用官方免费的 Adobe DNG Converter 批量转换为标准线性 DNG 格式。

针对 8-bit JPEG 格式，由于其在扫描或拍摄阶段已经被相机或扫描软件施加了非线性的 sRGB 色调曲线并裁剪了暗部与高光，导致片基色罩与微弱暗部信号相互混淆。在 NexFilm 中处理此类文件时，若自动反相效果不理想，建议通过“印片灯光”面板手动微调三通道曝光增益进行补偿。

## 有界代理架构与低内存极速索引 {#bounded-proxy-architecture}

处理整卷 36 张 6000 万像素相机 RAW 文件，或者 4×5 大画幅高分辨率 TIFF 扫描件时，传统修图软件往往会在导入阶段启动全局全图解码。内存很快被占满，CPU 持续满载，界面也会卡上一段时间。

NexFilm 重构了这种传统模型，全面采用**双阶有界代理架构（Two-Stage Bounded Proxy Architecture）**：

<div class="doc-callout">
    <div class="doc-callout-title">“有界代理”是什么</div>
    <div class="doc-callout-body">可以把它理解成一张专门给屏幕操作用的工作副本：长边被限制在 <code>2560 px</code>，足以支撑 4K 屏上的放大、平移和实时调色，但不会把整张高分辨率原片一直塞在显存里。真正导出时，NexFilm 会重新读取原始文件做全分辨率计算，所以代理图只影响工作台里的预览速度，不决定最终画质。</div>
</div>

1. **第一阶段：轻量元数据快速注册与内嵌预览提取**
   用户选定扫描文件并点击导入时，引擎仅扫描文件头，以很低的解算开销提取文件中已预先生成的内嵌缩略图（Embedded Preview）与 Exif 拍摄元数据，并在本地数据库中建立轻量索引。几十张到上百张照片通常能在几秒内完成入库，在“图库”中呈现完备网格，应用内存占用也保持在较低水平。
2. **第二阶段：进入工作台时按需生成有界高精度代理**
   只有当用户双击某张底片进入“工作台”时，后台计算引擎才按需唤起全管线解码器，针对该画面计算长边限定为 `2560 px` 的高质量工作代理图（Bounded Proxy）。该代理图能够充分满足 4K 乃至 5K 显示器上的实时交互精修需求，同时将实时调色计算的纹理显存牢牢锁定在极低水平。

在最终执行“导出”时，引擎会跳过代理图，重新从磁盘流式读取原始源文件，以最高位深和全分辨率执行完整的调色与锐化矩阵计算，让成片保留原始扫描的细节。

## 导入对话框的三种入口与元数据注入 {#tri-state-import}

为了适应摄影师在不同场景下的摄录习惯，NexFilm 在 **Import Method** 对话框里提供了三种入口。它们看起来并列，实际用途并不一样：前两种负责把新文件带进工作区，第三种只是重新打开已经归档的胶卷。

<div class="doc-ui-demo doc-ui-demo-import">
  <div class="doc-ui-demo-topbar">
    <span class="doc-ui-demo-window-dots" aria-hidden="true"><i></i><i></i><i></i></span>
    <span class="doc-ui-demo-window-title">NEXFILM / IMPORT METHOD</span>
    <span class="doc-ui-demo-badge">交互示意</span>
  </div>
  <input class="doc-ui-demo-radio" type="radio" name="doc-import-mode-zh" id="doc-import-roll-zh" checked>
  <input class="doc-ui-demo-radio" type="radio" name="doc-import-mode-zh" id="doc-import-loose-zh">
  <input class="doc-ui-demo-radio" type="radio" name="doc-import-mode-zh" id="doc-import-continue-zh">
  <div class="doc-ui-demo-choices">
    <label class="doc-ui-demo-choice" for="doc-import-roll-zh">
      <span class="doc-ui-demo-index">01</span>
      <span class="doc-ui-demo-choice-copy"><strong>按胶卷导入</strong><small>整卷归档，登记相机与胶片</small></span>
      <span class="doc-ui-demo-arrow">→</span>
    </label>
    <label class="doc-ui-demo-choice" for="doc-import-loose-zh">
      <span class="doc-ui-demo-index">02</span>
      <span class="doc-ui-demo-choice-copy"><strong>散张导入</strong><small>单张或多张，不建立胶卷</small></span>
      <span class="doc-ui-demo-arrow">→</span>
    </label>
    <label class="doc-ui-demo-choice" for="doc-import-continue-zh">
      <span class="doc-ui-demo-index">03</span>
      <span class="doc-ui-demo-choice-copy"><strong>继续编辑胶卷</strong><small>打开已有胶卷继续调色</small></span>
      <span class="doc-ui-demo-arrow">→</span>
    </label>
  </div>
  <div class="doc-ui-demo-panels">
    <section class="doc-ui-demo-panel doc-ui-demo-panel-roll" aria-label="按胶卷导入示意">
      <div class="doc-ui-demo-window">
        <div class="doc-ui-demo-window-head"><span>ROLL METADATA</span><span>下一步：选择影像</span></div>
        <div class="doc-ui-demo-fields">
          <div class="doc-ui-demo-field"><span>FORMAT</span><strong>135 (35mm)</strong></div>
          <div class="doc-ui-demo-field"><span>CAMERA</span><strong>Leica M6</strong></div>
          <div class="doc-ui-demo-field"><span>FILM</span><strong>Kodak Portra 400</strong></div>
          <div class="doc-ui-demo-field"><span>DATE</span><strong>2026-11-12</strong></div>
        </div>
        <div class="doc-ui-demo-window-foot"><span>用于整卷批处理与共享白点</span><span class="doc-ui-demo-primary">选择影像 →</span></div>
      </div>
    </section>
    <section class="doc-ui-demo-panel doc-ui-demo-panel-loose" aria-label="散张导入示意">
      <div class="doc-ui-demo-window">
        <div class="doc-ui-demo-window-head"><span>LOOSE IMPORT</span><span>下一步：选择文件</span></div>
        <div class="doc-ui-demo-dropzone"><strong>拖入图像文件</strong><span>RAW / TIFF / PNG / JPEG</span></div>
        <div class="doc-ui-demo-window-foot"><span>不绑定胶卷，直接进入图库</span><span class="doc-ui-demo-primary">选择文件 →</span></div>
      </div>
    </section>
    <section class="doc-ui-demo-panel doc-ui-demo-panel-continue" aria-label="继续编辑胶卷示意">
      <div class="doc-ui-demo-window">
        <div class="doc-ui-demo-window-head"><span>CONTINUE EDITING ROLL</span><span>选择胶卷档案</span></div>
        <div class="doc-ui-demo-select"><span>ROLL ARCHIVE</span><strong>Portra 400 · 2026-10-18</strong><b>⌄</b></div>
        <p class="doc-ui-demo-note">这个入口只打开已归档胶卷，不会向卷内新增扫描文件。</p>
        <div class="doc-ui-demo-window-foot"><span>回到原胶卷继续编辑</span><span class="doc-ui-demo-primary">打开 →</span></div>
      </div>
    </section>
  </div>
  <p class="doc-ui-demo-caption">点击上方任一模式查看下一步。此为界面示意，不会执行真实导入。</p>
</div>

### 1. 按卷导入（Import as Roll）
此模式是暗房标准化作业的首选流程，专为按卷冲洗、翻拍或扫描的成套素材量身定制：
- 打开导入对话框，选择“按胶卷导入”。
- **全画幅谱系选择**：系统内置覆盖主流银盐格式的画幅预设：
  - 135 全画幅（35mm）、135 半格（Half-frame）、APS；
  - 120 中画幅全系列：`120 (645)`、`120 (6x6)`、`120 (6x7)`、`120 (6x9)`、`120 (6x12)`、`120 (6x17)`；
  - 大画幅座机：`4x5`、`5x7`、`8x10`。
- **设备与乳剂档案注入**：选择或输入拍摄机身（如 Leica M6、Hasselblad 503CW）与胶卷型号（内置柯达 Gold 200、Portra 系列、富士 C200、伊尔福 HP5 Plus、电影卷 5219 等全系参数，并支持添加自定义型号）。
- **时间与胶卷备注（Roll Notes）**：录入拍摄日期与自由文本备注（例如“第 03 卷、E100 增感一档、黄昏棚拍”）。这些信息将成为后续批量重命名模板与导出 Exif 标签的物理源。

### 2. 散张导入
适用于单张扫描排查、临时镜头测试片或不希望归入特定胶卷项目的散落底片：
- 点击导入对话框中的“散张导入”，或直接将一个或多个图像文件拖拽至主窗口中央。
- 散张导入的画面独立存在于图库中，拥有完备的单帧物理反相与微调能力，但默认不强制绑定胶卷元数据，也不参与整卷物理标定。
- 软件具备强大的重复导入识别机制：当拖入的文件路径已存在于当前项目时，系统会自动跳过重复项，防止索引冗余。

### 3. 继续编辑已有胶卷（Continue Editing Roll）
如果一卷底片之前已经归档，现在想回到原来的胶卷里继续调色，可以在 **Import Method** 中选择 **Continue Editing Roll**。接下来从 **Roll Archive** 下拉列表中选中目标胶卷，点击 **Open & Edit**，软件会把这一卷重新载入图库与底片条。

这里需要说明清楚：**Continue Editing Roll 只负责打开已有胶卷，不会向卷内追加新文件。** 如果后来补扫了几张同卷底片，目前需要通过按胶卷导入重新建立一卷，再分别管理两批扫描。文档不会把没有出现在界面里的追加操作写成现有功能。

## 历史胶卷记录多维筛选与归档管理 {#rolls-management}

点击顶栏的“历史胶卷记录”即可进入本地数字胶片档案库。随着拍摄项目的日积月累，这里将清晰归档用户处理过的每一卷底片资产。

Rolls 视图的核心功能包括：
- **左侧多维组合筛选器**：左侧面板支持按“画幅”（全画幅谱系分类）、“相机机身”以及“拍摄日期”进行交集组合筛选。面板顶部实时更新显示当前筛选条件下的确切胶卷数与画面总数，点击 **Clear** 按钮可一键复位筛选。
- **胶卷卡片摘要矩阵**：右侧以画册式网格陈列胶卷卡片，直观呈现每卷的封面缩略图、总张数、画幅标签、相机、胶卷型号与创建日期。
- **胶卷详情与操作中枢**：选中某卷进入胶卷内容视图后，可展开全卷底片微缩阵列，并执行以下关键动作：
  1. **提升至图库（Promote to Library / 加入图库）**：将该归档卷的所有画面载入当前活跃图库，方便重新进入工作台精修。
  2. **编辑胶卷信息（Edit Info）**：支持随时修正或补充相机型号、胶卷名称、日期以及详细的胶卷备注，修改后将自动同步至未来导出的 Exif 元数据中。
  3. **导出接触印样**：一键生成带有物理齿孔、片边字码与卷标头的数字暗房接触印样。
  4. **删除胶卷记录**：点击删除时，系统会弹出双重安全选择：仅从 NexFilm 本地数据库中注销该卷索引，或者在注销索引的同时将磁盘上的原始文件移入操作系统废纸篓。选择后者时，若某些源片同时被其他项目引用，系统会保留原文件，降低误删风险。

## 源文件离线与定位文件断链修复 {#locate-missing-files}

NexFilm 始终践行轻量化与非破坏性设计原则：**软件不会复制或挪动用户的原始素材文件**。在数据库 `nexfilm_user.db` 中，仅记录了源文件在本地文件系统中的绝对物理路径。

当用户在操作系统中重命名了存放底片的文件夹、移动了目录结构，或者拔出了外置移动硬盘时，工作台与图库中对应的画面会进入“离线脱机状态”：
- 画面卡片上会出现明显的警告图标，提示“文件路径未找到”。
- 得益于有界代理机制，画面的本地缓存缩略图依然可以正常浏览，但无法进行全分辨率导出或重新计算反相。

**断链修复流程（Locate File）**：
1. 选中处于脱机状态的画面，点击界面上弹出的 **定位文件** 操作按钮。
2. 在系统文件浏览窗口中，定位至该文件当前所在的新路径并点击确认。
3. NexFilm 底层引擎会自动校正该记录的绝对路径，并智能探测同目录下其他同样处于脱机状态的相邻画面。若文件名与序列匹配，引擎将自动批量修复整卷的路径关联，无需用户逐张手动重新查找。

通过严密的格式矩阵支持、零内存压力的代理架构以及稳健的档案管理机制，NexFilm 为严肃胶片摄影师构筑了一套值得长期信赖的数字化资产基石。
