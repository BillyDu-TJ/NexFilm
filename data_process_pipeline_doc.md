# NexFilm 数据处理与硬件校准管线

## 文档状态

本文是 v1.1 的目标架构、理论说明和实施计划。它取代此前以输入格式和工作色域为主线的设计草案。

本文会严格区分：

- **v1.0.2 当前实现**：已经发布并可使用的兼容路径。
- **v1.1 必做范围**：使输入成为可测量、可追溯数据所需的工程基础。
- **v1.1 校准功能**：基于参考目标拟合设备密度响应的实用方案。
- **高级科学路径**：窄谱或多光谱硬件支持；允许在 v1.1 中实验，但不能以未验证的近似替代。

本文中的“科学”指：变换的输入、输出、物理目标、适用设备和误差均可说明并验证。它不等于“画面一定更好看”。显示参考、端到端校色和风格处理仍然是合理功能，但必须与测量校准分开命名和保存。

---

## 1. 三个数学域

整条负片管线必须按 `log` 的位置分成三个域。不同域的矩阵不能因为都是 `3x3` 就互换。

### 1.1 透射率域：`log` 之前

这里处理相机、扫描仪、灯板和胶片透射光。目标是得到正值、线性、具有明确设备含义的相对透射率：

```text
T = transmitted_light / open_gate_light
```

暗场、平场、曝光归一化、相机通道分离和窄谱 LED 分离都属于这一域。

一般情况下：

```text
-log10(Ax) != A[-log10(x)]
```

因此，本域中的设备分离矩阵不能移动到密度域；普通 RGB 色彩空间矩阵也不能在未经验证时被当成密度校准。

### 1.2 密度域：`log` 之后

相对透射率转换为光学密度：

```text
D_raw = -log10(max(T, epsilon))
D_net = D_raw - D_base
```

片基扣除、Status M 密度对齐、printing density 转换、数字 mask 和胶片染料层串扰校正属于这一域。

### 1.3 正片与输出域

准确的胶片密度仍不是场景线性 RGB。必须经过胶片型号、曝光与冲洗条件相关的特性曲线或重建模型，才能得到正片信号。Linear ProPhoto RGB 是这一阶段之后的统一工作空间，而不是原始密度测量空间。

LUT、饱和度、色温、显示变换、OETF、ICC 嵌入和输出量化属于正片与输出域。

---

## 2. 总体目标管线

```text
负片本体
  coloured coupler / DIR coupler 已在胶片中产生物理影响
        |
        v
RAW mosaic / Scanner RGB
        |
        |  黑电平、暗场、平场、坏点、曝光与饱和检测
        v
线性 Camera Native f32
        |
        |  解马赛克；固定且可记录的通道增益
        v
Capture Separation / Input Characterization
        |
        |  相机 x 灯板响应 -> Density Input RGB 透射率
        v
正值相对透射率 T
        |
        |  -log10(T)
        v
采集密度 D_raw
        |
        |  同条件未曝光片基 D_base
        v
净密度 D_net
        |
        |  Status M 对齐 / 数字 mask / printing-density 转换
        v
校准密度 D_calibrated
        |
        |  胶片特性曲线、趾部、肩部、通道 gamma
        v
正片相对曝光或显示参考信号
        |
        |  转换到 Linear ProPhoto RGB
        v
调色、LUT、显示、ICC、OETF、量化与导出
```

管线中的校准操作分为三个独立层：

1. **Capture Profile**：校准采集硬件如何读取透射率，作用于 `log` 前。
2. **Density Profile**：校准采集密度如何对应 Status M、printing density 或其他参考密度，作用于 `log` 后。
3. **Film Profile**：校准胶片密度如何对应原场景曝光或目标正片，作用于密度校正后。

把这三层合并成一个端到端 LUT 可以得到好看的结果，但会失去物理可解释性、可移植性和故障诊断能力。

---

## 3. v1.0.2 当前实现与边界

v1.0.2 已完成两个重要改进：

- RAW 完整解码延后到 Develop 阶段，预览、Auto Invert 和密度分析复用有界代理缓存。
- LibRaw 在 Camera Native RGB 完成黑电平、相机白平衡和解马赛克后，由 NexFilm 在 `f32` 中应用 camera-to-linear-sRGB 矩阵，避免 LibRaw 在输出色域矩阵阶段直接写入无符号 16 位所导致的逐通道截断。

