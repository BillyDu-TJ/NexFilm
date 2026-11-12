# NexFilm 官网与双端同源文档体系 — 架构与实现规范（v2.0）

> **状态**：架构需求已对齐（v2.0 正式规范） · 待按 Phase 1–4 执行工程实现  
> **初版**：2026-10-18 · **架构重构修订**：2026-11-04  
> **配套执行提示词**：见 [`docs/website-overhaul-phase-prompts.md`](./website-overhaul-phase-prompts.md)

---

## 1. 重构背景与现状审查诊断

在初版官网原型（原本地目录 `/Users/apple/Code/nexfilm-web`）审查中，定位出三大核心结构性缺陷，本次 v2.0 架构围绕这三大问题彻底重构：

### 1.1 前端视觉、比例与交互动效问题
1. **三套色板互相冲突**：`BaseLayout.astro` 强行写入暖炭黑 `#181819`，而 `Header.astro` 与 `global.css` 沿用冷蓝灰 SaaS 模板色 `#0F1117` / `#1A1D27`，按钮又混用刺眼亮黄 `#F59E0B`（`amber-500`）、霓虹蓝黄渐变标题与桌面端真实的暗房铜琥珀 `#c4814f`，导致顶栏与页面背景色温割裂、气质分裂。
2. **排版比例失衡与公式裸露**：
   - 首页 H1 在 `max-w-3xl` 与 `text-6xl` 组合下把“工作台”硬断成 `工作` / `台` **孤字挂单**。
   - 一半页面标题用 `Noto Serif SC` 衬线宋体，另一半页面用无衬线 `font-extrabold` 强行覆盖。
   - `features.astro`、`faq.astro`、`docs/index.astro` 未接入构建期 KaTeX，直接把 `\log_{10}(T) \quad \text{其中}` 源码原样裸露在页面上甚至被 `truncate` 截断。
3. **顶栏导航严重过载**：平铺了 9 个一级导航项 + 外部第三方 `buttons.github.io` iframe（加载抖动且样式格格不入），无当前路由高亮指示。
4. **首屏与画廊严重穿帮**：`BeforeAfter.astro` 前后均传入同一张旧版**浅色空白 UI 截图** `nexfilm-v1.0-develop.png`，左侧直接套 CSS `filter: invert(1) hue-rotate(180deg)` 将整个软件白色外壳反相成深褐色，且画廊 4 组样片全复用该图。
5. **动效缺失**：缺少跨页平滑过渡、首屏层级错峰揭示、顶栏激活滑块与能直观传达“物理去色罩 + 光学密度反相”价值的微交互体验。

### 1.2 下载链接 404 与版本日志不同步根因
1. **资产文件名硬编码错误（404）**：GitHub Releases（如 `v1.1.0`）由 Tauri `productName: "NexFilm Engine"` 打包，真实资产名为：
   - `NexFilm.Engine_1.1.0_aarch64.dmg`（7.31 MB）
   - `NexFilm.Engine_1.1.0_x64-setup.exe`（5.34 MB）
   - `NexFilm.Engine_1.1.0_x64.dmg`（7.57 MB）
   而原型代码硬编码成了错误的 `NexFilm_1.1.0_...`（漏掉 `.Engine`），且虚构了 `~42–48 MB` 体积，并捆绑了不可靠的第三方镜像 `ghproxy.net`。
2. **更新日志虚构且静态死写**：`changelog.astro` 手写编造了版本历史，未拉取仓库真实的 4 个 GitHub Releases（`v1.0.0`、`v1.0.1`、`v1.0.2`、`v1.1.0`），未来发版后也无法自动同步。

### 1.3 文档站退化为简陋摘要且与桌面端脱节
- 原型 `/zh/docs/` 仅把 9 段简短摘要堆在单页卡片中，丢失了桌面端内置 [`ui/documentation-data.js`](../ui/documentation-data.js)（1838 行双语严谨交互手册）和 [`data_process_pipeline_doc.md`](../data_process_pipeline_doc.md)（718 行三域数学管线文档）的核心内容，甚至编造了软件不存在的按钮与 LUT 名称（如 `Load 3D LUT`、`Kodak Vision3 Cinema`）。
- 缺乏一流开源项目文档必备的多路由章节树、右侧本页目录（On This Page）、`⌘K` 全文搜索、公式/管线图渲染，以及面向普通用户、调色专家与开源贡献者的分层体系。

---

## 2. 已确认的架构决策总表

