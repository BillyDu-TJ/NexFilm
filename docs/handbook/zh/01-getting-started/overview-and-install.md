---
id: overview-and-install
track: getting-started
trackOrder: 1
chapterOrder: 2
kicker: "01.2 / 快速上手"
title: "认识 NexFilm：原理、安装与工作区全览"
subtitle: "从光学密度反相的基本思路，到安装包选择、系统放行与工作区结构。"
readingTime: 8
inApp: true
sinceVersion: "v1.1.0"
aliases: ["nexfilm-overview"]
---

## 核心设计理念与物理反相基础 {#core-principles}

彩色负片反相长期依赖通用图像处理软件的显示伽马曲线硬拉或预设套用。在传统数码暗房软件中，用户导入数码相机翻拍或扫描仪获取的负片 RAW/TIFF 文件后，通常通过反转 RGB 曲线端点、反复拉扯色阶滑块或应用经验黑盒色彩查找表来尝试消除橙褐色片基色罩。

这种操作路径存在根本性的物理缺陷：数码相机传感器记录的 RAW 数据处于线性透射率域，而胶片乳剂层中染料吸收光线的规律遵循比尔-朗伯定律，其光学密度与光线透射率之间呈对数比例关系。在非线性的显示伽马空间直接反转像素值，容易导致通道间数学关系错位，引发亮部高光色偏、暗部细节泥泞以及同一卷胶卷各画面色彩无法对齐等问题。

<div class="doc-callout">
    <div class="doc-callout-title">先把几个词说明白</div>
    <div class="doc-callout-body"><p><strong>光学密度</strong>可以理解成胶片“挡住多少光”的对数刻度；底片越黑，密度越高。<strong>D-Min</strong> 是未曝光片基的密度，也就是橙褐色色罩所在的位置；<strong>D-Max</strong> 是全曝光片头或最深银盐区域的密度，代表这一卷能达到的深黑端。</p><p><strong>印片光</strong>来自传统暗房：放大机灯头里加多少红、绿、蓝滤光片，会改变相纸收到的三色曝光量。NexFilm 用三个对数滑块复现这件事，所以它调整的是颜色平衡，而不只是普通的 RGB 亮度。</p></div>
</div>

NexFilm Engine 的设计核心建立在统一光学密度管线之上。软件将输入的传感器原始透射率数据通过严格的数学变换对数化为真实光学密度值，并在密度域中执行未曝光片基（$D_{\min}$）与全曝光化学上限（$D_{\max}$）的物理锚定。在密度域中，色彩平衡调整等价于向各通道密度施加纯粹的加法偏移，这完全等价于光学暗房中通过彩色放大机透镜滤光片控制红、绿、蓝印片曝光时间的物理过程。NexFilm 坚持物理可解释的算法设计，软件内部提供的 40 余款胶卷型号列表仅用于元数据归档、EXIF 信息写入以及接触印样抬头排版，实际反相运算完全依靠画面中真实测量的片基物理光学密度，而非依赖不可验证的黑盒胶卷风格滤镜。

通过理清透射率域、光学密度域与正片输出色彩空间之间的数学边界，NexFilm 实现了从单张底片快速反相到整卷胶卷色彩高度一致的工程化处理。无论面对 135 规格彩色负片、120 中画幅底片、大画幅散页片，还是大幅面滚筒扫描仪生成的线性 TIFF，软件均能以可重复、可逆且完全非破坏性的方式还原乳剂记录的原始色调。

## 跨平台安装包选型与系统要求 {#package-selection}

NexFilm 采用基于 Rust 与现代化 Web 渲染引擎的桌面架构，兼具系统底层的内存安全性与 GPU 片段着色器的高速并行计算能力。软件面向主流操作系统提供独立编译的原生安装程序。用户在部署前应核对硬件架构与系统版本，以获得稳定的运行体验。

<table class="doc-table">
    <thead>
        <tr><th>操作系统架构</th><th>安装包文件名模式</th><th>适用硬件环境</th><th>底层解码加速特性</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Windows x64</strong></td>
            <td><code>NexFilm.Engine_{version}_x64-setup.exe</code></td>
            <td>Windows 10 / 11 64 位操作系统，配备支持 WebGL 2.0 的独立显卡或集成显卡</td>
            <td>多线程 LibRaw 解码引擎，支持 Direct3D 11 后端 WebGL2 实时渲染</td>
        </tr>
        <tr>
            <td><strong>macOS Apple Silicon</strong></td>
            <td><code>NexFilm.Engine_{version}_aarch64.dmg</code></td>
            <td>搭载 Apple M1 / M2 / M3 / M4 系列芯片的 Mac 电脑，macOS 11.0 Big Sur 及更高版本</td>
            <td>原生 ARM64 架构指令集执行，集成 Metal 后端 WebKit 硬件色彩渲染加速</td>
        </tr>
        <tr>
            <td><strong>macOS Intel</strong></td>
            <td><code>NexFilm.Engine_{version}_x64.dmg</code></td>
            <td>搭载 64 位 Intel 处理器的旧款 Mac 电脑，macOS 10.15 Catalina 及更高版本</td>
            <td>x86_64 原生编译，AVX2 矢量指令集加速，支持 AMD / Intel 显卡渲染</td>
        </tr>
    </tbody>