当前实际路径仍是：

```text
LibRaw Camera RGB u16
-> camera-to-linear-sRGB matrix in f32
-> positive-domain gamut compression
-> quantize to u16 proxy
-> -log10
-> estimated base subtraction
-> hard-coded legacy density matrix
-> D-Min/D-Max display normalization
```

因此，“使用 f32 变换”只解决了输出色域矩阵阶段的明显溢出问题，并不表示已经实现端到端 `f32` 测量：

- 解马赛克后的 Camera RGB 仍以 `u16` 传递。
- camera-to-sRGB 后会调用 `compress_linear_srgb_for_density()`；该函数保持 Rec.709 亮度并压缩色度，是工程性 gamut compression，不是物理透射率校准。
- 压缩结果在 `-log10` 前重新量化为 `u16`。
- Camera Native 到 linear-sRGB 的普通色度矩阵不保证输出通道是适合密度计算的物理基底。
- LibRaw 相机白平衡仍会把通道增益、灯板颜色和设备校准耦合在一起。
- 当前没有暗场、无片光源和平场参考。
- 当前片基主要通过画面统计估计，不等同于同批胶片的实测未曝光片基。
- 当前 `status_m_crosstalk_matrix()` 没有附带参考测量、适用胶片、硬件条件或误差报告，必须视为 **Legacy Estimate**。
- 当前正片是显示参考结果，不是经过胶片特性曲线反演得到的场景线性数据。

v1.1 必须保留 v1.0.2 兼容路径，但不能继续将其描述为完整的 Status M 或科学密度测量。

---

## 4. 分阶段理论与处理要求

### 4.1 负片本体：物理 mask 与 DIR

彩色负片不是三个互不相关的透明 RGB 通道。其染料密度谱可简写为：

```text
d_neg(lambda) = sum_j D_dye[j] * m_j(lambda)
T_neg(lambda) = 10 ^ (-d_neg(lambda))
```

coloured coupler 在胶片内部产生一阶密度耦合，用于补偿相纸读取染料层时的串扰。DIR coupler 还会产生密度相关、空间相关和近似二阶的影响。

这些作用已经固化在冲洗后的负片里。扫描软件不能“移除橙色”就自动恢复理论层密度；软件必须校正扫描设备的光谱读取方式与胶片原设计目标之间的差异。

文章 `reference_v777.md` 的模型提供了重要的一阶解释，但它仍包含理想化假设：胶片特性曲线局部线性、染料密度谱可由三个基函数表示、光源或接收器足够窄谱、杂散光与散射可忽略。硬件和数据不满足这些条件时，固定 `3x3` 矩阵只能是近似。

### 4.2 RAW 基础测量校正

科学路径不应把自动白平衡后的相机图像直接当成透射率。最小采集合同包括：

- 固定 ISO、快门、光圈、焦距和对焦位置。
- 关闭自动白平衡、自动亮度、降噪、局部色调与相机风格。
- 保存相机型号、序列号、镜头、光圈、灯板和曝光元数据。
- 获取暗场帧，用于传感器偏置、热噪声和漏光估计。
- 获取无片光源帧，用于光源强度和曝光归一化。
- 获取平场数据，用于灯板不均匀、镜头渐晕和 CFA 通道位置差异校正。
- 在 RAW/CFA 阶段检查黑电平、白电平、坏点与饱和；饱和样本不得进入拟合。

对普通单张白光翻拍，可写为近似形式：

```text
x = flat_correct(raw - dark)
```

对 RGB LED 分时采集，应为每一种灯光分别保存暗场、无片场、曝光和 SPD 标识。

### 4.3 解马赛克与数值表示

解马赛克后直到最终输出量化前，主计算缓冲区应使用 `f32`：

- 不在相机矩阵后立即量化回 `u16`。
- 不以 `[0, 1]` 作为中间计算的通用合法范围。
- 不对负值、超范围值或非有限值静默 clamp。
- 每个阶段记录负值、饱和、epsilon 替代和非有限样本的数量。

密度输入最终必须为正值，但正值约束应由可信的设备分离、参考归一化和有效性判断建立，而不是由面向显示色域的压缩函数制造。

几何变换、缩放和代理生成应声明自己发生在透射率域还是密度域。校准拟合应优先使用未缩放、未锐化数据；预览代理不得改变最终导出的校准结果。

