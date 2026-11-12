---
id: export-and-contact-sheets
track: user-manual
trackOrder: 2
chapterOrder: 6
kicker: "02 / 工作台手册"
title: "母带级导出、色彩空间管理与接触印样"
subtitle: "七种导出格式的取舍、七种输出色彩空间与 ICC 匹配、批量令牌重命名，以及物理拟真接触印样"
readingTime: 13
inApp: true
sinceVersion: "v1.1.0"
---

## 七种导出格式的对比与选型 {#format-decision-matrix}

在暗房工作流中，导出是把工作台里的非破坏性调色参数与像素矩阵最终固化的关键工序。NexFilm 没有把所有输出都塞进同一个设定，而是针对网页展示、出版印刷、长期档案和影视后期准备了不同的格式与色彩空间。

<div class="doc-callout">
    <div class="doc-callout-title">线性文件、ICC 与 DNG 分别是什么</div>
    <div class="doc-callout-body"><p><strong>线性 TIFF</strong> 指的是不套显示伽马的像素文件，数值更接近传感器或扫描仪记录到的光强，适合交给 Resolve、Nuke 这类后期软件继续处理。<strong>ICC 配置文件</strong>则是在文件里注明“这些数字属于哪个色彩空间”，让支持色彩管理的软件知道该如何显示。</p><p><strong>Linear DNG</strong> 是 NexFilm 反相后的成品，只是换成了 Adobe 能识别的线性 DNG 容器；<strong>Camera RAW DNG</strong> 则是把相机原始马赛克数据原样打包，里面没有反相，也没有调色。</p></div>
</div>

下表系统梳理了各导出格式的技术规格、数据线性度与典型应用场景：

<table class="doc-table">
    <thead>
        <tr><th>导出格式名称</th><th>位深度与编码</th><th>数据线性度</th><th>是否内嵌调色结果</th><th>下游推荐适用软件</th><th>文件体积与画质权衡</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>JPEG · 8-bit</strong></td><td>8-bit 整型，有损压缩</td><td>非线性显示空间（套用色彩空间伽马）</td><td>是（完整包含印片光、曲线、细节与相纸 LUT）</td><td>全平台浏览器、移动端相册、社交网络与即时通讯软件。</td><td>极小体积（通常 2–8 MB），质量滑块提供 40–100（默认 92），适合日常快速分发与客户样片预览。</td></tr>
        <tr><td><strong>PNG · 16-bit</strong></td><td>16-bit 整型，无损压缩</td><td>非线性显示空间</td><td>是（完整包含所有调色结果）</td><td>专业图片查看器、网页归档与印前排版软件。</td><td>中等体积（约 30–70 MB），无损保留 16 位灰阶阶调，避开 JPEG 的 DCT 块状压缩伪影。</td></tr>
        <tr><td><strong>TIFF · 8-bit</strong></td><td>8-bit 整型，无损压缩</td><td>非线性显示空间</td><td>是（完整包含所有调色结果）</td><td>传统印前制版系统、桌面出版与旧版图像处理工具。</td><td>适中体积，专为必须使用 TIFF 容器但受限于系统内存的旧版下游管线设计。</td></tr>
        <tr><td><strong>TIFF · 16-bit</strong></td><td>16-bit 整型，无损压缩</td><td>非线性显示空间</td><td>是（完整包含所有调色结果）</td><td>Adobe Photoshop、Lightroom、Capture One 与艺术微喷打印软件。</td><td>较大体积（80–180 MB），专业母带交付与高质量画册印刷首选，保留最充沛的后期分层微调余量。</td></tr>
        <tr><td><strong>TIFF · 16-bit linear</strong></td><td>16-bit 整型，无损压缩</td><td><strong>纯物理线性光空间</strong>（不套显示 Gamma）</td><td>是（包含反相与印片光，但剥离显示曲线）</td><td>DaVinci Resolve、Autodesk Flame、Nuke 影视合成软件。</td><td>专业影视交付格式。内嵌线性 ICC 配置文件，像素数值与物理透射光强成正比，供调色师在宽色域中继续精修。</td></tr>
        <tr><td><strong>DNG · linear</strong></td><td>16-bit 线性浮点/整型 DNG</td><td><strong>纯物理线性光空间</strong></td><td>是（写成 LinearRaw 规范 DNG 容器）</td><td>Lightroom、Adobe Camera Raw、Photoshop。</td><td>将反相后的正片原样封入 Adobe LinearRaw 规范中，下游软件能够像读取原生数码 RAW 一样无损拉动高光与阴影。</td></tr>
        <tr><td><strong>DNG · camera RAW</strong></td><td>原始相机 Bayer 马赛克数据</td><td>原始传感器线性数据</td><td><strong>否（完全不含任何调色与反相数据）</strong></td><td>所有兼容 Adobe DNG 标准的原始 RAW 解析工具。</td><td><strong>纯原始数据封包归档</strong>。将相机拍摄的原始未修改马赛克像素直封入 DNG，仅供原始档案异地备份。</td></tr>
    </tbody>