</table>

<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">安装包下载来源辨析</div>
    <div class="doc-callout-body">从 GitHub Releases 官方发布页获取安装文件时，必须选择上方表格中对应的安装包资产。请勿下载发布页底部由 Git 自动打包的 <code>Source code (zip)</code> 或 <code>Source code (tar.gz)</code> 源码压缩包。源码压缩包不包含预编译的系统二进制程序和前端运行时，无法直接双击运行。</div>
</div>

在图像格式支持方面，NexFilm 支持数码单反与无反相机拍摄的标准 RAW 文件（如索尼 `.ARW`、佳能 `.CR2` / `.CR3`、尼康 `.NEF`、富士 `.RAF` 等）、哈苏与易麦康（Imacon / Flextight）专业底片扫描仪生成的 `.fff` 格式、各型扫描仪输出的 16 位线性 TIFF，以及标准 8 位与 16 位 TIFF/PNG 图像。需要特别注意：由于当前底层 LibRaw 依赖库的技术限制，尼康 Z8 与 Z9 机身内录制的 **HE** 与 **HE\*** 高效率压缩 NEF 格式尚无法直接被解码读取。使用此类机身翻拍底片的用户，需在相机菜单中将 RAW 录制格式设置为无损压缩，或先借助 Adobe DNG Converter 批量转为标准线性 DNG 后再导入本软件。

## 系统安全拦截与首次放行流程 {#security-clearance}

NexFilm 是一款由开源社区独立维护的自由软件。由于开发团队未向商业认证机构购买高昂的企业级代码签名证书，各操作系统内置的安全防护机制在首次启动时可能会触发安全警告或拦截提示。这些提示属于未签名开源软件的通用安全行为，软件内部不包含任何遥测追踪、广告插件或联网上传逻辑。请按照以下规范流程放行运行权限。

<div class="doc-callout">
    <div class="doc-callout-title">系统完整性保护与安全性原则</div>
    <div class="doc-callout-body">在配置 macOS 权限时，任何指导用户在终端执行 <code>csrutil disable</code> 关闭系统全局完整性保护（SIP）的行为均存在系统安全风险。NexFilm 仅需按照常规未公证应用的方式在安全设置中单独授权，严禁诱导或关闭系统全局安全防护功能。</div>
</div>

### Windows 系统的首次启动放行 {#first-launch-authorization-on-windows}

双击运行 `.exe` 安装程序时，Windows Defender SmartScreen 界面可能会弹出蓝色警告卡片，提示“Windows 已保护你的电脑，Microsoft Defender SmartScreen 已阻止启动未知应用”。放行步骤如下：

1. 在 SmartScreen 弹窗正文中，点击灰色的 **更多信息** 文本超链接。
2. 界面下方将显示该程序的实际名称与未知发布者状态，右下角将出现 **仍要运行** 操作按钮。
3. 点击 **仍要运行**，安装向导将正常启动并解压相关运行时组件至系统程序目录。

### macOS 系统的首次启动放行 {#first-launch-authorization-on-macos}

双击打开下载的 `.dmg` 磁盘映像，将 `NexFilm Engine.app` 拖拽至 **应用程序** 文件夹。首次打开时，macOS Gatekeeper 可能提示“Apple 无法检查其是否包含恶意软件”，或提示应用“已损坏”。请使用终端命令统一解除应用隔离属性：

1. 打开 **启动台** → **其他** → **终端**（也可在 **访达** → **应用程序** → **实用工具** 中打开终端）。
2. 复制下面的完整命令，粘贴到终端窗口后按 <kbd>Return</kbd>：

```bash
xattr -cr "/Applications/NexFilm Engine.app"
```

3. 如果系统要求输入密码，请输入当前 Mac 登录密码并按 <kbd>Return</kbd>。终端输入密码时不会显示字符，这是正常现象。
4. 返回 **应用程序** 文件夹，重新打开 `NexFilm Engine`。

<div class="doc-callout">
    <div class="doc-callout-title">命令说明</div>
    <div class="doc-callout-body">该命令会递归清除 <code>NexFilm Engine.app</code> 的扩展属性与下载隔离标记。它只作用于这一个应用，不会关闭 Gatekeeper、系统完整性保护（SIP）或其他全局安全功能。请确认应用已安装到 <code>/Applications</code>；如果安装在其他位置，需要将命令中的路径替换为实际路径。</div>
</div>

## 数据目录结构与源片只读保障 {#storage-architecture}

图像资产的完整性是摄影后期处理的生命线。NexFilm 在架构层面执行严格的**源片只读原则**。在导入底片、分析片基、生成 WebGL 预览以及最终批量导出的整个生命周期中，软件仅对原始文件执行流式只读读取，不会改写、覆盖、重命名或重定位用户的任何原始 RAW 或 TIFF 扫描文件。

所有的胶卷归档元数据、各帧裁剪框坐标、测光与片基几何标记、调色参数以及缩略图缓存，均集中保存在当前用户操作系统的标准应用支持目录中。