### 4.4 Capture Profile：采集设备分离

传感器通道的观测一般为：

```text
y_i = integral L(lambda) * T_neg(lambda) * s_i(lambda) dlambda
```

其中 `L` 是灯板 SPD，`s_i` 是传感器通道光谱响应。对于宽谱灯板，这个积分经过胶片指数透射后通常不能由一个通用 `3x3` 矩阵完美反演。

若 R/G/B 光源足够窄并分别采集，可近似为：

```text
y = S * t
t = inverse(S) * y
```

这里的 `t` 是胶片在三个 LED 峰值处的透射率，`S` 是传感器对三个 LED 的混合响应。`inverse(S)` 就是 `reference_v777.md` 所称的 IDT。

为了避免与标准 ACES Input Transform 混淆，NexFilm 将它命名为：

> **Capture Separation Transform，采集分离变换**

它必须满足：

- 在线性相机信号上应用。
- 位于 `-log10` 之前。
- 与相机、灯板 SPD、ISO/增益和曝光合同绑定。
- 输出为可归一化的正值 Density Input RGB，而不是为了保持 XYZ 色度的工作 RGB。

无片参考可以表示为：

```text
u_sample = B * (y_sample - y_dark)
u_light  = B * (y_light  - y_dark)
T        = u_sample / u_light
```

实际实现可以联合拟合 `B`、曝光系数和平场，但保存的配置必须说明输出定义和归一化方法。

#### DCP 与标准 ACES IDT 的位置

DCP 和标准 ACES IDT 同样作用于线性相机数据，但通常优化的是：

```text
Camera RGB -> XYZ / ACES relative exposure / working RGB
```

它们不是为胶片染料透射率设计的。DCP 的 ColorMatrix、ForwardMatrix 可以作为没有专用 Capture Profile 时的色度后备；Hue/Sat Map、Look Table、Tone Curve 和主观风格不能进入密度分支。

专用 Capture Separation Transform 与普通 DCP 是同一位置上的不同目标方案，不能默认串联使用。

### 4.5 透射率有效性与密度转换

只有在 Capture Profile 输出域已经校准且保持正值时，才能计算：

```text
D_raw = -log10(T)
```

实现要求：

- `T <= 0`、非有限值和饱和样本标记为无效，而不是静默变成正常密度。
- `epsilon` 只用于保证数值稳定，并记录替代计数；不能把替代值当作真实的高密度测量。
- 报告设备在当前曝光下可用的最小透射率和最大可靠密度。
- 通过中性阶梯验证线性、重复性、噪声和 flare；仅靠矩阵拟合不能修复严重杂散光。

### 4.6 片基扣除

片基密度必须尽可能来自：

- 同一卷胶片的未曝光片头、片尾或边缘。
- 同一相机、镜头、灯板和曝光条件。
- 同一冲洗条件；胶片批次和冲洗漂移应记录。

```text
D_net = D_raw - D_base
```

画面百分位自动估计可以继续用于快速 Auto Invert，但应标记为 **Estimated Base**。硬件校准模式必须优先使用 **Measured Base**，并保存取样区域、统计方法和置信度。

### 4.7 Density Profile：Status M、printing density 与数字 mask

#### Status M 的定义

Status M 是 ISO 5-3 定义的一组彩色密度测量光谱条件。概念上，每个通道的测量是对胶片透射谱施加规定的光源和接收器权重：

```text
D_M,c = -log10(
    integral W_M,c(lambda) * T_neg(lambda) dlambda
    / integral W_M,c(lambda) dlambda
)
```

因此：

- Status M 是参考测量坐标，不是某一个固定矩阵。
- “支持 Status M”必须说明输入如何与具有 Status M 参考值的目标对齐。
- 当前硬编码 `3x3` 矩阵不是 Status M 本身。

若目标具有可信的 Status M 参考密度，可拟合：

```text
D_status_m = A_device * D_capture + b_device
```

这个变换主要绑定采集设备、灯板和目标材料。若宽谱硬件产生明显密度相关残差，线性模型应判定不合格，而不是无条件增加高阶参数。

#### Printing density 与数字 mask

负片的物理 mask 原本针对相纸谱敏感度设计。扫描设备的有效谱峰不同，会得到不同的染料混合密度。数字 mask 的任务是把扫描或 Status M 密度转换到目标 printing density 或理论层密度：