| 编号 | 决策维度 | 最终确认方案 |
|---|---|---|
| **D1** | **视觉母题与字体** | 统一对齐桌面端**「光学暗房工业美学」**：暖炭黑基底（`#161618` / `#1b1b1c`）+ 片基铜琥珀主强调色（`#c4814f`）+ 现代无衬线 UI 字体与等宽数据字体（辅以极克制的局部衬线点缀），移除冷蓝灰背景、霓虹渐变字与亮黄按钮。 |
| **D2** | **顶栏导航与信息架构** | 顶栏精简为 **5 个核心一级入口**（`功能` · `画廊` · `文档` · `更新日志` · `下载`）+ **1 个「社区」下拉菜单**（收纳 `常见问题` · `反馈与赞赏` · `关于`）；移除外部 `buttons.github.io` 脚本，改用原生暗房风 GitHub Star 徽章。 |
| **D3** | **首屏穿帮修复与样片策略** | 彻底删除对 UI 截图套 `filter: invert()` 的穿帮做法；首屏采用**「深色工作台高保真外框展示 + 零依赖光学密度微交互实验室 + 标准化样片对比滑块」**；建立声明式 `gallery-manifest.ts` 与高质感程序化/SVG 光学演示态兜底，预留 `website/public/gallery/` 标准槽位供随时放入真实 v1.1 暗色截图与负/正片成对样张即刻生效。 |
| **D4** | **前端动效尺度** | **「光学仪器级」克制精密动效**：启用 Astro View Transitions 无缝跨页过渡、首屏错峰渐入（遵循 `prefers-reduced-motion`）、顶栏滚动紧凑态与激活胶囊滑块、视口轻量揭示，以及首页内嵌零外部依赖的**「片基扣除 & 印片光原理微交互演示器」**。 |
| **D5** | **下载与发版自动同步** | **「构建期拉取快照 + 浏览器运行时实时追新」双保险架构**：按正则匹配真实 `NexFilm.Engine_*` 资产直链，展示真实 MB 大小、SHA256 校验摘要与当前版本 GitHub Release 发布页链接（`/releases/tag/<tag>`）；当 GitHub 发布新版本（如 `v1.2.0`）时，即使静态站尚未重新构建，前端也会秒级自动更新顶栏版本号、下载直链与 Changelog 置顶日志；彻底移除 `ghproxy.net`。 |
| **D6** | **开源级文档分层版图** | 建设对标一流开源项目的**四大分层双语文档矩阵**：① 快速上手与翻拍实战（普通用户） ② 完整功能与工作台手册（全量对齐真实 UI） ③ 色彩科学与光学密度白皮书（专业调色师/硬核用户） ④ 架构设计与开源贡献指南（开源开发者）。 |
| **D7** | **仓库拓扑与双端文档同源** | **Monorepo 同仓共享架构**：移除根目录下未建远程库的 `website` 软链，将官网源码直接纳入主仓库 `website/` 目录管理；在主仓库建立唯一文档事实源 `docs/handbook/{zh,en}/`，由 `scripts/compile-handbook.mjs` 同时编译出桌面端离线 `ui/documentation-data.js` 并直接供给 `website/` 构建多页在线文档。 |
| **D8** | **双端文档收录边界** | 通过 Markdown Frontmatter `inApp: boolean` 精准分流：**板块 ①②③（`inApp: true`）在桌面端与官网 100% 同步**，并在编译期预渲染离线数学公式与内联 SVG 管线图（桌面端零联网依赖）；**板块 ④（`inApp: false`，Rust 架构、编译与 PR 规范）仅在官网展示**，保持桌面端纯粹服务摄影用户。 |
| **D9** | **部署与域名解耦** | 托管平台与学生包域名留待正式上线阶段再定；工程侧通过环境变量 `PUBLIC_SITE_URL`（默认 `https://nexfilm.app`）与 `PUBLIC_BASE_PATH`（默认 `/`）解耦站点根路径，确保本地开发、未来云服务器、Cloudflare Pages 或 GitHub Pages 均可零改代码直接部署。 |

---

## 3. Monorepo 目录拓扑与 `.gitignore` 规范

### 3.1 仓库目录结构