</table>

> [!IMPORTANT] 严防混淆线性 DNG 与 Camera RAW DNG
> **Linear DNG（线性 DNG）是经过 NexFilm 物理反相的成品**：反相、印片光、局部校色与几何裁切均已完成，仅保留了无损线性动态；而 **Camera RAW DNG 仅仅是原始底片素材的无损打包**：它完全不包含任何调色、反相或裁切，输出色彩空间与输出锐化对其完全无效，且该选项仅在源文件本身为相机 RAW 时可用（扫描仪 TIFF 无法导出 Camera RAW DNG）。

## 七大输出色彩空间 ICC 选择与色域映射 {#color-spaces-and-icc}

色彩空间决定了导出的像素数值如何映射到显示器或印刷油墨的色域中。NexFilm 内置了从普通屏幕到专业电影母带的 7 种输出色彩空间，并会把对应的 ICC 配置写入文件：

<table class="doc-table">
    <thead>
        <tr><th>色彩空间标识</th><th>标准全称与色域范围</th><th>推荐交付目标与匹配介质</th><th>使用注意事项与红线原则</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>sRGB</strong></td><td>sRGB IEC 61966-2.1（标准网络色域）</td><td>互联网网页发布、微信/小红书/Instagram 等社交网络分享、普通办公显示器。</td><td>兼容性最稳妥的色域。未做色彩管理的设备通常会按 sRGB 处理，因此不容易出现意外偏色。</td></tr>
        <tr><td><strong>Display P3</strong></td><td>Apple Display P3（DCI-P3 广色域配对 D65）</td><td>MacBook Pro、iPhone、iPad 以及高端广色域 OLED/Mini-LED 显示设备。</td><td>相比 sRGB 能在绿色与红色方向呈现更生动浓郁的饱和度，非常适合苹果生态内的高清影集浏览。</td></tr>
        <tr><td><strong>Adobe RGB (1998)</strong></td><td>Adobe RGB (1998)（专业广色域）</td><td>专业艺术微喷输出（Giclée Printing）、高阶画册印刷制版、传统专业摄影显示器。</td><td>能够完整覆盖大部分青色与蓝绿色印刷油墨色域，是印前数字暗房的行业金标准。</td></tr>
        <tr><td><strong>ITU-R BT.2020</strong></td><td>Rec.2020（超高清电视色域）</td><td>4K/8K 超高清电视广播、HDR 视频工作流与下一代宽色域显示终端。</td><td>色域极其宽广，适合与未来现代影视制作管线接轨。</td></tr>
        <tr><td><strong>ProPhoto RGB</strong></td><td>ProPhoto RGB / ROMM RGB（超广色域）</td><td>16-bit 格式长期存档、底片高饱和色彩保留、Photoshop 深度精修。</td><td>覆盖了 90% 以上的人眼可见光谱，能给大多数胶片染料色彩留出余量。<strong>不要用于 8-bit JPEG</strong>。</td></tr>
        <tr><td><strong>ACEScg</strong></td><td>ACEScg (AP1 色彩空间，D60 白点)</td><td>电影视觉特效（VFX）、CGI 资产合成与数字调色工作色彩空间。</td><td>影视工业数字中间件标准，适合无缝交付给 DaVinci Resolve 调色项目。</td></tr>
        <tr><td><strong>ACES2065-1</strong></td><td>ACES2065-1 (AP0 超大色域，D60 白点)</td><td>电影艺术与科学学院（AMPAS）官方母带长期归档与多机构交换标准。</td><td>完全包络全可见光谱甚至包含部分虚数虚色坐标，专为长期不可替代资产保存而设。</td></tr>
    </tbody>
</table>

> [!WARNING] 严禁在 8-bit JPEG 中强行套用超宽色域
> 当导出格式选择为 8-bit JPEG 时，每个通道只有 256 级可用。如果硬选 ProPhoto RGB 或 ACES 这类超宽色域，这 256 级会被拉开去覆盖很大的色彩范围，蓝天、肤色这类平滑渐变就很容易出现明显的**色彩断层（Banding / Posterization）**。导出 8-bit JPEG 时，请搭配 **sRGB** 或 **Display P3**。