```text
D_print = A_film * D_status_m + b_film
```

它位于 `log` 和片基扣除之后，并通常依赖：

- 胶片型号和乳剂设计。
- 目标相纸或 printing-density 定义。
- 冲洗条件；至少需要验证其敏感度。

Kodak 等资料表明 Status M 与 printing density 的关系会随胶片变化。因此 NexFilm 不应提供一个无条件适用于所有摄影负片的 “Status M to Print” 矩阵。

当一阶矩阵在独立验证集上不满足误差目标时，可以评估受约束的二阶模型。但二阶模型不能被描述成从宽谱扫描中完整恢复了丢失的光谱信息。

### 4.8 Film Profile：从密度到正片

校准密度只是胶片上的记录量。若目标是场景线性正片，需要反演或拟合：

- 分通道 H-D 曲线。
- 趾部、直线段和肩部。
- 胶片曝光光源与原场景白点。
- 通道 gamma 和必要的层间残差。
- 胶片型号、批次、曝光条件和冲洗条件。

若缺少 Film Profile，NexFilm 可以继续使用 D-Min/D-Max、Gamma、Printer Lights 和自动白平衡生成显示参考正片，但元数据必须标记：

```text
reconstruction = display-referred-estimate
```

只有经过验证的重建模型才能标记：

```text
reconstruction = scene-relative
```

### 4.9 工作色域、风格与输出

完成正片重建后，统一转换到 Linear ProPhoto RGB：

```text
positive reconstruction
-> Linear ProPhoto RGB
-> creative controls / LUT
-> target linear RGB
-> target OETF
-> quantization and ICC embedding
```

要求：

- Linear ProPhoto 不再被定义为默认 Density Input RGB。
- LUT 必须声明输入色域、传递函数和显示/场景参考属性。
- 不明域 `.cube` 只能作为显示参考风格。
- WebGL 预览转换到显示 sRGB。
- 输出链只应用一次 OETF。
- ICC 必须与像素编码一致。
- 锐化与最终量化只发生在输出端。

---

## 5. 校准方法的作用位置与边界

| 方法或目标 | 数据来源 | 拟合目标 | 生效位置 | 科学边界 |
| --- | --- | --- | --- | --- |
| LibRaw 相机矩阵 | 相机元数据/厂商矩阵 | Camera RGB 到通用 RGB | `log` 前 | 色度后备，不是胶片透射率标定 |
| DCP ColorMatrix / ForwardMatrix | 反射色卡或相机特性数据 | XYZ/工作 RGB | `log` 前 | 可改善普通相机色度；不能自动恢复染料层密度 |
| 标准 ACES Input Transform | 相机线性化和色彩特性 | ACES 相对曝光 | `log` 前 | 目标是场景色彩，不等于文中的窄谱分离 IDT |
| Capture Separation Transform | 分时窄谱 LED、光谱数据或透射目标 | Density Input RGB 透射率 | `log` 前 | 负片扫描的首选设备校准 |
| 扫描仪 ICC / IT8 | 透射或反射 IT8 | 原稿 XYZ/Lab | 通常 `log` 前 | 对目标染料有效；与负片染料可能存在材料同色异谱误差 |
| Status M 参考目标 | 认证密度测量 | Status M 密度 | `log` 后 | Status M 是参考坐标，不是固定矩阵 |
| 数字 mask | printing-density 或层密度参考 | 校正扫描峰值与相纸峰值差异 | `log` 和片基扣除后 | 通常依赖胶片型号与目标相纸 |
| 直接拍摄 24 色卡制作 DCP | 相机直接观察色卡 | 场景 XYZ/Lab | `log` 前 | 适合相机色度，不足以辨识负片三层密度 |
| 色卡拍到胶片后端到端拟合 | 拍摄、胶片、冲洗、扫描的组合 | 最终正片色彩 | 跨越多个阶段 | 容易得到好看结果，但误差全部耦合且难以移植 |
| 胶片特性曲线/控制条 | 已知曝光与处理目标 | 密度到场景曝光/正片 | 密度校正后 | 得到场景相对结果所必需 |

### 5.1 24 色卡的三种不同用途

“使用 24 色卡”本身不能说明校准了什么：