```text
NexFilm/                                  # 主仓库根（Tauri 2 + Rust + UI + Website Monorepo）
├─ src/                                   # Rust 核心引擎（只读参考，网页重构不改动业务逻辑）
├─ ui/
│  ├─ documentation.js                    # 桌面端文档视图渲染器（保持现有契约或平滑支持分组标题）
│  ├─ documentation-data.js               # [自动生成产物] 由 scripts/compile-handbook.mjs 编译生成，严禁手改
│  └─ style.css                           # 桌面端样式（按需补充离线公式/SVG 图表轻量排版样式）
├─ docs/
│  ├─ website-plan.md                     # 本架构与实现规范文档
│  ├─ website-overhaul-phase-prompts.md   # 分阶段工程执行 Prompts
│  ├─ handbook/                           # [NEW · 唯一文档事实源 SSOT]
│  │  ├─ schema.json                      # Frontmatter 字段约束说明
│  │  ├─ zh/
│  │  │  ├─ 01-getting-started/           # 板块 ①：快速上手与翻拍实战 (inApp: true)
│  │  │  ├─ 02-user-manual/               # 板块 ②：完整功能与工作台手册 (inApp: true)
│  │  │  ├─ 03-color-science/             # 板块 ③：色彩科学与光学密度白皮书 (inApp: true)
│  │  │  └─ 04-architecture/              # 板块 ④：架构设计与开源贡献指南 (inApp: false)
│  │  └─ en/                              # 英文版严格一一镜像同文件名与同锚点 ID
│  │     ├─ 01-getting-started/
│  │     ├─ 02-user-manual/
│  │     ├─ 03-color-science/
│  │     └─ 04-architecture/
├─ scripts/
│  ├─ compile-handbook.mjs                # [NEW] Markdown SSOT → ui/documentation-data.js 离线编译器
│  ├─ verify-handbook-sync.cjs            # [NEW] 校验 documentation-data.js 与 docs/handbook 无漂移、双语成对
│  └─ verify-*.cjs                        # 既有桌面端契约测试脚本（必须保持 100% 通过）
└─ website/                               # [NEW · 实体目录取代旧软链] Astro 5 官方网站工程
   ├─ astro.config.mjs                    # 支持 PUBLIC_SITE_URL / PUBLIC_BASE_PATH、MDX、KaTeX、Sitemap
   ├─ tailwind.config.mjs                 # 光学暗房色板与字体 Token
   ├─ package.json
   ├─ public/
   │  ├─ nexfilm-logo.svg
   │  ├─ nexfilm-logo-dark.svg
   │  ├─ favicon.png
   │  ├─ assets/                          # 赞赏码、工作台实拍图槽位
   │  └─ gallery/                         # 样片对比图标准槽位（支持放入真实负/正片自动替换兜底演示）
   ├─ scripts/
   │  └─ verify-site.mjs                  # 构建产物验收：死链检查、zh/en 成对、无裸露 LaTeX、无旧 404 下载链
   └─ src/
      ├─ content.config.ts                # Astro 5 Content Layer 直接挂载 ../docs/handbook/{zh,en}
      ├─ data/
      │  ├─ releases-fallback.json        # 真实 GitHub Releases 离线兜底快照（含 v1.0.0–v1.1.0 真实数据）
      │  ├─ release-en-notes.ts           # 历史纯中文 Release 的精准英文摘要/翻译补充映射
      │  └─ gallery-manifest.ts           # 画廊样片元数据（胶片型号、画幅、扫描设备、LUT、配图路径/演示参数）
      ├─ utils/
      │  ├─ github-releases.ts            # 构建期拉取、资产正则解析、Markdown 清洗与兜底合并
      │  └─ urls.ts                       # BASE_PATH 安全的链接拼接工具
      ├─ scripts/
      │  └─ release-hydrator.ts           # 浏览器端 Releases 实时追新与平台智能高亮（含 sessionStorage 缓存）
      ├─ layouts/
      │  ├─ BaseLayout.astro              # 全局暗房骨架、View Transitions、SEOHead、KaTeX 样式
      │  └─ DocsLayout.astro              # 一流开源三栏文档布局（左目录树 + 中正文 + 右 On This Page + ⌘K 搜索）
      ├─ components/
      │  ├─ Header.astro                  # 5 核心入口 + 社区下拉 + 胶囊滑块 + 原生 Star 徽章 + 同步版本号
      │  ├─ Footer.astro
      │  ├─ DownloadButtons.astro         # 三平台卡片 + SHA256 复制 + Release Tag 入口
      │  ├─ DensityInteractiveDemo.astro  # 零依赖「片基扣除 → 对数密度 → RGB 印片光」实时微交互演示台
      │  ├─ WorkspaceShowcase.astro       # 深色 Develop 工作台高保真展示框（支持实拍图与高保真组件态）
      │  ├─ BeforeAfter.astro             # 真双源负片/正片零延迟对比器（彻底废除 UI 截图反相滤镜）
      │  └─ docs/
      │     ├─ DocsSidebar.astro
      │     ├─ DocsToc.astro
      │     ├─ DocsSearchModal.astro      # Pagefind ⌘K / Ctrl+K 搜索模态框
      │     └─ DocsPager.astro
      ├─ pages/
      │  ├─ index.astro                   # 语言检测跳转
      │  ├─ 404.astro
      │  ├─ zh/ (index, features, gallery, download, changelog, faq, feedback, about, docs/[...slug])
      │  └─ en/ (严格对称路由)
      └─ styles/
         ├─ global.css                    # 暗房设计 Token、防孤字排版、微动效工具类
         └─ docs-prose.css                # 开源技术文档专属排版规范（表格、Callout、公式、代码块、快捷键）
```

