// NexFilm Engine documentation content.
//
// Chapters are written for the people who use the application, not for the
// people who build it. Button names in this file follow the interface in both
// languages, and every claim here should be checkable in the running app.
//
// Structure:
//   DOCS[locale] -> [chapter, ...]
//   chapter = { id, kicker, title, subtitle, readingTime, sections }
//   section = { id, heading, content }   // content is trusted local HTML
(function (global) {
    'use strict';

    const CHAPTERS = {
        'zh-CN': [
            {
                id: 'quick-start',
                kicker: '01 / 快速开始',
                title: '快速开始',
                subtitle: '安装软件，导入一卷底片，完成第一次反相与导出。',
                readingTime: 6,
                sections: [
                    {
                        id: 'install',
                        heading: '安装与首次启动',
                        content: `
<p>NexFilm Engine 提供 Windows 10/11（x64）和 macOS（Apple Silicon、Intel）安装包。请从仓库的 Releases 页面下载，不要下载自动生成的 Source code 压缩包，后者不是可运行的安装程序。</p>
<ul>
    <li><strong>Windows</strong>：运行 <code>.exe</code> 安装程序。安装包尚未进行代码签名，SmartScreen 可能提示"未知发布者"。</li>
    <li><strong>macOS</strong>：打开 <code>.dmg</code>，把 NexFilm Engine 拖入"应用程序"。DMG 尚未经过 Apple 公证，首次启动被拦截时，可在 Finder 中右键应用并选择"打开"，或前往"系统设置 → 隐私与安全性"允许启动。</li>
</ul>
<p>首次启动时，应用会在系统用户目录下创建数据目录，用于保存图库索引、编辑参数和预览图。软件不会修改或移动原始扫描文件，工作过程中也不需要联网。</p>
<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">处理真实胶卷之前</div>
    <div class="doc-callout-body">请始终保留原始扫描文件的备份。第一次使用时，建议先导入少量画面，从导入、反相到导出完整走一遍，确认结果符合预期之后，再处理整卷。</div>
</div>
`
                    },
                    {
                        id: 'tour',
                        heading: '界面总览',
                        content: `
<p>窗口从上到下分为四个区域：</p>
<table class="doc-table">
    <thead>
        <tr><th>区域</th><th>用途</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>菜单栏</strong></td>
            <td>左上角的 <strong>setting</strong> 用于切换界面语言和浅色/深色主题；<strong>View</strong> 用于在视图之间跳转；<strong>Help</strong> 提供"关于 NexFilm"。</td>
        </tr>
        <tr>
            <td><strong>顶栏与视图标签</strong></td>
            <td>显示 NexFilm 版本，并提供五个视图：<strong>图库</strong>、<strong>工作台</strong>、<strong>历史胶卷记录</strong>、<strong>硬件校正</strong>、<strong>使用文档</strong>。右侧固定放置<strong>导出胶卷</strong>、<strong>导入胶卷</strong>和<strong>导出</strong>三个操作按钮。</td>
        </tr>
        <tr>
            <td><strong>主工作区</strong></td>
            <td>当前视图的内容。图库显示缩略图网格，工作台显示画布和右侧检查器，历史胶卷记录显示已归档的胶卷，硬件校正显示采集校正配置。</td>
        </tr>
        <tr>
            <td><strong>底片条</strong></td>
            <td>工作台底部按顺序列出当前胶卷的画面，用于快速切换。底片条可见时，<kbd>←</kbd> 和 <kbd>→</kbd> 也可以切换上一张和下一张。</td>
        </tr>
    </tbody>
</table>
<p>本文档中的按钮名称与中文界面一致。切换到英文界面后，五个视图对应 Library、Develop、Rolls、Hardware Calibration 和 Documentation。</p>
`
                    },
                    {
                        id: 'first-frame',
                        heading: '处理第一张底片',
                        content: `
<p>下面是一条最短的完整流程。熟练之后，一张底片通常在一分钟内就能处理完。</p>
<ol>
    <li><strong>导入。</strong>点击右上角的<strong>导入胶卷</strong>。整卷扫描选择<strong>按胶卷导入</strong>，填写画幅、相机、胶片型号和日期；单张或零星扫描可以选择<strong>散张导入</strong>，也可以直接把文件拖进窗口。导入只建立索引与预览，不会复制或改写原片。</li>
    <li><strong>进入工作台。</strong>在<strong>图库</strong>中双击任意一张缩略图，或点击顶部的<strong>工作台</strong>标签。</li>
    <li><strong>确认胶片范围。</strong>点击工具栏的<strong>设置胶片范围</strong>，再点击<strong>自动识别</strong>。检查框线是否紧贴画面边缘：尺孔和胶片边缘外露的白色灯板必须排除在外。必要时拖动四角或边缘手柄修正，然后点击<strong>保存范围</strong>。</li>
    <li><strong>反相。</strong>点击<strong>自动反相</strong>。软件会测量这一帧的片基并生成正片。同卷画面位置一致时，确认第一张后可以用<strong>批量应用</strong>把胶片范围和片基标记下发到其余画面。</li>
    <li><strong>微调。</strong>在右侧检查器中调整密度范围、印片灯光、美学等参数。所有调整都是非破坏性的，可以随时用 <kbd>Ctrl</kbd>+<kbd>Z</kbd>（macOS 为 <kbd>⌘</kbd>+<kbd>Z</kbd>）撤销。</li>
    <li><strong>导出。</strong>点击右上角的<strong>导出</strong>输出选中画面，或点击<strong>导出胶卷</strong>输出整卷。在对话框中确认格式、尺寸和目标文件夹后开始导出。</li>
</ol>
<div class="doc-callout">
    <div class="doc-callout-title">关于胶片范围的一条实用规则</div>
    <div class="doc-callout-body">范围可以略微包含片基，但不要包含尺孔和灯板。灯板的透光率远高于胶片上的任何区域，一旦被框进范围，它会成为画面中最亮的像素，把正常曝光的影调整体压低，反相结果就会发灰、发暗。</div>
</div>
`
                    },
                    {
                        id: 'whole-roll',
                        heading: '整卷的推荐做法',
                        content: `
<p>整卷处理的关键，是让同一卷的画面共享同一套物理基准。</p>
<ol>
    <li>导入整卷后，在<strong>图库</strong>中选中画面，点击工具栏右侧的<strong>标定片头片基</strong>，按提示分别采样未曝光片基和全曝光片头。片基决定色罩，片头决定密度上限；两者确定后，整卷共用同一组物理锚点。</li>
    <li>如果胶卷没有可用的片头，例如 120 胶卷，或片头已经被裁掉，跳过标定直接使用<strong>自动反相</strong>即可。此时密度上限由画面内容确定，软件会对通道跨度做有界补偿。</li>
    <li>真正需要逐张判断的是<strong>胶片范围</strong>。机位固定、画面位置一致时用<strong>批量应用</strong>一次下发；位置有明显偏移时逐张确认。</li>
    <li>调好一张基准画面后，用<strong>复制设置</strong>把影调参数应用到同场景的其他画面。复制面板可以按类别勾选要复制的内容。</li>
</ol>
<p>整卷反相也可以交给软件连续处理：当整卷已经采样了片基与片头之后，单帧的<strong>自动反相</strong>旁边会出现<strong>整卷批处理</strong>，它会先测量整卷最亮的一帧作为白点基准，再依次反相其余画面，完成后报告成功与失败的数量。</p>
`
                    },
                    {
                        id: 'where-to-look',
                        heading: '遇到问题时',
                        content: `
<ul>
    <li>反相结果不符合预期时，先检查<strong>胶片范围</strong>，再检查片基采样是否正确。多数偏色和发灰问题都来自这两步。</li>
    <li>具体现象的排查步骤见<strong>常见问题与故障排查</strong>。</li>
    <li>需要了解算法细节、输入域解析或色彩管理时，见<strong>色彩科学参考</strong>。</li>
    <li>本文档支持搜索：在右上角的搜索框中输入按钮名称或关键词，可以定位到相关小节。</li>
</ul>
`
                    }
                ]
            },
            {
                id: 'interface-basics',
                kicker: '02 / 界面与基本概念',
                title: '界面与基本概念',
                subtitle: '五个视图各自的职责，以及文档中反复出现的术语。',
                readingTime: 5,
                sections: [
                    {
                        id: 'views',
                        heading: '五个视图',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>视图</th><th>英文名称</th><th>用途</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>图库</strong></td><td>Library</td><td>当前工作胶卷的缩略图网格。选择画面、批量选择、删除画面，以及启动片头片基标定都在这里进行。</td></tr>
        <tr><td><strong>工作台</strong></td><td>Develop</td><td>处理单张画面的地方。左侧是画布和几何工具栏，右侧是检查器，底部是底片条。</td></tr>
        <tr><td><strong>历史胶卷记录</strong></td><td>Rolls</td><td>已归档胶卷的列表。可以按画幅、相机和日期筛选，查看某卷内容，继续编辑，或导出接触印相。</td></tr>
        <tr><td><strong>硬件校正</strong></td><td>Hardware Calibration</td><td>为固定的翻拍或扫描设备保存校正配置：相机、光源、镜头和参考文件。详见"硬件校正"一章。</td></tr>
        <tr><td><strong>使用文档</strong></td><td>Documentation</td><td>当前页面。</td></tr>
    </tbody>
</table>
<p>右上角的<strong>导入胶卷</strong>、<strong>导出</strong>和<strong>导出胶卷</strong>在所有视图中都可用。导入和导出都在后台执行，处理期间可以继续浏览和编辑其他画面。</p>
`
                    },
                    {
                        id: 'filmstrip',
                        heading: '底片条',
                        content: `
<p>工作台底部的底片条按顺序列出当前胶卷的全部画面。点击其中一帧即可切换，当前帧会高亮显示。底片条可见时，<kbd>←</kbd> 和 <kbd>→</kbd> 可以切换相邻画面；鼠标滚轮在底片条上滚动会横向移动列表。</p>
<p>缩略图会随编辑更新，因此在整卷处理过程中，底片条也可以用来快速比较不同画面的影调是否一致。</p>
`
                    },
                    {
                        id: 'inspector',
                        heading: '检查器',
                        content: `
<p>工作台右侧的检查器按功能分组排列：示波器、反相与重置、密度范围、印片灯光、观感调整、齿孔设置、输入色彩科学、印片胶片模拟。检查器左缘的模块导航条每个点对应一组，点击即可跳到该组，滚轮在检查器空白处滚动会翻动分组。校正配置与扫描仪输入两个 Profile 下拉框位于<strong>输入色彩科学</strong>分组内，不再单独占一组。</p>
<p>两组操作细节值得记住：</p>
<ul>
    <li>滑块需要先点击一次，之后才能用鼠标滚轮微调。这样可以避免误触滚轮改掉参数。</li>
    <li>滑块右侧的数值可以直接点击后输入，按 <kbd>Enter</kbd> 确认，按 <kbd>Esc</kbd> 取消。</li>
</ul>
<p>画布上的鼠标滚轮用于缩放预览，按住空格键拖动画面可以平移。</p>
`
                    },
                    {
                        id: 'glossary',
                        heading: '术语表',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>术语</th><th>含义</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>胶卷</strong>（Roll）</td><td>一次导入的一组画面，带画幅、相机、胶片型号和日期等资料。散张导入的画面不归属于任何胶卷。</td></tr>
        <tr><td><strong>画面</strong>（Frame）</td><td>胶卷中的一张底片，也是编辑和导出的最小单位。</td></tr>
        <tr><td><strong>胶片范围</strong>（Film Area）</td><td>框定画面有效成像区域的四边形。它决定密度计算使用哪些像素，也会排除边缘外的高光干扰。</td></tr>
        <tr><td><strong>片基</strong>（Film base）</td><td>底片上未曝光的透明部分。彩色负片的片基带有橙色色罩，测得的颜色即为去色罩的基准。</td></tr>
        <tr><td><strong>片头</strong>（Film leader）</td><td>底片最前端完全曝光、显影彻底的部分。它的密度代表这卷胶卷能达到的密度上限。</td></tr>
        <tr><td><strong>密度</strong>（Density）</td><td>透射率取负对数：阻光越多，密度越大。软件在密度域完成反相和影调映射，而不是直接在 RGB 数值上做减法。</td></tr>
        <tr><td><strong>D-Min / D-Max</strong></td><td>检查器中"密度范围"的两个端点，分别对应片基附近的低密度端和片头附近的高密度端。</td></tr>
        <tr><td><strong>印片灯光</strong>（Printer Lights）</td><td>模拟暗房放大机的曝光与滤色，按通道改变最终亮度与色彩平衡。</td></tr>
        <tr><td><strong>色罩</strong>（Mask）</td><td>彩色负片自身带有的橙色偏色。去色罩就是把这个偏色作为基准扣除。</td></tr>
        <tr><td><strong>接触印相</strong>（Contact sheet）</td><td>把整卷画面拼成一张带片边码的索引图。</td></tr>
        <tr><td><strong>输入域</strong>（Input domain）</td><td>解码后像素数值所代表的意义，包括基色、传递曲线和是否已经线性化。软件会为每个文件单独解析。</td></tr>
    </tbody>
</table>
`
                    }
                ]
            },
            {
                id: 'import-and-rolls',
                kicker: '03 / 导入与胶卷管理',
                title: '导入与胶卷管理',
                subtitle: '支持的格式、两种导入方式，以及历史胶卷记录中的常用操作。',
                readingTime: 6,
                sections: [
                    {
                        id: 'formats',
                        heading: '支持的格式',
                        content: `
<p>导入选择器接受的相机 RAW 格式包括 DNG、NEF/NRW、CR2/CR3、ARW/SRF/SR2、RAF、RW2、ORF/ORI、SRW、PEF、3FR、ERF、KDC/DCR、IIQ、MOS、MRW、X3F、RWL、FFF 和 RAW，同时也接受 TIFF、JPEG 和 PNG。实际兼容性取决于内置解码器和具体机型；遇到无法导入的文件时，请记下相机或扫描仪型号、文件格式和错误信息。</p>
<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">Nikon Z8 的 HE / HE* 压缩 NEF</div>
    <div class="doc-callout-body">这类文件目前无法导入，因为内置解码器不支持该压缩方式，软件内无法绕过。需要在 NexFilm 中处理时，请在相机里改用常规 RAW 记录格式重新拍摄。</div>
</div>
<p>JPEG 没有留下完整的线性数据，扫描仪输出的 JPEG 通常也不包含片基区域，因此反相和校色效果无法保证。使用 JPEG 时，如果结果不理想，可以在<strong>印片灯光</strong>中手动补偿。</p>
`
                    },
                    {
                        id: 'import-roll',
                        heading: '按胶卷导入',
                        content: `
<p>适用于按整卷扫描或翻拍的素材。</p>
<ol>
    <li>点击右上角的<strong>导入胶卷</strong>，在导入方式中选择<strong>按胶卷导入</strong>。</li>
    <li>填写画幅、相机、胶片型号和日期。相机和胶片型号可以直接输入新值，之后会出现在下拉列表中。</li>
    <li>选择文件。导入过程中会生成缩略图和预览，原始文件保持原样。</li>
</ol>
<p>这些资料会保存在胶卷记录中，并用于导出时的命名模板和 EXIF 写入。整卷导入的画面在<strong>历史胶卷记录</strong>中会作为一组出现，而不是散落在图库里。</p>
`
                    },
                    {
                        id: 'loose-import',
                        heading: '散张导入与拖放',
                        content: `
<p>适用于单张扫描、测试片，或者暂时不需要归档到某一卷的画面。</p>
<ul>
    <li>点击<strong>导入胶卷</strong>后选择<strong>散张导入</strong>，或直接在图库空状态中点击<strong>从磁盘导入</strong>。</li>
    <li>也可以把文件直接拖到窗口内。松开鼠标后文件会作为散张画面加入。</li>
</ul>
<p>散张画面没有胶卷资料，因此始终按单张进行分析：不参与整卷标定，也不会被整卷批处理覆盖。已经导入过的文件再次拖入时会被识别出来，不会重复添加。</p>
`
                    },
                    {
                        id: 'roll-info',
                        heading: '胶卷信息与图库',
                        content: `
<p>图库显示当前工作胶卷。如果同时处理多个胶卷，可以在<strong>历史胶卷记录</strong>中选中某一卷并选择<strong>继续编辑胶卷</strong>，它会成为当前工作卷；也可以从那里把整卷<strong>加入图库</strong>。</p>
<p>胶卷的画幅、相机、胶片型号和日期可以在历史胶卷记录中用<strong>编辑信息</strong>修改。修改后，导出命名模板中的对应字段和写入 EXIF 的内容会随之更新。</p>
<p>从图库中删除画面时，对话框会询问两种做法：只移除 NexFilm 中的记录，或者同时把源文件移入系统回收站。选择移入回收站时，仍被其他胶卷引用的文件会被保留；误删的文件可以从系统回收站恢复。</p>
`
                    },
                    {
                        id: 'rolls-view',
                        heading: '历史胶卷记录',
                        content: `
<p>左侧筛选面板可以按画幅、相机和日期组合筛选，面板顶部会显示符合条件的确切胶卷数与画面数，<strong>Clear</strong> 用于清除全部筛选条件。</p>
<p>选中一卷进入内容视图后，可以：</p>
<ul>
    <li>查看该卷的全部画面，并<strong>继续编辑胶卷</strong>或<strong>加入图库</strong>；</li>
    <li>用<strong>编辑信息</strong>修改胶卷资料；</li>
    <li>用<strong>导出接触印相</strong>生成索引图；</li>
    <li>用<strong>删除胶卷</strong>移除记录，或同时删除源文件。</li>
</ul>
`
                    },
                    {
                        id: 'missing-files',
                        heading: '源文件被移动或删除之后',
                        content: `
<p>NexFilm 只记录源文件的路径，不复制原图。源文件被移动、重命名或删除后，对应画面会显示为离线状态，缩略图仍然可见，但无法反相或导出。</p>
<p>此时使用画面上的<strong>定位文件</strong>重新关联源文件即可。如果原图已经不在，可以从图库或胶卷中移除对应记录。整个数据目录和原图目录都建议定期备份。</p>
`
                    }
                ]
            },
            {
                id: 'develop-workspace',
                kicker: '04 / 处理工作台',
                title: '处理工作台',
                subtitle: '胶片范围、片头片基标定、反相，以及检查器中的每一个面板。',
                readingTime: 12,
                sections: [
                    {
                        id: 'film-area',
                        heading: '胶片范围与几何',
                        content: `
<p>胶片范围是密度计算的边界：只有框内的像素参与片基估计和影调分析。它同时承担两个作用——确定画面边缘，以及把尺孔、片边和灯板排除在分析之外。</p>
<p>框选时的判断标准：</p>
<ul>
    <li>框线应紧贴画面的有效成像区域。常规做法是切除全部外围黑边和白边。</li>
    <li>希望软件用片边估计片基时，可以沿一侧留出 1 到 2 毫米的均匀未曝光片边，但不要露出发白的灯板。</li>
    <li>范围只影响分析，不会自动改变画面透视，也不会裁掉导出的像素。构图裁切使用工具栏的<strong>裁切</strong>。</li>
</ul>
<p>几何工具位于画布上方的工具栏：<strong>裁切</strong>与<strong>重置裁切</strong>、<strong>拉直</strong>、<strong>设置胶片范围</strong>、逆时针与顺时针旋转、水平与垂直翻转。透视和镜头畸变校正在检查器的几何分组中，可以修正翻拍架轻微的俯仰或镜头的桶形、枕形形变。</p>
`
                    },
                    {
                        id: 'auto-area',
                        heading: '自动识别与手动修正',
                        content: `
<p>点击<strong>设置胶片范围</strong>进入范围编辑状态，工具栏会切换到<strong>自动识别</strong>和<strong>保存范围</strong>。自动识别基于已缓存的缩略图估计边界，速度快，但结果需要人工确认，尤其是画面本身有大面积低反差天空或深色阴影时。</p>
<ol>
    <li>点击<strong>自动识别</strong>，等待框线出现。</li>
    <li>拖动四角或边缘手柄修正。四个角点可以独立移动。</li>
    <li>确认无误后点击<strong>保存范围</strong>。未保存就切换画面不会写入结果。</li>
</ol>
<p>同卷画面位置一致时，工具栏中的<strong>批量应用</strong>可以把当前范围下发到其他画面。批量应用只处理几何位置与片基标记；每一帧的片基仍会在目标画面上重新测量，以适应光源照度的轻微不均匀。</p>
`
                    },
                    {
                        id: 'film-calibration',
                        heading: '片头片基标定',
                        content: `
<p>片头片基标定用整卷采样确定两个物理锚点：未曝光片基的颜色，以及全曝光片头的密度上限。标定完成后，整卷共用同一组基准，跨帧的色彩一致性通常比逐张自动估计更好。</p>
<ol>
    <li>回到<strong>图库</strong>，用复选框选中包含未曝光片基和全曝光片头的画面。一卷可以选多张参考图。</li>
    <li>点击图库工具栏右侧的<strong>标定片头片基</strong>。</li>
    <li>在对话框中分别选择<strong>吸取片基</strong>和<strong>吸取片头</strong>，然后点击图片上的对应区域。点击时软件会取一个小范围的平均值，因此不必精确到单个像素，但应避开划痕、灰尘和明显的密度渐变。</li>
    <li>两个参考都完成采样后点击<strong>确定</strong>保存。</li>
</ol>
<div class="doc-callout">
    <div class="doc-callout-title">没有片头时怎么办</div>
    <div class="doc-callout-body">只采样片基也可以保存。缺少片头时，密度上限由画面内容确定，软件会限制通道跨度之间的补偿幅度，避免因为画面本身缺少纯黑而把反差拉得过高。120 胶卷和片头已被裁掉的 135 胶卷都属于这种情况。</div>
</div>
<p>标定以胶卷为单位保存，不会写入原始文件。需要重新标定时再次执行即可。</p>
`
                    },
                    {
                        id: 'invert',
                        heading: '反相',
                        content: `
                    <p>检查器的反相区域默认只有<strong>自动反相</strong>和<strong>Reset</strong>两个按钮，对应按卷未标定与 Loose Import；整卷采样了片基与片头之后，中间才会出现<strong>整卷批处理</strong>，因为整卷共用同一个白点依赖这一对物理锚点。</p>
                    <table class="doc-table">
                        <thead>
                            <tr><th>按钮</th><th>作用</th></tr>
                        </thead>
                        <tbody>
                            <tr><td><strong>自动反相</strong></td><td>处理当前画面。软件测量这一帧的片基与内容范围，生成正片并写入当前画面的参数。</td></tr>
                            <tr><td><strong>整卷批处理</strong></td><td>仅在整卷已标定片基与片头时出现。先测量整卷中最亮的一帧作为白点参考，再依次反相其余画面，最后报告成功与失败的数量。适合机位固定的整卷翻拍。</td></tr>
                            <tr><td><strong>Reset</strong></td><td>清除当前画面的色彩调整，回到未反相状态。</td></tr>
                        </tbody>
                    </table>
<p>反相是可重复执行的。调整胶片范围或片基标定之后重新执行一次，结果会按新的基准重新计算。</p>
<p>画面模式在反相按钮下方的<strong>Color</strong>和<strong>B&amp;W</strong>之间切换。黑白模式只保留密度转换，不做通道平衡，适合黑白负片和需要单独通道处理的素材。</p>
`
                    },
                    {
                        id: 'inspector-panels',
                        heading: '检查器面板',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>面板</th><th>可调整的内容</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>示波器</strong></td><td>直方图和波形图。两者都可以切换到显示通道或亮度，用于判断影调分布和色彩是否溢出。</td></tr>
        <tr><td><strong>密度范围</strong></td><td>Master D-Min 与 Master D-Max 两个端点，以及三个通道各自的微调。下面的读数显示本帧实际测得的密度端点。</td></tr>
        <tr><td><strong>印片灯光</strong></td><td>曝光、红青、绿品红、蓝黄四组控制。右上角的吸管是<strong>白平衡吸管</strong>，点击画面中的中性灰区域即可校正整体偏色，它调整的是通道曝光偏移，不是片基采样。</td></tr>
        <tr><td><strong>美学</strong></td><td>对比度、高光、阴影、饱和度、色温和色调等观感参数。这些参数作用在反相之后，不会改变物理基准。</td></tr>
        <tr><td><strong>齿孔设置</strong></td><td><strong>采样齿孔</strong>用于在正片边框上去除齿孔痕迹，另有容差与羽化两个参数控制识别范围与过渡。</td></tr>
        <tr><td><strong>输入色彩科学</strong></td><td>显示当前画面的采集色彩空间，并承载校正配置与扫描仪输入两个 Profile 下拉框。解码阶段确定后一般不需要改动。</td></tr>
        <tr><td><strong>印片胶片模拟</strong></td><td>选择内置的印片胶片与相纸 LUT，或载入自定义 <code>.cube</code> 文件，并用不透明度控制强度。选择"无内置 LUT"表示不做模拟。</td></tr>
    </tbody>
</table>
<p><strong>输入色彩科学</strong>分组内还有两个下拉框：<strong>Calibration Config Profile</strong> 按胶卷绑定硬件校正配置，<strong>Scanner Input Profile</strong> 为扫描仪指定输入配置。两者都默认使用自动判断，只有在校正配置已经建立时才需要手动选择。</p>
<p>输出的色彩空间不在检查器中设置，而是在导出对话框里选择。</p>
`
                    },
                    {
                        id: 'batch-vs-copy',
                        heading: '批量应用与复制设置',
                        content: `
<p>这两个功能都用于把一张画面的处理结果带到其他画面，但分工不同。</p>
<table class="doc-table">
    <thead>
        <tr><th></th><th>批量应用</th><th>复制 / 粘贴设置</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>位置</strong></td><td>画布上方工具栏</td><td>检查器顶部</td></tr>
        <tr><td><strong>传递内容</strong></td><td>胶片范围与片基标记</td><td>勾选的影调与几何参数</td></tr>
        <tr><td><strong>目标画面的片基</strong></td><td>在目标画面上重新测量</td><td>不改动目标画面的物理基准</td></tr>
        <tr><td><strong>适用场景</strong></td><td>整卷机位固定，需要统一范围和色罩基准</td><td>同场景画面需要统一的影调风格</td></tr>
    </tbody>
</table>
<p>批量应用之所以在每张画面上重新测量片基，是因为即使机位固定，光源照度分布、镜头暗角和胶片的轻微位移都会让不同画面的片基读数有细微差别。直接沿用源帧的数值会把这点差别放大成偏色。只有在目标画面完全测不到片基时，软件才沿用继承值，并提示确认胶片范围后重新反相。</p>
<p>粘贴设置的对话框按类别分组：扫描与密度、片基去色罩与反相、胶片模式、密度范围、变换、透视调整、印片胶片模拟、内置或自定义 LUT、齿孔采样点。面板上方提供"全部可用设置""影调与色彩""仅几何调整"三个预设，也可以逐项勾选。</p>
`
                    },
                    {
                        id: 'history-undo',
                        heading: '撤销与重置',
                        content: `
<p>所有编辑步骤都会记录在撤销历史中：<kbd>Ctrl</kbd>+<kbd>Z</kbd> 撤销，<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> 或 <kbd>Ctrl</kbd>+<kbd>Y</kbd> 重做（macOS 使用 <kbd>⌘</kbd>）。在输入框中编辑文字时，这两个快捷键不会拦截键盘输入。</p>
<p>检查器中的 <strong>Reset</strong> 清除当前画面的色彩调整；工具栏的<strong>重置裁切</strong>和检查器几何分组中的重置按钮只影响几何参数。两者互不影响。</p>
`
                    }
                ]
            },
            {
                id: 'export-and-contact-sheets',
                kicker: '05 / 导出与接触印相',
                title: '导出与接触印相',
                subtitle: '导出对话框中的每一项设置，以及整卷索引图的生成方式。',
                readingTime: 7,
                sections: [
                    {
                        id: 'export-basics',
                        heading: '导出画面',
                        content: `
<p>在<strong>图库</strong>中勾选画面后，点击右上角的<strong>导出</strong>按钮（按钮上的数字是当前选中的画面数），或者打开<strong>View → 工作台</strong>后单独导出。导出对话框打开时会显示选中数量。</p>
<p>导出在后台执行，写入文件期间可以继续浏览和编辑。每个选中的画面都会以全分辨率重新解码，原始扫描文件不会被覆盖。</p>
<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">批量导出前先试一张</div>
    <div class="doc-callout-body">正式导出整卷之前，建议先用相同设置导出其中一张，检查影调、色彩和尺寸是否符合预期。</div>
</div>
`
                    },
                    {
                        id: 'export-format',
                        heading: '格式与色彩空间',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>格式</th><th>说明</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>TIFF · 16-bit</strong></td><td>适合存档和后续精修，保留的层次最多。</td></tr>
        <tr><td><strong>TIFF · 8-bit</strong></td><td>需要 TIFF 容器但不需要高位深时使用。</td></tr>
        <tr><td><strong>PNG · 16-bit</strong></td><td>无损压缩，适合归档或交付。</td></tr>
        <tr><td><strong>JPEG · 8-bit</strong></td><td>体积小，适合分享和预览。选择后会出现质量滑块，范围 40 到 100，默认 92。</td></tr>
    </tbody>
</table>
<p>输出色彩空间可选 sRGB IEC 61966-2.1、Display P3、Adobe RGB (1998)、ITU-R BT.2020、ProPhoto RGB (ROMM RGB)、ACEScg (AP1) 和 ACES2065-1 (AP0)。选定的 ICC 配置文件会随文件一同写入。面向网络发布时选 sRGB，需要更宽色域继续处理时选 ProPhoto RGB 或 ACES 系列。</p>
<p>输出锐化提供<strong>无</strong>（保留颗粒）、<strong>低</strong>（网页与屏幕）、<strong>标准</strong>（默认）和<strong>高</strong>（小尺寸输出）四档。</p>
`
                    },
                    {
                        id: 'export-size',
                        heading: '尺寸',
                        content: `
<p>尺寸策略有两种：</p>
<ul>
    <li><strong>原始尺寸</strong>：按解码后的全分辨率输出。</li>
    <li><strong>指定长边</strong>：输入目标长边像素值，预设提供 1024、2048、4096 和 8192，默认 2048。缩放保持宽高比。</li>
</ul>
<p>勾选<strong>允许放大</strong>后，长边小于目标值的画面也会被放大；默认不勾选，避免把小图插值放大。</p>
`
                    },
                    {
                        id: 'export-files',
                        heading: '命名与重名处理',
                        content: `
<p>文件名模板支持以下变量，默认值为 <code>{Roll}_{Seq}</code>：</p>
<table class="doc-table">
    <thead>
        <tr><th>变量</th><th>含义</th></tr>
    </thead>
    <tbody>
        <tr><td><code>{Roll}</code></td><td>胶卷标识</td></tr>
        <tr><td><code>{Camera}</code></td><td>相机名称</td></tr>
        <tr><td><code>{Film}</code></td><td>胶片型号</td></tr>
        <tr><td><code>{Date}</code></td><td>拍摄日期</td></tr>
        <tr><td><code>{Original}</code></td><td>原始文件名</td></tr>
        <tr><td><code>{Seq}</code></td><td>画面序号</td></tr>
    </tbody>
</table>
<p>对话框下方会实时显示命名预览。文件名中的非法字符会被自动替换。遇到同名文件时可以选择：<strong>保留两者</strong>（自动添加后缀，默认）、<strong>替换</strong>或<strong>跳过</strong>。软件不会在未告知的情况下覆盖已有文件。</p>
<p>勾选<strong>将胶卷、胶片、相机与日期写入 EXIF</strong> 后，这些资料会随导出的文件一起保存。</p>
`
                    },
                    {
                        id: 'export-roll',
                        heading: '导出整卷',
                        content: `
<p>右上角的<strong>导出胶卷</strong>按钮直接导出当前胶卷的全部画面，使用的是同一套导出设置。如果该卷中有源文件缺失或画面缺少已保存的处理状态，软件会先提示数量并中止导出，而不是跳过这些画面。</p>
<p>导出整卷同样在后台执行，完成后会提示保存位置和成功的画面数。</p>
`
                    },
                    {
                        id: 'contact-sheet',
                        heading: '接触印相',
                        content: `
<p>接触印相把整卷画面拼成一张带片边码的索引图，用于快速浏览和归档。</p>
<ol>
    <li>打开<strong>历史胶卷记录</strong>，选中目标胶卷并进入内容视图。</li>
    <li>点击<strong>导出接触印相</strong>。</li>
    <li>选择保存位置。生成过程可能需要几秒钟。</li>
</ol>
<p>每行的画面数量由画幅决定：135 每行 6 帧，645 与 6×6 每行 4 帧，6×7 每行 3 帧，6×9 和 6×12 每行 2 帧，6×17 每行 1 帧。当前的网格不可自定义。</p>
<p>输出为 JPEG，文件名格式为 <code>contact_sheet_胶卷标识_相机.jpg</code>。片边码包含胶片型号、画面编号和 NexFilm 字标；拍摄时间和曝光参数不会印在印相上。</p>
`
                    }
                ]
            },
            {
                id: 'troubleshooting',
                kicker: '06 / 常见问题与故障排查',
                title: '常见问题与故障排查',
                subtitle: '按现象查找原因，以及每一步可以确认什么。',
                readingTime: 8,
                sections: [
                    {
                        id: 'flat',
                        heading: '反相之后画面发灰、发暗或缺少对比度',
                        content: `
<p>按顺序检查以下三项：</p>
<ol>
    <li><strong>胶片范围是否包含了灯板。</strong>这是最常见的原因。灯板的亮度远高于胶片上的任何区域，一旦进入范围就会成为分析用的最亮像素，把正常影调整体压低。重新进入<strong>设置胶片范围</strong>，把尺孔和白色底板排除在框外。</li>
    <li><strong>片基是否采样正确。</strong>片基点如果取到了画面内容而不是未曝光片基，去色罩的基准就会偏差。用<strong>标定片头片基</strong>重新采样，采样点应落在均匀的透明片边上。</li>
    <li><strong>密度范围端点是否需要微调。</strong>在<strong>密度范围</strong>面板中调整 Master D-Min 和 Master D-Max。面板下方的读数显示本帧实际测得的端点，可以作为参考。</li>
</ol>
<p>如果画面本身是阴天、雾天或夜景，缺少纯黑或纯白是正常现象。软件不会强行把内容拉满，此时适度的低反差就是正确结果。</p>
`
                    },
                    {
                        id: 'color-cast',
                        heading: '反相之后整体偏色',
                        content: `
<p>先判断偏色的性质：</p>
<ul>
    <li><strong>整卷方向一致的偏色</strong>通常来自片基基准。检查标定用的片基画面是否真的未曝光，以及是否有明显的密度渐变。</li>
    <li><strong>单张偏色且该帧画面以某种颜色为主</strong>可能是通道跨度补偿在正常工作。软件的补偿幅度有上限，不会为了迁就大面积单色而把画面拉平。如果结果仍不满意，用<strong>印片灯光</strong>手动校正。</li>
    <li><strong>边缘偏色</strong>常见于翻拍光源照度不均。让胶片范围稍微收紧，避开照度变化最明显的边缘，然后重新反相。</li>
</ul>
<p><strong>印片灯光</strong>面板右上角的吸管是白平衡吸管：点击画面中的中性灰区域，软件会据此调整通道曝光偏移。它改变的是印片灯光，不会改写片基标定结果。</p>
`
                    },
                    {
                        id: 'batch-tint',
                        heading: '批量应用之后个别画面偏色',
                        content: `
<p>批量应用会下发胶片范围与片基标记，但每一帧的片基是在目标画面上重新测量的，因为光源照度、镜头暗角和胶片位移都会让不同画面的片基读数有细微差别。如果目标画面测不到可用的片基，软件会沿用继承值并给出提示。</p>
<p>处理办法：确认该画面的胶片范围是否留出了可用的片边，然后重新运行<strong>自动反相</strong>。如果这一帧的拍摄位置与整卷差异较大，逐张确认范围比批量下发更可靠。</p>
`
                    },
                    {
                        id: 'choose-format',
                        heading: '导出时选 TIFF 还是 JPEG',
                        content: `
<ul>
    <li>需要存档、继续精修或用于打印时选 <strong>TIFF · 16-bit</strong> 或 <strong>PNG · 16-bit</strong>，并选择与之匹配的输出色彩空间。</li>
    <li>只用于分享、预览或交付小图时选 <strong>JPEG · 8-bit</strong>，色彩空间通常选 sRGB。</li>
</ul>
<p>导出体积取决于画面内容、尺寸、位深和压缩质量，没有固定的对应关系。需要比较时，用相同设置导出少量画面即可。</p>
`
                    },
                    {
                        id: 'raw-import',
                        heading: '某些 RAW 文件无法导入',
                        content: `
<p>RAW 兼容性取决于内置解码器与具体机型。可以尝试的排查：</p>
<ul>
    <li>确认文件扩展名与实际格式一致。部分相机的"RAW"实际是高效率压缩格式。</li>
    <li>Nikon Z8 的 HE 与 HE* 压缩 NEF 目前无法解码，需要在相机中改用常规 RAW 记录格式。</li>
    <li>换成 DNG 或其他被支持的格式重新导出一次源文件。</li>
    <li>如果问题依旧，请记录相机或扫描仪型号、文件格式和错误信息，通过 Issues 反馈。请勿上传含有隐私内容的原片。</li>
</ul>
`
                    },
                    {
                        id: 'preview-vs-export',
                        heading: '预览和导出结果不完全一致',
                        content: `
<p>工作台预览使用有界代理图以保持交互流畅：默认长边 2560 像素，需要时会提高到 4096。导出会对每个选中画面做全分辨率解码。两者使用同一套密度管线，因此影调与色彩应当一致，但在像素级别上，锐化、噪点和极细的边纹仍可能出现轻微差别。正式批量输出前导出样片核对，是最稳妥的做法。</p>
`
                    },
                    {
                        id: 'originals',
                        heading: '软件会修改原始文件吗',
                        content: `
<p>不会。NexFilm 不覆盖也不移动源扫描文件，图库、编辑参数和预览图都保存在应用的数据目录中。删除画面时，软件会询问是只移除记录，还是同时把源文件移入系统回收站；选择后者时，仍被其他胶卷引用的文件会被保留。</p>
<p>源文件被移动或重命名后，对应画面会离线，需要用<strong>定位文件</strong>重新关联。</p>
`
                    },
                    {
                        id: 'backup',
                        heading: '如何备份工作',
                        content: `
<p>需要备份两部分内容：原始扫描文件，以及应用的数据目录。数据目录中的主数据库 <code>nexfilm_user.db</code> 保存了胶卷资料、编辑参数和预览图；它记录的是源文件路径，不包含原图本身。只备份数据目录无法恢复被删除的原片，只备份原片则需要在重新导入后重做调整。</p>
<p>数据目录的具体位置见附录。</p>
`
                    }
                ]
            },
            {
                id: 'color-science',
                kicker: '07 / 色彩科学参考',
                title: '色彩科学参考',
                subtitle: '面向需要了解算法细节的读者：光学密度、统一密度管线与色彩管理。',
                readingTime: 12,
                sections: [
                    {
                        id: 'optical-density',
                        heading: '光学密度',
                        content: `
<p>底片的颜色不是可以直接读取的像素值。光穿过胶片时被部分吸收，不同感光层吸收的比例不同，测量这一比例才能还原被摄物的颜色。描述阻光程度的量就是光学密度：</p>
<div class="doc-code-block"><code>D = -log10(T)　　T = 透射光强 ÷ 入射光强</code></div>
<p>密度与透射率之间是非线性的对数关系。透射率减半时密度增加约 0.30，透射率降到十分之一时密度增加 1.00。这意味着同一片密度变化，在透射率较高和较低的区域对应完全不同的亮度差。直接在透射率数值上做加减，等于让阴影和高光的响应比例不一致；先取对数进入密度域，再处理影调，计算才与胶片的实际阻光行为对应。</p>
<p>片基对应密度的下限。彩色负片的片基带有橙色色罩，测得它的颜色之后，把它当作零点扣除，画面的颜色才回到中性。片头是全曝光、显影彻底的部分，对应密度的上限。两者合起来确定了这一卷可以使用的密度范围。</p>
`
                    },
                    {
                        id: 'pipeline',
                        heading: '统一密度管线',
                        content: `
<p>不论素材是整卷翻拍的 RAW、扫描仪的 FFF，还是散张导入的 TIFF，反相都会经过同一条管线。输入类型只影响第一步的输入域解析，不改变后面的密度数学。</p>
<table class="doc-table">
    <thead>
        <tr><th>步骤</th><th>内容</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>1. 解码与输入域解析</strong></td><td>确定这份文件代表什么：基色、传递曲线、是否已经线性化。解码结果统一转换到线性 ProPhoto RGB 的 32 位浮点工作域。</td></tr>
        <tr><td><strong>2. 扣除片基</strong></td><td>逐通道减去测得的片基，得到透射率，再取 <code>-log10</code> 进入密度域。</td></tr>
        <tr><td><strong>3. 建立共享窗口</strong></td><td>有可用片基时，三个通道共享同一个窗口原点，不额外施加每通道的密度偏移。</td></tr>
        <tr><td><strong>4. 通道跨度响应</strong></td><td>比较三个通道测得的跨度，在受限范围内让各通道的窗口宽度跟随实际响应。详见下一节。</td></tr>
        <tr><td><strong>5. 显示映射</strong></td><td>ProPhoto RGB 转到显示色彩空间，再应用伽马与 LUT。</td></tr>
    </tbody>
</table>
<p>如果完全测不到可用的片基，管线会退回内容对齐：用画面内容估算窗口原点，并把对齐幅度限制在 0.20 密度以内，避免画面被强行拉平。</p>
`
                    },
                    {
                        id: 'anchors',
                        heading: '物理锚点与内容范围',
                        content: `
<p>把"底片的物理基准"和"画面内容的明暗分布"分开对待，是这套管线与单纯拉伸直方图的主要区别。</p>
<p>片基与片头由胶片的化学性质决定，不随拍摄内容变化。同一卷里，无论拍的是雪山还是夜景，片基的颜色和片头的密度都是同一个物理量。因此标定完成后，整卷可以共享同一组锚点。</p>
<p>而画面内容的明暗分布是另一回事。阴雾天、夜景和舞台照片本来就没有纯黑或纯白，把直方图强行拉满会破坏真实的层次，并把暗部噪点放大。软件因此不会让内容范围覆盖物理锚点，只会用内容信息来估计锚点缺失的那一端。</p>
`
                    },
                    {
                        id: 'channel-span',
                        heading: '有界通道跨度响应',
                        content: `
<p>真实扫描中，三个通道对同一张画面的响应不会完全一致。差异可能来自胶片的感光层、扫描仪的通道增益或翻拍光源的光谱。如果完全忽略，画面会留下稳定的偏色；如果完全按测量值分离通道，色彩主体单一的画面又会被压平。软件的折中办法是给补偿设置上限和死区。</p>
<table class="doc-table">
    <thead>
        <tr><th>通道跨度比</th><th>行为</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>低于 1.15</strong></td><td>视为三通道响应一致，共享窗口保持原样，不做通道分离。</td></tr>
        <tr><td><strong>1.15 到 1.35</strong></td><td>按测量到的比例逐步放开各通道的窗口宽度。</td></tr>
        <tr><td><strong>到 1.35 为止</strong></td><td>完全采用测量值。补偿最多让各通道相差共享跨度的 1.35 倍。</td></tr>
        <tr><td><strong>超过 2.0</strong></td><td>视为上游有通道被截断（例如拼接扫描中某个通道只剩其他通道三分之一的密度范围），上限放宽到 3 倍，避免留下无法消除的偏色。</td></tr>
    </tbody>
</table>
<p>补偿后的窗口不会窄于 0.2 密度，以防退化成没有层次的显示范围。</p>
`
                    },
                    {
                        id: 'input-domain',
                        heading: '输入域解析',
                        content: `
<p>同一份像素数据，可能来自相机 RAW，也可能来自线性扫描仪输出，两者代表的意义完全不同。导入时软件按以下顺序判断输入域，先命中的规则生效：</p>
<ol>
    <li><strong>内嵌 ICC 配置文件</strong>：以文件自带的色彩配置为准，可信度最高。</li>
    <li><strong>扫描仪容器记录</strong>：Flextight / Imacon 的 FFF 及部分 TIFF 在容器中记录了基色与 Gamma，按记录值处理；记录不可读时使用文档默认值并标记为估计。</li>
    <li><strong>扫描仪输入配置</strong>：在"硬件校正"中为设备建立的输入配置，适用于非 RAW 文件与 DNG。</li>
    <li><strong>DNG</strong>：线性 RAW 的扫描仪 DNG 按线性 sRGB 读取；其余 DNG 按相机 RAW 处理。</li>
    <li><strong>其他 RAW</strong>：使用相机原生基色与相机 RAW 传递曲线。</li>
    <li><strong>兜底</strong>：其余文件按 sRGB 解释，并标记为估计结果。</li>
</ol>
<p>解析结果会随画面一起保存，可以在技术报告中查看该文件实际命中了哪一条规则。</p>
`
                    },
                    {
                        id: 'output-color',
                        heading: '输出色彩',
                        content: `
<p>输出色彩空间在导出对话框中选择，可选 sRGB、Display P3、Adobe RGB (1998)、Rec.2020、ProPhoto RGB、ACEScg 和 ACES2065-1。选定的 ICC 配置文件会写入文件，使其他软件以相同方式解释像素值。</p>
<ul>
    <li>面向网页、社交平台和大多数看图软件：<strong>sRGB</strong>。</li>
    <li>需要更宽的色域并继续在专业软件中处理：<strong>ProPhoto RGB</strong> 或 <strong>ACES</strong> 系列。</li>
    <li>匹配广色域显示器或特定印刷流程：<strong>Display P3</strong> 或 <strong>Adobe RGB (1998)</strong>。</li>
</ul>
<p>如果输出文件在别的软件中看起来偏色，先确认该软件是否读取了内嵌的 ICC 配置文件。未做色彩管理的看图工具会把任何色彩空间都当作 sRGB 显示。</p>
`
                    },
                    {
                        id: 'capture-tips',
                        heading: '翻拍与扫描建议',
                        content: `
<p>以下做法可以让后续处理更稳定：</p>
<ul>
    <li><strong>保持机位固定。</strong>整卷在同一位置拍摄，胶片范围和片基读数才能共享，批量应用也才有意义。</li>
    <li><strong>让光源均匀。</strong>平板光源边缘的照度通常比中心低几个百分点，胶片范围越靠近画面中心越安全。</li>
    <li><strong>保留一段未曝光的片边。</strong>片基是去色罩的基准，画面外留一点片边，软件就能测到真实色罩。</li>
    <li><strong>遮住环境光。</strong>翻拍时围住胶片周围，避免环境光反射到画面上。</li>
    <li><strong>用 RAW 或 16 位输出。</strong>高位深保留了更多密度层次，尤其是暗部。</li>
    <li><strong>避免把灯板拍进画面。</strong>灯板的高光会主导分析，是发灰最常见的来源。</li>
</ul>
`
                    }
                ]
            },
            {
                id: 'hardware-calibration',
                kicker: '08 / 硬件校正',
                title: '硬件校正（实验性）',
                subtitle: '为固定的翻拍或扫描设备保存采集配置，以及这项功能目前能做什么。',
                readingTime: 5,
                sections: [
                    {
                        id: 'status',
                        heading: '当前状态',
                        content: `
<p>硬件校正是较新的功能，界面中相关的验证入口也标注为实验性。它的定位是：把固定设备的采集条件记录下来，并在处理时复用，减少每次都要重新判断的环节。</p>
<p>需要明确的是，登记参考文件不等于完成了密度标定。界面中会直接提示"已配置的参考文件不是实测密度"。在验证通过之前，处理仍然使用逐帧的自动分析。</p>
<p>如果只是想处理手上的胶卷，这一章可以跳过。片头片基标定已经能覆盖绝大多数整卷需求，且不依赖硬件校正配置。</p>
`
                    },
                    {
                        id: 'profiles',
                        heading: '校正配置',
                        content: `
<p>每个配置记录一套固定采集条件：相机、光源、镜头，以及参考文件。参考文件按用途登记，包括暗帧、开放片门、平场、透射标板、片基参考、全曝光参考和光谱采集等类型。</p>
<p>配置的采集管线按顺序包含这些环节：</p>
<table class="doc-table">
    <thead>
        <tr><th>环节</th><th>内容</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>暗场扣除</strong></td><td>采集域的暗电流与黑电平参考。</td></tr>
        <tr><td><strong>开放片门归一</strong></td><td>采集域的照度与通道归一化。</td></tr>
        <tr><td><strong>平场校正</strong></td><td>采集域的照度不均与灰尘参考。</td></tr>
        <tr><td><strong>采集分离</strong></td><td>可选的设备特性拟合，在取对数之前进行。</td></tr>
        <tr><td><strong>密度参考</strong></td><td>可选的、取对数之后使用的实测标板。</td></tr>
        <tr><td><strong>胶卷锚点</strong></td><td>片基与全曝光锚点仍以胶卷为单位。</td></tr>
        <tr><td><strong>胶片重建</strong></td><td>为将来的实测胶片模型预留。</td></tr>
    </tbody>
</table>
`
                    },
                    {
                        id: 'create',
                        heading: '建立与导入配置',
                        content: `
<ol>
    <li>打开<strong>硬件校正</strong>视图。</li>
    <li>点击<strong>新建配置</strong>，填写相机、光源和镜头，并添加对应的参考文件；也可以点击<strong>导入扫描仪配置</strong>导入已有的设备配置。</li>
    <li>保存后，可以在配置详情中运行<strong>验证采集（实验性）</strong>，检查参考文件是否可用。</li>
</ol>
<p>参考文件被移动或修改后，配置详情会出现提示，此时运行时不会使用该配置。</p>
`
                    },
                    {
                        id: 'use',
                        heading: '在处理时使用',
                        content: `
<p>工作台的检查器在<strong>输入色彩科学</strong>分组内有两个下拉框：</p>
<ul>
    <li><strong>Calibration Config Profile</strong>：按胶卷绑定硬件校正配置，默认值为 Smart Auto。绑定结果会随胶卷保存，导出画面时也会保留该绑定。</li>
    <li><strong>Scanner Input Profile</strong>：为扫描仪指定输入配置，影响输入域解析中"扫描仪输入配置"这一条规则。</li>
</ul>
<p>如果绑定的配置不可用或需要处理，软件会给出提示并退回逐帧自动分析。</p>
`
                    },
                    {
                        id: 'limits',
                        heading: '当前限制',
                        content: `
<ul>
    <li>硬件校正目前提供的是采集配置管理，不代表已经完成密度标定；胶片密度环节仍标注为未验证。</li>
    <li>散张导入始终使用逐帧自动分析，不参与整卷配置。</li>
    <li>可用的参考文件类型与验证方式会随版本变化，请以界面提示为准。</li>
</ul>
`
                    }
                ]
            },
            {
                id: 'appendix',
                kicker: '09 / 附录',
                title: '附录',
                subtitle: '快捷键、数据文件位置、版本信息、已知限制与反馈渠道。',
                readingTime: 4,
                sections: [
                    {
                        id: 'shortcuts',
                        heading: '快捷键',
                        content: `
<p>应用中的全局快捷键只有下表这些。其余操作都通过界面按钮完成。</p>
<table class="doc-table">
    <thead>
        <tr><th>按键</th><th>作用</th><th>生效条件</th></tr>
    </thead>
    <tbody>
        <tr><td><kbd>Ctrl</kbd>+<kbd>Z</kbd> / <kbd>⌘</kbd>+<kbd>Z</kbd></td><td>撤销</td><td>不在文本输入框中编辑时</td></tr>
        <tr><td><kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> 或 <kbd>Ctrl</kbd>+<kbd>Y</kbd></td><td>重做</td><td>不在文本输入框中编辑时</td></tr>
        <tr><td><kbd>←</kbd> / <kbd>→</kbd></td><td>切换到上一张或下一张画面</td><td>底片条可见时</td></tr>
        <tr><td><kbd>Enter</kbd></td><td>确认裁切</td><td>裁切模式中</td></tr>
        <tr><td><kbd>Esc</kbd></td><td>关闭当前对话框、菜单或取消数值输入</td><td>对话框或菜单打开时</td></tr>
        <tr><td>按住 <kbd>Space</kbd> 拖动</td><td>平移画布</td><td>已打开画面时</td></tr>
        <tr><td>鼠标滚轮</td><td>缩放预览；在底片条上横向翻动</td><td>工作台中</td></tr>
        <tr><td>鼠标滚轮</td><td>微调滑块数值</td><td>先点击过该滑块</td></tr>
    </tbody>
</table>
<p>画布上还支持以下鼠标操作：拖动平移，滚轮以指针位置为中心缩放。检查器中的滑块支持拖动和点击轨道定位。</p>
`
                    },
                    {
                        id: 'data-locations',
                        heading: '数据文件位置',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>平台</th><th>数据目录</th></tr>
    </thead>
    <tbody>
        <tr><td>Windows 正式版</td><td><code>%APPDATA%\\NexFilm Engine\\</code></td></tr>
        <tr><td>macOS 正式版</td><td><code>~/Library/Application Support/NexFilm Engine/</code></td></tr>
        <tr><td>Debug 构建</td><td>仓库工作目录</td></tr>
    </tbody>
</table>
<p>主数据库为 <code>nexfilm_user.db</code>，保存胶卷资料、画面索引、编辑参数、输入域记录和预览图，不包含完整的原图副本。<code>rolls.json</code> 是兼容用的镜像文件。</p>
<p>备份时请同时保留原图目录与数据目录。数据库记录的是源文件的绝对路径，移动原图后需要用<strong>定位文件</strong>重新关联。</p>
`
                    },
                    {
                        id: 'version',
                        heading: '版本与更新',
                        content: `
<p>当前稳定版本为 <strong>v1.0.2</strong>。安装包与更新说明统一发布在仓库的 Releases 页面，请以那里为准。</p>
<p>升级不会删除数据目录，但涉及数据库结构变化时软件会在首次启动时自动迁移。跨越较大版本升级之前，建议先备份数据目录。</p>
`
                    },
                    {
                        id: 'known-limits',
                        heading: '已知限制',
                        content: `
<ul>
    <li>Windows 与 macOS 安装包尚未进行代码签名，macOS DMG 尚未经过 Apple 公证。</li>
    <li>Nikon Z8 的 HE 与 HE* 压缩 NEF 目前无法解码。</li>
    <li>不同相机、扫描仪与 FFF 文件的兼容性仍需在更多设备上验证。</li>
    <li>目前没有经过验证的 Linux 正式安装包。</li>
    <li>数据库引用源文件路径，不复制原图；移动或删除源文件会导致画面离线。</li>
    <li>预览使用有界代理图；与全分辨率导出在像素级别上可能仍有细微差别。</li>
    <li>硬件校正中的采集验证仍为实验性。</li>
</ul>
`
                    },
                    {
                        id: 'license',
                        heading: '许可与致谢',
                        content: `
<p>NexFilm Engine 以 <strong>GNU General Public License v3.0 only</strong> 发布，完整文本见仓库根目录的 LICENSE 文件。使用、修改和再分发请遵循该许可的条款。</p>
<p>感谢知乎用户 @黄昊Haosky 与 @V777 分享的科学去色罩理论，以及所有提交问题与建议的使用者。内置的印片胶片与相纸 LUT 来自仓库中随应用分发的 <code>.cube</code> 文件，可以在"印片胶片模拟"面板中使用。</p>
`
                    },
                    {
                        id: 'feedback',
                        heading: '反馈与贡献',
                        content: `
<p>问题与建议请提交到仓库的 Issues。提交问题时请包含以下信息：</p>
<ul>
    <li>操作系统版本与软件版本号；</li>
    <li>相机或扫描仪型号、文件格式；</li>
    <li>复现步骤与完整的错误信息。</li>
</ul>
<p>请勿上传包含隐私内容的原片。代码贡献请保持改动聚焦，并在 Pull Request 中说明验证方式。</p>
`
                    }
                ]
            },
        ],
        'en': [
            {
                id: 'quick-start',
                kicker: '01 / Quick Start',
                title: 'Quick Start',
                subtitle: 'Install the app, import a roll, and finish your first inversion and export.',
                readingTime: 6,
                sections: [
                    {
                        id: 'install',
                        heading: 'Installing and first launch',
                        content: `
<p>NexFilm Engine ships installers for Windows 10/11 (x64) and macOS (Apple Silicon and Intel). Download from this repository's Releases page. The automatically generated Source code archives are not runnable installers.</p>
<ul>
    <li><strong>Windows</strong>: run the <code>.exe</code> installer. The package is not code-signed, so SmartScreen may warn about an unknown publisher.</li>
    <li><strong>macOS</strong>: open the <code>.dmg</code> and drag NexFilm Engine into Applications. The DMG is not notarized by Apple; if the first launch is blocked, right-click the app in Finder and choose Open, or allow it under System Settings → Privacy &amp; Security.</li>
</ul>
<p>On first launch the app creates a data directory in your user account to hold the library index, edit parameters, and previews. NexFilm does not modify or move your source scans, and it does not need a network connection to work.</p>
<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">Before you process real film</div>
    <div class="doc-callout-body">Always keep a backup of the original scans. For a first run, import a few frames and take them all the way through inversion and export before committing a whole roll.</div>
</div>
`
                    },
                    {
                        id: 'tour',
                        heading: 'The window at a glance',
                        content: `
<p>The window is divided into four areas from top to bottom:</p>
<table class="doc-table">
    <thead>
        <tr><th>Area</th><th>What it does</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Menu bar</strong></td>
            <td><strong>setting</strong> switches the interface language and the light/dark theme. <strong>View</strong> jumps between views. <strong>Help</strong> opens About NexFilm.</td>
        </tr>
        <tr>
            <td><strong>Header and view tabs</strong></td>
            <td>Shows the NexFilm version and the five views: <strong>Library</strong>, <strong>Develop</strong>, <strong>Rolls</strong>, <strong>Hardware Calibration</strong>, and <strong>Documentation</strong>. The right side holds <strong>Export Roll</strong>, <strong>Import Roll</strong>, and <strong>Export</strong>.</td>
        </tr>
        <tr>
            <td><strong>Main workspace</strong></td>
            <td>The content of the active view: a thumbnail grid in Library, the canvas and inspector in Develop, archived rolls in Rolls, and capture profiles in Hardware Calibration.</td>
        </tr>
        <tr>
            <td><strong>Filmstrip</strong></td>
            <td>Lists the frames of the current roll along the bottom of Develop for quick switching. While it is visible, <kbd>←</kbd> and <kbd>→</kbd> also move between frames.</td>
        </tr>
    </tbody>
</table>
<p>Button names in this manual match the English interface. In the Chinese interface the five views are 图库, 工作台, 历史胶卷记录, 硬件校正, and 使用文档.</p>
`
                    },
                    {
                        id: 'first-frame',
                        heading: 'Your first frame',
                        content: `
<p>This is the shortest complete path. With a little practice, a single frame takes about a minute.</p>
<ol>
    <li><strong>Import.</strong> Choose <strong>Import Roll</strong> in the upper-right corner. Use <strong>Import by Roll</strong> for a complete roll and fill in format, camera, film stock, and date. Use <strong>Loose Import</strong> for single scans, or drag files into the window. Import only creates an index and previews; originals are never copied or changed.</li>
    <li><strong>Open Develop.</strong> Double-click any thumbnail in <strong>Library</strong>, or select the <strong>Develop</strong> tab.</li>
    <li><strong>Confirm the film area.</strong> Choose <strong>Set Film Area</strong>, then <strong>Auto Area</strong>. Check that the outline hugs the image: sprocket holes and any bare light panel must stay outside. Adjust corners or edges if needed, then choose <strong>Save Area</strong>.</li>
    <li><strong>Invert.</strong> Choose <strong>Auto Invert</strong>. The app measures the film base for this frame and produces a positive. When every frame sits in the same place, use <strong>Batch Apply</strong> to send the area and base marker to the rest of the roll.</li>
    <li><strong>Refine.</strong> Adjust Density Limits, Printer Lights, Aesthetics, and the other groups in the inspector. Every adjustment is non-destructive, and <kbd>Ctrl</kbd>+<kbd>Z</kbd> (<kbd>⌘</kbd>+<kbd>Z</kbd> on macOS) undoes it.</li>
    <li><strong>Export.</strong> Use <strong>Export</strong> for the selected frames or <strong>Export Roll</strong> for the whole roll, confirm format, size, and destination, then start the export.</li>
</ol>
<div class="doc-callout">
    <div class="doc-callout-title">One practical rule for the film area</div>
    <div class="doc-callout-body">The area may include a little film base, but it must not include sprocket holes or the light panel. The panel transmits far more light than any part of the film; if it is inside the area it becomes the brightest pixel, the normal exposure range gets pushed down, and the positive looks flat and dark.</div>
</div>
`
                    },
                    {
                        id: 'whole-roll',
                        heading: 'How to handle a whole roll',
                        content: `
<p>The key to a consistent roll is letting every frame share the same physical reference.</p>
<ol>
    <li>After importing a roll, select frames in <strong>Library</strong> and choose <strong>Calibrate Film</strong> in the toolbar. Sample the unexposed film base and the fully exposed film leader. The base sets the mask reference and the leader sets the density ceiling; once both are set, the roll shares one pair of anchors.</li>
    <li>If the roll has no usable leader, for example 120 film or a 135 roll whose leader was cut off, skip the calibration and use <strong>Auto Invert</strong>. The density ceiling then comes from the frame content, with bounded compensation between channels.</li>
    <li>The step that genuinely needs a human decision is the <strong>film area</strong>. When the camera position is fixed, <strong>Batch Apply</strong> sends it to the whole roll in one action; when frame placement varies, confirm each frame.</li>
    <li>Once one reference frame looks right, use <strong>Copy Settings</strong> to carry the tone parameters to similar frames. The copy panel lets you tick the categories you want.</li>
</ol>
<p>The app can also invert a roll continuously: once the roll has sampled film base and leader, <strong>Invert Whole Roll</strong> appears next to <strong>Auto Invert</strong>, measuring the brightest frame as a white point and then processing the rest, reporting how many frames succeeded and failed.</p>
`
                    },
                    {
                        id: 'where-to-look',
                        heading: 'When something looks wrong',
                        content: `
<ul>
    <li>If the positive does not look right, check the <strong>film area</strong> first, then the film base sample. Most casts and flat results come from those two steps.</li>
    <li>See <strong>FAQ &amp; Troubleshooting</strong> for symptom-by-symptom checks.</li>
    <li>See <strong>Color Science Reference</strong> for the algorithm, input domain resolution, and color management.</li>
    <li>This manual is searchable: type a button name or keyword in the search box to jump to the matching section.</li>
</ul>
`
                    }
                ]
            },
            {
                id: 'interface-basics',
                kicker: '02 / Interface & Concepts',
                title: 'Interface and Key Concepts',
                subtitle: 'What each view is for, and the terms used throughout this manual.',
                readingTime: 5,
                sections: [
                    {
                        id: 'views',
                        heading: 'The five views',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>View</th><th>Chinese name</th><th>Purpose</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Library</strong></td><td>图库</td><td>Thumbnail grid of the current roll. Select and delete frames, and start film calibration here.</td></tr>
        <tr><td><strong>Develop</strong></td><td>工作台</td><td>Single-frame work: canvas and geometry toolbar on the left, inspector on the right, filmstrip along the bottom.</td></tr>
        <tr><td><strong>Rolls</strong></td><td>历史胶卷记录</td><td>Archived rolls. Filter by format, camera, and date; open a roll, continue editing, or export a contact sheet.</td></tr>
        <tr><td><strong>Hardware Calibration</strong></td><td>硬件校正</td><td>Saved capture profiles for a fixed scanning or copy setup: camera, light source, lens, and reference files. See the Hardware Calibration chapter.</td></tr>
        <tr><td><strong>Documentation</strong></td><td>使用文档</td><td>This page.</td></tr>
    </tbody>
</table>
<p><strong>Import Roll</strong>, <strong>Export</strong>, and <strong>Export Roll</strong> stay available in every view. Both import and export run in the background, so you can keep browsing and editing while they work.</p>
`
                    },
                    {
                        id: 'filmstrip',
                        heading: 'The filmstrip',
                        content: `
<p>The filmstrip at the bottom of Develop lists every frame of the current roll in order. Click a frame to switch to it; the active frame is highlighted. While the filmstrip is visible, <kbd>←</kbd> and <kbd>→</kbd> move between adjacent frames, and the mouse wheel scrolls the strip sideways.</p>
<p>Thumbnails update as you edit, so the filmstrip is also a quick way to check whether the tone of a roll is consistent.</p>
`
                    },
                    {
                        id: 'inspector',
                        heading: 'The inspector',
                        content: `
<p>The inspector on the right of Develop is grouped by function: Scopes, Invert &amp; Reset, Density Limits, Printer Lights, Aesthetics, Sprocket Settings, Input Color Science, Print Film Emulation. Each dot on the module strip beside the inspector maps to one group, and the wheel scrolls between groups when the pointer is over empty inspector space. The Calibration Config Profile and Scanner Input Profile selectors live inside the <strong>Input Color Science</strong> group rather than occupying groups of their own.</p>
<p>Two interaction details are worth remembering:</p>
<ul>
    <li>A slider must be clicked once before the mouse wheel will adjust it. This prevents accidental changes while scrolling the panel.</li>
    <li>The numeric readout beside each slider can be clicked and typed into. <kbd>Enter</kbd> confirms and <kbd>Esc</kbd> cancels.</li>
</ul>
<p>On the canvas, the mouse wheel zooms the preview and holding <kbd>Space</kbd> while dragging pans it.</p>
`
                    },
                    {
                        id: 'glossary',
                        heading: 'Glossary',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>Term</th><th>Meaning</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Roll</strong></td><td>One imported set of frames with format, camera, film stock, and date. Loose frames belong to no roll.</td></tr>
        <tr><td><strong>Frame</strong></td><td>A single negative in a roll, and the smallest unit of editing and export.</td></tr>
        <tr><td><strong>Film area</strong></td><td>The quadrilateral marking the usable image region. It decides which pixels feed the density analysis and excludes bright edges.</td></tr>
        <tr><td><strong>Film base</strong></td><td>The unexposed, clear part of the negative. On color negative film it carries the orange mask, and measuring it is the reference for mask removal.</td></tr>
        <tr><td><strong>Film leader</strong></td><td>The fully exposed, fully developed start of the roll. Its density is the upper limit the film can reach.</td></tr>
        <tr><td><strong>Density</strong></td><td>Negative log of transmission: the more light an area blocks, the higher its density. Inversion and tone mapping happen in the density domain rather than by subtracting RGB values.</td></tr>
        <tr><td><strong>D-Min / D-Max</strong></td><td>The two endpoints in the Density Limits panel, near the film base and near the leader respectively.</td></tr>
        <tr><td><strong>Printer Lights</strong></td><td>An emulation of enlarger exposure and filtration: per-channel changes to final brightness and color balance.</td></tr>
        <tr><td><strong>Mask</strong></td><td>The orange cast built into color negative film. Removing the mask means subtracting it as the reference.</td></tr>
        <tr><td><strong>Contact sheet</strong></td><td>One index image containing every frame of the roll with edge codes.</td></tr>
        <tr><td><strong>Input domain</strong></td><td>What the decoded pixel values mean: primaries, transfer curve, and whether they are linear. Resolved per file.</td></tr>
    </tbody>
</table>
`
                    }
                ]
            },
            {
                id: 'import-and-rolls',
                kicker: '03 / Import & Roll Management',
                title: 'Import and Roll Management',
                subtitle: 'Supported formats, the two import paths, and everyday work in Rolls.',
                readingTime: 6,
                sections: [
                    {
                        id: 'formats',
                        heading: 'Supported formats',
                        content: `
<p>The import picker accepts common camera RAW formats: DNG, NEF/NRW, CR2/CR3, ARW/SRF/SR2, RAF, RW2, ORF/ORI, SRW, PEF, 3FR, ERF, KDC/DCR, IIQ, MOS, MRW, X3F, RWL, FFF, and RAW, plus TIFF, JPEG, and PNG. Compatibility depends on the bundled decoder and the specific device. If a file will not import, note the camera or scanner model, the file format, and the error message.</p>
<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">Nikon Z8 HE / HE* compressed NEF</div>
    <div class="doc-callout-body">These files cannot be imported at present because the bundled decoder does not support that compression, and the app cannot work around it. To use NexFilm, switch the camera to a standard RAW recording format.</div>
</div>
<p>JPEG does not retain complete linear data, and scanner JPEGs usually contain no film base region, so inversion and grading results cannot be guaranteed. If a JPEG result is disappointing, compensate manually with <strong>Printer Lights</strong>.</p>
`
                    },
                    {
                        id: 'import-roll',
                        heading: 'Import by Roll',
                        content: `
<p>Use this for material scanned or copied as a complete roll.</p>
<ol>
    <li>Choose <strong>Import Roll</strong> and select <strong>Import by Roll</strong>.</li>
    <li>Fill in format, camera, film stock, and date. New camera and film names can be typed directly and will appear in the lists afterwards.</li>
    <li>Select the files. Thumbnails and previews are generated during import; the originals stay untouched.</li>
</ol>
<p>These details are stored with the roll and feed the export filename template and EXIF writing. A roll imported this way appears as a group in <strong>Rolls</strong> rather than as loose frames in the library.</p>
`
                    },
                    {
                        id: 'loose-import',
                        heading: 'Loose Import and drag and drop',
                        content: `
<p>Use this for single scans, test frames, or images that do not need to belong to a roll.</p>
<ul>
    <li>Choose <strong>Import Roll</strong> and then <strong>Loose Import</strong>, or use <strong>Import From Disk</strong> from the empty library state.</li>
    <li>Files can also be dragged straight into the window. They are added as loose frames when the drop is released.</li>
</ul>
<p>Loose frames have no roll metadata and are always analysed individually. They never take part in roll calibration and are not covered by Invert Whole Roll. Files that were already imported are recognised and are not added twice.</p>
`
                    },
                    {
                        id: 'roll-info',
                        heading: 'Roll details and the library',
                        content: `
<p>Library shows the current working roll. When more than one roll is in play, select one in <strong>Rolls</strong> and choose <strong>Continue Editing Roll</strong> to make it current, or use <strong>Promote to Library</strong> to bring it in.</p>
<p>Format, camera, film stock, and date can be changed with <strong>Edit Info</strong> in Rolls. The export filename template and the EXIF fields follow those values.</p>
<p>When deleting frames from the library, the dialog asks whether to remove only the NexFilm record or to move the source files to the system trash as well. Files still referenced by another roll are kept, and anything sent to the trash can be restored from there.</p>
`
                    },
                    {
                        id: 'rolls-view',
                        heading: 'Rolls',
                        content: `
<p>The filter panel on the left combines format, camera, and date filters and shows the exact number of matching rolls and frames. <strong>Clear</strong> removes every filter.</p>
<p>Open a roll to:</p>
<ul>
    <li>review every frame and use <strong>Continue Editing Roll</strong> or <strong>Promote to Library</strong>;</li>
    <li>change roll details with <strong>Edit Info</strong>;</li>
    <li>generate an index with <strong>Export Contact Sheet</strong>;</li>
    <li>remove the record, or the record and its source files, with <strong>Delete Rolls</strong>.</li>
</ul>
`
                    },
                    {
                        id: 'missing-files',
                        heading: 'After source files move or disappear',
                        content: `
<p>NexFilm stores the path to each source file rather than a copy of it. If a file is moved, renamed, or deleted, the frame goes offline: the thumbnail remains but the frame cannot be inverted or exported.</p>
<p>Use <strong>Locate File</strong> on the frame to reconnect it. If the original is gone for good, remove the record from the library or the roll. Back up both your originals and the application data directory.</p>
`
                    }
                ]
            },
            {
                id: 'develop-workspace',
                kicker: '04 / Develop',
                title: 'The Develop Workspace',
                subtitle: 'Film area, film calibration, inversion, and every inspector panel.',
                readingTime: 12,
                sections: [
                    {
                        id: 'film-area',
                        heading: 'Film area and geometry',
                        content: `
<p>The film area is the boundary for density analysis: only pixels inside it feed the base estimate and the tone analysis. It does two jobs at once, defining the image edges and keeping sprocket holes, film edges, and the light panel out of the analysis.</p>
<p>How to judge it:</p>
<ul>
    <li>The outline should hug the usable image region. The usual practice is to exclude all surrounding black and white borders.</li>
    <li>If you want the base estimated from the film edge, leave 1 to 2 mm of even, unexposed edge along one side, but never expose the light panel.</li>
    <li>The area only affects analysis. It does not change perspective and does not crop exported pixels; composition cropping uses <strong>Crop</strong> in the toolbar.</li>
</ul>
<p>Geometry tools sit above the canvas: <strong>Crop</strong> and <strong>Reset Crop</strong>, <strong>Straighten</strong>, <strong>Set Film Area</strong>, rotate counterclockwise and clockwise, and horizontal and vertical flip. Perspective and lens distortion correction live in the geometry group of the inspector, and can correct a slight copy-stand tilt or barrel and pincushion distortion.</p>
`
                    },
                    {
                        id: 'auto-area',
                        heading: 'Auto Area and manual correction',
                        content: `
<p>Choose <strong>Set Film Area</strong> to enter area editing; the toolbar switches to <strong>Auto Area</strong> and <strong>Save Area</strong>. Auto Area estimates the boundary from the cached thumbnail, which is fast but always needs a human check, especially when the picture contains large low-contrast sky or deep shadow.</p>
<ol>
    <li>Choose <strong>Auto Area</strong> and wait for the outline.</li>
    <li>Drag the corners or edge handles to correct it. The four corners move independently.</li>
    <li>Choose <strong>Save Area</strong> when it is right. Switching frames without saving discards the result.</li>
</ol>
<p>When every frame in the roll sits in the same place, <strong>Batch Apply</strong> in the toolbar sends the current area to the others. It only transfers geometry and the base marker; each target frame still re-measures its own film base to absorb slight unevenness in the light source.</p>
`
                    },
                    {
                        id: 'film-calibration',
                        heading: 'Film calibration',
                        content: `
<p>Film calibration uses roll-wide samples to establish two physical anchors: the colour of the unexposed film base and the density ceiling of the fully exposed leader. Once calibrated, the whole roll shares one reference, which usually gives more consistent colour across frames than per-frame estimates.</p>
<ol>
    <li>Go back to <strong>Library</strong> and tick the frames that contain unexposed base and a fully exposed leader. A roll may use several reference frames.</li>
    <li>Choose <strong>Calibrate Film</strong> in the library toolbar.</li>
    <li>In the dialog, pick <strong>Sample film base</strong> or <strong>Sample film leader</strong> and click the matching area on the image. A small neighbourhood is averaged, so the click does not need to be pixel-exact, but avoid scratches, dust, and obvious density gradients.</li>
    <li>Once both references are sampled, choose <strong>Confirm</strong> to save.</li>
</ol>
<div class="doc-callout">
    <div class="doc-callout-title">When there is no leader</div>
    <div class="doc-callout-body">Sampling only the base is allowed. Without a leader, the density ceiling comes from frame content, and the app limits how far the channel spans may be pulled apart so that a picture with no true black does not get an exaggerated contrast. 120 film and 135 rolls with the leader cut off both behave this way.</div>
</div>
<p>Calibration is stored per roll and never written to the original files. Run it again whenever you want to re-anchor the roll.</p>
`
                    },
                    {
                        id: 'invert',
                        heading: 'Inverting',
                        content: `
<p>The inspector ships with two inversion buttons by default, <strong>Auto Invert</strong> and <strong>Reset</strong>, which cover roll imports without sampled anchors and Loose Import. <strong>Invert Whole Roll</strong> only appears once the roll has sampled film base and leader, because a roll-wide white point depends on that pair of physical anchors.</p>
                    <table class="doc-table">
                        <thead>
                            <tr><th>Button</th><th>What it does</th></tr>
                        </thead>
                        <tbody>
                            <tr><td><strong>Auto Invert</strong></td><td>Processes the current frame, measuring its base and content range, writing the result into that frame's parameters.</td></tr>
                            <tr><td><strong>Invert Whole Roll</strong></td><td>Only shown when the roll has sampled base and leader anchors. Measures the brightest frame of the roll as a white point, then inverts the rest, reporting how many frames succeeded and failed. Suited to rolls copied with a fixed setup.</td></tr>
                            <tr><td><strong>Reset</strong></td><td>Clears the colour adjustments on the current frame and returns it to the un-inverted state.</td></tr>
                        </tbody>
                    </table>
<p>Inversion can be run repeatedly. After changing the film area or the calibration, run it again and the result is recomputed from the new reference.</p>
<p>Film mode switches between <strong>Color</strong> and <strong>B&amp;W</strong> below the buttons. Black and white keeps the density conversion and drops channel balancing, which suits black and white negatives and material you want to handle channel by channel.</p>
`
                    },
                    {
                        id: 'inspector-panels',
                        heading: 'Inspector panels',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>Panel</th><th>What you can change</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Scopes</strong></td><td>Histogram and waveform, each switchable between channels and luminance, for judging tone distribution and clipping.</td></tr>
        <tr><td><strong>Density Limits</strong></td><td>Master D-Min and Master D-Max plus per-channel trims. The readout below shows the endpoints actually measured for this frame.</td></tr>
        <tr><td><strong>Printer Lights</strong></td><td>Exposure plus red/cyan, green/magenta, and blue/yellow. The eyedropper in the panel header is the <strong>White Balance Eyedropper</strong>: click a neutral grey area in the picture to correct an overall cast. It adjusts channel exposure offsets, not the film base sample.</td></tr>
        <tr><td><strong>Aesthetics</strong></td><td>Contrast, highlights, shadows, saturation, temperature, and tint. These act after inversion and do not change the physical reference.</td></tr>
        <tr><td><strong>Sprocket Settings</strong></td><td><strong>Sample Sprocket Hole</strong> removes perforation marks from the positive border, with tolerance and feather controls for detection range and transition.</td></tr>
        <tr><td><strong>Input Color Science</strong></td><td>Shows the capture colour space of the current frame and holds the Calibration Config and Scanner Input profile selectors. Once decoding has resolved it, it rarely needs changing.</td></tr>
        <tr><td><strong>Print Film Emulation</strong></td><td>Choose a bundled print film or paper LUT, or load a custom <code>.cube</code> file, with opacity control. "No Built-in LUT" disables the emulation.</td></tr>
    </tbody>
</table>
<p>Two selectors inside the <strong>Input Color Science</strong> group: <strong>Calibration Config Profile</strong> binds a hardware calibration profile to the roll, and <strong>Scanner Input Profile</strong> names the input profile for a scanner. Both default to automatic and only need changing after a profile exists.</p>
<p>The output colour space is not set in the inspector; it is chosen in the export dialog.</p>
`
                    },
                    {
                        id: 'batch-vs-copy',
                        heading: 'Batch Apply versus Copy Settings',
                        content: `
<p>Both carry work from one frame to others, but they carry different things.</p>
<table class="doc-table">
    <thead>
        <tr><th></th><th>Batch Apply</th><th>Copy / Paste Settings</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Where</strong></td><td>Toolbar above the canvas</td><td>Top of the inspector</td></tr>
        <tr><td><strong>What it carries</strong></td><td>Film area and base marker</td><td>The ticked tone and geometry parameters</td></tr>
        <tr><td><strong>Base on the target</strong></td><td>Re-measured on each target frame</td><td>Leaves the target's physical reference alone</td></tr>
        <tr><td><strong>Typical use</strong></td><td>One fixed setup for the whole roll</td><td>One look across frames from the same scene</td></tr>
    </tbody>
</table>
<p>Batch Apply re-measures the base on every frame because even with a fixed setup, uneven illumination, lens vignetting, and small film shifts make base readings differ slightly between frames. Reusing one reading would turn that difference into a colour cast. Only when a target frame has no measurable base does the app keep the inherited value, and it tells you to confirm the film area and run Auto Invert again.</p>
<p>The paste dialog groups settings: Scan and Density, Film Base and Inversion, Film Mode, Density Limits, Transform, Perspective, Print Film Emulation, built-in or custom LUT, and the sampled sprocket point. Three presets above the list select all settings, tone and colour, or geometry only, and individual rows can be ticked.</p>
`
                    },
                    {
                        id: 'history-undo',
                        heading: 'Undo and reset',
                        content: `
<p>Editing steps are recorded in the undo history: <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes and <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> or <kbd>Ctrl</kbd>+<kbd>Y</kbd> redoes (macOS uses <kbd>⌘</kbd>). While a text field has focus, these shortcuts do not intercept typing.</p>
<p><strong>Reset</strong> in the inspector clears the colour adjustments for the current frame. <strong>Reset Crop</strong> in the toolbar and the reset buttons in the geometry group only affect geometry. The two do not interfere.</p>
`
                    }
                ]
            },
            {
                id: 'export-and-contact-sheets',
                kicker: '05 / Export & Contact Sheets',
                title: 'Export and Contact Sheets',
                subtitle: 'Every setting in the export dialog, and how the roll index is produced.',
                readingTime: 7,
                sections: [
                    {
                        id: 'export-basics',
                        heading: 'Exporting frames',
                        content: `
<p>Tick frames in <strong>Library</strong>, then choose <strong>Export</strong> in the upper-right corner (the number on the button is the current selection), or open <strong>View → Develop</strong> and export from there. The dialog opens with the selection count.</p>
<p>Export runs in the background, so you can keep browsing and editing while files are written. Every selected frame is decoded at full resolution, and source scans are never overwritten.</p>
<div class="doc-callout doc-callout-warn">
    <div class="doc-callout-title">Export one test frame first</div>
    <div class="doc-callout-body">Before committing a full roll, export a single frame with the same settings and check tone, colour, and dimensions.</div>
</div>
`
                    },
                    {
                        id: 'export-format',
                        heading: 'Format and colour space',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>Format</th><th>Notes</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>TIFF · 16-bit</strong></td><td>Best for archiving and further editing; keeps the most tonal information.</td></tr>
        <tr><td><strong>TIFF · 8-bit</strong></td><td>For workflows that need a TIFF container without high bit depth.</td></tr>
        <tr><td><strong>PNG · 16-bit</strong></td><td>Lossless compression, suitable for archive or delivery.</td></tr>
        <tr><td><strong>JPEG · 8-bit</strong></td><td>Small files for sharing and review. Choosing it reveals a quality slider from 40 to 100, default 92.</td></tr>
    </tbody>
</table>
<p>Output colour space can be sRGB IEC 61966-2.1, Display P3, Adobe RGB (1998), ITU-R BT.2020, ProPhoto RGB (ROMM RGB), ACEScg (AP1), or ACES2065-1 (AP0). The chosen ICC profile is written into the file. Use sRGB for the web, and ProPhoto RGB or an ACES space when you need a wider gamut for further work.</p>
<p>Output sharpening offers <strong>None</strong> (preserve grain), <strong>Low</strong> (web and screens), <strong>Standard</strong> (default), and <strong>High</strong> (small output).</p>
`
                    },
                    {
                        id: 'export-size',
                        heading: 'Dimensions',
                        content: `
<p>There are two resize policies:</p>
<ul>
    <li><strong>Original dimensions</strong> exports at the full decoded resolution.</li>
    <li><strong>Set long edge</strong> takes a target in pixels, with presets of 1024, 2048, 4096, and 8192 and a default of 2048. Aspect ratio is preserved.</li>
</ul>
<p><strong>Allow enlargement</strong> also scales frames smaller than the target. It is off by default so small sources are not interpolated upward.</p>
`
                    },
                    {
                        id: 'export-files',
                        heading: 'Naming and existing files',
                        content: `
<p>The filename template accepts these tokens, with a default of <code>{Roll}_{Seq}</code>:</p>
<table class="doc-table">
    <thead>
        <tr><th>Token</th><th>Value</th></tr>
    </thead>
    <tbody>
        <tr><td><code>{Roll}</code></td><td>Roll identifier</td></tr>
        <tr><td><code>{Camera}</code></td><td>Camera name</td></tr>
        <tr><td><code>{Film}</code></td><td>Film stock</td></tr>
        <tr><td><code>{Date}</code></td><td>Capture date</td></tr>
        <tr><td><code>{Original}</code></td><td>Original file name</td></tr>
        <tr><td><code>{Seq}</code></td><td>Frame sequence number</td></tr>
    </tbody>
</table>
<p>A live preview of the resulting name appears below the field. Invalid filename characters are replaced automatically. When a file already exists you can <strong>Keep both</strong> (adds a suffix, the default), <strong>Replace existing file</strong>, or <strong>Skip existing file</strong>. NexFilm does not overwrite silently.</p>
<p>Ticking <strong>Write Roll, film, camera and date to EXIF</strong> stores those details in the exported files.</p>
`
                    },
                    {
                        id: 'export-roll',
                        heading: 'Exporting a whole roll',
                        content: `
<p><strong>Export Roll</strong> in the header exports every frame of the current roll with the same output settings. If some frames have missing source files or no saved processing state, the app reports how many and stops rather than skipping them quietly.</p>
<p>A roll export also runs in the background and reports the destination and the number of frames written when it finishes.</p>
`
                    },
                    {
                        id: 'contact-sheet',
                        heading: 'Contact sheets',
                        content: `
<p>A contact sheet lays the whole roll out as one index image with edge codes, useful for reviewing and archiving.</p>
<ol>
    <li>Open <strong>Rolls</strong>, select the roll, and enter its contents view.</li>
    <li>Choose <strong>Export Contact Sheet</strong>.</li>
    <li>Pick a destination folder. Generating the sheet can take a few seconds.</li>
</ol>
<p>Frames per row follow the format: 6 for 135, 4 for 645 and 6×6, 3 for 6×7, 2 for 6×9 and 6×12, and 1 for 6×17. The grid is not user-configurable at present.</p>
<p>The output is a JPEG named <code>contact_sheet_roll_camera.jpg</code>. Edge codes carry the film stock, frame number, and the NexFilm wordmark; capture time and exposure settings are not printed.</p>
`
                    }
                ]
            },
            {
                id: 'troubleshooting',
                kicker: '06 / FAQ & Troubleshooting',
                title: 'FAQ and Troubleshooting',
                subtitle: 'Find the cause by symptom, and what to confirm at each step.',
                readingTime: 8,
                sections: [
                    {
                        id: 'flat',
                        heading: 'The positive looks flat, dark, or lacks contrast',
                        content: `
<p>Check these three things in order:</p>
<ol>
    <li><strong>Does the film area include the light panel?</strong> This is the most common cause. The panel is far brighter than any part of the film, so once it is inside the area it becomes the brightest pixel used for analysis and pushes the normal range down. Re-enter <strong>Set Film Area</strong> and put sprocket holes and the white panel outside the outline.</li>
    <li><strong>Was the film base sampled correctly?</strong> If the base sample landed on picture content instead of unexposed film, the mask reference is offset. Re-sample with <strong>Calibrate Film</strong>, placing the sample on an even, clear film edge.</li>
    <li><strong>Do the density endpoints need a trim?</strong> Adjust Master D-Min and Master D-Max in <strong>Density Limits</strong>. The readout below the sliders shows the endpoints measured for this frame.</li>
</ol>
<p>If the scene really was overcast, foggy, or shot at night, the absence of true black or white is correct. The app will not force the content to fill the range, and a moderate contrast is the right result.</p>
`
                    },
                    {
                        id: 'color-cast',
                        heading: 'The positive has an overall colour cast',
                        content: `
<p>First decide what kind of cast it is:</p>
<ul>
    <li><strong>The whole roll leans the same way</strong>: usually the base reference. Check that the calibration frame really contains unexposed base and has no strong density gradient.</li>
    <li><strong>One frame leans, and the picture is dominated by one colour</strong>: the channel span compensation may be working as intended. Its range is limited so that a single dominant colour cannot flatten the picture. If the result is still not right, correct it with <strong>Printer Lights</strong>.</li>
    <li><strong>The cast appears at the edges</strong>: usually uneven copy lighting. Tighten the film area to avoid the worst edges and invert again.</li>
</ul>
<p>The eyedropper in the <strong>Printer Lights</strong> header is the white balance eyedropper: click a neutral grey area and the app adjusts the channel exposure offsets. It changes printer lights, not the film calibration.</p>
`
                    },
                    {
                        id: 'batch-tint',
                        heading: 'A few frames are tinted after Batch Apply',
                        content: `
<p>Batch Apply carries the film area and the base marker, but every frame re-measures its own base, because illumination, vignetting, and small film shifts make readings differ. If a target frame has no usable base, the app keeps the inherited value and says so.</p>
<p>Check whether that frame's film area leaves a usable film edge, then run <strong>Auto Invert</strong> again. If the frame was placed noticeably differently from the rest of the roll, confirming its area by hand is more reliable than batch transfer.</p>
`
                    },
                    {
                        id: 'choose-format',
                        heading: 'TIFF or JPEG for export',
                        content: `
<ul>
    <li>For archiving, further editing, or printing, choose <strong>TIFF · 16-bit</strong> or <strong>PNG · 16-bit</strong> with a matching output colour space.</li>
    <li>For sharing, review, or small deliverables, choose <strong>JPEG · 8-bit</strong>, usually in sRGB.</li>
</ul>
<p>Export size depends on picture content, dimensions, bit depth, and compression quality, so there is no fixed correspondence. When it matters, export a few frames with the same settings and compare.</p>
`
                    },
                    {
                        id: 'raw-import',
                        heading: 'Some RAW files will not import',
                        content: `
<p>RAW compatibility depends on the bundled decoder and the specific device. Things worth trying:</p>
<ul>
    <li>Confirm that the extension matches the real format; some cameras record a high-efficiency compressed format behind a RAW extension.</li>
    <li>Nikon Z8 HE and HE* compressed NEF cannot be decoded at present; switch the camera to a standard RAW recording format.</li>
    <li>Re-export the source as DNG or another supported format.</li>
    <li>If it still fails, report the camera or scanner model, file format, and error message through Issues. Do not upload private scans.</li>
</ul>
`
                    },
                    {
                        id: 'preview-vs-export',
                        heading: 'Preview and export are not identical',
                        content: `
<p>Develop uses a bounded proxy to stay responsive: 2560 px on the long edge by default, raised to 4096 when needed. Export decodes every selected frame at full resolution. Both go through the same density pipeline, so tone and colour should match, but at pixel level, sharpening, noise, and very fine edges can still differ slightly. Exporting a test frame before a full batch is the reliable check.</p>
`
                    },
                    {
                        id: 'originals',
                        heading: 'Does NexFilm modify my originals?',
                        content: `
<p>No. NexFilm does not overwrite or move source scans. The library, edit parameters, and previews live in the application data directory. When deleting frames, the app asks whether to remove the record only or to move the source files to the system trash as well; in the second case, files still referenced by another roll are kept.</p>
<p>If a source file is moved or renamed, the frame goes offline and must be reconnected with <strong>Locate File</strong>.</p>
`
                    },
                    {
                        id: 'backup',
                        heading: 'How to back up your work',
                        content: `
<p>Back up two things: your original scans and the application data directory. The main database <code>nexfilm_user.db</code> holds roll details, edit parameters, and previews; it stores paths to source files, not the images themselves. Backing up only the data directory cannot recover deleted originals, and backing up only the originals means re-importing and redoing the edits.</p>
<p>The exact data directory is listed in the appendix.</p>
`
                    }
                ]
            },
            {
                id: 'color-science',
                kicker: '07 / Color Science Reference',
                title: 'Color Science Reference',
                subtitle: 'For readers who want the algorithm: optical density, the unified pipeline, and colour management.',
                readingTime: 12,
                sections: [
                    {
                        id: 'optical-density',
                        heading: 'Optical density',
                        content: `
<p>The colour of a negative cannot be read directly from its pixel values. Light passing through film is partly absorbed, each layer absorbing a different proportion, and measuring that proportion is what recovers the subject's colour. The quantity describing how much light is blocked is optical density:</p>
<div class="doc-code-block"><code>D = -log10(T)　　T = transmitted intensity ÷ incident intensity</code></div>
<p>Density and transmission are related logarithmically. Halving transmission adds about 0.30 to density; reducing transmission to a tenth adds 1.00. The same density change therefore corresponds to a very different brightness change in thin and dense areas. Adding and subtracting transmission values directly gives shadow and highlight responses in the wrong proportion; taking the logarithm first and working in the density domain matches how the film actually blocks light.</p>
<p>The film base sets the lower end of the density range. On colour negative film the base carries an orange mask, and measuring it and subtracting it as the zero point is what returns the picture to neutral. The fully exposed leader sets the upper end. Together they define the density range available to the roll.</p>
`
                    },
                    {
                        id: 'pipeline',
                        heading: 'The unified density pipeline',
                        content: `
<p>Whatever the material, a roll of copied RAW files, a scanner FFF, or a loose TIFF, inversion goes through the same pipeline. Input type affects only the first step, input domain resolution; the density mathematics afterwards does not change.</p>
<table class="doc-table">
    <thead>
        <tr><th>Step</th><th>What happens</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>1. Decode and resolve the input domain</strong></td><td>Determine what the file represents: primaries, transfer curve, and whether it is already linear. The decoded result is converted into a 32-bit floating-point linear ProPhoto RGB working space.</td></tr>
        <tr><td><strong>2. Subtract the film base</strong></td><td>Remove the measured base per channel to obtain transmission, then take <code>-log10</code> to enter the density domain.</td></tr>
        <tr><td><strong>3. Build the shared window</strong></td><td>With a usable base, all three channels share one window origin, with no extra per-channel density offset.</td></tr>
        <tr><td><strong>4. Channel span response</strong></td><td>Compare the measured spans of the three channels and let each channel's window width follow its response within limits. See the next section.</td></tr>
        <tr><td><strong>5. Display mapping</strong></td><td>Convert ProPhoto RGB to the display space, then apply gamma and any LUT.</td></tr>
    </tbody>
</table>
<p>When no usable film base can be measured at all, the pipeline falls back to content alignment: the window origin is estimated from picture content and the alignment is limited to 0.20 density so the picture is not flattened.</p>
`
                    },
                    {
                        id: 'anchors',
                        heading: 'Physical anchors and content range',
                        content: `
<p>Treating the film's physical reference and the picture's tonal distribution as separate things is the main difference between this pipeline and simply stretching a histogram.</p>
<p>The base and the leader are set by the chemistry of the film and do not change with the subject. Within one roll, the base colour and the leader density are the same physical quantities whether the picture shows snow or a night street. Once calibrated, the whole roll can share those anchors.</p>
<p>The tonal distribution of a picture is a different matter. An overcast scene, a night shot, or a stage photograph may simply contain no true black or white, and forcing the histogram to fill the range would destroy genuine tonal separation and amplify shadow noise. The app therefore does not let content range override the physical anchors; it uses content only to estimate an endpoint that is missing.</p>
`
                    },
                    {
                        id: 'channel-span',
                        heading: 'Bounded channel span response',
                        content: `
<p>In real scans the three channels do not respond identically to the same picture. The difference can come from the film layers, the scanner's channel gains, or the spectrum of the copy light. Ignoring it leaves a stable cast; following the measured values exactly can flatten a picture dominated by one colour. The compromise is a dead band and a ceiling.</p>
<table class="doc-table">
    <thead>
        <tr><th>Channel span ratio</th><th>Behaviour</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Below 1.15</strong></td><td>Treated as equal response; the shared window is kept exactly as measured, with no channel separation.</td></tr>
        <tr><td><strong>1.15 to 1.35</strong></td><td>Channel window widths open up gradually according to the measured ratios.</td></tr>
        <tr><td><strong>Up to 1.35</strong></td><td>Measured values are trusted in full. Compensation may bend the channels apart by at most 1.35 times the shared span.</td></tr>
        <tr><td><strong>Above 2.0</strong></td><td>Treated as an upstream-truncated channel (for example a stitched scan where one channel kept only a third of the others' density range). The ceiling widens to 3 times so an unfixable cast is not left behind.</td></tr>
    </tbody>
</table>
<p>A compensated window is never allowed to collapse below 0.2 density, so the display range cannot degenerate into a flat strip.</p>
`
                    },
                    {
                        id: 'input-domain',
                        heading: 'Input domain resolution',
                        content: `
<p>The same pixel values may come from a camera RAW or a linear scanner output, and the two mean entirely different things. At import the app checks these rules in order and the first match wins:</p>
<ol>
    <li><strong>Embedded ICC profile</strong>: the file's own colour profile is used, with the highest confidence.</li>
    <li><strong>Scanner container record</strong>: Flextight / Imacon FFF files and some TIFFs record primaries and gamma in the container, which are used directly; if the record cannot be read, documented defaults are used and marked as estimated.</li>
    <li><strong>Scanner input profile</strong>: a profile created under Hardware Calibration for the device, applied to non-RAW files and DNG.</li>
    <li><strong>DNG</strong>: a linear RAW scanner DNG is read as linear sRGB; other DNGs are treated as camera RAW.</li>
    <li><strong>Other RAW</strong>: camera native primaries with the camera RAW transfer curve.</li>
    <li><strong>Fallback</strong>: everything else is interpreted as sRGB and marked as estimated.</li>
</ol>
<p>The resolved record is stored with the frame, and the technical report shows which rule matched for that file.</p>
`
                    },
                    {
                        id: 'output-color',
                        heading: 'Output colour',
                        content: `
<p>The output colour space is chosen in the export dialog: sRGB, Display P3, Adobe RGB (1998), Rec.2020, ProPhoto RGB, ACEScg, or ACES2065-1. The chosen ICC profile is written into the file so other software interprets the values the same way.</p>
<ul>
    <li>For the web, social platforms, and most image viewers: <strong>sRGB</strong>.</li>
    <li>To carry a wider gamut into professional software: <strong>ProPhoto RGB</strong> or an <strong>ACES</strong> space.</li>
    <li>To match a wide-gamut display or a specific print pipeline: <strong>Display P3</strong> or <strong>Adobe RGB (1998)</strong>.</li>
</ul>
<p>If an exported file looks wrong in another application, first check whether that application honours the embedded ICC profile. Viewers without colour management display every space as if it were sRGB.</p>
`
                    },
                    {
                        id: 'capture-tips',
                        heading: 'Copy and scanning advice',
                        content: `
<p>These habits make everything downstream more predictable:</p>
<ul>
    <li><strong>Keep the camera fixed.</strong> Copied in the same position, a whole roll can share the film area and base readings, and Batch Apply becomes meaningful.</li>
    <li><strong>Even out the lighting.</strong> A flat panel is usually a few percent dimmer at the edges than in the centre; the closer the film area stays to the middle, the safer the result.</li>
    <li><strong>Leave a strip of unexposed film edge.</strong> The base is the reference for mask removal, and a little edge outside the picture lets the app measure the real mask.</li>
    <li><strong>Block ambient light.</strong> Surround the film while copying so stray light does not reflect into the frame.</li>
    <li><strong>Use RAW or 16-bit output.</strong> Higher bit depth preserves more density separation, especially in the shadows.</li>
    <li><strong>Keep the light panel out of frame.</strong> Its highlight dominates the analysis and is the most common cause of a flat positive.</li>
</ul>
`
                    }
                ]
            },
            {
                id: 'hardware-calibration',
                kicker: '08 / Hardware Calibration',
                title: 'Hardware Calibration (Experimental)',
                subtitle: 'Saved capture profiles for a fixed scanning or copy setup, and what the feature can do today.',
                readingTime: 5,
                sections: [
                    {
                        id: 'status',
                        heading: 'Current status',
                        content: `
<p>Hardware Calibration is a newer feature, and the validation entry points in the interface are labelled experimental. Its purpose is to record the capture conditions of a fixed setup so they can be reused, instead of being re-judged every time.</p>
<p>It is important to be clear that registering reference files is not the same as completing a density calibration. The interface says so directly: configured references are not measured density. Until validation passes, processing continues to use per-frame analysis.</p>
<p>If you only want to process the film in front of you, this chapter can be skipped. Film calibration already covers most roll-wide needs and does not depend on a hardware calibration profile.</p>
`
                    },
                    {
                        id: 'profiles',
                        heading: 'Capture profiles',
                        content: `
<p>Each profile records one fixed capture setup: camera, light source, lens, and reference files. References are registered by kind, including dark frame, open gate, flat field, transmission target, film base reference, full-exposure reference, and spectral capture.</p>
<p>The capture pipeline lists these stages in order:</p>
<table class="doc-table">
    <thead>
        <tr><th>Stage</th><th>What it covers</th></tr>
    </thead>
    <tbody>
        <tr><td><strong>Dark subtraction</strong></td><td>Capture-domain black level and dark-current reference.</td></tr>
        <tr><td><strong>Open-gate normalization</strong></td><td>Capture-domain illumination and channel normalization.</td></tr>
        <tr><td><strong>Flat-field correction</strong></td><td>Capture-domain shading and dust reference.</td></tr>
        <tr><td><strong>Capture separation</strong></td><td>Optional device characterisation before the log conversion.</td></tr>
        <tr><td><strong>Density reference</strong></td><td>Optional verified target used after the log conversion.</td></tr>
        <tr><td><strong>Roll anchors</strong></td><td>Film base and full-exposure anchors remain roll-scoped.</td></tr>
        <tr><td><strong>Film reconstruction</strong></td><td>Reserved for a later measured film model.</td></tr>
    </tbody>
</table>
`
                    },
                    {
                        id: 'create',
                        heading: 'Creating and importing profiles',
                        content: `
<ol>
    <li>Open the <strong>Hardware Calibration</strong> view.</li>
    <li>Choose <strong>New Profile</strong>, fill in camera, light source, and lens, and add the matching reference files. Alternatively, choose <strong>Import Scanner Profile</strong> to bring in an existing device profile.</li>
    <li>After saving, run <strong>Validate Capture (Experimental)</strong> in the profile detail to check that the references are usable.</li>
</ol>
<p>If a reference file is moved or modified, the profile detail reports it and the profile is not used at runtime.</p>
`
                    },
                    {
                        id: 'use',
                        heading: 'Using a profile while processing',
                        content: `
<p>Two selectors inside the Input Color Science group of the Develop inspector are relevant:</p>
<ul>
    <li><strong>Calibration Config Profile</strong> binds a hardware calibration profile to a roll. Its default is Smart Auto. The binding is stored with the roll and is kept when the frames are exported.</li>
    <li><strong>Scanner Input Profile</strong> names the input profile for a scanner, which feeds the "scanner input profile" rule in input domain resolution.</li>
</ul>
<p>If a bound profile is unavailable or needs attention, the app says so and falls back to per-frame analysis.</p>
`
                    },
                    {
                        id: 'limits',
                        heading: 'Current limits',
                        content: `
<ul>
    <li>Hardware Calibration manages capture configuration; it is not a completed density calibration, and the film-density stage is still marked unvalidated.</li>
    <li>Loose frames always use per-frame analysis and never take part in a roll profile.</li>
    <li>The available reference kinds and validation steps will change between versions. Treat the interface as the source of truth.</li>
</ul>
`
                    }
                ]
            },
            {
                id: 'appendix',
                kicker: '09 / Appendix',
                title: 'Appendix',
                subtitle: 'Shortcuts, data locations, version information, known limits, and where to send feedback.',
                readingTime: 4,
                sections: [
                    {
                        id: 'shortcuts',
                        heading: 'Keyboard shortcuts',
                        content: `
<p>These are the only global shortcuts in the application. Everything else is done with the on-screen controls.</p>
<table class="doc-table">
    <thead>
        <tr><th>Key</th><th>Action</th><th>When it applies</th></tr>
    </thead>
    <tbody>
        <tr><td><kbd>Ctrl</kbd>+<kbd>Z</kbd> / <kbd>⌘</kbd>+<kbd>Z</kbd></td><td>Undo</td><td>While not editing a text field</td></tr>
        <tr><td><kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> or <kbd>Ctrl</kbd>+<kbd>Y</kbd></td><td>Redo</td><td>While not editing a text field</td></tr>
        <tr><td><kbd>←</kbd> / <kbd>→</kbd></td><td>Previous or next frame</td><td>While the filmstrip is visible</td></tr>
        <tr><td><kbd>Enter</kbd></td><td>Confirm the crop</td><td>In crop mode</td></tr>
        <tr><td><kbd>Esc</kbd></td><td>Close the current dialog or menu, or cancel a value edit</td><td>While a dialog or menu is open</td></tr>
        <tr><td>Hold <kbd>Space</kbd> and drag</td><td>Pan the canvas</td><td>While a frame is open</td></tr>
        <tr><td>Mouse wheel</td><td>Zoom the preview; scroll the filmstrip sideways</td><td>In Develop</td></tr>
        <tr><td>Mouse wheel</td><td>Adjust a slider value</td><td>After clicking that slider once</td></tr>
    </tbody>
</table>
<p>The canvas also supports dragging to pan and wheel zooming centred on the pointer. Sliders in the inspector support dragging and clicking the track to jump.</p>
`
                    },
                    {
                        id: 'data-locations',
                        heading: 'Data file locations',
                        content: `
<table class="doc-table">
    <thead>
        <tr><th>Platform</th><th>Data directory</th></tr>
    </thead>
    <tbody>
        <tr><td>Windows release</td><td><code>%APPDATA%\\NexFilm Engine\\</code></td></tr>
        <tr><td>macOS release</td><td><code>~/Library/Application Support/NexFilm Engine/</code></td></tr>
        <tr><td>Debug build</td><td>Repository working directory</td></tr>
    </tbody>
</table>
<p>The main database is <code>nexfilm_user.db</code>. It stores roll details, the frame index, edit parameters, input domain records, and previews, but not a complete copy of the originals. <code>rolls.json</code> is a compatibility mirror.</p>
<p>Back up the originals and the data directory together. The database records absolute paths to source files, so moved originals must be reconnected with <strong>Locate File</strong>.</p>
`
                    },
                    {
                        id: 'version',
                        heading: 'Version and updates',
                        content: `
<p>The current stable release is <strong>v1.0.2</strong>. Installers and release notes are published on the repository's Releases page; treat that page as the source of truth.</p>
<p>Upgrading does not delete the data directory, but when the database schema changes the app migrates it automatically on first launch. Before a large version jump, back up the data directory first.</p>
`
                    },
                    {
                        id: 'known-limits',
                        heading: 'Known limitations',
                        content: `
<ul>
    <li>Windows and macOS packages are unsigned, and macOS DMGs are not notarized by Apple.</li>
    <li>Nikon Z8 HE and HE* compressed NEF files cannot currently be decoded.</li>
    <li>Compatibility across cameras, scanners, and FFF variants still needs wider validation.</li>
    <li>No verified Linux release package is currently provided.</li>
    <li>The database references source paths rather than copying originals, so moved or deleted files take frames offline.</li>
    <li>Develop uses a bounded proxy; pixel-level differences against a full-resolution export can remain.</li>
    <li>Capture validation in Hardware Calibration is still experimental.</li>
</ul>
`
                    },
                    {
                        id: 'license',
                        heading: 'Licence and credits',
                        content: `
<p>NexFilm Engine is released under the <strong>GNU General Public License v3.0 only</strong>. The full text is in the LICENSE file at the repository root; use, modification, and redistribution follow its terms.</p>
<p>Thanks to the Zhihu users @黄昊Haosky and @V777 for sharing their work on scientific film-mask removal, and to everyone who files issues and suggestions. The bundled print film and paper LUTs are the <code>.cube</code> files distributed with the application and are available in the Print Film Emulation panel.</p>
`
                    },
                    {
                        id: 'feedback',
                        heading: 'Feedback and contributions',
                        content: `
<p>Report problems and suggestions through the repository's Issues page. Please include:</p>
<ul>
    <li>your operating system version and the NexFilm version;</li>
    <li>the camera or scanner model and the file format;</li>
    <li>reproduction steps and the complete error message.</li>
</ul>
<p>Do not upload private scans. Keep code contributions focused and describe how you verified them in the pull request.</p>
`
                    }
                ]
            },
        ]
    };

    const LOCALES = ['zh-CN', 'en'];

    function normalizeLocale(locale) {
        const value = String(locale || '').toLowerCase();
        return value.startsWith('zh') ? 'zh-CN' : 'en';
    }

    function getChapters(locale) {
        return CHAPTERS[normalizeLocale(locale)] || CHAPTERS.en;
    }

    function getChapterById(id, locale) {
        return getChapters(locale).find(chapter => chapter.id === id) || null;
    }

    function getFallbackChapter(locale) {
        const chapters = getChapters(locale);
        return chapters.length > 0 ? chapters[0] : null;
    }

    global.NexFilmDocsData = {
        locales: LOCALES,
        getChapters: getChapters,
        getChapterById: getChapterById,
        getFallbackChapter: getFallbackChapter
    };
})(window);