<table class="doc-table">
    <thead>
        <tr><th>运行平台</th><th>本地主数据库与工作配置目录</th><th>关键文件清单与物理用途</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Windows</strong></td>
            <td><code>%APPDATA%\NexFilm Engine\</code></td>
            <td><code>nexfilm_user.db</code>（核心 SQLite 数据库，记录图库索引与参数）<br /><code>rolls.json</code>（向后兼容的胶卷镜像归档文件）<br /><code>profiles/</code>（硬件校正采集配置文件目录）</td>
        </tr>
        <tr>
            <td><strong>macOS</strong></td>
            <td><code>~/Library/Application Support/NexFilm Engine/</code></td>
            <td><code>nexfilm_user.db</code>（核心 SQLite 数据库，保存几何与调色历史）<br /><code>rolls.json</code>（格式镜像快照）<br /><code>profiles/</code>（平场暗场与色彩校准数据）</td>
        </tr>
    </tbody>
</table>

由于软件仅在数据库中记录原始文件的绝对存储路径，当用户在操作系统中移动了原始扫描文件夹、重命名了外置移动硬盘卷标或更换了工作目录时，数据库中的文件引用将失效，在图库中会显示红色的离线标记。此时无需重新导入或重新调色，只需在图库或工作台中点击 **定位文件** 按钮，在文件选择对话框中重新指向新的文件夹路径，NexFilm 将自动基于文件名对整卷底片完成重关联并恢复所有编辑历史。

<div class="doc-callout">
    <div class="doc-callout-title">版本升级后的缩略图与缓存刷新建议</div>
    <div class="doc-callout-body">如果用户从旧版（如 <code>v1.0.2</code>）升级至 <code>v1.1.0</code> 或更新版本，由于新版引擎引入了改进的片基采样加权与有界跨度反相算法，旧版保存的缩略图配方可能显示较暗。建议打开既有胶卷，在工作台中重新点击一次 <strong>自动反相</strong>，即可刷新该帧的高清缩略图与正片配方。在初次使用本软件处理具有珍贵价值的商业胶卷前，建议先导入两三张样本底片完整演练导入、反相与导出流程，确认各环节符合预期后再进行批量作业。</div>
</div>

## 界面四大核心功能区域速览 {#interface-layout}

NexFilm 界面严格遵循暗房工作流人体工程学设计，整体布局清晰收敛，最大限度为图像预览与精密参数调节留出操作空间。软件窗口自顶向下划分为四大功能区域：

<table class="doc-table">
    <thead>
        <tr><th>界面区域</th><th>物理位置与构成要素</th><th>核心交互职责与操作特征</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>菜单栏</strong></td>
            <td>窗口最左上方导航区域，包含 <code>NEXFILM</code> 品牌标识、<strong>设置</strong>、<strong>视图</strong> 与 <strong>帮助</strong> 下拉菜单</td>
            <td>管理全局应用偏好。在 <strong>设置</strong> 中可切换中英文界面显示语言（zh-CN / en）以及浅色/深色主题；在 <strong>视图</strong> 中可快捷跳转至各子模块；在 <strong>帮助</strong> 中可查看版本声明与关于信息。</td>
        </tr>
        <tr>
            <td><strong>顶栏视图标签与全局动作</strong></td>
            <td>居中为五大主视图标签切换器，右侧固定放置三大全局主操作按钮</td>
            <td>五大视图分别为：<strong>图库</strong>、<strong>工作台</strong>、<strong>历史胶卷记录</strong>、<strong>硬件校正</strong> 与 <strong>使用文档</strong>。右侧常驻的 <strong>导出胶卷</strong>、<strong>导入胶卷</strong> 与 <strong>导出</strong> 按钮支持在任意视图下直接触发核心批量或单帧导出流程。</td>
        </tr>
        <tr>
            <td><strong>主工作区</strong></td>
            <td>占据窗口中央的主视口区域，内容随上方选中的视图标签动态切换</td>
            <td>在 <strong>图库</strong> 下呈现高响应缩略图网格与整卷元数据抬头；在 <strong>工作台</strong> 下左侧为主画布（支持 WebGL 2.0 实时显色、对焦缩放与平移），右侧为参数检查器（包含直方图、波形图、胶片范围、几何校正、片基标定、印片光与曲线等面板）；在 <strong>历史胶卷记录</strong> 下呈现过往胶卷归档档案。</td>
        </tr>
        <tr>
            <td><strong>底片条</strong></td>
            <td>工作台底部的横向可收展底片抽屉</td>
            <td>按实际曝光顺序水平陈列当前胶卷的所有画幅缩略图。底片条显示时，可通过鼠标滚轮横向滚动，或直接使用键盘快捷键 <kbd>←</kbd> 与 <kbd>→</kbd> 快速切换前一帧或后一帧。</td>
        </tr>
    </tbody>
</table>

熟练掌握各区域的分工能够显著提升暗房后期的吞吐效率。在接下来的章节中，我们将进一步深入探讨采集端的光学物理标准，为获得高保真度的色彩还原打下坚实的前期基础。