### 3.2 根目录 `.gitignore` 必改规则（防采坑硬约束）
主仓库当前 `.gitignore` 含有全局通配忽略 `*.jpg`、`*.jpeg`、`*.png`、`*.svg` 以及最后一行的 `website`！在迁入同仓时必须精确调整：
1. 删除 `.gitignore` 末尾的 `website` 行。
2. 新增忽略网站构建与依赖产物：`website/node_modules/`、`website/dist/`、`website/.astro/`、`website/.pagefind/`。
3. 新增白名单豁免：`!website/public/**`、`!website/src/**`、`!docs/handbook/**`，确保官网 SVG 图标、样片图片与文档插图能正常纳入 Git 版本控制。

---

## 4. 视觉设计系统规范（Optical Darkroom Design System）

### 4.1 色彩 Token（全站统一，彻底消除冷暖打架）
完全对齐桌面端 [`ui/style.css`](../ui/style.css) 的暖炭黑暗房基底与胶片铜琥珀色系：

| Token 名称 | 色值 | 用途说明 |
|---|---|---|
| `darkroom-canvas` | `#121214` | 最深层暗房画布底色、代码块底色、页脚背景 |
| `darkroom-bg` | `#161618` | 全站 `<body />` 默认主背景（取代旧 `#0F1117` 与 `#181819` 混用） |
| `darkroom-chrome` | `#1d1d20` | 顶栏毛玻璃基底（`rgba(29, 29, 32, 0.86)` + `backdrop-blur-md`）与侧栏底色 |
| `darkroom-panel` | `#222225` | 标准卡片、文档提示框、下载卡片表面 |
| `darkroom-raised` | `#2a2a2e` | 悬浮态表面、下拉浮层、激活态胶囊背景 |
| `darkroom-line` | `rgba(255, 255, 255, 0.08)` | 常规精细分割线与卡片边框（1px 仪器刻度感） |
| `darkroom-line-strong` | `rgba(255, 255, 255, 0.16)` | 悬停边框、焦点轮廓 |
| `accent` | `#c4814f` | 主强调色（源自彩色负片片基铜琥珀色）：主 CTA 按钮、激活锚点、关键读数 |
| `accent-hover` | `#d28e59` | 主按钮悬停高亮态 |
| `accent-soft` | `rgba(196, 129, 79, 0.15)` | 徽章底色、选中高亮背景 |
| `printer-r / g / b` | `#e06c65` / `#6bc48d` / `#62a6e5` | 仅用于印片光三通道、密度曲线示意与功能状态指示点 |
| `text-primary` | `#f0f0f1` | 主标题与正文核心文字 |
| `text-secondary` | `#aaaab0` | 副标题、长文阅读次级段落 |
| `text-muted` | `#74747b` | 元数据、文件体积、辅助标注 |

> **禁用项**：禁止再使用 Tailwind 默认 `bg-amber-500` 刺眼亮黄按钮、禁止使用 `from-blue-400 via-sky-300 to-amber-300` 廉价霓虹渐变文字、禁止出现 `#0F1117` / `#14161C` 等偏蓝冷灰卡片。

### 4.2 字体层级与中文防孤字规范
- **无衬线主字体（`font-sans`）**：`"Inter"`, `"SF Pro Text"`, `"PingFang SC"`, `"Noto Sans SC"`, `"Microsoft YaHei"`, `sans-serif`。用于导航、所有章节与卡片标题、长文正文（保证技术文档与工具软件的干净利落）。
- **等宽仪器字体（`font-mono`）**：`"JetBrains Mono"`, `"SFMono-Regular"`, `"Cascadia Mono"`, `monospace`。用于版本号、光学公式读数（`D_min` / `D_max`）、SHA256、快捷键、终端命令与模块编号。
- **克制衬线点缀（`font-editorial`）**：仅限首页 Hero Slogan 局部关键词或引语点缀使用 `"Noto Serif SC"`，严禁全站 `h1, h2` 全局强套宋体。
- **防孤字硬约束（Orphan Prevention）**：
  - 所有大标题统一启用 `text-wrap: balance`（段落启用 `text-wrap: pretty`）。
  - 中文复合核心词组（如 `<span class="inline-block">工作台</span>`、`<span class="inline-block">密度域反转</span>`）使用 `inline-block` 语义包裹，杜绝在任何视口宽度下断出单字挂单行。
  - 首屏中文 H1 字号阶梯控制在 `clamp(2.25rem, 4.6vw, 3.75rem)`，行高 `1.22`，容器放宽至 `max-w-4xl`。

### 4.3 动效与首屏微交互实验室设计
1. **全局过渡与微交互**：
   - 接入 Astro `<ClientRouter />` 实现同站跨页平滑过渡，导航栏持久化不闪烁。
   - 顶栏当前所在频道配备平滑过渡的铜色底边/胶囊指示器；向下滚动超过 `24px` 时顶栏高度与边框对比度微调收紧。
   - 首屏元素采用 `80ms` 阶梯渐入（`opacity` + `translateY(10px)`，`cubic-bezier(0.22, 1, 0.36, 1)`），当系统开启 `prefers-reduced-motion: reduce` 时立即退化为瞬时显示。
