### v1.1 目标色彩管线（设计草案）

> [!IMPORTANT]
> 本节是 v1.1 的实现约束，是科学去色罩流程的系统阐述，不代表当前版本已经完成。当前版本不支持自制DCP或24色卡校正，且内部工作色域为Linear sRGB. Linear ProPhoto RGB 是统一的广色域正片工作空间，但不是“绝对光子数据”，也不应在未经验证时直接等同于胶片密度的测量域。内部主处理缓冲区应使用 `f32` 并保留超出 `[0, 1]` 的值；16 位整数只适合作为有明确定标和范围的缓存或文件格式。

#### 1. 输入识别与线性化

- **相机 RAW / 带拜尔阵列的 FFF**：解码为 Camera Native 线性响应；使用与相机、翻拍光源匹配的 DCP/输入配置文件，或 LibRaw 相机矩阵作为后备。密度分支只使用色度校准部分，不烘焙 DCP 的 Look Table、Tone Curve 或主观风格。
- **扫描仪 FFF**：按文件内容而不是仅按扩展名识别扫描仪 RGB。优先通过内嵌或指定的扫描仪 ICC 完成 TRC 反解和色彩变换；只有在没有可靠配置文件且编码曲线已知时，才使用 Gamma 1.8 等显式后备，避免重复去 Gamma。
- **带 ICC 的 TIFF/JPEG/PNG**：由内嵌配置文件一次性完成 TRC 反解，并经 PCS 转换到 Linear ProPhoto RGB。
- **无 ICC 的 JPEG/PNG**：默认按 sRGB 解释，同时在元数据中记录该假设。
- **无 ICC 的专业 TIFF**：不得静默猜测；要求用户选择输入配置文件和传递函数，例如“Adobe RGB，Gamma 2.2”或“Scanner RGB，Linear”。

```text
相机 RAW / RAW 型 FFF
Camera Native → LibRaw/DCP 相机矩阵 → Linear ProPhoto → log

扫描仪 FFF
Scanner RGB → 去除 1.8 曲线 → ICC/扫描仪描述 → Linear ProPhoto → log

带 ICC 的 TIFF/JPEG/PNG
Embedded ICC + inverse TRC → Linear ProPhoto → log

无 ICC 的 JPEG/PNG
默认按 sRGB解释 → Linear ProPhoto → log

无 ICC 的 TIFF
不能自动确定；必须让用户选择 sRGB、Adobe RGB、ProPhoto、
Linear RGB 或 Scanner RGB 假设
```

该阶段应同时保留输入配置文件、白点、传递函数和设备标识。UI 中的 **Input Space** 表示源数据的解释方式；**Working Space: Linear ProPhoto RGB** 是固定的正片调色空间，两者不是同一个选项。

#### 2. 透射率与密度

1. 在可能时使用暗场和无片光源参考计算相对透射率：`T = (scan - dark) / (light - dark)`；没有参考帧时必须把结果标记为相对测量。
2. 在经过标定且保持正值的 **Density Input RGB** 中计算 `D = -log10(max(T, epsilon))`，并记录被下限替代的无效样本。不能以“ProPhoto 色域很大”为由假设变换后一定没有负值。
3. 扣除同一采集条件下的片基密度：`D_net = D_image - D_base`。
4. 彩色负片可应用已标定的设备/光源/胶片密度变换，即Status M；未标定时使用 Identity 或明确标为 **Legacy Estimate** 的兼容矩阵。
5. 黑白负片在密度域合并为单通道。`0.2126 / 0.7152 / 0.0722` 只对应 Linear sRGB/Rec.709 坐标；若密度输入域改变，权重也必须由该域或实测响应推导，不能原样照搬。

这里必须保留一个关键约束：一般情况下 `-log10(Ax) != A[-log10(x)]`。因此不能用普通 RGB 基变换把现有矩阵从 Linear sRGB “换算”到 Linear ProPhoto。v1.1 可以把 Linear ProPhoto 作为统一交换与调色空间，但密度计算应留在经过验证的正值采集域；只有测试证明满足误差目标后，才能把 Linear ProPhoto 直接定义为 Density Input RGB。

#### 3. 反相与正片重建

- Rust 后端在统一密度数据上生成 65,536 档直方图，估计有效 D-Min/D-Max，并排除边框、齿孔、裁切外区域和无效样本。
- D-Min/D-Max 归一化、曝光、高光、阴影、白平衡及分通道参数必须声明作用域。当前兼容路径产生显示参考的正片信号；未来若要得到真正的场景线性 ProPhoto，应加入胶片型号/冲洗条件相关的特性曲线或经过验证的重建模型，不能只把归一化密度改名为“线性光”。
- Auto Invert 的估计值和用户调整保持可逆、可复制，并记录所用输入配置文件、密度校准配置和算法版本。

#### 4. 风格与输出

- LUT 必须声明预期输入域和传递函数；不明域的 `.cube` 只能作为显示参考风格使用。
- 场景线性路径执行 `Linear ProPhoto -> 目标线性 RGB -> 目标 OETF`；显示参考兼容路径必须先按其实际传递函数解码，再转换到目标空间。整个输出链只应用一次 OETF，避免重复 Gamma。
- WebGL 预览转换到显示 sRGB；导出可写入 16 位 TIFF/PNG 或 8 位 JPEG，并嵌入与像素编码一致的 ICC。量化和锐化只发生在最终输出端。

#### Status M 与标定边界

Status M 本质上是一组规定光谱响应的彩色密度测量条件，不是一个对所有相机、灯板和胶片都通用的去串扰矩阵。当前硬编码矩阵没有配套的测量数据、适用设备和误差报告，因此在工程上应视为绑定于当前处理域的经验估计；切换密度域后不能默认继续使用。

可行的标定方式是采集带参考 Status M 或光谱数据的透射目标、控制条或分层阶梯片，在固定相机、镜头、ISO、灯板光谱与曝光条件下获得 RAW 测量，然后拟合 `D_reference = A * D_capture + b`。应使用独立验证样本报告密度残差和最终色差，并把结果保存为带设备、光源、胶片及冲洗条件标识的版本化校准配置。

24 色卡适合建立相机/光源 DCP 或拟合端到端正片色彩，但不足以单独辨识负片三层染料串扰。RGB 窄谱灯板可以增加可观测性，但必须分别拍摄 R、G、B 三次、测量或记录各通道 SPD，并固定 RAW 曝光流程；一张三色合成白光照片无法独立求出三路响应。公开的胶片特性曲线和控制条目标值可以作为参考，不能替代本机采集系统与实物样本的成对测量。