1. **相机直接拍色卡**：适合拟合 DCP 或标准 ACES Input Transform。
2. **色卡先拍到胶片，再扫描匹配最终色彩**：适合端到端显示校色或外观配置。
3. **使用色卡实测光谱模拟胶片和相纸响应**：可以生成理论参考密度，但结果依赖曝光光源、胶片模型和相纸模型的准确性。

24 个色块为线性模型提供的标量观测数量通常足够，但“方程数量足够”不代表目标物理量可辨识。若参考值只有反射 Lab，就不能据此声称恢复了负片三层染料密度。

### 5.2 IT8 的两种不同用途

- 使用 IT8 提供的 XYZ/Lab 参考值，可建立扫描仪输入 ICC。
- 使用 IT8 的实测光谱透射率，可拟合或验证采集硬件的光谱响应，再推导 Capture Separation Transform 或指定密度标准。

前者是成熟、易实现的色彩管理；后者更接近科学负片扫描，但需要光谱数据和明确的物理模型。

---

## 6. 方法选择与产品分级

### 6.1 最高科学完整度：光谱或多光谱测量

使用光谱仪、单色仪、多波段扫描器或足够多波长的窄谱 LED，测量胶片透射谱，再计算任意 Status、printing density 或相纸响应。

优点：

- 保留的信息最多。
- 目标定义清楚。
- 可以模拟不同密度标准和相纸。

限制：

- 硬件昂贵，采集慢。
- 标定与数据处理复杂。
- 不适合作为 v1.1 的大众必需功能。

### 6.2 科学且现实：RGB 窄谱 LED 分时采集

分别点亮 R/G/B 窄谱 LED，记录 SPD 和曝光，求采集分离矩阵；随后使用密度参考目标求 Density Profile。

优点：

- 把传感器通道混合和胶片密度串扰分开处理。
- 符合 `log` 前分离、`log` 后密度校正的物理顺序。
- 硬件成本和数据规模可控。

限制：

- 三个波长仍是完整光谱的降维近似。
- 必须分时拍摄，不能把三色 LED 同时点亮的一张照片当作三次独立测量。
- LED SPD、曝光稳定性和相机线性必须验证。

这是 NexFilm 高级硬件校准的首选方向。

### 6.3 v1.1 最适合落地：认证透射目标到 Status M 的密度拟合

使用普通稳定灯板、暗场、无片场和平场，扫描具有 Status M 参考值的透射目标，在密度域拟合 `A * D + b`。

优点：

- 硬件要求低。
- 工作流可以做成向导。
- 能产生明确的验证残差。
- 明显优于无来源的通用矩阵。

限制：

- 宽谱积分造成的非线性不一定可逆。
- 结果可能受目标染料材料影响。
- 得到的是 Status M 对齐，不自动等于 printing density 或理论层密度。

这是 v1.1 建议正式交付的校准能力。

### 6.4 易实现且视觉效果好：胶片 24 色卡端到端校色

把色卡拍到胶片上，冲洗后扫描，使最终正片匹配参考色彩。

优点：实现直接，对常用胶片、冲洗和硬件组合往往有很好的视觉结果。

限制：拍摄光源、拍摄相机、胶片、冲洗、扫描灯板和扫描相机全部耦合；配置不容易跨设备或跨冲洗条件使用。

产品中应命名为 **End-to-End Color Calibration** 或 **Film Look Calibration**，不能命名为 Density Calibration。

### 6.5 成熟后备：DCP 与 IT8 ICC

DCP 和扫描仪 ICC 已有成熟工具链，适合普通图像、反转片、正片和色度管理。对彩色负片，它们可以作为没有专用硬件校准时的输入后备，但不能代替 Density Profile。

### 6.6 兼容路径：通用硬编码矩阵

保留当前矩阵可以维持旧项目外观和零校准体验，但必须标记为：

```text
calibration = legacy-estimate
measurement_standard = unspecified
```

它不能参与“Status M 已校准”“printing density 已恢复”等精确性声明。

---

## 7. 校准配置模型

### 7.1 Capture Profile

建议字段：

```text
profile_id
profile_version
camera_make / camera_model / camera_serial
lens / aperture / focus_distance
iso / exposure_contract
white_balance_mode = fixed | disabled
light_source_make / model / serial
light_mode = broad | rgb-sequential | multispectral
light_spd_reference[]
raw_decoder / raw_decode_version / demosaic_method
dark_reference
flat_field_reference
open_gate_reference
separation_transform
valid_transmission_range
calibration_date
```