2. **首屏零依赖微交互组件 — `DensityInteractiveDemo.astro`**：
   - 为让新访客在 **15 秒内亲手体验 NexFilm 为什么不同于普通反相滤镜**，在首页核心位置嵌入纯原生 Canvas/SVG 驱动的轻量实验室组件（包体积 < 8KB JS，零外部库）：
     - **阶段切换开关**：`① 原始翻拍透射率 T（带橙色片基色罩）` → `② 逐通道片基扣除 T / T_base` → `③ 对数密度域显影 D = -log₁₀(T)`。
     - **实时 Printer Lights 推子**：提供 Red / Green / Blue 三个对数印片光推子（`-12` 至 `+12` pt）与一键复位，实时在画布与一旁的三通道密度跨度条（R/G/B Density Span Readout）上联动反馈。
3. **样片对比组件 `BeforeAfter.astro` 与 `gallery-manifest.ts`**：
   - **严禁再使用 CSS `invert` 伪造负片**。`BeforeAfter.astro` 必须接收真实的独立双源（`negativeSrc` 与 `positiveSrc`），支持鼠标拖拽、触屏滑动及键盘左右方向键无障碍微调。
   - 在用户尚未放入实拍大图前，组件根据 `gallery-manifest.ts` 渲染基于物理色罩与对数密度模型预生成的真实胶片画幅 SVG/Canvas 演示帧（带真实 135/120 齿孔片边、胶片边码与乳剂色彩差异），并在文档中写明：只要将实拍图放入 `website/public/gallery/<slug>-neg.webp` 与 `<slug>-pos.webp` 并更新 Manifest 一行字段，即刻无缝切换为实拍对比。

---

## 5. GitHub Releases 双保险同步架构（零人工维护）

### 5.1 解决的问题
1. 修正所有下载直链为真实的 `NexFilm.Engine_<ver>_*` 资产路径。
2. 确保每次在 GitHub 发布新版本（如 `v1.1.1`、`v1.2.0`）后，**顶栏版本号、首屏下载按钮、下载页三平台卡片（含体积与 SHA256）、Release Tag 跳转链接以及更新日志页**全部自动同步。

### 5.2 资产匹配契约（Asset Matcher）
不再拼接字符串猜测文件名，而是遍历 GitHub Release API 返回的 `assets[]` 数组按规则提取：
- **macOS Apple Silicon**：`/aarch64\.dmg$/i` 或 `/arm64\.dmg$/i`
- **macOS Intel**：`/(x64|x86_64)\.dmg$/i`
- **Windows x64**：`/(x64-setup\.exe|x64_en-US\.msi|nsis.*\.exe)$/i`
- 每个匹配资产提取：`name`、`browser_download_url`、`size`（格式化为精确 `X.XX MB`，如 `7.31 MB`）、`digest`（提取 `sha256:...` 供用户点击复制校验）。
- 同时保留并突出展示**当前版本官方发布页入口**：`release.html_url`（例如 `https://github.com/BillyDu-TJ/NexFilm/releases/tag/v1.1.0`），当某平台资产暂未构建完成时优雅回退至该发布页。

### 5.3 双层同步机制
1. **第 1 层：构建期静态快照（Build-time SSG）**
   - `website/src/utils/github-releases.ts` 在 `astro build` 期间请求 `https://api.github.com/repos/BillyDu-TJ/NexFilm/releases?per_page=20`（若存在 `GITHUB_TOKEN` 环境变量则自动附带 Bearer 头）。
   - 拉取成功后规范化数据并用于静态渲染所有页面；若离线开发或网络超时，自动读取仓库内的真实快照 `website/src/data/releases-fallback.json`（已预置 `v1.1.0`、`v1.0.2`、`v1.0.1`、`v1.0.0` 完整真实数据），保证离线构建 100% 成功。
   - **Release Notes 清洗与双语适配**：自动剥离 GitHub Release 正文末尾面向 GitHub 页面的重复 `### 📥 下载指南` 段落，将其余 Markdown（`### 新功能`、`### 修复与优化`、`### 升级提示`、`### 已知限制`）渲染为结构化 HTML；英文站优先匹配 Release 正文中的英文分节（若未来发版含 `<!-- en -->` 分隔符）或 `release-en-notes.ts` 翻译映射，未配置的新版本自动展示原始 Release Notes 并附注 GitHub 原文链接。
