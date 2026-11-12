---
id: three-mathematical-domains
track: color-science
trackOrder: 3
chapterOrder: 1
kicker: "Color Science 01 / 数学域解耦"
title: "负片反相的三大数学域"
subtitle: "透射率域、光学密度域与正片输出域的严谨物理推导与非线性不可交换性定理。"
readingTime: 14
inApp: true
sinceVersion: "v1.1.0"
aliases: ["color-science"]
---

## 透射率域：对数变换前的光电响应与设备基底 {#transmittance-domain-pre-log}

在胶片数字化工程中，相机传感器或扫描仪线阵电荷耦合器件接收到的物理量并非密度，而是通过胶片乳剂层后到达感光单元的光子通量。透射率域特指对数变换之前、与物理光强呈正比例关系的线性光域。此阶段的目标是获取具有明确物理意义、严格位于正值区间的相对透射率向量。

假定入射胶片平面的开场照明强度为 $I_{\text{open}}(\lambda)$，穿透显影后底片的光强为 $I_{\text{transmitted}}(\lambda)$，传感器内部暗电流与环境杂散光通量为 $I_{\text{dark}}(\lambda)$，则波长 $\lambda$ 处的相对透射率定义为：

$$T(\lambda) = \frac{I_{\text{transmitted}}(\lambda) - I_{\text{dark}}(\lambda)}{I_{\text{open}}(\lambda) - I_{\text{dark}}(\lambda)}$$

在离散多通道采集设备中，传感器通道响应是透射光谱与滤光片光谱透过率及传感器量子效率的积分。令设备三通道原生输出为 $I_{\text{device}} = [R, G, B]^T$。在透射率域内，所有物理运算必须严格保持光子通量的线性叠加性：

1. **暗电平与暗场扣除**：减去传感器读出热噪声与光机腔体内部杂散反射，确保完全无光透射时的物理读数为零。
2. **平场增益归一化**：通过无底片透光的背光场测量消除透镜边缘余弦四次方照度衰减及灯板各区域色温漂移。
3. **设备通道分离**：消除相机拜尔滤色片或扫描仪分光棱镜在三原色之间的光谱重叠，使得分离后的通道能够准确衡量底片青色、品红与黄色染料层的光吸收。

对相机 RAW，Phase 4 使用 `NormalizedAsShot Camera Native RGB` 作为 log 前的密度基底，不把 camera→sRGB 显示矩阵当作采集通道分离。彩色负片的橙色色罩会让蓝通道透射率很低；相机矩阵的负绿系数与基于亮度的 gamut 压缩会在对数之前改写蓝天、湖水等关键蓝色。该矩阵改在正片归一化之后应用，并随帧记录为 `camera_to_display`。扫描仪 DNG、扫描 FFF、TIFF 与 JPEG 仍使用 Linear ProPhoto Estimate。

如果采集端未施加硬件级的通道分离校准，则直接进入后续计算会导致各染料层的吸收信息严重混叠。透射率数据必须钳位在微小正实数门限 $\epsilon$ 以上，以防止后续对数运算出现奇异值：

$$T_{\text{valid}} = \max(T, \epsilon), \quad \epsilon = 1.0 \times 10^{-6}$$

> [!IMPORTANT]
> 透射率域的数据本质是辐射度量学物理量。任何属于显示器特性的伽马编码、非线性对比度曲线或带有感知权重的色调映射，均严禁在此阶段介入。

## 光学密度域：对数变换后的吸收叠加与片基扣除 {#optical-density-domain-post-log}

透射率信号经过常用对数变换后，进入光学密度域。根据比尔-朗伯吸收定律，平行单色光穿过均匀吸收介质时，光强呈指数级衰减。取对数运算将乘性衰减转换为加性吸收，从而为胶片三层染料吸收分析提供了线性代数基础。

未校正的原始采集密度向量 $D_{\text{raw}}$ 定义为相对透射率倒数的常用对数：

$$D_{\text{raw}} = -\log_{10}(T_{\text{valid}}) = \begin{bmatrix} -\log_{10}(T_R) \\ -\log_{10}(T_G) \\ -\log_{10}(T_B) \end{bmatrix}$$

在彩色负片工艺中，未经曝光但经过标准 C-41 工艺显影的胶卷边缘存在由未反应的彩色成色剂构成的橙黄色基底，即橙色色罩。该基底引入了显著的恒定偏置。设同卷同批次测得的未曝光片基密度为 $D_{\text{base}}$，则净光学密度 $D_{\text{net}}$ 为原始采集密度与片基密度的逐通道代数差：

$$D_{\text{net}} = D_{\text{raw}} - D_{\text{base}} = \begin{bmatrix} D_{\text{raw}, R} - D_{\text{base}, R} \\ D_{\text{raw}, G} - D_{\text{base}, G} \\ D_{\text{raw}, B} - D_{\text{base}, B} \end{bmatrix}$$