### 7.2 Density Profile

```text
profile_id
profile_version
capture_profile_id
reference_standard = status-m | printing-density | spectral | custom
reference_target_make / serial / batch
model_type = identity | matrix-affine | constrained-quadratic
matrix / offset / optional_higher_order_terms
base_handling = measured | estimated | included-in-target
training_sample_count
validation_sample_count
validation_metrics
applicable_film_stock[]
applicable_process[]
```

若 Density Profile 只校准设备到 Status M，通常不应绑定单一胶片；若包含 Status-M-to-printing-density 数字 mask，则必须绑定适用胶片和目标 printing-density 定义。

### 7.3 Film Profile

```text
profile_id
profile_version
film_stock / emulsion / batch
process / lab / process_control_reference
exposure_illuminant
characteristic_curves_rgb
toe_shoulder_model
reference_white / reconstruction_target
validation_metrics
```

### 7.4 处理追溯

每个项目和导出应记录：

- Capture、Density、Film Profile 的 ID 与版本。
- RAW 解码和管线算法版本。
- 片基来源与取样区域。
- 是否存在饱和、无效透射率或 epsilon 替代。
- 输出属于 `legacy-display`、`calibrated-display` 还是 `scene-relative`。

---

## 8. v1.1 实施计划

### Phase 1：测量基础与端到端 f32（v1.1 必做）

1. 新建 `f32` Camera Native / Density Input RGB 图像缓冲，不在 `-log10` 前量化回 `u16`。
2. 把显示用 gamut compression 从科学密度分支移除；只允许在明确的预览/显示路径使用。
3. 为科学路径关闭自动白平衡，保存固定相机增益和 RAW 黑/白电平。
4. 支持暗场、无片光源和平场参考，并明确参考帧的设备与曝光绑定。
5. 为每个阶段增加饱和、负值、非有限值、epsilon 替代和有效密度范围诊断。
6. 保证 Develop 预览、Auto Invert、直方图和全分辨率导出使用同一校准数学合同。
7. 保留 v1.0.2 Legacy 路径，以项目版本或处理模式显式选择。

### Phase 2：配置架构与基础 UI（v1.1 必做）

1. 实现 Capture Profile、Density Profile、Film Profile 的版本化存储与导入导出。
2. UI 将 `Input Space`、`Density Calibration`、`Film Reconstruction`、`Working Space` 分开显示。
3. 提供三种清晰状态：
   - `Uncalibrated / Identity`
   - `Legacy Estimate`
   - `Measured Calibration`
4. 配置不匹配相机、灯板、曝光或胶片时发出警告，不静默套用。
5. DCP/ICC 仅作为输入特性配置；DCP Look Table、Tone Curve 不进入密度路径。

### Phase 3：Status M 实用校准向导（v1.1 正式功能）

1. 用户选择或拍摄暗场、无片场、平场和认证透射目标。
2. 自动检测色块/阶梯区域，并允许人工修正。
3. 排除饱和、低信噪比、边缘污染和异常样本。
4. 拟合 `D_status_m = A * D_capture + b`；默认从 affine `3x3 + offset` 开始。
5. 按目标设计划分训练集和独立验证集，不能只报告训练误差。
6. 保存原始参考值、拟合参数、设备元数据和验证报告。
7. 若验证残差显示明显密度相关或材料相关误差，将配置标记为近似，不自动升级为高阶模型。

### Phase 4：RGB 窄谱灯板高级模式（v1.1 实验功能）

1. 支持 R/G/B 三次独立采集及其暗场、无片场和曝光元数据。
2. 记录每路 LED 的峰值、带宽和实测/厂商 SPD。
3. 拟合 Capture Separation Transform，并报告矩阵条件数与噪声放大。
4. 在独立透射目标上验证输出透射率，再进入密度拟合。
5. 不把三色 LED 同时点亮的白光帧解释成三次独立谱测量。
6. 功能在达到验收阈值前标记为 Experimental。

### Phase 5：Film Profile 与场景相对重建（可作为 v1.1 实验或后续版本）

1. 支持导入厂商特性曲线、控制条和自定义曝光阶梯数据。
2. 拟合分通道趾部、直线段和肩部。
3. 区分显示参考重建与场景相对重建。
4. 研究 Status-M-to-printing-density 的胶片专用数字 mask；不得继续使用无胶片绑定的通用矩阵。