2. **第 2 层：浏览器运行时实时追新（Client-side Hydration）**
   - `website/src/scripts/release-hydrator.ts` 在客户端启动时检查 `sessionStorage`（缓存键 `nexfilm_latest_release_v1`，TTL 15 分钟，规避 GitHub 匿名 API 60 次/小时限流）。
   - 向 `https://api.github.com/repos/BillyDu-TJ/NexFilm/releases/latest` 发起轻量请求（Changelog 页面请求 `/releases?per_page=15`）。
   - 若线上返回的 `tag_name` 新于当前 HTML 构建时的静态版本（或资产列表有更新），脚本立即原地无感更新：
     - 顶栏与首屏徽标的最新版本号（如 `v1.1.0` → `v1.2.0`）及 Release Tag 链接；
     - 首屏智能平台下载按钮与下载页三张卡片的 `href`、文件名、体积、SHA256；
     - 更新日志页顶部动态插入线上新发布的版本时间线卡片！
   - 若访客网络无法访问 `api.github.com` 或触发限流，静默保持构建期静态渲染结果，零报错、零白屏。

---

## 6. 一流开源文档工程与双端同源架构（SSOT Handbook）

### 6.1 双端同源编译流水线（Single Source of Truth）

```text
                docs/handbook/{zh,en}/**/*.md
         (唯一人工维护源：YAML Frontmatter + 标准 Markdown + LaTeX 公式)
                               │
        ┌──────────────────────┴──────────────────────┐
        ▼                                             ▼
[编译脚本 scripts/compile-handbook.mjs]       [Astro 5 Content Layer (website/)]
  仅筛选 frontmatter `inApp: true` (板块 ①②③)     加载全部板块 ①②③④ (`inApp: true/false`)
  Markdown → HTML + 离线 KaTeX MathML/HTML        多路由静态生成 `/zh/docs/<slug>/`
  输出至 `ui/documentation-data.js`               三栏布局 + 右侧目录 + Pagefind 全文搜索
  严格保持 `window.NexFilmDocsData` 接口契约       支持 Mermaid/SVG 图表、代码复制、上/下章翻页
```

#### Markdown Frontmatter 规范
每篇文档置于 `docs/handbook/{zh,en}/<track-dir>/<slug>.md`，中英文保持完全一致的 `slug` 与小节 `#anchor` ID：
```yaml
---
id: unified-density-pipeline          # 唯一章节 ID（桌面端 #doc/<id> 与官网 URL 末段共用）
track: color-science                  # getting-started | user-manual | color-science | architecture
trackOrder: 3                         # 板块排序 (1..4)
chapterOrder: 1                       # 板块内排序
kicker: "03.1 / 色彩科学"             # 章节眉标
title: "三大数学域与统一光学密度管线"
subtitle: "为什么透射率域、光学密度域与正片输出域绝不可混用矩阵。"
readingTime: 9                        # 预计阅读分钟数
inApp: true                           # true = 同步编译进桌面端；false = 仅官网展示
sinceVersion: "v1.1.0"
---
```

#### 桌面端兼容性保障
- `scripts/compile-handbook.mjs` 按 `## 二级标题 {#section-id}` 自动切分生成 `sections: [{ id, heading, content }]`，与现有 [`ui/documentation.js`](../ui/documentation.js) 的路由 `#doc/<chapterId>/<sectionId>`、页内目录与本地搜索**零破坏兼容**。
- 可选轻量增强：在 `chapter` 对象上附加 `track` 与 `trackTitle` 字段，`ui/documentation.js` 若检测到相邻章节 `track` 变化则在左侧列表渲染克制的分组小标题（如 `快速上手` / `工作台手册` / `色彩科学`），让桌面端阅读层次同样跃升至专业级。
- 数学公式在编译进 `ui/documentation-data.js` 时通过 `katex.renderToString(..., { output: 'mathml' })` 直接输出标准原生 `<math>` MathML 标签（现代 macOS WebKit 与 Windows WebView2 均原生支持渲染 MathML，**无需加载任何外部字体文件或联网 CSS**！）。

### 6.2 四大板块完整章节蓝图（中英双语严格对等）

#### 板块 ①：快速上手与翻拍实战（`01-getting-started` · `inApp: true` · 面向普通与新晋用户）
1. **`overview-and-install` — 认识 NexFilm 与安装放行**：项目定位（物理可解释 vs 经验黑盒）、各平台系统要求、Windows SmartScreen 与 macOS Gatekeeper / `xattr` 放行完整图解、离线数据承诺。
2. **`scanning-best-practices` — 翻拍与扫描前期准备规范**：高显指背光要求（CRI ≥ 95 / TLCI ≥ 98）、遮光防杂散光、相机原生 ISO 与向右曝光（ETTR）不截断原则、**为什么必须保留未曝光片基边缘**。
3. **`first-roll-in-five-minutes` — 五分钟完成第一卷反相与导出**：从 `导入胶卷` → 进入 `工作台` → `设置胶片范围（自动识别）` → `自动反相` → `印片光微调` → `导出` 的端到端标准闭环。
4. **`roll-vs-loose-workflows` — 按卷标定工作流 vs 散片快速工作流**：整卷 `标定片头片基` + `整卷批处理`（共享最亮帧白点防跳色）、120 无片头胶卷自动回退机制、散张拖入即修的一致性保证。