在净光学密度域内，负片的显影产物与原始景物曝光量呈现出高度规律的反相响应。若胶片乳剂未进入饱和曝光肩部或严重欠曝趾部，净密度向量与感光卤化银晶体受光量 $E$ 的对数满足经典 Hurter-Driffield 关系：

$$D_{\text{net}, c} \approx \gamma_c \cdot \log_{10}(E_c) + C_c, \quad c \in \{R, G, B\}$$

其中 $\gamma_c$ 为该染料层的显影反差系数。在光学密度域中，暗房工艺的核心操作得以在简单的代数加减法中完成：

| 密度域核心运算 | 对应物理现象与暗房操作 | 运算数学表达式 |
| --- | --- | --- |
| 片基密度扣除 | 消除片基聚酯/三醋酸片基与未曝光橙色色罩吸收 | $D_{\text{net}} = D_{\text{raw}} - D_{\text{base}}$ |
| 印片光对数增益 | 模拟光学放大机色头红绿蓝加色/减色滤镜调节 | $D_{\text{graded}} = D_{\text{net}} + \Delta P_{\text{light}}$ |
| 密度动态范围缩放 | 将胶片记录的高反差密度映射至显示参考窗口 | $D_{\text{norm}} = (D_{\text{high}} - D) / (D_{\text{high}} - D_{\text{low}})$ |
| 染料串扰去耦合 | 校正品红染料副吸收导致的高密度区域偏青发暗 | $D_{\text{calibrated}} = M_{\text{crosstalk}} \cdot D_{\text{net}}$ |

## 正片与输出域：从光学密度到工作色域与显示呈现 {#positive-print-and-output-domain}

经过片基扣除与印片光调整后的胶片净密度，依然属于物理吸收测量空间，而不是适合人眼观看或进行数字艺术创作的正片图像。正片与输出域的目标，是将经过反相映射后的相对曝光量转换到具有严格色度学定义的线性工作色彩空间，并最终通过色调映射输出给目标设备。

NexFilm 引擎对非相机 RAW 输入使用线性 ProPhoto RGB 作为正片工作空间；相机 RAW 的归一化正片则先经由该帧的 camera→linear sRGB 矩阵进入同一显示与调色阶段。两种输入都必须先完成密度反相与归一化，显示矩阵不会前移到 log 之前：

1. **工作空间线性化**：将归一化对数密度转化为正片线性相对曝光 $E_{\text{pos}} = 10^{-\alpha \cdot D_{\text{graded}}}$；相机 RAW 随后应用 `camera_to_display`，ProPhoto 输入应用 ProPhoto→sRGB，使正片信号进入显示与交付阶段。
2. **分通道与合成色调曲线**：红、绿、蓝分通道曲线优先作用于各独立色阶，合成 RGB 曲线随后作用于整体亮度，模拟传统暗房相纸的感光反差特性曲线。
3. **3D 相纸模拟查找表**：导入经过严格光谱拟合的经典彩色印相纸（如 Kodak 2383、Kodak Vision Premier 2393）三维 LUT，赋予数字反相作品深沉的黑色层次与丰富的染料光泽。
4. **输出色彩空间转换与 OETF 编码**：经由矩阵变换适配目标色彩空间（如 sRGB、Display P3、Adobe RGB 或 ACEScg），并施加目标标准的光电传递函数（OETF）与色域量化裁切。

> [!NOTE]
> 在正片输出域中进行的饱和度微调、反差增强以及高光阴影压缩，属于感知色调调整范畴。这些操作不应回头改变光学密度的测量基底。

## 非线性不可交换性定理：矩阵作用域的严谨数学证明 {#non-commutative-matrix-proof}

在负片数字处理管线中，一个致命的架构错误是混淆透射率域矩阵与密度域矩阵。常有经验式软件直接在密度域施加普通显示器色彩空间转换矩阵，声称其等价于硬件分光校准。本节通过严格的泛函代数推导证明该做法在物理和数学上均属谬误。

设三通道输入透射率向量为 $x = [T_1, T_2, T_3]^T \in \mathbb{R}_{>0}^3$。设采集设备的光学通道分离校准矩阵为 $A \in \mathbb{R}^{3 \times 3}$，其元素为 $a_{ij}$。

在透射率域，硬件校准矩阵的作用是混合光通量，校正滤镜串扰：

$$y_{\text{trans}} = A x, \quad y_i = \sum_{j=1}^3 a_{ij} x_j$$

若在此之后施加对数变换进入密度域，所获得的密度响应向量为：