## 几何缩放策略、画质保护与输出锐化 {#dimensions-and-sharpening}

在导出对话框的“尺寸”区域，摄影师可针对下游用途定制精确的几何分辨率策略：

### 1. 原始尺寸
直接按底层全分辨率流式渲染导出，保留相机传感器或扫描仪的原始像素矩阵，是制作高阶出版母带与大画幅微喷的必选配置。

### 2. 指定长边
输入长边目标像素数值，范围覆盖 **`256` 至 `32768` 像素**。面板提供了 1024、2048、4096 与 8192 四组快捷预设，例如 2048 px 就适合大多数手机端分享。系统在缩放时按源片长宽比等比重采样，不会把画面拉变形。

### 3. 禁止小图放大保护
界面提供了“允许放大”复选框，**默认不勾选**。当某张原片的原始长边本来就小于所设目标长边时，系统会保持原片大小导出，不用生硬的插值把它放大，也就减少了颗粒被糊成一片的情况。

### 4. 四档输出锐化矩阵
输出锐化是独立于工作台细节面板的二次微调工序，专门作用于图像重采样缩放后的最终交付像素上：
- **None（无锐化）**：完全不进行重采样补偿锐化，适合追求纯粹银盐大颗粒质感或后续准备在 Photoshop 中手工精细打磨的画面。
- **Low（低锐化）**：温和补偿轻微的下采样模糊，适合在 Retina 广色域高密度屏幕上浏览。
- **Standard（标准锐化，默认推荐）**：经过大量测试优化的均衡锐化，赋予画面清澈坚挺的轮廓感，兼顾屏幕浏览与桌面喷墨打印。
- **High（高锐化）**：强化高频边缘反差，专为大幅缩小至 1024 px 以下的微缩索引图或长边极小的移动端预览图设计。

## 批量命名令牌系统与文件重名策略 {#naming-tokens-and-conflicts}

批量导出数十张底片时，规范的文件命名是避免项目混乱的生命线。NexFilm 内置了基于令牌的智能文件名重构引擎，支持以下 6 个核心命名变量：

<table class="doc-table">
    <thead>
        <tr><th>命名令牌变量</th><th>变量提取来源</th><th>实际渲染范例</th><th>应用价值</th></tr>
    </thead>
    <tbody>
        <tr><td><code>{Roll}</code></td><td>胶卷元数据中的胶卷唯一标识或自定卷名。</td><td><code>Roll01</code>, <code>2024_Hokkaido</code></td><td>将胶卷归属明确锁死在文件名头部。</td></tr>
        <tr><td><code>{Camera}</code></td><td>胶卷记录中填写的拍摄相机机身名称。</td><td><code>LeicaM6</code>, <code>Hasselblad503CW</code></td><td>便于按设备管理历史摄影作品。</td></tr>
        <tr><td><code>{Film}</code></td><td>胶卷记录中填写的感光乳剂型号。</td><td><code>Portra400</code>, <code>Tri-X400</code></td><td>直观区分不同乳剂的色彩风格。</td></tr>
        <tr><td><code>{Date}</code></td><td>导入时录入的拍摄日期或数字化日期。</td><td><code>2024-11-05</code>, <code>Undated</code></td><td>建立时间序列档案。</td></tr>
        <tr><td><code>{Original}</code></td><td>扫描文件或相机 RAW 在磁盘上的原始主文件名。</td><td><code>DSC_0428</code>, <code>scan_frame01</code></td><td>保留与源片物理扫描资产的溯源关联。</td></tr>
        <tr><td><code>{Seq}</code></td><td>三位补零的卷内自增序列序号。</td><td><code>001</code>, <code>002</code>, <code>036</code></td><td>保证多张导出文件在文件管理器中按序陈列。</td></tr>
    </tbody>
</table>

**实用命名模板组合示例**：
- `{Roll}_{Seq}` → 渲染为 `Roll01_001.jpg`（经典标准模板，默认推荐）；
- `{Date}_{Film}_{Camera}_{Seq}` → 渲染为 `2024-11-05_Portra400_LeicaM6_001.tif`（详尽档案库模板）；
- `{Roll}_{Original}_{Seq}` → 渲染为 `2024_Tokyo_DSC_1024_001.png`（追溯原片模板）。

**非法字符清洗机制**：
模板在生成过程中会自动清洗 `< > : " / \ | ? *` 以及 ASCII 控制字符（统一转换为下划线 `_`），并智能拦截 Windows 平台历史保留字（如 `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`），自动在前端补加下划线，确保跨平台文件系统百分之百安全落盘。