#### 板块 ②：完整功能与工作台手册（`02-user-manual` · `inApp: true` · 全面对齐软件真实 UI）
5. **`workspace-and-views` — 界面总览与五大视图分工**：菜单栏、`图库 (Library)`、`工作台 (Develop)`、`历史胶卷记录 (Rolls)`、`硬件校正 (Hardware Calibration)`、`使用文档 (Documentation)` 与底片条交互。
6. **`import-formats-and-library` — 格式支持矩阵与本地胶卷库管理**：相机 RAW（LibRaw / Rawler 支持范围与 Nikon HE/HE* 说明）、Imacon/Flextight `.fff` 与百兆/GB 级流式 TIFF、SQLite `nexfilm_user.db` 存储机制与源文件移动重定位。
7. **`film-area-and-geometry` — 胶片范围（Film Area）、几何校正与防漏光法则**：深入图解为什么圈入 100% 透光白灯板或齿孔会导致整幅画面灰暗发沉；自动片门检测、四角手柄微调、旋转/翻转/畸变校正与批量应用。
8. **`density-grading-and-curves` — 密度窗口、暗房印片光与四通道曲线**：`D_min / D_max` 窗口理解、RGB 印片光对数加减色法则、Master/R/G/B 控制点曲线与实时直方图/波形图、对比度/曝光/高光/阴影、按住 `Alt` 键实时查看高反差黑白锐化边缘蒙版、复制粘贴设置。
9. **`print-film-emulation` — 相纸与印片模拟（3D LUT）及黑白工作流**：准确列出并讲解软件真实内置 `.cube` LUT（`Kodak 2383`、`Kodak 2393`、`Kodak Portra Endura`、`Kodak Supra/Ultra Endura`、`Kodak Ektacolor Edge`、`Ilford MGFB 00–5` 号反差相纸）、外部 `.cube` 导入与强度混合、黑白模式滤光响应。
10. **`export-and-contact-sheets` — 母带级导出、色彩空间与接触印样**：8/16-bit TIFF、JPEG、PNG、16-bit 线性 TIFF、线性 DNG、原样封装相机 RAW DNG 的适用场景；sRGB / Display P3 / Adobe RGB / Rec.2020 / ProPhoto RGB / ACEScg ICC 选择指南；接触印样（Contact Sheet）排版与 EXIF 注入。
11. **`troubleshooting-and-appendix` — 实战故障排查、快捷键与附录**：发灰/偏色/高光贴顶/大图内存/升级后刷新缩略图（Auto Invert）决策树、完整全局快捷键表、本地数据库备份路径与开源致谢。

#### 板块 ③：色彩科学与光学密度白皮书（`03-color-science` · `inApp: true` · 面向进阶玩家与专业调色师）
12. **`three-mathematical-domains` — 负片管线的三大数学域**：严格推导透射率域 $T$（`log` 之前）、光学密度域 $D = -\log_{10}(T)$（`log` 之后）与正片输出域的物理含义，证明为什么 $-\log_{10}(Ax) \neq A[-\log_{10}(x)]$ 以及为何设备分离矩阵绝不能与密度矩阵混用。
13. **`input-domain-and-unified-pipeline` — 五级输入域解析与统一密度管线**：详解 `resolve_input_domain` 五级优先级（内嵌 ICC → FFF/扫描仪容器记录 → RAW/LinearRaw 元数据 → 机型默认 → sRGB 兜底与置信度标记）、片基采样三级优先级与质量门限、共享窗口原点与**有界通道跨度响应**（死区 `1.15×`、常规上限 `1.35×`、截断通道 `3.0×`，以及为何拒绝破坏影调的“灰世界”电平强拉）。
14. **`hardware-calibration-guide` — 硬件校准原理与实验功能指南**：诚实界定 `Smart Auto (ProPhoto Estimate)`、`Capture Corrected`（`dark` 暗场与 `open gate` 平场相对透射率归一化）与 `Capture Characterized`（透射色卡 `CaptureSeparation3x3` 拟合及病态条件自动回退机制）的物理边界与实操流程。
15. **`cinema-and-aces-workflow` — 线性交付与 DaVinci Resolve / ACEScg 后期衔接**：如何导出未施加显示伽马的 16-bit 线性正片，在达芬奇与色彩管理管线中进行无损二级调色。

#### 板块 ④：架构设计与开源贡献指南（`04-architecture` · `inApp: false` 官网专属 · 面向开源开发者）
16. **`system-architecture` — 系统总体架构与前后端渲染一致性**：Tauri 2 进程模型、导入期仅提缩略图（零全量 RAW 解码）与 Develop 按需生成 16-bit 线性代理的异步流水线、WebGL2 实时片段着色器与 Rust CPU 全分辨率导出器的数学等价契约（WYSIWYG）。
17. **`core-modules-walkthrough` — 核心源码模块导览**：`src/commands.rs` 编排层、`src/pipeline.rs` 与 `src/core_math.rs`、`src/color_science.rs`、`src/raw_backend.rs`（LibRaw + C++ mosaic shim）与 `src/rawler_backend.rs`、按行流式 TIFF/FFF 读取器、`src/calibration_fit.rs`、`src/persistence.rs` SQLite 迁移机制。
18. **`building-and-contributing` — 跨平台源码编译、契约测试与贡献规范**：Windows / macOS（Apple Silicon & Intel + LibRaw 0.22+）/ Linux 环境配置与编译命令、`cargo test` 与 `scripts/verify-*.cjs` 自动化回归契约解读、Issue 报告要素与 GPL-3.0 PR 提交规范。