$$D_{\text{correct}} = -\log_{10}(A x) = \begin{bmatrix} -\log_{10}\left(\sum_{j=1}^3 a_{1j} x_j\right) \\ -\log_{10}\left(\sum_{j=1}^3 a_{2j} x_j\right) \\ -\log_{10}\left(\sum_{j=1}^3 a_{3j} x_j\right) \end{bmatrix}$$

假定某种错误实现将矩阵操作挪移到对数之后，即先计算各通道独立密度 $D_{\text{raw}} = -\log_{10}(x)$，再应用矩阵 $A$：

$$D_{\text{wrong}} = A [-\log_{10}(x)] = \begin{bmatrix} \sum_{j=1}^3 a_{1j} [-\log_{10}(x_j)] \\ \sum_{j=1}^3 a_{2j} [-\log_{10}(x_j)] \\ \sum_{j=1}^3 a_{3j} [-\log_{10}(x_j)] \end{bmatrix} = \begin{bmatrix} -\log_{10}\left(\prod_{j=1}^3 x_j^{a_{1j}}\right) \\ -\log_{10}\left(\prod_{j=1}^3 x_j^{a_{2j}}\right) \\ -\log_{10}\left(\prod_{j=1}^3 x_j^{a_{3j}}\right) \end{bmatrix}$$

对比两式的真数部分，欲使 $D_{\text{correct}} \equiv D_{\text{wrong}}$ 恒成立，必须对所有定义域内的 $x$ 满足：

$$\sum_{j=1}^3 a_{ij} x_j \equiv \prod_{j=1}^3 x_j^{a_{ij}}, \quad \forall i \in \{1, 2, 3\}$$

对该恒等式两端求关于 $x_k$ 的偏导数：

左端对 $x_k$ 的一阶偏导数为常数：

$$\frac{\partial}{\partial x_k} \left(\sum_{j=1}^3 a_{ij} x_j\right) = a_{ik}$$

右端对 $x_k$ 的一阶偏导数为：

$$\frac{\partial}{\partial x_k} \left(\prod_{j=1}^3 x_j^{a_{ij}}\right) = a_{ik} x_k^{a_{ik}-1} \prod_{j \neq k} x_j^{a_{ij}}$$

若使两端偏导数对所有 $x \in \mathbb{R}_{>0}^3$ 恒等成立，矩阵 $A$ 必须且仅能退化为单位矩阵 $I$ 或仅包含对角元素的缩放矩阵。一旦矩阵含有非对角互串扰元素（即存在 $j \neq k$ 使得 $a_{ij} \neq 0$），该恒等式必然破裂：

$$-\log_{10}(A x) \neq A [-\log_{10}(x)]$$

**结论**：光通量的物理线性叠加发生在对数变换之前；染料吸光度的物理代数叠加发生对数变换之后。试图在密度域施加普通色度矩阵来代替透射率域分光校正，不仅无法校正硬件色偏，反而在高光与阴影过渡区域引入严重的伪色与非线性畸变。

## 信号流图与 NexFilm 真实物理能力边界 {#signal-flow-and-physical-boundaries}

为了让开发者与专业调色师清晰掌握数据在引擎内的生命周期，下图展示了 NexFilm v1.1.0 的端到端信号流图。图中用不同颜色明确标识了三大数学域的分界：