**文件重名冲突策略**：
当导出目标目录中已存在同名文件时，摄影师可选择三种确定性策略：
1. **Keep both · add a suffix（保留两者，追加递增后缀，默认推荐）**：自动追加 `_1`、`_2` 后缀，降低覆写原文件的风险；
2. **Replace existing file（替换已有文件）**：直接覆盖旧文件，适合重复导出同一批样片时的覆写更新；
3. **Skip existing file（跳过已有文件）**：跳过已存在的文件，仅导出新追加的画面。

勾选对话框底部的 **将胶卷、胶片、相机与日期写入 EXIF** 复选框，系统会在文件编码时将元数据真实注入图像的标准 Exif 与 IPTC 标签中，使导出的成品无论流转到何处均完整携带原始胶卷信息。

## 接触印样物理拟真生成机理 {#contact-sheet-generation}

在传统暗房中，摄影师将整卷剪成数段的底片药膜面朝下直接紧压在 8×10 英寸感光相纸上，开灯进行短暂的一对一漫射曝光，显影出的微缩索引相纸被称为 **接触印样**。接触印样不仅是挑选底片的历史工具，更代表了胶片时代严谨纯正的暗房仪式感。

NexFilm 在 **历史胶卷记录→ 胶卷详情** 视图中，提供了一键 **导出接触印样** 功能。软件配备了基于物理真实尺寸拟真的接触印样渲染引擎：

### 1. 135 全画幅物理齿孔与条形码渲染
对于 135 胶卷，接触印样完全遵循 ISO 1007 国际几何规范排布：
- 每行固定排布 6 帧画面，帧与帧之间保持真实的 2 毫米间距；
- 上下片边绘制精确且连续的物理齿孔（KS 规约齿孔，严格按 4.75 毫米节距无缝贯穿整条胶卷，跨帧之间节奏完全均匀）；
- 在片边精确模拟橙红色银盐字码，包含真实帧号、半格标记以及拟真的片边潜影条形码。

### 2. 120 中画幅多画幅自适应与乳剂字码
针对 120 胶卷没有齿孔的物理特性，引擎自动检测其长宽比并智能调整排布网格：
- `6x4.5`（竖向构图）：每行排布 4 帧；
- `6x6`（正方形）：每行排布 4 帧；
- `6x7`：每行排布 3 帧；
- `6x9` 与 `6x12`：每行排布 2 帧；
- `6x17` 宽幅全景：每行排布 1 帧。
- 120 片边上方渲染大写的胶卷型号抬头、帧号以及带有指向箭头的标准方向标记，再现纯正中画幅片边魅力。

### 3. 印样版式、底色与未反相状态标记
- **画册级版面设计**：整张接触印样以微暖的哑光暗房相纸色（`#E9E9E7`）为底衬，底片条呈纯黑色，底部以高精度矢量路径绘制官方 **NEXFILM** 品牌锁扣标识，并清晰列明胶卷编号、相机型号、乳剂规格与处理日期。
- **未反相画面的处理**：若某卷底片中部分画面尚未在工作台中完成反相调色，引擎不会把未反相的原始负片硬塞进接触印样，而是在对应格子里显示中性灰底色和 **NOT DEVELOPED** 提示。整张印样保持统一，也能看出还有哪些画面没有处理。

生成的接触印样自动以 `contact_sheet_{roll_id}_{camera}.jpg` 命名保存至指定目录，是向客户展示整卷成片或在社交平台上呈现作品集的绝佳暗房载体。

## 整卷后台导出与多任务队列保障 {#batch-export-queue}

无论是导出包含几十张 16-bit TIFF 的整卷母带，还是并发生成大幅面接触印样，NexFilm 的导出作业全部由底层的独立并发队列接管：

- **导出完整性预检**：点击导出后，系统会先做一次快速检查，确认源文件仍可读取，并检查每张画面是否具有合法的反相配方。若检测到脱机文件，系统会暂停并列出明细，不会等到导出数小时后才报错中断。
- **后台流式执行与多线程加速**：导出线程池会利用多核 CPU，同时把文件读写控制在合理范围内，避免机械硬盘频繁寻道。导出期间，摄影师可以继续在工作台精修其他底片。
- **暗房试片黄金法则**：在正式执行耗费磁盘空间与时间的整卷母带导出前，**强烈建议先单独勾选一张光影最丰富的代表性画面导出试片**，在目标显示器或校色软件中复核其影调与锐度，确认完全达到预期后再启动整卷批处理。