### 6.3 在线文档阅读器（`DocsLayout.astro`）对标一流开源项目的体验标准
- **经典三栏响应式架构**：
  - **左栏（260px Sticky）**：四大板块分组可折叠章节树，高亮当前章节并显示预计阅读时长，顶部内置 `⌘K` / `Ctrl+K` 搜索触发框。
  - **中栏（`max-w-3xl` 黄金行宽 65–75 字符）**：顶部板块面包屑 + 版本适用徽标（`v1.1.0+`）→ 结构化正文（支持 GitHub 风格 `[!NOTE]` / `[!TIP]` / `[!IMPORTANT]` / `[!WARNING]` 警示框、构建期 KaTeX 高清数学公式、暗房风 SVG 数据流图、带语言标签与一键复制按钮的 Shiki 代码块、立体 `<kbd>` 按键样式）→ 底部上/下一篇章节导航卡片 + `在 GitHub 上查看/编辑此页` 链接（指向 `BillyDu-TJ/NexFilm/blob/main/docs/handbook/...`）。
  - **右栏（220px Sticky，`xl` 以上可见）**：`本页目录 (On This Page)`，基于 `IntersectionObserver` 实时追踪滚动高亮当前小节标题，移动端收敛为正文顶部可展开的悬浮目录条。
- **零成本毫秒级离线全文搜索（Pagefind）**：`astro build` 后执行 `npx pagefind --site dist`，支持中英文分词检索章节标题与正文段落并直接跳转定位小节。

---

## 7. 分阶段实施路线与验收契约

为保证工程实现高质可控、每一步均可独立编译回归，整体工程划分为 **4 个有序 Phase**（具体执行提示词见 [`docs/website-overhaul-phase-prompts.md`](./website-overhaul-phase-prompts.md)）：

| Phase | 阶段名称 | 核心交付物 | 关键验收指标 |
|---|---|---|---|
| **Phase 1** | **Monorepo 收敛、Release 双保险同步与暗房设计基座** | `website/` 迁入同仓 + `.gitignore` 修正；`github-releases.ts` + `release-hydrator.ts`；暗房色板 Token；精简版 `Header`/`Footer`；修复后的 `DownloadButtons`、`download` 与真实 `changelog` 双语页 | 离线/在线 `npm run build` 均成功；彻底消除 `NexFilm_1.1.0_` 404 链接与 `ghproxy`；桌面端既有 `scripts/verify-*.cjs` 全部通过 |
| **Phase 2** | **首屏重构、光学微交互实验室与营销/社区页面精修** | 首屏防孤字排版 + 错峰入场动效；`DensityInteractiveDemo`（片基扣除+印片光实时交互）；`WorkspaceShowcase` + 真双源 `BeforeAfter` + `gallery-manifest`；`features` / `gallery` / `faq` / `feedback` / `about` / `404` 双语全面升级与 KaTeX 公式渲染 | 页面零裸露 LaTeX 字符串、零 UI 截图 `invert` 穿帮、零虚构功能/预设名；`375px / 768px / 1280px / 1440px` 四档视口排版匀称 |
| **Phase 3** | **双端同源文档编译器、三栏文档阅读器与板块 ①② 精编** | `docs/handbook/{zh,en}/` 基础架构；`scripts/compile-handbook.mjs` 生成 `ui/documentation-data.js`（含离线 MathML）；三栏 `DocsLayout.astro`；板块 ①（4 章）与板块 ②（7 章）中英双语全量精编 | `node scripts/compile-handbook.mjs` 幂等输出；桌面端文档视图契约无损；官网 `/zh/docs/*` 与 `/en/docs/*` 路由一一对应 |
| **Phase 4** | **板块 ③④ 深度白皮书/架构指南、Pagefind 搜索与全链路验收** | 板块 ③（色彩科学 4 章，`inApp: true` 同步桌面端）与板块 ④（架构与贡献 3 章，官网专属）中英双语全量撰写；Pagefind `⌘K` 全局搜索；`verify-handbook-sync.cjs` 与 `website/scripts/verify-site.mjs` 全量自动化验收 | 18 章 × 中英双语 = 36 篇开源级文档齐备；Pagefind 中英搜索秒回；死链/双语对称/同源一致性自动化检查 100% 绿灯 |
