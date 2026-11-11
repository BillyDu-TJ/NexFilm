---
id: cinema-and-aces-workflow
track: color-science
trackOrder: 3
chapterOrder: 4
kicker: "Color Science 04 / 院线管线"
title: "16-bit 线性交付与 DaVinci Resolve / ACEScg 专业后期衔接"
subtitle: "无损母带级导出节点截断机制、ACEScg 转换契约与达芬奇专业二级调色实操。"
readingTime: 14
inApp: true
sinceVersion: "v1.1.0"
---

## 线性母带交付与展示级成品导出的本质分歧 {#linear-mastering-vs-display-rendering}

在胶片数字化制作中，导出文件的定位直接决定了后期调色的自由度。传统反相工具通常只提供面向最终观看的展示级成品导出，例如 8-bit JPEG 或施加了 sRGB 伽马的普通 TIFF。这类文件在导出前已经固化了软件内部的色调曲线、相纸模拟 3D LUT、高光反差压缩以及显示器光电转换函数（OETF）。当调色师试图在专业调色软件中对这类文件进行较大幅度的曝光提升或阴影提亮时，往往会迅速遇到阶调断层、色相偏移以及噪点异常放大的困境。

与展示级导出不同，**线性母带级导出**旨在为电影院线与专业后期工业流程提供场景线性级别的中间交付物。其核心思想是将数据流直接截断在光学反相与工作空间映射完成之后、任何感知色调曲线与显示伽马介入之前的纯物理线性阶段。

```text
NexFilm 核心处理流水线
┌────────────────────────────────────────────────────────┐
│  透射率计算 -> 净密度扣除 -> 印片光调节 -> 正片线性反相  │
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼
                  [ 线性截断分流节点 ]
                 /                    \
                /                      \
        [ 母带级无损导出 ]        [ 显示级渲染管线 ]
        - 16-bit 线性 TIFF        - 四通道分通道色调曲线
        - LinearRaw DNG           - 胶片模拟 3D LUT (Kodak 2383 等)
        - Raw Mosaic DNG          - 空间域反差锐化与色调映射
        (无伽马/保留全动态)       - sRGB / Display P3 OETF 编码
                │                 - 8/16-bit TIFF / JPEG 成品
                ▼
        达芬奇 / ACEScg 调色台
```

经由此截断节点导出的 16-bit 线性 ProPhoto RGB 图像，其各像素数值严格与原始胶片透过的正片光照能量成正比，具有与数字电影摄影机（如 ARRI ALEXA LogC 或 RED Log3G10）完全等价的无损调色宽容度。调色师可以在 DaVinci Resolve 中将其作为广色域场景线性素材进行无损的二级调色与母带制作。

## 三大母带级导出格式与精确截断节点剖析 {#three-master-export-formats}

为了适配不同的专业后期环境与长期归档需求，NexFilm 在底层（`src/dng_writer.rs` 与 `src/commands.rs`）构建了三种无损母带级导出规格：

1. **16-bit 线性 TIFF（16-bit Linear TIFF）**  
   数据在完成光学密度反相与基底扣除后，直接转换为线性 ProPhoto RGB 浮点光强，并在未施加任何伽马矫正的前提下量化为 16-bit 无损 TIFF 格式。文件内部写入了 `Gamma = 1.0` 与标准 ROMM RGB 原色色度坐标。该格式是进入达芬奇调色系统最直接、兼容性最广泛的工业桥梁。
2. **线性正片 DNG**  
   若后期流程偏好基于 Adobe Camera Raw 或 Lightroom 进行多帧协同管理，用户可选用 LinearRaw DNG。该格式遵循标准 TIFF/EP 规范，在单 IFD 容器中写入 `PhotometricInterpretation = 34892 (LinearRaw)` 标签，并内嵌 D50 与 D65 标准观察光源校准矩阵。软件读取该文件时会自动将其识别为具备原始传感器级别的 16 位线性正片，支持自由调整白平衡与曝光色温。
3. **相机马赛克 RAW DNG**  
   该格式面向具有极高归档严谨度的档案馆与摄影机构。系统在导出时不进行任何解马赛克、色彩插值或非线性运算，而是将数码相机采集到的拜尔或 X-Trans CFA 原生光电采样点，原汁原味地重封装为标准 DNG 容器（`PhotometricInterpretation = 32803`）。原始硬件元数据、黑电平与白平衡乘数完整保留，为未来数十年后的算法复现提供不可替代的一手数字底片。

> [!IMPORTANT]
> 导出 16-bit 线性 TIFF 或 LinearRaw DNG 时，NexFilm 界面中设定的对比度滑块、阴影高光滑块以及相纸 3D LUT 将被自动旁路。导出的图像在普通图像查看器中观察时会呈现极度灰暗与低反差的外观，这属于线性辐射数据的正常物理表现。

## DaVinci Resolve 色彩管理系统配置全流程 {#davinci-resolve-color-management}

将 NexFilm 导出的 16-bit 线性 ProPhoto RGB 母带导入 DaVinci Resolve 进行院线级制作时，推荐采用色彩管理系统规范色彩流向。以下详细介绍两种主流工程配置方案：

### 方案 A：DaVinci YRGB Color Managed 工作流（推荐影视后期团队）