---

## 9. 校准验收标准

每一个“Measured Calibration”至少应报告：

- 每通道密度 RMSE。
- 每通道最大绝对密度残差。
- 中性阶梯的通道差和斜率误差。
- 训练集与独立验证集误差。
- 重复拍摄的均值、标准差和漂移。
- 变换矩阵条件数或噪声放大指标。
- 饱和、无效透射率和 epsilon 替代比例。
- 有效密度范围；范围外不得外推为“已校准”。

场景相对 Film Profile 还应报告：

- 曝光阶梯恢复误差。
- 趾部、直线段和肩部的残差。
- 中性轴与通道 gamma 一致性。
- 独立色块的最终色差；色差只能作为正片重建指标，不能代替密度误差。

具体阈值应由 v1.1 的实测目标、硬件重复性和用户样本确定。在阈值确定前，软件应展示数值报告而不是使用“精准”“完美”等不可验证措辞。

---

## 10. 测试与兼容性要求

### 10.1 数值测试

- 验证所有 `log` 前矩阵只作用于线性信号。
- 验证所有密度矩阵只作用于 `-log10` 后的数据。
- 验证 Identity Profile 不改变有效样本。
- 验证暗场、无片场和平场公式的合成数据结果。
- 验证零值、负值、NaN、Infinity、饱和及退化矩阵不会静默产生正常输出。
- 验证 `f32` 管线在预览、分析和导出之间数值一致。

### 10.2 校准回归测试

- 保存小型参考 RAW、暗场、无片场和目标数据夹具。
- 为每个受支持校准版本保存预期参数和验证误差范围。
- 修改 RAW 解码、解马赛克、代理缩放或颜色数学后，必须重新运行校准回归。
- 配置中记录 `raw_decode_version` 和 `calibration_algorithm_version`，不允许算法升级后静默复用不兼容配置。

### 10.3 旧项目迁移

- v1.0.2 项目默认保持旧外观，不自动切换到新科学路径。
- 旧硬编码矩阵迁移为内置 `Legacy Estimate` Profile。
- 用户主动选择新 Capture/Density Profile 后，创建新的可撤销处理版本。
- 批量复制设置时，几何、显示参数和硬件校准配置必须分开选择。

---

## 11. 术语规范

NexFilm 文档和 UI 统一使用以下术语：

| 术语 | 含义 |
| --- | --- |
| Camera Native RGB | RAW 解马赛克后的相机本征线性通道 |
| Density Input RGB | 经过采集校准、保持正值、可用于 `-log10` 的透射率通道 |
| Capture Separation Transform | 文中窄谱灯板/传感器分离 IDT；避免与 ACES IDT 混淆 |
| Capture Profile | `log` 前的设备、灯板和曝光校准配置 |
| Status M Density | 按 ISO 5-3 Status M 光谱条件定义的密度坐标 |
| Density Profile | `log` 后从采集密度到参考密度的配置 |
| Digital Mask | 从扫描/Status M 密度到 printing density 或理论层密度的胶片相关变换 |
| Film Profile | 从校准密度到正片或场景相对曝光的胶片/冲洗模型 |
| Linear ProPhoto RGB | 正片重建后的宽色域工作空间，不是默认密度输入域 |
| Legacy Estimate | 缺少参考测量和误差报告的兼容算法 |
| End-to-End Color Calibration | 直接优化最终正片色彩、但不分离中间物理误差的校准 |

---

## 12. 参考资料

- `reference_v777.md`：负片 mask、DIR、扫描设备分离和数字 mask 的理论讨论。
- ISO 5-3:2009, *Photography and graphic technology - Density measurements - Part 3: Spectral conditions*。
- ACES Documentation, *Input Transforms*。
- Adobe, *Digital Negative (DNG) Specification*。
- International Color Consortium, ICC profile specification and input-device profiling guidance。
- Kodak, *Digital LAD Test Image - User's Guide and Digital Recorder Calibration and Aims*；其中明确说明 Status M 与 printing density 的关系随胶片变化。

本文采用的最终工程原则是：

> `log` 前校准硬件如何读取透射率；`log` 后校准扫描密度如何对应胶片或相纸密度；最后使用胶片模型重建正片。任何跨越这些边界的端到端方法都可以作为实用校色，但必须与可测量的科学管线分开标识。