<svg viewBox="0 0 860 380" width="100%" height="auto" xmlns="http://www.w3.org/2000/svg" style="background:#161618; border-radius:12px; border:1px solid #27272a; font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;">
<rect x="20" y="20" width="250" height="340" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="35" y="48" fill="#e4e4e7" font-size="12" font-weight="bold">透射率域</text>
<text x="35" y="66" fill="#a1a1aa" font-size="10">物理光子通量 · 线性辐射度</text>
<rect x="35" y="85" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="47" y="104" fill="#fafafa" font-size="11">RAW / Scanner RGB</text>
<text x="47" y="118" fill="#a1a1aa" font-size="9">黑电平解马赛克 / 坏点消除</text>
<rect x="35" y="142" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="47" y="161" fill="#fafafa" font-size="11">暗场扣除与平场归一化</text>
<text x="47" y="175" fill="#a1a1aa" font-size="9">T = (I - dark) / (open - dark)</text>
<rect x="35" y="199" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="47" y="218" fill="#fafafa" font-size="11">Capture Separation 3x3</text>
<text x="47" y="232" fill="#a1a1aa" font-size="9">相机x背光透射率通道解耦</text>
<rect x="35" y="256" width="220" height="42" rx="6" fill="#3f3f46" stroke="#fbbf24" stroke-width="1.5"/>
<text x="47" y="275" fill="#fbbf24" font-size="11" font-weight="bold">对数变换门限钳位</text>
<text x="47" y="289" fill="#fef08a" font-size="9">D_raw = -log10(max(T, eps))</text>
<line x1="145" y1="127" x2="145" y2="142" stroke="#71717a" stroke-width="1.5" marker-end="url(#arrow)"/>
<line x1="145" y1="184" x2="145" y2="199" stroke="#71717a" stroke-width="1.5" marker-end="url(#arrow)"/>
<line x1="145" y1="241" x2="145" y2="256" stroke="#71717a" stroke-width="1.5" marker-end="url(#arrow)"/>
<line x1="270" y1="277" x2="305" y2="277" stroke="#fbbf24" stroke-width="2"/>
<rect x="305" y="20" width="250" height="340" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="320" y="48" fill="#e4e4e7" font-size="12" font-weight="bold">光学密度域</text>
<text x="320" y="66" fill="#a1a1aa" font-size="10">比尔-朗伯吸光度 · 加性代数空间</text>
<rect x="320" y="85" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="104" fill="#fafafa" font-size="11">片基采样与色罩扣除</text>
<text x="332" y="118" fill="#a1a1aa" font-size="9">D_net = D_raw - D_base</text>
<rect x="320" y="142" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="161" fill="#fafafa" font-size="11">暗房印片光微调</text>
<text x="332" y="175" fill="#a1a1aa" font-size="9">Printer Lights 对数加减色平移</text>
<rect x="320" y="199" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="332" y="218" fill="#fafafa" font-size="11">有界通道跨度响应</text>
<text x="332" y="232" fill="#a1a1aa" font-size="9">1.15x死区 / 1.35x上限 / 3x截断</text>
<rect x="320" y="256" width="220" height="42" rx="6" fill="#3f3f46" stroke="#34d399" stroke-width="1.5"/>
<text x="332" y="275" fill="#34d399" font-size="11" font-weight="bold">密度反相与曝光归一化</text>
<text x="332" y="289" fill="#a7f3d0" font-size="9">E_pos = 10^(-alpha * D)</text>
<line x1="430" y1="127" x2="430" y2="142" stroke="#71717a" stroke-width="1.5"/>
<line x1="430" y1="184" x2="430" y2="199" stroke="#71717a" stroke-width="1.5"/>
<line x1="430" y1="241" x2="430" y2="256" stroke="#71717a" stroke-width="1.5"/>
<line x1="555" y1="277" x2="590" y2="277" stroke="#34d399" stroke-width="2"/>
<rect x="590" y="20" width="250" height="340" rx="8" fill="#1c1c1f" stroke="#3f3f46" stroke-width="1.5"/>
<text x="605" y="48" fill="#e4e4e7" font-size="12" font-weight="bold">正片与输出域 (Print/Output)</text>
<text x="605" y="66" fill="#a1a1aa" font-size="10">Linear ProPhoto · 色彩空间变换</text>
<rect x="605" y="85" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="104" fill="#fafafa" font-size="11">Linear ProPhoto 统一基底</text>
<text x="617" y="118" fill="#a1a1aa" font-size="9">无损正片线性光强中间表示</text>
<rect x="605" y="142" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="161" fill="#fafafa" font-size="11">四通道曲线与印相纸 LUT</text>
<text x="617" y="175" fill="#a1a1aa" font-size="9">RGB分离曲线与 Kodak 2383 等</text>
<rect x="605" y="199" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="218" fill="#fafafa" font-size="11">细节增强与色调压缩</text>
<text x="617" y="232" fill="#a1a1aa" font-size="9">高反差边缘锐化 / 纹理 / 阴影</text>
<rect x="605" y="256" width="220" height="42" rx="6" fill="#27272a" stroke="#52525b" stroke-width="1"/>
<text x="617" y="275" fill="#fafafa" font-size="11">显示与母带导出</text>
<text x="617" y="289" fill="#a1a1aa" font-size="9">sRGB / P3 / ACEScg / 16-bit TIFF</text>
<line x1="700" y1="127" x2="700" y2="142" stroke="#71717a" stroke-width="1.5"/>
<line x1="700" y1="184" x2="700" y2="199" stroke="#71717a" stroke-width="1.5"/>
<line x1="700" y1="241" x2="700" y2="256" stroke="#71717a" stroke-width="1.5"/>
</svg>

最后，本白皮书在此郑重界定 NexFilm v1.1.0 已落地能力与未来规划边界，拒绝任何技术浮夸：

1. **已落地能力**：三大数学域的清晰分离；暗场与开场平场相对透射率归一化；透射色卡采样拟合与条件数安全回退；基于有界通道跨度的抗偏色统一管线；16-bit 线性母带与相机 RAW 原样重封装 DNG 导出。
2. **当前技术边界**：当前版本未内置物理 Status M 滤光片加权积分测定库，`status_m_crosstalk_matrix()` 属于历史经验参考；未实现基于具体胶片乳剂批次物理显影参数的非线性 H-D 曲线反演。所有反相计算均基于实测透射率与严密相对密度代数模型。