1. 打开 DaVinci Resolve `Project Settings (项目设置)` -> `Color Management (色彩管理)`。
2. 将 `Color Science (色彩科学)` 设置为 `DaVinci YRGB Color Managed`。
3. 取消勾选 `Automatic color management (自动色彩管理)`，将 `Color processing mode (色彩处理模式)` 设置为 `Custom (自定义)`。
4. **Input Color Space (输入色彩空间)**：设置为 `Linear` 传递函数，基色选用 `ProPhoto RGB`（或 `ROMM RGB`）。
5. **Timeline Color Space (时间线色彩空间)**：设置为 `DaVinci Wide Gamut / Intermediate`。此时整个时间线调色节点均运行于高宽容度的对数对数空间内。
6. **Output Color Space (输出色彩空间)**：根据最终监看设备设定（如 SDR 选用 `Rec.709 Gamma 2.4`，HDR 交付选用 `P3-D65 ST2084 1000 nits`）。

### 方案 B：ACES 学院色彩编码系统工作流（推荐院线视效协同）

1. 在 `Project Settings` 中将 `Color Science` 选择为 `ACEScc` 或 `ACEScct`（推荐选用具备平滑趾部过渡特性的 `ACEScct`）。
2. **ACES Version (ACES 版本)**：选择 `ACES 1.3`。
3. **ACES Input Transform (IDT)**：由于 ProPhoto 线性空间不属于 ACES 预设硬件机型列表，在此处将 IDT 设置为 `No Input Transform`。后续在时间线节点上通过色彩空间转换工具手动加载。
4. **ACES Output Transform (ODT)**：设置为目标母带监视标准（如 `Rec.709` 或 `DCI-P3 D65`）。

## ACEScg 节点树架构与无损二级调色实操 {#acescg-grading-node-tree}

在确立色彩管理基底后，调色师即可在节点图中构建专业胶片调色拓扑。标准节点树应当严格遵循从全局光照平衡到局部风格化的逻辑分层：

```text
[Node 01: CST 输入] -> [Node 02: 曝光平衡] -> [Node 03: 密度曲线]
                                                    │
                                                    ▼
[Node 06: 输出变换] <- [Node 05: 胶片颗粒] <- [Node 04: 二级选色]
```

各节点的具体配置与调色实操规范如下：

1. **节点 1：输入色彩空间转换**  
   若采用 ACEScct 工作流，在首节点加载 `Color Space Transform (色彩空间转换)` OFX 插件。输入色彩空间设置为 `ProPhoto RGB`，输入 Gamma 设置为 `Linear`；输出色彩空间设置为 `ACEScg`，输出 Gamma 设置为 `ACEScct`。Tone Mapping（色调映射）保持为 `None`，完成无损数学投射。
2. **节点 2：印片光曝光与白平衡调节**  
   在 ACEScct 对数空间内，使用色轮面板的 `Offset (全局平移)` 控制轴进行微调。该操作在数学上完全等价于光学暗房中调整放大机加色滤镜的曝光时长，能够以完全线性的方式校正轻微色偏，绝不产生高光畸变。
3. **节点 3：胶片特性密度曲线**  
   使用自定义曲线构建模拟真实胶片洗印相纸的 S 型反差曲线。重点控制高光肩部的平滑滚降与暗部趾部的适度抬升，还原真实银盐照片的浑厚质感。
4. **节点 4：二级色度选色与肤色保护**  
   利用 HSL 选色器或 Magic Mask 精准捕获人物肤色或特定景物，微调其饱和度与色相，消除不同光照环境下的杂色干扰。
5. **节点 5：光学杂质与颗粒质感重构**  
   根据需要加载胶卷颗粒插件或光晕特效，为画面增添符合特定胶片乳剂年代感的有机物理纹理。
6. **节点 6：最终显示适配与限幅（Output Transform / Limiter）**  
   确保所有高光与暗部信号平滑落入输出设备的广播安全色域范围内。

> [!NOTE]
> 在 ACEScct 节点树中，所有曝光提升与压暗均应优先在 Node 02 的 Offset 轮上完成。避免直接使用 Primary 调色轮中的 Gain 或 Lift，以免破坏线性物理能量与真实色彩饱和度之间的自然比例关系。

## 规格全景对比：母带与成品导出选型矩阵 {#export-specification-matrix}

为了便于不同制作环节的技术决策，下表完整总结了 NexFilm 导出矩阵的核心指标与应用建议：

| 导出格式与规格 | 颜色深度 | 色彩空间与 Gamma 传递 | 图像处理管线截断节点 | 核心用途与下游协同软件 |
| --- | --- | --- | --- | --- |
| **16-bit 线性 TIFF** | 16-bit 整数 | Linear ProPhoto RGB (Gamma 1.0) | 密度反相完成立即截断，无任何调色曲线 | DaVinci Resolve、Nuke、Flame 等工业级院线合成与调色 |
| **LinearRaw DNG** | 16-bit 整数 | LinearRaw 空间，内嵌 D50/D65 标定矩阵 | 线性化正片阶段截断，嵌入标准 DNG 标签 | Adobe Lightroom、Photoshop ACR、Capture One 协同调色 |
| **Raw Mosaic DNG** | 原生位深 (12/14/16-bit) | 传感器原生 CFA Bayer / X-Trans 矩阵 | 仅做文件重封装，完全不介入图像算法 | 原始数字底片长期归档、科研测定与独立离线存储 |
| **8/16-bit 展示级 TIFF** | 8 或 16-bit | sRGB / Display P3 / Adobe RGB (含 OETF) | 全管线完整执行，烘焙四通道曲线与相纸 LUT | 艺术微喷高质量打印、展览画册交付与出版物制作 |
| **高质量 JPEG** | 8-bit | 标准 sRGB (IEC 61966-2.1) | 全管线完整执行，施加感知色调与质量压缩 | Web 网页展示、社交媒体分享与客户快速审片 |
