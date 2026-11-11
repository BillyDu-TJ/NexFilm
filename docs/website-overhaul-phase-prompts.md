# NexFilm 官网重构与双端同源文档工程 — 分阶段执行 Prompts

> **使用方式**：本文档包含 **Phase 1 → Phase 4** 共四个独立、边界严密、可逐步验证的工程任务 Prompt。  
> 请在工程对话中按顺序依次投递每个 Phase 的代码块内容，待当前 Phase 的验收命令全部通过后再进入下一 Phase。  
> **架构总纲**：[`docs/website-plan.md`](./website-plan.md)

---

## Phase 1：Monorepo 收敛、GitHub Release 双保险同步引擎与光学暗房设计基座

```markdown
你现在负责执行 **NexFilm 官网重构工程 — Phase 1（Monorepo 收敛、Release 双保险同步引擎与光学暗房设计基座）**。

### 一、必读架构与事实源文件（动手前先完整阅读）
1. `docs/website-plan.md`（v2.0 权威架构与实现规范，严格遵循其中的色板、拓扑、下载同步契约）
2. `.gitignore` 与 `.github/workflows/release.yml`（了解仓库忽略规则与真实发版产物规范）
3. `ui/style.css`（前 130 行，对齐桌面端真实的光学暗房暖灰与铜琥珀 `#c4814f` 视觉变量）
4. 当前网站原型目录 `/Users/apple/Code/nexfilm-web`（当前根目录下的 `website` 是指向它的软链接）

### 二、本阶段核心交付任务

#### 1. 将官网迁入主仓库 `website/` 实体目录并修正 `.gitignore`
- 移除主仓库根目录下的符号链接 `website`（**不要删除外部 `/Users/apple/Code/nexfilm-web` 原目录，作为安全备份保留**），将 `/Users/apple/Code/nexfilm-web` 中的源码（排除 `node_modules`、`dist`、`.astro`）复制到主仓库实体目录 `/Users/apple/Code/NexFilm/website/`。
- **关键防坑：修改根目录 `.gitignore`**：
  - 删除末尾忽略整个 `website` 的规则；
  - 新增忽略：`website/node_modules/`、`website/dist/`、`website/.astro/`、`website/.pagefind/`；
  - 因为根 `.gitignore` 存在全局 `*.jpg`、`*.jpeg`、`*.png`、`*.svg` 通配忽略，必须添加明确取反白名单（如 `!website/public/**`、`!website/src/**`、`!docs/handbook/**`），并通过 `git status` 验证 `website/public/` 下的 `.svg`/`.png`/`.jpg` 均未被意外忽略！
- 在 `website/astro.config.mjs` 中支持通过环境变量 `PUBLIC_SITE_URL`（默认 `'https://nexfilm.app'`）与 `PUBLIC_BASE_PATH`（默认 `'/'`）配置 `site` 与 `base`，并提供统一的 `website/src/utils/urls.ts` 路径辅助函数，确保未来无论部署在根域名还是子路径都无需重构路由。

#### 2. 实现 GitHub Releases「构建期快照 + 运行时实时追新」双保险引擎
- **离线兜底快照 `website/src/data/releases-fallback.json`**：
  - 拉取 `https://api.github.com/repos/BillyDu-TJ/NexFilm/releases` 当前真实的 4 个正式版（`v1.1.0`、`v1.0.2`、`v1.0.1`、`v1.0.0`）写入该 JSON 作为离线兜底数据。
- **构建期工具 `website/src/utils/github-releases.ts`**：
  - 构建时优先请求 GitHub Releases API（若存在 `process.env.GITHUB_TOKEN` 则附带 Authorization 头；请求失败或离线时无缝回退至 `releases-fallback.json`）。
  - 过滤掉 `draft: true` 的草稿，按发布时间倒序排列。
  - **动态资产正则匹配（严禁再拼写错误的 `NexFilm_1.1.0_...`！）**：
    - `macArm`: `/aarch64\.dmg$/i` 或 `/arm64\.dmg$/i`
    - `macIntel`: `/(x64|x86_64)\.dmg$/i`
    - `winX64`: `/(x64-setup\.exe|x64_en-US\.msi|nsis.*\.exe)$/i`
  - 为每个平台提取真实的 `name`（如 `NexFilm.Engine_1.1.0_aarch64.dmg`）、`browser_download_url`、精确格式化后的体积（如 `7.31 MB`，不再写假体积 `~45 MB`）、`sha256` 摘要（来自 GitHub `asset.digest` 字段）以及当前版本的 Release Tag 页面地址 `html_url`（如 `https://github.com/BillyDu-TJ/NexFilm/releases/tag/v1.1.0`）。
  - 清洗 Release `body`：剥离末尾面向 GitHub 页面的重复 `### 📥 下载指南` 及其后段落，将其余 Markdown（`### 新功能`、`### 修复与优化`、`### 升级提示`、`### 已知限制`）通过轻量 Markdown 渲染器（如 `marked`）转为干净安全的 HTML。
  - 新增 `website/src/data/release-en-notes.ts`：为现有纯中文的 `v1.0.0`–`v1.1.0` 提供准确的英文 Release Notes 翻译映射；若未来新发布的版本尚未收录人工英文映射（且正文不含英文分隔标记），英文更新日志页自动展示 GitHub 原始 Release Notes 并标注原文说明，确保未来发新版零人工阻塞。
- **客户端实时追新脚本 `website/src/scripts/release-hydrator.ts`**：
  - 使用 `sessionStorage`（15 分钟缓存 TTL）请求 `https://api.github.com/repos/BillyDu-TJ/NexFilm/releases/latest`（在 Changelog 页请求 `/releases?per_page=15`）及仓库信息 `/repos/BillyDu-TJ/NexFilm`（用于原生 Star 数更新）。
  - 当检测到线上最新 `tag_name` 新于构建期版本（或用于客户端平台自动识别高亮）时，原地无感更新：顶栏版本徽标、首屏主下载按钮文案与直链、`DownloadButtons` 三平台卡片直链/文件名/大小/SHA256/Release Tag 链接，以及在 Changelog 顶部动态插入新发布的版本卡片！
  - API 失败或限流时静默保持静态 HTML，零报错。**彻底移除所有 `ghproxy.net` 第三方镜像链接与文案**。

#### 3. 建立「光学暗房工业美学」设计系统与全局导航骨架
- 重构 `website/tailwind.config.mjs` 与 `website/src/styles/global.css`：
  - 严格落地 `docs/website-plan.md` 第 4.1–4.2 节定义的暖炭黑与铜琥珀 Token（`#121214` / `#161618` / `#1d1d20` / `#222225` / `#2a2a2e` / `#c4814f` / `#d28e59`），彻底清除 `#0F1117` 冷蓝灰背景、`bg-amber-500` 刺眼亮黄按钮与全局强制 `h1, h2 { font-family: Noto Serif SC }` 覆盖。
  - 安装并接入 `katex` 样式（供后续公式零闪烁静态渲染），在 `BaseLayout.astro` 启用 Astro `<ClientRouter />`（View Transitions），移除第三方慢速 `https://buttons.github.io/buttons.js`。
- 重构 `Header.astro` 与 `Footer.astro`（中英双语一致）：
  - 顶栏精简为 **5 个一级导航**（`功能` / `画廊` / `文档` / `更新日志` / `下载`）+ **1 个「社区」可无障碍键盘展开下拉菜单**（收纳 `常见问题`、`反馈与赞赏`、`关于`）。
  - 增加当前激活路由的铜色胶囊/底边指示器、原生暗房风 GitHub Star 徽章（带仓库链接与缓存计数）、语言切换（保持当前子路径切换 `/zh/` ↔ `/en/`）及动态同步的最新版本号徽标。
  - 优化移动端抽屉菜单展开/收起过渡与分组层次。
- 重构 `DownloadButtons.astro`、`src/pages/{zh,en}/download.astro`、`src/pages/{zh,en}/changelog.astro`：
  - 下载卡片展示真实资产直链、精确体积、一键复制 `SHA256` 按钮，以及醒目的「查看当前版本 GitHub Release 发布页（`v1.1.0`）」官方入口。
  - 更新日志页 100% 基于真实 GitHub Releases 渲染时间线，附带每个历史版本的直达 Release Tag 链接与资产折叠下载列表。

### 三、红线约束（Do Not Break）
- 严禁修改 `src/*.rs`、`Cargo.toml` 或破坏任何现有桌面端验证脚本 `scripts/verify-*.cjs`。
- 严禁保留任何 `NexFilm_1.1.0_` 错误链接或 `ghproxy` 字样。

### 四、强制验收门槛（全部通过方可结束）
1. 在 `website/` 下执行 `npm install && npm run build`，构建零报错通过；断网/屏蔽 GitHub API 模拟离线环境下 `npm run build` 依然 100% 成功。
2. 全局检索 `website/dist/`：确认包含 `NexFilm.Engine_1.1.0_aarch64.dmg`、`NexFilm.Engine_1.1.0_x64-setup.exe`、`NexFilm.Engine_1.1.0_x64.dmg` 与 `releases/tag/v1.1.0`，且 `NexFilm_1.1.0_` 与 `ghproxy` 匹配数为 **0**。
3. 在仓库根目录运行既有桌面端契约脚本（如 `node scripts/verify-runtime-assets.cjs`、`node scripts/verify-i18n-contract.cjs`）全部通过，且 `git status` 确认 `website/public/` 静态资源未被 `.gitignore` 误伤。
```

---

## Phase 2：首屏微调收口、光学原理微交互实验室与全站营销/社区页面精修

```markdown
你现在负责执行 **NexFilm 官网重构工程 — Phase 2（首屏微调收口、光学原理微交互实验室与全站营销/社区页面精修）**。

### 一、必读上下文文件（动手前先完整阅读）
1. `docs/website-plan.md`（重点阅读第 4 章视觉设计系统、微交互实验室规范、样片与工作台展示规范）
2. `data_process_pipeline_doc.md`（前 150 行）与 `docs/unified-density-pipeline.md`（确保页面上所有色彩科学术语、公式、参数上限与内置 `.cube` LUT 名称 100% 忠于真实代码，零虚构！）
3. `assets/luts/` 目录列表与 `ui/i18n.js`（核对真实内置相纸/印片 LUT 文件名与中英文界面术语）
4. Phase 1.1 已完成的 `website/src/` 布局、组件与 `website/scripts/` 自动化测试脚本

### 二、本阶段核心交付任务

#### 0. 先行清零 Phase 1.1 架构审查捕获的 2 处边缘缺陷（必做）
1. **修复根路由重定向脚本语法报错（`website/src/pages/index.astro`）**：
   - 第 13 行 `<script is:inline define:vars={{ targetZh, targetEn }}>` 使用了 `is:inline`，Astro 不会对其做 TypeScript 转译，导致编译产物 `dist/index.html` 原样输出了 `(navigator as any).userLanguage`，在浏览器触发 `Uncaught SyntaxError: Unexpected identifier 'as'`，致使英文浏览器访问 `/` 时 JS 重定向失效并被迫落入 `/zh/`。
   - 请改为纯原生合法 JS：`const userLang = navigator.language || navigator['userLanguage'] || '';`，并在无头浏览器测试中验证英文 `navigator.language = 'en-US'` 访问 `/` 能零报错跳转至 `/en/`。
2. **消除平板 `768px` 视口顶栏横向溢出（`website/src/components/Header.astro`）**：
   - 在 `768px` 宽度下，英文版顶栏同时展开 6 个桌面导航项 + Star 徽章 + 语言切换 + Download 按钮会导致页面出现横向滚动条（`scrollWidth > clientWidth`）。
   - 请将顶栏桌面导航与移动端汉堡按钮的响应式断点从 `md:`（`768px`）调整为 `lg:`（`1024px`）（即桌面 `<nav>` 使用 `hidden lg:flex`，移动端触发区与抽屉使用 `lg:hidden`），确保 `768px` 视口下中英文页面均 `overflowX === false`。

#### 1. 首屏（Hero）排版比例微调与入场动效（`src/pages/{zh,en}/index.astro`）
- **【用户钦定视觉铁律 · 严禁回退】**：
  - **必须完整保留**中英文首页 H1 主标题的 `font-serif`（`Noto Serif SC`）衬线字体，以及核心词组「`胶片密度域反转`」/「`Film Density Inversion`」的 `bg-gradient-to-r from-blue-400 via-sky-300 to-amber-300` 三色渐变与防降部截断样式（`pb-3 -mb-3 pt-1 -mt-1`）！**严禁将其改回无衬线或单色铜琥珀**。
- **优化英文版 H1 断行比例**：当前 `/en/` 的 H1 在 `1280px` / `1440px` 桌面视口下因 `max-w-4xl` + `lg:text-6xl` 偏宽，被硬拆成 `Scientific` / `Film Density Inversion` / `Workspace` 三行独立短句；请适度放宽英文 H1 容器宽度（如 `max-w-5xl`）并微调桌面端字号阶梯（如 `text-4xl sm:text-5xl lg:text-[3.35rem] xl:text-6xl`），使其在宽屏下自然排布为均衡的两行（或一行），且小写字母 `y` 降部完整无截断。
- **首屏错峰揭示动效**：实现克制的光学仪器级渐入（80ms Staggered Reveal），严格适配 `@media (prefers-reduced-motion: reduce)`（开启减弱动态效果时直接静态呈现）。

#### 2. 打造零依赖「片基扣除 & 印片光原理微交互实验室」（`DensityInteractiveDemo.astro`）
- 在中英文首页（首屏对比区与核心特性区之间）构建一个纯原生 Canvas 2D / SVG 驱动、零第三方重型库的实时光学原理交互组件，并完整适配 `astro:page-load` 生命周期（幂等挂载，跨页返回依然可用）：
  - 内置高质感合成胶片测试样张（含未曝光橙褐片基边框、135 齿孔、高光层次、中性灰阶、肤色与暗部细节），支持高分屏 `devicePixelRatio` 清晰绘制。
  - **三阶段物理模式切换按钮**：
    1. `01 原始透射率 T` / `01 Raw Transmittance T`（呈现彩色负片橙褐色罩原貌）
    2. `02 逐通道片基归一化 T / T_base` / `02 Base Normalized T / T_base`（剥离橙色色罩后的线性负像）
    3. `03 光学密度显影 D = -log₁₀(T)` / `03 Optical Density D = -log₁₀(T)`（真实对数密度反相正片）
  - **实时 RGB Printer Lights 微调推子**：提供 R（红-青）、G（绿-品红）、B（蓝-黄）三路对数印片光滑块（`-12` ~ `+12`）与 `Reset` 复位键，画布与右侧实时等宽仪器读数面板（显示 `D_base`、`D_net` 与三通道跨度条）60fps 同步响应。

#### 3. 重构工作台展示框与真双源样片对比器（`WorkspaceShowcase.astro` + `BeforeAfter.astro` + `gallery-manifest.ts`）
- **彻底废除对浅色空白 v1.0 UI 截图（`nexfilm-v1.0-develop.png`）强套 `filter invert hue-rotate-180` 冒充负片的穿帮做法**！
- **【交互基座锁死】**：重构 `BeforeAfter.astro` 视觉内容时，**必须 100% 保留 Phase 1.1 已验证通过的 Pointer Events + `setPointerCapture` + `draggable="false"` + `touch-action: pan-y` + 键盘 `ArrowLeft`/`ArrowRight` + `astro:page-load` 幂等绑定机制**，且双语标签（如底栏操作提示）根据传入的 `lang` 属性正确切换中英文！
- 建立 `website/src/data/gallery-manifest.ts` 与 `website/public/gallery/README.md` 素材接入规范：
  - 定义标准样片数据结构（`slug`、胶片型号如 `Kodak Portra 400` / `Fujifilm Superia X-TRA 400` / `Ilford HP5 Plus 400` / `Kodak Vision3 500T (5219)`、画幅 `135` / `120`、采集设备、输入域类型、所用真实内置 LUT 如 `Kodak Portra Endura` / `Kodak 2383` / `Ilford MGFB 2`、`negativeSrc`、`positiveSrc`）。
  - 生成或内置至少 **4 组**高分辨率、视觉说服力强的**真实配对「负片原貌（含典型 C-41 橙褐色罩或 B&W 银盐负像特征） ↔ 密度反转正片」双源图像资产**（SVG/WebP/PNG 静态资产或高精 Canvas 预渲染资产均可，严禁出现任何桌面 UI 按钮或白色窗口截图，严禁使用 CSS `filter: invert` 伪造！），供首页首屏与 `gallery.astro` 画廊页调用；当用户日后按 `public/gallery/README.md` 放入实拍文件时可直接替换路径生效。
- 打造 `WorkspaceShowcase.astro` 替换首页的旧版浅色空白截图：
  - 若 `website/public/assets/workspace-v1.1-dark.png` 实拍图存在则优先展示；否则渲染严格还原 **NexFilm v1.1 真实暗色工业界面**的高保真组件化/矢量工作台预览（还原顶栏 `Library / Roll Calibration / Hardware Calibration / Develop / Documentation` 视图切换、左侧裁切与 `Set Film Area` 工具、中央底片画布与黄色有效片门框、右侧 `Density Balance` / `RGB Printer Lights` / `Tone Curve` / `Histogram & Waveform` 检查器、底部胶卷底片条），彻底告别旧版浅色空白窗口。

#### 4. 全站营销/社区内页精修与构建期 KaTeX 公式渲染（中英双语严格对称）
- 编写构建期公式渲染工具 `website/src/utils/math.ts`（基于已安装的 `katex.renderToString`，支持行内与块级模式，`throwOnError: true` 确保构建期捕获拼写错误）。
- 全面升级 `{zh,en}/features.astro`、`{zh,en}/gallery.astro`、`{zh,en}/faq.astro`、`{zh,en}/feedback.astro`、`{zh,en}/about.astro` 与 `404.astro`：
  - 将 `features.astro` 与 `faq.astro` 中所有裸露的 LaTeX 字符串（如 `D = -\log_{10}(T)`、`\text{Film Area} \subset ...`、`$D = -\log_{10}(T)$` 等）全部改为构建期 `katex` 静态 HTML 渲染；**移除公式容器上的 `truncate` 截断类**，改为横向自适应或优雅换行，严禁公式被切成省略号 `...`！
  - 清除 `gallery.astro` 与 `about.astro` 中遗留的旧语义类名（如 `text-brand-light`、`bg-surface-card`、`border-surface-border`），统一对齐 `text-accent`、`bg-darkroom-panel`、`border-darkroom-line` 暗房色板。
  - **100% 对齐真实代码与内置 LUT 资产**：核对仓库 `assets/luts/` 下的 14 个真实 `.cube` 文件（彩色：`Kodak 2383`、`Kodak 2393`、`Kodak Endura Premier`、`Kodak Portra Endura`、`Kodak Supra Endura`、`Kodak Ultra Endura`、`Kodak Ektacolor Edge`；黑白相纸反差号：`Ilford MGFB 00`、`0`、`1`、`2`、`3`、`4`、`5`），修正 `features.astro` 中关于内置模拟预设的描述（如实说明内置 Kodak 2383/2393 印片与 Endura 相纸、Ilford MGFB 多反差号银盐相纸 `.cube` LUT，删除不存在的 Fuji 内置 LUT 宣传；同时检查印片光范围等参数描述忠于真实桌面端实现）。

### 三、红线约束（Do Not Break）
- 严禁修改 `docs/website-plan.md`、`src/*.rs`、`Cargo.toml`。
- 严禁破坏 Phase 1.1 已通过的 `npm run verify:static` 与全部 5 项浏览器自动化测试（请扩展 `test-browser-integration.cjs` 覆盖本阶段新增项！）。

### 四、强制验收门槛（全部绿灯方可交付）
1. `cd website && npm run verify:static && npm run build` 零错误零警告通过。
2. 扫描 `website/dist/**/*.html`：
   - 确认不存在任何裸露未渲染的 `\log_{10}`、`\quad`、`\xrightarrow`、`\subset` 字符串；
   - 确认不存在 `filter invert` 穿帮类名，也不再在 `BeforeAfter` 或 `WorkspaceShowcase` 中引用旧浅色图 `nexfilm-v1.0-develop.png`；
   - 确认 `dist/index.html` 不含 `as any` 语法错误。
3. 扩展并运行 `cd website && npm run test:browser`（无头 Chrome 实测）：
   - 原有 5 项测试继续 100% PASS；
   - 新增验证：英文环境访问 `/` 自动零报错跳转至 `/en/`；
   - 新增验证：首页 `DensityInteractiveDemo` 点击切换三阶段模式与拖动印片光滑块时，实时读数与 Canvas 画面正常更新；
   - 新增验证：在 `375px`、`768px`、`1280px`、`1440px` 四档视口下巡检 `/zh/`、`/en/`、`/zh/features/`、`/en/features/`、`/zh/gallery/`，全部满足 `document.documentElement.scrollWidth <= document.documentElement.clientWidth`（零横向溢出）且零 `pageerror`。
4. 桌面端防回归：`node scripts/verify-runtime-assets.cjs && node scripts/verify-i18n-contract.cjs` 100% 通过。
```

---

## Phase 2.1：首页真实软件 HTML 沙箱内嵌与功能页用户导向分类重构（产品审查修正阶段）

```markdown
你现在负责执行 **NexFilm 官网重构工程 — Phase 2.1（首页真实软件 HTML 沙箱内嵌与功能页用户导向分类重构）**。

Phase 2 虽已解决底层语法与响应式溢出问题，但在**产品定位、交互真实感与用户需求分析**上未通过架构审查，存在四大核心问题，必须在本阶段彻底重构收敛后再进入 Phase 3：
1. **首页「亲自体验」微交互组件（`DensityInteractiveDemo.astro`）冗余无意义**：抽象色块推子不仅拉长首屏动线，且对真实用户毫无说服力——必须从首页彻底移除。
2. **首页工作台展示（`WorkspaceShowcase.astro`）手搓假 UI 失真且不可操作**：Phase 2 手写的静态假界面甚至画错了软件真实布局（凭空捏造了左侧几何栏与假柱状图）。而 **NexFilm 桌面端前端本身就是纯正的原生 HTML/CSS/JS（`ui/index.html`）**！应在「专为暗房物理调色编排的工业级界面」窗口框内**直接内嵌真实的软件前端 HTML 交互沙箱**，让访客无需下载即可在网页中直接点击各个视图、菜单、面板、推子、曲线与弹窗，零距离感受真实软件操作！
3. **样片图（`website/public/gallery/*.svg`）过于简陋**：当前 3KB 卡通无脸小人矢量图严重拉低专业感，必须升级为高分辨率、具备真实胶片颗粒与自然光影层次的光栅配对样张（负片原貌 ↔ 反转正片）。
4. **功能页（`features.astro`）与首页特性卡片严重脱离用户视角**：全页堆砌 6 个无差别的数学公式模块，却遗漏了胶片用户真正高频刚需的核心功能（如**一键反转、整卷批量反转、按卷/画幅/相机/日期归纳管理、一键导出 Contact Sheet 接触印样、选择性参数复制粘贴、自动片门与畸变校正**等），缺乏轻重缓急与分类结构。

---

### 一、必读上下文文件（动手前先完整阅读）
1. 桌面端真实前端源码（**只读参考，严禁直接修改 `ui/` 下任何源文件！**）：
   - `ui/index.html`（完整核对顶栏菜单 `#native-menu-bar`、六大视图 `Library / Develop / Rolls / Hardware Calibration / Documentation / Sponsor`、右侧检查器各折叠面板、以及 `Import Roll` / `Copy Settings` / `Export` / `Export Contact Sheet` / `Film Calibration` 等全部真实功能与弹窗结构）
   - `ui/main.js`（重点阅读第 1–20 行 `__TAURI__` 调用入口、第 4215–4325 行 `nexfilm-proxy` 28 字节二进制代理纹理协议、第 6840–6995 行 `switch_active_image` 状态结构、第 10250–10275 行 `DOMContentLoaded` 初始化协议、第 10725–10825 行深浅色主题与多语言菜单逻辑）
   - `ui/i18n.js` 与 `ui/contact-sheet.js`（核对真实中英文功能术语与 135/120 接触印样能力）
2. 当前官网相关文件：`website/src/pages/{zh,en}/index.astro`、`website/src/pages/{zh,en}/features.astro`、`website/src/components/WorkspaceShowcase.astro`、`website/src/components/BeforeAfter.astro`、`website/src/data/gallery-manifest.ts`。

---

### 二、本阶段四大核心重构任务

#### 任务 1：首页结构收敛 —— 移除冗余实验室，保留首屏单张对比 + 下方内嵌真实软件 HTML 操作台
1. **删除冗余组件**：
   - 从 `website/src/pages/{zh,en}/index.astro` 中彻底移除 `<DensityInteractiveDemo />` 引用，并删除 `website/src/components/DensityInteractiveDemo.astro`（多组样片对比统一归口至 `/zh/gallery/` 与 `/en/gallery/` 画廊页，首页只保留首屏顶部的那一个 `<BeforeAfter />` 图片对比）。
   - **锁死首屏钦定基线**：100% 保留首屏 H1 的 `font-serif`、`bg-gradient-to-r from-blue-400 via-sky-300 to-amber-300` 三色渐变、`pb-3 -mb-3 pt-1 -mt-1` 防截断样式，以及 `BeforeAfter.astro` 已验证通过的 Pointer Events 拖拽机制。
2. **构建零侵入「真实桌面前端 Web 交互沙箱」（`website/scripts/sync-workspace-demo.mjs` + `WorkspaceShowcase.astro`）**：
   - **【铁律】严禁修改主仓库 `ui/*` 任何源文件**，确保桌面端构建与 `scripts/verify-*.cjs` 零受影响！
   - 编写同步构建脚本 `website/scripts/sync-workspace-demo.mjs`，并在 `website/package.json` 的 `dev` / `build` 前置钩子（如 `predev`、`prebuild`）中自动执行（同时将生成目录 `website/public/workspace-demo/` 加入根目录 `.gitignore`）：
     1. 将 `../ui/` 下的前端静态资产（`index.html`、`*.css`、`*.js`、`assets/`）同步拷贝至 `website/public/workspace-demo/`；
     2. 在拷贝后的 `website/public/workspace-demo/index.html` 中，于 `<script src="geometry.js"></script>` 之前注入 `<script src="tauri-web-shim.js"></script>`；
     3. 编写 `tauri-web-shim.js`（模拟 `window.__TAURI__.core.invoke`、`window.__TAURI__.core.convertFileSrc` 与 `window.__TAURI__.event.listen`），提供开箱即用的纯前端演示数据与无报错交互闭环：
        - **语言与主题同步**：读取 iframe URL 查询参数 `?lang=zh-CN` 或 `?lang=en` 设置初始语言，默认启用暗房深色主题（`nexfilm-theme = 'dark'`，且允许用户在顶栏 `setting` 菜单自由切换深/浅色主题与中/英文！）；
        - **预置丰富演示数据**：预置 3 卷不同画幅与胶片的归档数据（如 `Kodak Portra 400 · 135`、`Ilford HP5 Plus · 120 6x7`、`Kodak Vision3 500T · 135`，包含拍摄相机、日期、冲扫备注）及 4~6 帧演示底片缩略图，使访客切到 `Library` 与 `Rolls` 视图时能真实点击左侧画幅/相机/日期筛选器、查看胶卷详情、点击 `Export Contact Sheet` 预览印样流程；预置 1 套硬件标定演示 Profile 供 `Hardware Calibration` 视图浏览；`Documentation` 与 `Sponsor` 视图均可正常点击浏览；
        - **Develop 视图开箱可点可调**：按照 `ui/main.js` `loadProxyImage` 的 28 字节二进制头契约（`width`, `height`, `base_density[3]`, `isFullProxy = 1`, `flags = 1` + `Uint16Array` RGBA16UI 像素缓冲），在前端内存中为演示帧生成轻量级胶片负片纹理并通过 `Blob` URL 由 `convertFileSrc(id, 'nexfilm-proxy')` 返回；初始化完成后自动打开并聚焦演示卷首帧进入 `Develop` 视图！让访客可以直接在网页里点击体验：**顶栏裁切/旋转/翻转/`Set Film Area` 工具、右侧 `Histogram / Waveform` 示波器切换、`Auto Invert` / `Reset`、`Color / B&W` 切换、拖拽 `Printer Lights` / `Exposure` / `Contrast` 滑块与 `Tone Curve` 控制点曲线、切换内置 14 款 LUT 下拉框（内置轻量 identity/色调曲线 `.cube` 模拟数据防报错）、底部胶片条切帧、以及点开 `Import Roll` / `Copy Settings` / `Batch Apply` / `Export` 真实弹窗**！（涉及调用系统原生文件选择器的动作如选择本地文件夹时，以非阻塞 Toast 友好提示「当前为网页交互预览，下载桌面版即可导入与导出本地 RAW 原片」）。
   - 在 `WorkspaceShowcase.astro`（「专为暗房物理调色编排的工业级界面」板块）中：
     - 外层保留精致的暗房窗口边框与状态说明栏（附带「交互式真实界面预览 · 可直接点击体验菜单、视图切换与调色面板」提示徽章及「复位演示台」按钮）；
     - 桌面与平板横屏端直接内嵌响应式高宽比的 `<iframe src={withBase('/workspace-demo/index.html?lang=...')} loading="lazy" title="NexFilm Interactive Workspace" />`（注意禁止 iframe 内部抢占外层页面滚动焦点）；在超窄手机屏（`< 768px`）下提供横向可滑动/缩放视口或全屏展开体验按钮，保证手机端外层页面绝不横向溢出。

#### 任务 2：升级高质感配对样片资产（告别简陋卡通 SVG）
- 编写可复现的样片生成脚本或高精像素渲染器，将 `website/public/gallery/` 下那 4 组简陋的几何火柴人 SVG 升级为**高分辨率（建议 `1600×1000` WebP/PNG）、富有真实胶片摄影质感（具备细腻银盐颗粒、自然天空/建筑/街景/静物层次、高光滚降与暗部细节）的「负片原貌（含真实 C-41 橙褐色罩或黑白银盐负像密度特征） ↔ 密度反转正片」配对图像资产**，同步更新 `gallery-manifest.ts`，使首屏 `<BeforeAfter />` 与画廊页呈现令人信服的专业影像观感。

#### 任务 3：以「真实胶片用户需求」为核心，系统化重构功能页（`{zh,en}/features.astro`）与首页特性卡片
彻底摒弃「6 张平铺数学公式卡」的工程师自嗨写法，按照**用户感知优先级（轻重缓急）+ 结构化四大业务板块**重塑 `{zh,en}/features.astro`（中英双语严格对称）：

1. **首页（`{zh,en}/index.astro`）核心亮点区同步换血**：
   - 将原本偏底层的 4 张卡片调整为直击用户痛点的四大核心价值：**① 一键反相与整卷零跳色批处理**、**② 胶卷归纳检索与一键接触印样**、**③ 60fps 暗房印片光与 14 款相纸模拟**、**④ 光学密度底座与 100% 本地离线**，并在区块底部提供醒目入口跳转至 `/zh/features/`（`/en/features/`）查看完整功能矩阵。

2. **功能页（`{zh,en}/features.astro`）顶部增设「结构化分类导览栏（Sticky Category Nav）」**：
   - 页面 Hero 下方设置吸顶分类锚点栏，清晰呈现四大层级，点击平滑跳转：
     - `01 极速反转与批处理`（Core Inversion & Batch）
     - `02 胶卷归纳与接触印样`（Roll Archive & Contact Sheet）
     - `03 暗房精修与相纸模拟`（Darkroom Grading & Print LUTs）
     - `04 色彩科学底座与专业交付`（Color Science & Pro Delivery）

3. **四大板块内容轻重缓急编排（严禁凭空捏造软件不存在的功能，全部对齐 `ui/index.html` 真实能力）**：
   - **板块 01【第一优先级 · 效率核心】极速反转与整卷一致性工作流（解决「翻拍后怎么最快调正、整卷不跳色」的头号痛点）**：
     - **单帧一键自动反相（`Auto Invert`）**：自动识别片基色罩与有效密度范围，瞬间还原准确正片色彩，告别手动拉反相曲线导致的交叉偏色。
     - **整卷标定与一键整卷反相（`Calibrate Film` + `Auto Invert Roll`）**：采样同卷透明片基（Film Base）与曝光片头（Film Leader）锁定物理锚点，按全卷最亮画面标定共享白点（自动跳过标定帧），一键批量反转全卷 36 张（135）或 12 张（120），同光线下影调严丝合缝。
     - **细粒度选择性复制粘贴与批量片门同步（`Copy Settings` + `Batch Apply`）**：支持「全部 / 仅影调色彩 / 仅几何」快捷预设，或跨扫描密度、印片光、美学影调、曲线、细节、几何变换、相纸模拟、齿孔设置等 10 大参数组自由勾选同步；固定翻拍机位下一键将有效片门与片基应用至全卷。
   - **板块 02【第二优先级 · 资产沉淀】胶卷数字化归纳管理与接触印样（解决「几十卷底片怎么管、怎么出归档小样」的高频刚需）**：
     - **贴合胶片直觉的三态导入与 40+ 胶卷型号库**：完整支持「按卷导入（Import by Roll）」、「散张快修（Loose Import）」、「续编已有胶卷（Continue Roll）」；内置 40+ 款彩色负片（Portra / Gold / Ultramax / Ektar / Superia / Cinestill / Phoenix / 乐凯等）、黑白负片（Tri-X / T-Max / HP5+ / Delta / Acros II / Fomapan 等）、反转片与电影卷（5203 / 5207 / 5219 等）词典，并自动记忆用户自定义机身与胶卷。
     - **多维胶卷档案库检索（`Rolls` 视图）**：支持按 **全系画幅（135 全格/半格、APS、120 645~6×17、4×5 / 5×7 / 8×10 大画幅）、拍摄相机、冲洗/拍摄日期** 秒级交叉筛选，支持随时查阅与修改每卷专属备注（Roll Notes）及将散张升级归档入库。
     - **一键生成专业接触印样（`Export Contact Sheet`）**：无需外部排版软件，一键生成带拟真 **135 齿孔片边编码 / 120 画幅片边字码** 及卷名、机身、胶卷、日期抬头的整卷高清接触印样，兼顾实体暗房仪式感与高效选片归档。
     - **令牌化批量重命名与 EXIF 注入**：支持 `{Roll}`、`{Camera}`、`{Film}`、`{Date}`、`{Original}`、`{Seq}` 自由组合命名模板与重名冲突策略，并可将胶卷、相机与日期一键写入导出图像 EXIF。
   - **板块 03【第三优先级 · 创作精修】暗房光学微调、底片几何与经典相纸（解决「怎么修得干净、修出暗房味」的进阶诉求）**：
     - **智能片门识别、齿孔净边与光学几何校正**：`Auto Area` 一键识别有效画幅剔除背光板高光白边与片夹黑框；配备齿孔取样消除（Sprocket Sampling）、多比例裁切、微调拉直、水平/垂直透视校正及微距翻拍镜头桶形/枕形畸变补偿。
     - **60fps 实时暗房调色台与专业示波器**：还原传统彩色放大机红/绿/蓝互补印片光推子（RGB Printer Lights）与中性灰吸管；配备 RGB 四通道控制点曲线、曝光/反差/高光/阴影/色温色调精修、实时直方图与工业波形图（含 `!` 溢出预警），以及按住 `Alt` 实时预览边缘锐化遮罩与质感（Texture）/清晰度（Clarity）/去朦胧（Dehaze）。
     - **内置 14 款经典印片/银盐相纸 `.cube` LUT 与黑白模式**：开箱内置 7 款彩色印片与相纸（`Kodak 2383 / 2393` 及 `Endura` 系列）+ 7 款黑白银盐相纸（`Ilford Multigrade FB 00–5` 全号段反差），支持强度混合与加载自定义 `.cube` 文件，一键切换纯粹黑白显影流。
   - **板块 04【第四优先级 · 硬核底座】光学色彩科学、硬件标定与母带交付（压轴展示底层专业壁垒，精炼呈现物理公式而非喧宾夺主）**：
     - **统一对数光学密度管线**：在此处精炼展示核心物理定律 $D = -\log_{10}(T)$（构建期 KaTeX 渲染，零截断），阐明为何逐通道片基归一化能让相机 RAW、16-bit TIFF 与哈苏 Imacon `.fff` 获得统一、可解释的真实色彩。
     - **百兆级流式分行解码与广色域线性母带交付**：流式分行读取数百 MB 的 `.fff` 与 16-bit TIFF 扫描件；支持输出 16-bit/8-bit TIFF、16-bit PNG、JPEG，以及面向影视后期的 `16-bit Linear TIFF`、`Linear DNG`、`Camera RAW DNG`，内嵌 `sRGB`、`Display P3`、`Adobe RGB`、`Rec.2020`、`ProPhoto RGB`、`ACEScg (AP1)`、`ACES2065-1 (AP0)` 七大色彩空间。
     - **实验性翻拍台硬件标定与扫描仪校准（`Hardware Calibration`）**：支持暗场扣除、光源平场均匀度补偿、3×3 通道串扰分离矩阵拟合与扫描仪参考配置管理。
     - **100% 本地离线与非破坏性数据安全**：Rust + Tauri + 本地 SQLite（`nexfilm_user.db`）架构，原始底片永远只读不改，零联网上传、零隐私收集。

---

### 三、红线约束（Do Not Break）
- **严禁修改 `docs/website-plan.md`、`src/*.rs`、`Cargo.toml` 以及 `ui/` 目录下的任何桌面端源码文件**（所有针对网页内嵌沙箱的垫片注入必须在同步后的 `website/public/workspace-demo/` 副本上完成！）。
- 严禁破坏首屏 H1 钦定视觉样式、根路由语言重定向、顶栏 `768px` 防溢出与 `BeforeAfter` 拖拽交互。

---

### 四、强制验收门槛（全部绿灯方可交付）
1. `cd website && npm run verify:static && npm run build` 零错误通过，且 `website/dist/workspace-demo/index.html` 与 `tauri-web-shim.js` 成功生成。
2. 更新并运行无头 Chrome 集成测试 `cd website && npm run test:browser`：
   - 验证首页已移除 `DensityInteractiveDemo`，保留首屏单个 `BeforeAfter` 且拖拽正常；
   - 验证首页 `WorkspaceShowcase` 内嵌的真实 `ui/index.html` iframe 成功完成初始化（`#startup-loading` 自动消失，零未捕获 `pageerror`），在 iframe 内点击顶栏切换 `Rolls` / `Library` / `Develop` 视图、点击菜单切换深浅色主题或中英文、点击打开 `Copy Settings` 或 `Import Roll` 弹窗均真实响应！
   - 验证中英文 `/zh/features/` 与 `/en/features/` 四大分类板块与锚点导览完整渲染，覆盖 `Auto Invert`、`Auto Invert Roll`、`Rolls` 归纳筛选、`Contact Sheet` 接触印样、14 款内置 LUT 与光学密度底座，且 KaTeX 公式零裸露、零截断；
   - 验证 `375px`、`768px`、`1280px`、`1440px` 四档视口下 `/zh/`、`/en/`、`/zh/features/`、`/en/features/`、`/zh/gallery/` 全部 `scrollWidth <= clientWidth`（零横向溢出）。
3. 桌面端契约零污染验证：`git status ui/` 确认 `ui/` 目录 **0 修改**，且根目录 `node scripts/verify-runtime-assets.cjs && node scripts/verify-i18n-contract.cjs` 100% 通过。
```

---

## Phase 2.2：样片真实摄影级升级与首页 HTML 沙箱细节收口（轻量视觉修复）

```markdown
你现在负责执行 **NexFilm 官网重构工程 — Phase 2.2（样片真实摄影级升级与首页 HTML 沙箱 5 项视觉细节收口）**。

Phase 2.1 的架构重构、功能页四大业务板块与真实桌面端 HTML 内嵌底座已通过代码审查，但在真实无头浏览器截屏验收中发现 **5 处具象视觉与数据接线瑕疵**，请在保持其余页面不变的前提下做精准外科手术式修复：

### 一、精准修复清单（共 5 项）

1. **升级 4 组样片为真实摄影质感且 100% 像素级对齐的正负片（修复积木色块画与滑块上下跳动）**：
   - 当前 `website/public/gallery/*.webp` 仍是由脚本绘制的扁平长方形积木楼与锯齿三角形，且 `portra-400-neg.webp` 因上下叠加齿孔边框压缩了画面纵向比例，导致拖动首屏 `<BeforeAfter />` 滑块时左右楼房高度严重错位。
   - **修复方案**：
     1. 调用 `image-generator` 子代理（或使用高质量真实摄影样张）生成 4 张 `1600×1000` 具有真实银盐颗粒、丰富自然光影与场景细节的正片母版（对应 `Portra 400` 暖调街景建筑/人像、`Superia 400` 街头绿植光影、`HP5 Plus 400` 黑白山川纪实、`Vision3 500T` 钨丝灯夜景电影感，画面内绝不出现乱码文字）；
     2. 编写精确的像素级物理负片反演脚本生成配对 `*-neg.webp`：**严禁缩放或纵向挤压底层画面像素坐标**（确保在任意百分比拖动 `<BeforeAfter />` 分割线时，左右正负片的建筑轮廓、山脊与人物边缘 100% 亚像素严丝合缝对齐！彩色负片按真实 C-41 橙褐色罩光学透射率 $D_c = D_{\text{base},c} + \gamma_c \cdot L_c$、$T_c = 10^{-D_c}$ 映射，其中 $D_{\text{base}} = [0.16, 0.44, 0.76]$ 呈现真实橙褐色罩负像；黑白负片按单通道银盐密度反演）。

2. **修复 `tauri-web-shim.js` 中 `D_base` 通道写反导致的 `#preview-canvas` 深蓝黑波浪问题，并喂入真实样片纹理**：
   - 当前 `website/scripts/tauri-web-shim.js` 第 325–327 行与第 387 行将 C-41 片基密度写成了 `[0.85, 0.55, 0.35]`（红高蓝低），而像素透射率写了 `tR > tB`（物理上橙色色罩透红吸蓝，正确密度应为红低蓝高，如 `base_density = [0.16, 0.44, 0.76]`，`d_max = [1.86, 2.14, 2.46]`），导致真实 WebGL2 着色器反相后红色归零、画布变成一团深蓝正弦波！
   - **修复方案**：在 `tauri-web-shim.js` 中，将演示样片（如 `/gallery/portra-400-pos.webp` 等）绘制到离屏 `<canvas>`（如 `640×400`）读取真实像素，按与 `d_min = [0.16, 0.44, 0.76]`、`d_max = [1.86, 2.14, 2.46]` 严格互逆的物理公式生成每个演示帧专属的 28 字节头 + `RGBA16UI` 代理缓冲（`convertFileSrc(filePath, 'nexfilm-proxy')` 根据帧路径返回对应纹理），让 `#preview-canvas` 和右上角 `直方图 / 波形图` 渲染出清晰、层次丰富的真实彩色/黑白照片！

3. **修复 `Develop` 视图底部底片条（`#filmstrip-container`）完全空白的问题**：
   - 根因：`ui/main.js` 第 6523–6539 行在未激活胶卷编辑态（`currentRollViewId && isRollEditing`）时，底片条只显示 `roll_id === null` 或 `'LOOSE_DEFAULT'` 的帧。
   - **修复方案**：在 `tauri-web-shim.js` 初始化完成后，或通过模拟激活 `roll_portra_400` 进入工作集编辑态（例如触发 `Rolls` 中首卷的继续编辑流程，或在 `demoItems` 中提供一组默认处于当前工作会话的帧并同步真实缩略图），确保进入 `工作台 (Develop)` 时**底部底片条完整展示 4 张可点击切换的真实缩略图**，点击任意一帧均可实时切换 `#preview-canvas` 画面！

4. **修复 `Rolls` 胶卷卡片 8 格预览全黑 & 14 款内置 `.cube` LUT 切换无效果的问题**：
   - 在 `tauri-web-shim.js` 的 `get_roll_previews` 分支中，根据 `args.rollId` 返回该胶卷下对应 `demoItems` 的真实缩略图数组（不再返回空数组 `[]`）；
   - 在 `load_3d_lut` 分支中，通过 `fetch(args.path)` 真实读取并解析已同步至 `workspace-demo/assets/luts/*.cube` 的 `.cube` 文件（解析 `LUT_3D_SIZE N` 与 `N³` 行 RGB 浮点数据填充 RGBA `Float32Array`），让用户在沙箱下拉框切换 14 款 Kodak / Ilford 相纸时产生真实的胶片色彩与黑白号数反差变化！

5. **修复 `WorkspaceShowcase.astro` 按钮双重事件绑定 Bug**：
   - `website/src/components/WorkspaceShowcase.astro` 第 178–179 行同时调用了 `initWorkspaceShowcase()` 与 `document.addEventListener('astro:page-load', initWorkspaceShowcase)` 且缺少 `dataset` 幂等锁，导致首次加载时「全屏体验」与「复位演示台」绑定了两次点击事件。请移除裸调，仅保留 `astro:page-load` 并在按钮上增加 `dataset.wsBound === 'true'` 幂等守卫。

### 二、验收门槛
- **严禁修改 `ui/*`、`src/*`、`Cargo.toml`、`docs/website-plan.md`**。
- 运行 `cd website && npm run verify:static && npm run test:browser`，并在 `test-browser-integration.cjs` 中补充断言：`#workspace-demo-iframe` 内 `#filmstrip-container .film-item` 数量 `>= 4`、`#library-rolls-grid` 首张卡片内 `img` 数量 `>= 2`、`#preview-canvas` 非纯黑/纯蓝偏色。
```

---

## Phase 3A：双端同源文档编译器（含程序化防敷衍门禁）、三栏阅读器与板块 ① 精编

```markdown
你现在负责执行 **NexFilm 官网重构工程 — Phase 3A（双端同源文档编译器、三栏开源文档阅读器与板块 ① 快速上手双语精编）**。

> **【最高质量警告】**：为彻底杜绝大模型写文档常见的「大纲式敷衍、编造不存在的按钮/快捷键、充斥营销套话、中英不对称、破坏桌面端回归契约」五大顽疾，我们将 Phase 3 拆分为 **Phase 3A（编译器 + 阅读器 + 板块 ① 8 篇 + 桌面端存量色彩科学保底迁移）** 与 **Phase 3B（板块 ② 全工作台手册 14 篇深度撰写）**。你必须严格遵守下方每一条程序化门禁与事实铁律！

---

### 一、必读一手事实源（动笔前必须完整通读，严禁凭想象编造）
1. `docs/website-plan.md`（精读第 6 章：SSOT 双端编译流水线、Frontmatter 规范、三栏 `DocsLayout.astro` 交互标准）
2. **`ui/documentation-data.js`（必须通读全部 1838 行！）**：这是当前桌面端已验证的权威事实库。后续新文档必须在保留其 **100% 事实细节与操作准确性** 的基础上做结构化扩充与深度阐述，绝不允许丢失任何已有技术细节！
3. `ui/documentation.js` 与 `ui/style.css`（第 3174–3731 行）：核对桌面端渲染契约（`.doc-callout`、`.doc-callout-warn`、`.doc-table`、`.doc-code-block`、`#doc/<chapterId>/<sectionId>` 哈希路由）。
4. `scripts/verify-develop-detail-ui.cjs`（第 178–198 行与第 603–616 行）：**极其关键的桌面端回归红线**！该测试强制校验：
   - `'zh-CN'` 与 `'en'` 的全部章节 `section.id` 扁平数组必须 100% 相等；
   - 必须存在 `id === 'curve-and-detail'` 的小节；
   - 每个 `section.content` 中的 `<p>`, `<table>`, `<tr>`, `<td>`, `<ul>`, `<li>`, `<ol>`, `<strong>`, `<code>` 开闭标签数量必须严格相等！
5. `README.md`、`ui/i18n.js`、`ui/settings-copy.js`、`ui/export-settings.js`：核对中英文真实界面词条、参数名与导出令牌。

---

### 二、文档写作「事实铁律与文风红线」（违反任一条即判定不通过）

#### 1. 客观专业文风准则（对标 Apple Pro Apps 手册 / Darktable Manual / Astro Docs）
- **零感叹号、零营销吹捧、零 AI 套话**：正文中（除代码块或描述界面上的 `!` 溢出预警图标外）**严禁使用中文或英文感叹号（`！` / `!`）**；严禁出现 `颠覆`、`黑科技`、`秒杀`、`极致`、`震撼`、`完美无瑕`、`轻松拿捏`、`赋能`、`众所周知`、`总而言之`、`让我们一起`、`不仅如此` 以及英文 `blazingly`、`revolutionary`、`game-changing`、`magic`、`effortlessly`、`dive into`、`delve`、`in conclusion`、`unleash`。
- **坚持「现象/痛点 → 物理或工程原因 → 精确界面操作路径 → 可验证预期结果」四段式专业写法**：例如解释「胶片范围（Film Area）」时，不能只写“点击自动识别然后保存”，必须讲清“为什么观片灯板 100% 透光白边或齿孔一旦落入框内，会取代胶片最亮像素成为错误端值，导致整体正片影调被压低而发灰发暗”。
- **禁止空洞无主语列表堆砌**：严禁把小节写成三五行干瘪 bullet 大纲。每个 `## 二级小节` 必须有连贯的技术自然段论述，配合结构化表格、有序操作步骤与边界条件说明。
- **排版规范**：界面按钮与视图名统一用加粗标注（如点击右上角 **导入胶卷** → **按胶卷导入**）；键盘按键统一使用 `<kbd>Ctrl</kbd>+<kbd>Z</kbd>`；文件名、路径、令牌统一使用反引号（如 `nexfilm_user.db`、`{Roll}_{Seq}`）；英文版必须是地道的母语级技术文档英文，严禁生硬机翻腔。

#### 2. 事实零差错锁死词典（严禁超纲编造！）
- **全局快捷键锁死（仅此 9 项，严禁编造 `C` 裁切、`R` 旋转、`Ctrl+E` 导出等虚假快捷键！）**：
  1. `<kbd>Ctrl</kbd>+<kbd>Z</kbd>` / `<kbd>⌘</kbd>+<kbd>Z</kbd>`：撤销（不在文本输入框时）
  2. `<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd>` 或 `<kbd>Ctrl</kbd>+<kbd>Y</kbd>`：重做
  3. `<kbd>←</kbd>` / `<kbd>→</kbd>`：切换上一张/下一张画面（底片条可见时）
  4. `<kbd>Enter</kbd>`：裁切模式中确认裁切；滑块数值输入时确认输入
  5. `<kbd>Esc</kbd>`：关闭当前对话框、菜单或取消滑块数值输入
  6. 按住 `<kbd>Space</kbd>` 拖动：平移预览画布
  7. 画布鼠标滚轮：以指针为中心缩放预览；底片条鼠标滚轮：横向滚动列表；检查器空白处滚轮：翻动参数分组
  8. 滑块鼠标滚轮：必须**先点击一次该滑块激活焦点**后，滚轮才会微调数值（防误触设计）
  9. 按住 `<kbd>Alt</kbd>` 拖动锐化滑块：拖动 **数量 / 半径 / 细节** 显示锐化边缘信号的灰度视图；拖动 **蒙版** 显示锐化蒙版灰度视图（仅替换预览画布，不影响示波器、缩略图与导出）
- **真实中英文界面命名锁死（100% 对齐 `ui/i18n.js`）**：
  - 左上菜单栏：中文 `NEXFILM` · **设置** · **视图** · **帮助** / 英文 `NEXFILM` · **setting** · **View** · **Help**
  - 顶栏视图标签：中文 **图库** · **工作台** · **历史胶卷记录** · **硬件校正** · **使用文档** · **赞助** / 英文 **Library** · **Develop** · **Rolls** · **Hardware Calibration** · **Documentation** · **Sponsor**
  - 顶栏右侧全局按钮：中文 **导出胶卷** · **导入胶卷** · **导出** / 英文 **Export Roll** · **Import Roll** · **Export**
  - 导入三模式：**按胶卷导入（Import by Roll）**、**散张导入（Loose Import）**、**继续编辑胶卷（Continue Editing Roll）**
- **关键事实与已知边界锁死**：
  - 安装包（Windows x64 `.exe`、macOS Apple Silicon `aarch64.dmg`、macOS Intel `x64.dmg`）目前**未进行代码签名与 Apple 公证**：Windows 可能触发 SmartScreen“未知发布者”（点击“更多信息 → 仍要运行”）；macOS 拖入 Applications 后首次被拦截可在 Finder 右键选择 **打开** 或在 **系统设置 → 隐私与安全性** 点击 **仍要打开**（切勿诱导用户关闭系统全局 SIP！）；提醒切勿下载 Releases 底部的 `Source code (zip/tar.gz)`。
  - 数据目录：Windows 正式版 `%APPDATA%\NexFilm Engine\`，macOS 正式版 `~/Library/Application Support/NexFilm Engine/`，主数据库 `nexfilm_user.db`（`rolls.json` 为兼容镜像）。**源扫描文件永远只读不拷贝、不覆盖**；移动原片后需在应用内使用 **定位文件（Locate File）** 重新关联；从 `v1.0.2` 升级至 `v1.1.0` 后需对旧胶卷重新执行一次 **自动反相（Auto Invert）** 刷新旧配方缩略图。
  - Nikon Z8 的 **HE / HE\*** 高效率压缩 NEF 因底层 LibRaw 暂不支持无法直接解码（需在机内改用无损压缩 RAW，或先用 Adobe DNG Converter 转为线性/未压缩 DNG）。
  - 胶卷信息中的 40+ 款胶卷型号列表用于**元数据归档、接触印样抬头与 EXIF 写入**，NexFilm 的反相依靠实测片基密度而非套用黑盒胶卷预设；内置 `.cube` 相纸模拟 LUT 共 **14 款**（7 款 Kodak + 7 款 Ilford MGFB 00–5，严禁编造内置富士 LUT）。

---

### 三、Phase 3A 三项工程交付任务

#### 任务 1：建立 `docs/handbook/{zh,en}/` 唯一事实源与带「防敷衍质量门禁」的编译器 `scripts/compile-handbook.mjs`
1. 目录结构：
   - `docs/handbook/zh/01-getting-started/` 与 `docs/handbook/en/01-getting-started/`（本阶段精写全部 4 章 = 8 篇）
   - `docs/handbook/zh/02-user-manual/` 与 `docs/handbook/en/02-user-manual/`（本阶段先将现有 `ui/documentation-data.js` 中的 `interface-basics`、`import-and-rolls`、`develop-workspace`［**含必留的 `{#curve-and-detail}` 小节**］、`export-and-contact-sheets`、`troubleshooting`、`appendix` 无损迁移为合规 Markdown 基线，Phase 3B 再全面扩写重构为 7 章旗舰手册，确保本阶段桌面端内容零退化！）
   - `docs/handbook/zh/03-color-science/` 与 `docs/handbook/en/03-color-science/`（无损迁移现有 `color-science` 与 `hardware-calibration` 2 章作为 `inApp: true` 基线，Phase 4 扩写为 4 篇白皮书）
2. 编写 `scripts/compile-handbook.mjs`（并在根或 `website/package.json` 暴露 `compile:handbook` 命令，同时确保 `website/scripts/sync-workspace-demo.mjs` 在执行前或执行时同步最新生成的 `ui/documentation-data.js`！）：
   - 解析 YAML Frontmatter（`id`, `track`, `trackOrder`, `chapterOrder`, `kicker`, `title`, `subtitle`, `readingTime`, `inApp`, `sinceVersion`, 可选 `aliases: string[]`）。
   - 按 `## 小节标题 {#section-id}` 切分为 `sections: [{ id, heading, content }]`：
     - GitHub Alerts（`> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`）编译为桌面端原生 `.doc-callout` 与 `.doc-callout.doc-callout-warn`（含 `.doc-callout-title` 与 `.doc-callout-body`）；
     - Markdown 表格编译为 `<table class="doc-table"><thead>...</thead><tbody>...</tbody></table>`；
     - 代码块编译为 `<pre class="doc-code-block"><code>...</code></pre>`；
     - 行内 `$...$` 与块级 `$$...$$` 公式在编译进桌面端时使用 `katex.renderToString(..., { output: 'mathml', throwOnError: true })` 输出纯原生离线 `<math>` 标签；
   - **内置硬性编译门禁（任一不满足立即打印具体违规文件并 `process.exit(1)`）**：
     1. `zh` 与 `en` 文件集合、`id`、`track`、`inApp` 及每章有序 `section.id` 数组 100% 一致；
     2. 对所有标记为已精编章节（本阶段 `01-getting-started/*` 全部 4 章）：中文纯可见字符数 **≥ 1,800 字**，英文纯可见词数 **≥ 950 words**，小节数 **4～7 个**，每章至少包含 **1 张 `<table>`** 与 **1 个 `.doc-callout`**；
     3. 全文扫描拦截上述禁用营销词黑名单与正文感叹号；
     4. 全量执行与 `scripts/verify-develop-detail-ui.cjs` 相同的 HTML 闭合标签平衡校验（`p, table, tr, td, ul, li, ol, strong, code`）以及 `'curve-and-detail'` 小节存在性校验！
   - 输出 `ui/documentation-data.js`（顶部带 `// AUTO-GENERATED FROM docs/handbook — DO NOT EDIT MANUALLY`），`getChapterById(id, locale)` 支持 `chapter.id === id || (chapter.aliases || []).includes(id)`（将 `'quick-start'` 别名指向 `'overview-and-install'`），并在 `ui/documentation.js` / `ui/style.css` 中做克制的无损增强：非搜索态下当相邻章节 `track` 变化时渲染暗房风板块小标题（如 `快速上手` / `工作台手册` / `色彩科学`）。

#### 任务 2：构建三栏开源级在线文档系统（`DocsLayout.astro` 及多路由体系）
- 配置 `website/src/content.config.ts` 加载 `../docs/handbook/zh` 与 `../docs/handbook/en`。
- 构建路由：
  - `/zh/docs/` 与 `/en/docs/`：文档门户首页（按四大受众场景提供结构化导览卡：快速上手、工作台全手册、色彩科学参考、源码架构贡献，以及核心任务速查入口）。
  - `/zh/docs/[...slug]/` 与 `/en/docs/[...slug]/`（使用章节 `id` 或 `<track>/<id>` 清晰一致的路由结构，确保顶栏语言切换按钮在任意文档内页点击后**精准跳转到同一章节的对应英/中文页面**，绝不跳回首页或 404！）。
- 实现三栏 `DocsLayout.astro`、`DocsSidebar.astro`、`DocsToc.astro`、`DocsPager.astro` 与 `website/src/styles/docs-prose.css`：
  - **左栏（260px Sticky）**：按板块分组展示章节树、当前章节铜琥珀色激活态、阅读时长、移动端可展开目录抽屉、顶部搜索按钮占位（Phase 4 接入 Pagefind）。
  - **中栏（`max-w-3xl` 黄金阅读行宽）**：板块面包屑 + `适用于 v1.1.0+` 徽标 → H1 与 Subtitle → 高可读性技术排版（四色 GitHub Alerts、构建期 KaTeX 公式、响应式数据表格、带复制按钮的代码块、立体 `<kbd>` 按键）→ 底部上/下一章双向导航卡片 + `在 GitHub 上查看此页源码` 链接。
  - **右栏（220px Sticky，`xl` 以上显示）**：`本页目录 / On This Page`，提取 H2/H3 锚点并通过 `IntersectionObserver` 随滚动实时高亮。

#### 任务 3：高标准精写板块 ① `01-getting-started` 全部 4 章双语文档（共 8 篇 Markdown，每篇中文 ≥ 1,800 字 / 英文 ≥ 950 词）
1. **`overview-and-install.md`（`id: overview-and-install`, `aliases: ["quick-start"]`）—— 认识 NexFilm 与跨平台安装指南**
   - 必含深度内容：NexFilm 解决的核心问题（物理可解释的对数密度反相 vs 传统显示伽马曲线硬拉偏色）；Windows x64 / macOS Apple Silicon / macOS Intel 安装包选型对照表（明确勿下 Source code 压缩包）；Windows SmartScreen 与 macOS 未公证 DMG 首次启动放行的标准安全步骤；本地数据目录结构、源片只读承诺与首次试跑备份建议；界面四大区域（菜单栏、顶栏五大视图与全局按钮、主工作区、底片条）速览表。
2. **`scanning-best-practices.md`（`id: scanning-best-practices`）—— 相机翻拍与扫描仪前期采集规范**
   - 必含深度内容：为什么前期采集质量直接决定去色罩上限；背光光源显色性要求（CRI ≥ 95 / TLCI ≥ 98，为什么廉价三基色荧光灯或手机屏幕会导致通道串扰）；环境杂散光与片夹漏光遮挡规范（杂散光如何在高密度高光区引入伪雾翳）；数码相机翻拍参数设置表（原生最低 ISO、微距镜头最佳衍射光圈 f/5.6–f/8、关闭机内自动优化/D-Lighting、基于直方图的 ETTR 向右曝光不截断原则）；**保留未曝光透明片基边缘的核心铁律**；扫描仪输出线性 16-bit TIFF / `.fff` 关闭扫描软件自动反相与自动色阶的注意事项。
3. **`first-roll-in-five-minutes.md`（`id: first-roll-in-five-minutes`）—— 五分钟完成第一卷底片反相与导出**
   - 必含深度内容：面向首次打开软件用户的端到端闭环实操教程——① 导入选择（按卷填写画幅/机身/胶卷/日期 vs 散张拖入）；② 进入 **工作台（Develop）** 理解按需代理生成机制；③ **设置胶片范围 → 自动识别 → 保存范围** 的检验标准（含白灯板/齿孔进框导致画面灰暗的原理警示框）；④ 点击 **自动反相** 与固定机位 **批量应用** 下发范围；⑤ 使用 **印片灯光（Printer Lights）** 三通道对数微调与 `<kbd>Ctrl/⌘</kbd>+<kbd>Z</kbd>` 非破坏性撤销；⑥ 点击 **导出** 输出首张样片验证色彩后再批量导出整卷。
4. **`roll-vs-loose-workflows.md`（`id: roll-vs-loose-workflows`）—— 按卷物理标定工作流 vs 散张快速工作流**
   - 必含深度内容：两种日常工作模式的完整决策对比表；整卷一致性核心流程——在 **图库** 中使用 **标定片头片基（Calibrate Film）** 分别采样未曝光片基（锁定 `D-Min` 色罩基准）与全曝光片头（锁定 `D-Max` 化学密度上限），为什么被采样的标定帧会自动标记并跳过 Develop 与批量反相；**整卷批处理（Auto Invert Roll）** 如何扫描全卷最亮真实画面建立共享白点从而彻底消除同场景跳色；**120 中画幅或已剪除片头底片**的无片头回退机制（软件如何利用有界通道跨度响应平稳反相）；散张导入（`Loose Import`）与后续在 **历史胶卷记录** 中归档衔接的方法。

---

### 四、强制验收门槛（全部绿灯方可交付 Phase 3A）
1. `node scripts/compile-handbook.mjs` 零报错通过全部字数、结构、禁用词、双语对称与 HTML 标签平衡门禁，成功生成 `ui/documentation-data.js`。
2. 桌面端全套契约脚本回归（**一条都不能省**）：
   - `node --check ui/documentation-data.js && node --check ui/documentation.js`
   - `node scripts/verify-runtime-assets.cjs && node scripts/verify-i18n-contract.cjs && node scripts/verify-ui-startup-order.cjs && node scripts/verify-develop-detail-ui.cjs` 全部 100% 通过！
3. 官网构建与集成测试：`cd website && npm run verify:static && npm run test:browser` 全部通过（并在浏览器测试中新增对 `/zh/docs/`、`/en/docs/` 及内页路由渲染、右侧 TOC 存在性与顶栏中英切换精准跳转同章的断言）。
```

---

## Phase 3B：阅读器功能微瑕收口 + 板块 ②「完整功能与工作台手册」7 章双语深度精编与全站深链闭环

```markdown
你现在负责执行 **NexFilm 官网重构工程 — Phase 3B（Phase 3A 审计微瑕收口 + 板块 ② 工作台全功能手册 7 章×中英双语 = 14 篇旗舰级精编与全站文档深链闭环）**。

在撰写板块 ② 文档之前，请先用 5 分钟把 Phase 3A 功能审计查出的 5 处阅读器/样式细节缺陷全部修复，并清理临时迁移旧文件，然后再将主力精力投入板块 ②（`docs/handbook/{zh,en}/02-user-manual/`）全部 7 章共 14 篇双语手册的深度撰写。

---

### 零、Phase 3A 功能审计发现的 5 项必改微瑕（动笔前先清零！）
1. **修复右侧 TOC 三级标题（H3）锚点跳转失效与中文 Slug 截断**（`website/src/utils/docs.ts` & `scripts/compile-handbook.mjs`）：
   - **现状缺陷**：`docs.ts` 将 `### 三级标题` 提取进了 `toc`（`depth: 3`），但：① `marked` 默认渲染的 `<h3>` 没有 `id="..."` 属性，导致右侧 TOC 点击任何三级标题都无法定位，`IntersectionObserver` 的 `document.getElementById` 也返回 `null`；② 中文三级标题未写 `{#id}` 时走 `replace(/[^a-z0-9_-]+/g, '-')`，汉字被全部剥空，导致 Test 9 中 `/zh/docs/overview-and-install/` 的 `tocLinks` 出现畸形的 `'windows-'`、`'macos-'`（纯中文标题更会直接退化成 `'-'`）；③ 若三级标题写了 `{#custom-id}`，`marked` 会把字面量 `{#custom-id}` 原样渲染进正文。
   - **修复要求**：在 `docs.ts` 与 `compile-handbook.mjs` 中统一增强三级标题处理（自定义 `renderer.heading` 或预处理 `^###\s+`）：剥离末尾可选的 `{#custom-id}`，若未显式声明 `{#id}` 则使用支持 Unicode 字母数字的规则（如保留 `\p{L}\p{N}` 并确保章内去重非空，或强制要求所有 `###` 标题同样像 `##` 一样显式携带英文 `{#kebab-id}` 并在 `compile-handbook.mjs` 中校验对称性），且渲染出的 HTML 必须输出 `<h3 id="..." class="scroll-mt-24">...</h3>`。推荐直接规定：**所有 `###` 三级标题若使用，统一在末尾显式标注英文 `{#sub-id}`，编译器与 `docs.ts` 同步剥离 `{#sub-id}` 并写入 `<h3 id="sub-id" class="scroll-mt-24">`！**
2. **消除 `DocsLayout.astro` 同一章节重复 DOM `id`**（`website/src/layouts/DocsLayout.astro` 第 74–75 行）：
   - **现状缺陷**：`<section id={section.id} ...>` 与内部的 `<h2 id={section.id} ...>` 拥有完全相同的 `id`，违反 HTML 唯一 ID 规范。
   - **修复要求**：将外层 `<section>` 的 `id={section.id}` 改为 `data-section-id={section.id}`，仅在 `<h2 id={section.id} class="scroll-mt-24 group">` 上保留唯一锚点 `id`。
3. **修复 Astro `ClientRouter` 下的事件绑定与 Observer 泄漏**（`DocsSidebar.astro`、`DocsToc.astro`、`DocsLayout.astro`）：
   - `DocsSidebar.astro` 第 133–160 行：当前脚本未监听 `astro:page-load`，在启用 `ClientRouter` 跨页切换后移动端 `#docs-drawer-toggle` 引用旧 DOM 彻底失灵。请改为在 `document.addEventListener('astro:page-load', initDocsDrawer)` 中按最新 DOM 查询并用 `dataset.drawerBound === 'true'` 幂等绑定（同时支持按 `Esc` 或点击侧栏章节链接后自动收起抽屉）。
   - `DocsToc.astro` 第 47–90 行 & `DocsLayout.astro` 第 126–127 行：移除多余的顶层裸调用（启用 `ClientRouter` 时首次加载本就会触发 `astro:page-load`，同时写裸调用会导致首次执行两遍）；在 `DocsToc.astro` 模块顶层维护 `let activeObserver: IntersectionObserver | null = null`，每次 `setupTocHighlight()` 先调用 `activeObserver?.disconnect()` 释放旧观察器。
4. **修复英文侧边栏残留中文量词 `"篇"`**（`website/src/components/DocsSidebar.astro` 第 105 行）：
   - 将 `{group.chapters.length} 篇` 修正为按 `lang` 区分：`{lang === 'zh' ? `${group.chapters.length} 篇` : `${group.chapters.length}`}`。
5. **修复桌面端内置文档分组标题未定义的 CSS 变量**（`ui/style.css` 第 3295–3308 行）：
   - `.documentation-nav-group-header` 误用了不存在的 `var(--ui-muted)` 与 `var(--ui-line)`（`ui/style.css` 全局真实变量名为 `--muted` 与 `--line`）。请修正为 `color: var(--muted);` 与 `border-top: 1px solid var(--line);`。

> **⚠️ 重要文件清理提醒（严防新旧章节并存重复编译！）**：
> Phase 3A 在 `docs/handbook/{zh,en}/02-user-manual/` 下生成了 6 个临时迁移文件（`interface-basics.md`、`import-and-rolls.md`、`develop-workspace.md`、`export-and-contact-sheets.md`、`troubleshooting.md`、`appendix.md`）。
> 在本阶段完成板块 ② 全新 7 章精编时，**必须执行 `rm` 彻底删除被替代的 5 个旧文件名**（即中英目录下的 `interface-basics.md`、`import-and-rolls.md`、`develop-workspace.md`、`troubleshooting.md`、`appendix.md`，而 `export-and-contact-sheets.md` 原地覆写升级），确保 `docs/handbook/{zh,en}/02-user-manual/` 目录下**严格只保留规约定义的 7 个章节文件（中英共 14 个文件）**，并通过新章节 Frontmatter 的 `aliases` 无损承接所有旧路由与桌面端别名跳转！

---

### 一、写作前必查源码事实源（严禁凭记忆猜参数！）
动笔写对应章节前，必须逐一核对以下源码文件以确保每一个滑块名称、数值范围、下拉选项与交互细节 100% 真实准确：
1. `ui/documentation-data.js` 原有第 2–6 章与第 9 章附录全部细节（确保旧版所有知识点 100% 被吸收并深化，零遗漏！）
2. `ui/index.html` + `ui/i18n.js`（核对全部折叠面板标题、按钮文案、画幅选项：135、135 Half-frame、APS、120 645/6x6/6x7/6x9/6x12/6x17、4x5/5x7/8x10）
3. `ui/settings-copy.js`（核对复制设置的快捷预设与全部可勾选子项：`filmMode`, `filmBase`, `densityLimits`, `printerRed/Green/Blue`, `temperature`, `tint`, `exposure`, `gamma`, `contrast`, `highlights`, `shadows`, `saturation`, `curve`, `sharpen`, `texture`, `clarity`, `dehaze`, `lut`, `lutOpacity`, `sprocketPoint/Tolerance/Feather`, `workingSpace`, `crop`, `rotateFlip`, `perspective`）
4. `ui/export-settings.js` 与 `ui/contact-sheet.js`（核对导出格式、7 大色彩空间、长边缩放 `256–32768 px`、禁止放大小图默认策略、6 个命名令牌 `{Roll}/{Camera}/{Film}/{Date}/{Original}/{Seq}`、重名冲突策略、EXIF 写入，以及 135 齿孔条形码与 120 片边字码接触印样绘制逻辑）
5. `assets/luts/`（核对真实内置的 14 个 `.cube` 文件精确名称）

---

### 二、硬性质量与程序门禁要求
1. **延续 Phase 3A 全部文风红线与事实铁律**：零感叹号、零营销吹捧词与 AI 套话、零虚构快捷键、零虚构富士 LUT。
2. **在 `scripts/compile-handbook.mjs` 中将 `02-user-manual/*` 全部 7 章纳入精编字数与结构硬门禁**：
   - 常规章节每篇中文纯可见字符数 **≥ 1,800 字** / 英文 **≥ 950 words**；
   - 三大核心重磅章节（`density-grading-and-curves.md`、`export-and-contact-sheets.md`、`troubleshooting-and-appendix.md`）每篇中文纯可见字符数 **≥ 2,400 字** / 英文 **≥ 1,250 words**；
   - 每章 **4～7 个 `## 二级小节 {#section-id}`**，至少 **1 张 Markdown 表格** + **1 个 GitHub Alert 提示框**。
3. **桌面端契约死守**：
   - `density-grading-and-curves.md` 中**必须包含一个锚点 ID 精确为 `{#curve-and-detail}` 的二级小节**（满足 `scripts/verify-develop-detail-ui.cjs` 强断言！）；
   - `aliases` 兼容映射完整覆盖旧版章节 ID（`'interface-basics' → 'workspace-and-views'`、`'import-and-rolls' → 'import-formats-and-library'`、`'develop-workspace' → 'density-grading-and-curves'`、`'troubleshooting' → 'troubleshooting-and-appendix'`、`'appendix' → 'troubleshooting-and-appendix'`）。

---

### 三、板块 ② 全部 7 章（14 篇双语文档）逐章详尽内容规格

1. **`workspace-and-views.md`（`id: workspace-and-views`, `aliases: ["interface-basics"]`）—— 界面总览、五大视图分工与交互准则**
   - 详解菜单栏（设置切换中英与深浅色主题、视图跳转、帮助关于）、顶栏五大核心视图（**图库 / 工作台 / 历史胶卷记录 / 硬件校正 / 使用文档**）及右侧全局后台异步操作（**导出胶卷 / 导入胶卷 / 导出**）；
   - 详解底片条（横向滚轮、方向键切帧、实时同步调色缩略图、标定帧角标）；
   - 详解右侧检查器 10 大分组顺序、左侧圆点模块导航条、分组折叠状态持久化、滑块“先点击激活再用滚轮微调”防误触机制、右侧数值点击直输（`Enter` 确认 / `Esc` 取消）、画布空格平移与指针中心缩放、非破坏性撤销/重做栈；
   - 附核心暗房与软件术语对照全表（Roll、Frame、Film Area、Film Base、Film Leader、Optical Density、`D-Min / D-Max`、Printer Lights、Input Domain）。

2. **`import-formats-and-library.md`（`id: import-formats-and-library`, `aliases: ["import-and-rolls"]`）—— 格式支持矩阵、三态导入与胶卷档案库管理**
   - 完整输入格式支持矩阵表：相机 RAW（DNG, NEF/NRW, CR2/CR3, ARW/SRF/SR2, RAF, RW2, ORF/ORI, SRW, PEF, 3FR, ERF, KDC/DCR, IIQ, MOS, MRW, X3F, RWL, RAW）、哈苏 Imacon/Flextight `.fff`（说明 v1.1.0 不再依赖 FlexColor 设备字符串，杜绝误判为相机 RAW）、按行流式百兆/GB 级扫描仪 TIFF、以及 8/16-bit PNG/JPEG（客观说明为何 8-bit JPEG 因缺乏线性动态与片基难以保证自动校色精度）；明确警示框说明 Nikon Z8 `HE/HE*` NEF 限制与转 DNG 方案；
   - 详解导入阶段“仅提取内嵌预览建立索引、首次进入工作台才按需解算有界代理”的高速低内存设计；
   - 详解三态导入（按卷导入填写画幅/相机/胶卷型号/日期/胶卷备注、散张导入、继续编辑胶卷追加画面）；
   - 详解 **历史胶卷记录（Rolls）** 视图：左侧多维筛选器（全画幅谱系 135~8×10、机身、日期）、查看归档详情、编辑胶卷信息与备注（Roll Notes）、**提升至图库（Promote to Library）** 继续精修、删除记录（不删原片）及源文件移动后的 **定位文件（Locate File）** 断链修复流程。

3. **`film-area-and-geometry.md`（`id: film-area-and-geometry`）—— 胶片范围（Film Area）、几何变换与齿孔净边**
   - 深度阐明 **胶片范围（Film Area）** 与常规 **裁剪（Crop）** 的本质职责区别（前者界定参与片基/密度统计的有效感光区域以排除外部强光干扰，后者决定最终成片构图，二者共享同一画面坐标系）；
   - 图解/表格剖析常见框选错误后果（圈入 100% 透光观片灯板导致画面整体灰暗发沉、圈入片夹死黑边框导致暗部抬升发雾、135 齿孔边缘漏光干扰）；
   - 详解 **自动识别（Auto Area）**、四角与边缘手柄微调、**保存范围（Save Area）** 以及固定翻拍台机位下使用 **批量应用（Batch Apply）** 一键下发同卷范围；
   - 详解正交 90° 旋转、水平/垂直翻转、角度拉直（Straighten）、水平/垂直透视校正、纵横比与微距镜头桶形/枕形畸变补偿，以及检查器 **齿孔设置（Sprocket）** 面板中的齿孔采样（Sample Sprocket Hole）、容差与羽化控制。

4. **`density-grading-and-curves.md`（`id: density-grading-and-curves`, `aliases: ["develop-workspace"]`）—— 密度窗口、暗房印片光、曲线与细节精修**
   - 详解 **颜色 / 黑白（Color / B&W）** 模式切换、**自动反相（Auto Invert）**、**标定片头片基** 与 **整卷批处理** 的协作关系；
   - 详解 **密度范围（`D-Min / D-Max`）** 主滑块与三通道读数的物理含义（调整黑场起点、白场上限与反差张力）；
   - 详解 **印片灯光（Printer Lights）** 红/绿/蓝三通道对数互补调色法则（附偏色诊断与印片光加减方向速查表：如画面偏洋红、偏青、偏黄、偏绿分别该如何推拉 R/G/B）及中性灰校准思路；
   - **必含 `{#curve-and-detail}` 小节**：详解四通道控制点曲线（RGB 主曲线 + R/G/B 独立通道曲线，直方图背景衬底，加点、拖点、端点拉动与删点）、**观感调整**（曝光、Gamma、对比度、高光、阴影、饱和度、色温、色调）与 **细节（Detail）** 面板（锐化的数量/半径/细节/蒙版，纹理 Texture、清晰度 Clarity、去朦胧 Dehaze；说明半径按长边 `2560 px` 统一换算保证预览与导出一致，以及按住 `<kbd>Alt</kbd>` 拖动锐化参数实时预览边缘信号与蒙版灰度视图）；
   - 详解右上角 **直方图（Histogram）** 与 **波形图（Waveform）** 切换、高光暗部溢出 `!` 预警开关，以及 **复制设置 / 粘贴设置** 的快捷预设与细粒度参数勾选策略（对比 **批量应用** 与 **复制设置** 的使用边界）。

5. **`print-film-emulation.md`（`id: print-film-emulation`）—— 相纸与电影印片模拟（3D LUT）及银盐黑白工作流**
   - 阐述暗房“底片反相（还原场景密度） + 相纸/印片特性映射（赋予成片审美滚降）”的两段式物理逻辑；
   - 完整列出并逐类解析软件内置的全部 **14 款 `.cube` LUT**（提供完整特性与适用题材对照表）：
     - 2 款工业电影印片：`Kodak 2383`、`Kodak 2393`（配合 Vision3 500T/250D 等电影卷的高光压缩与浓郁暗部）；
     - 5 款经典彩色银盐相纸：`Kodak Portra Endura`（柔和肤色与低反差人像）、`Kodak Endura Premier`、`Kodak Supra Endura`、`Kodak Ultra Endura`（高饱和风光商业）、`Kodak Ektacolor Edge`；
     - 7 款黑白可变反差银盐相纸：`Ilford MGFB 00`、`0`、`1`（软调高宽容度）、`2`（标准中性号数）、`3`、`4`、`5`（硬调强烈银盐反差）；
   - 详解 **强度（Opacity 0–100%）** 混合控制、加载外部第三方标准 `.cube` 3D LUT 文件的规范，以及黑白胶卷从单通道光学密度反相到搭配 Ilford MGFB 号数相纸的标准暗房流程。

6. **`export-and-contact-sheets.md`（`id: export-and-contact-sheets`）—— 母带级导出、色彩空间管理与接触印样（Contact Sheet）**
   - 建立完整的 **6 大导出格式决策对比表**（位深、编码线性度、是否携带调色结果、适用下游软件与体积权衡）：常规展示与精修交付（`8-bit JPEG`、`16-bit PNG`、`8-bit TIFF`、`16-bit TIFF`） vs 影视/专业后期母带交付（`16-bit Linear TIFF`、`Linear DNG`） vs 原始扫描传感器归档（`Camera RAW DNG`——重点强调仅支持相机 RAW 源片且原样直封不含调色）；
   - 建立 **7 大输出色彩空间 ICC 选择指南表**（`sRGB` 网页与手机分享、`Display P3` 苹果广色域屏、`Adobe RGB (1998)` 艺术微喷印刷、`Rec.2020`、`ProPhoto RGB` 超宽色域 16-bit 归档、`ACEScg` 与 `ACES2065-1` 达芬奇影视管线，警示 8-bit JPEG 切勿乱选 ProPhoto RGB 以防色彩断层）；
   - 详解尺寸缩放策略（原始尺寸 vs 指定长边 `256–32768 px`、默认不放大小图开关、输出锐化等级）、令牌化批量重命名（`{Roll}`, `{Camera}`, `{Film}`, `{Date}`, `{Original}`, `{Seq}` 组合示例与非法字符自动清洗、重名冲突策略）及可选 EXIF 胶卷/相机/日期元数据写入；
   - 详解在 **历史胶卷记录 → 胶卷内容** 中一键 **导出接触印样（Export Contact Sheet）** 的画幅自适应机制（135 拟真齿孔、帧号与条形码 vs 120 片边乳剂字码、底部卷信息抬头，以及未显影画面的 `NOT DEVELOPED` 提示处理）。

7. **`troubleshooting-and-appendix.md`（`id: troubleshooting-and-appendix`, `aliases: ["troubleshooting", "appendix"]`）—— 实战故障排查树、全局快捷键、数据备份与开源附录**
   - 系统化「症状 → 根因诊断 → 逐步解决路径」排障手册：
     1. 反相后整张画面发灰、发暗、反差极低（灯板白边/齿孔进框 vs 曝光严重不足）；
     2. 画面出现顽固偏色或阴影彩斑（片基采样点误选到曝光区、扫描仪已做过非线性色阶裁剪、背光显指不足）；
     3. 整卷批量反相后部分帧跳色（标定帧混入、混扫不同光源或未统一胶片范围）；
     4. RAW 或 `.fff` 导入报错 / Nikon Z8 HE NEF 无法解码 / 超大 JPEG 内存受限；
     5. 工作台预览与最终全尺寸导出存在细微锐度或颗粒差异的原因（2560 有界代理 vs 全分辨率流式渲染）；
     6. 从 `v1.0.2` 升级至 `v1.1.0` 后底片条缩略图被清空或不一致（执行一次 **自动反相** 重建新配方缩略图）；
   - 完整且零编造的 **全局快捷键与鼠标手势速查全表**（严格锁死真实 9 项）；
   - 跨平台数据目录路径、本地备份与迁移指南（同时备份原片目录 + `nexfilm_user.db`）、完整 **已知限制（Known Limitations）** 诚实披露清单、**GNU GPL v3.0 only** 许可声明、问题反馈模板要素与理论致谢（`@黄昊Haosky`、`@V777`）。

---

### 四、全站文档深链更新与强制验收门槛
1. 更新全站所有指向文档的入口（`Header.astro`、`Footer.astro`、`{zh,en}/index.astro`、`{zh,en}/features.astro`、`{zh,en}/faq.astro`），使相关功能介绍精准深链至新建的对应独立文档章节路由，零死链。
2. 运行 `node scripts/compile-handbook.mjs`，确认板块 ①（4 章）+ 板块 ②（7 章）+ 板块 ③（2 章存量基线）全部通过编译门禁，生成最新 `ui/documentation-data.js`。
3. 桌面端全套契约测试 100% 绿灯：
   - `node --check ui/documentation-data.js && node --check ui/documentation.js`
   - `node scripts/verify-runtime-assets.cjs && node scripts/verify-i18n-contract.cjs && node scripts/verify-ui-startup-order.cjs && node scripts/verify-develop-detail-ui.cjs && node scripts/verify-contact-sheet.cjs && node scripts/verify-density-contract.cjs`
4. 官网构建与集成测试 100% 绿灯：`cd website && npm run verify:static && npm run test:browser`（并在 `test-browser-integration.cjs` 的 Test 9 中补强断言：① 右侧 TOC 每一个 `[data-toc-link]` 指向的 `id` 在正文 DOM 中均真实存在且全页 `document.querySelectorAll('#' + CSS.escape(id)).length === 1` 零重复 ID；② 英文侧边栏不含残留中文字符 `"篇"`；③ 同步后的首页内嵌沙箱 `使用文档 / Documentation` 视图能正常加载并浏览全部 11 篇桌面同步章节）。
```

---

## Phase 4：色彩科学白皮书（板块 ③）、架构贡献指南（板块 ④）、Pagefind 搜索与全链路终验

```markdown
你现在负责执行 **NexFilm 官网重构工程 — Phase 4（收官阶段：板块 ③ 色彩科学白皮书、板块 ④ 架构与开源贡献指南、Pagefind 全局搜索与全链路自动化终验）**。

### 零、界面细节顺手清零（在 `DocsLayout.astro`、`DocsSidebar.astro` 与 `docs/index.astro` 中同步完成）
1. **`DocsLayout.astro` 英文 `<title>` 与默认 `description` 本地化**（第 20–21 行）：
   - 当前硬编码了 `` `${chapter.title} · ${chapter.trackTitle} · 文档` `` 与中文默认描述，导致 `/en/docs/...` 浏览器标签页尾部残留中文 `· 文档`。请按 `lang === 'zh'` 严格区分中英文后缀与默认描述。
2. **`DocsSidebar.astro` 全局 `keydown` 幂等守卫**（第 163–165 行）：
   - `document.addEventListener('keydown', ...)` 随每次 `astro:page-load` 重复挂载在 `document` 上，请使用模块级布尔标记或 `AbortController` 确保全局只注册一次 `Escape` 监听。
3. **文档门户页 `website/src/pages/{zh,en}/docs/index.astro` 四大板块卡片闭环**：
   - 将板块 ③（`color-science`）卡片的 `targetSlug` 更新为 `'three-mathematical-domains'`，徽标更新为 `'4 篇进阶白皮书'` / `'4 Advanced Whitepapers'`。
   - 将板块 ④（`architecture`）卡片从外部 GitHub 链接切换为站内直链 `targetSlug: 'system-architecture'`（移除 `externalUrl` 兜底），徽标更新为 `'3 篇架构与贡献指南'` / `'3 Architecture Guides'`，使四大受众体系卡片 100% 闭环进入站内三栏阅读器。

---

### 一、必读一手工程与算法事实源（动笔前必须完整核查，严禁凭空编造算法或源码细节）
1. `docs/website-plan.md`（第 6.2–7 章）
2. `data_process_pipeline_doc.md`（三大数学域边界、`Smart Auto` / `Capture Corrected` / `Capture Characterized` 定义、已实现与未实现能力的诚实技术边界）
3. `docs/unified-density-pipeline.md`（`resolve_input_domain` 五级优先级、片基采样三级优先级、有界通道跨度响应 `1.15×` 死区与 `1.35×`/`3.0×` 上限、量化回归结果）
4. 核心源码（**只读核查真实结构，严禁修改 `src/` 或 `Cargo.toml` 任何一行代码**）：`src/app_state.rs`、`src/color_science.rs`、`src/pipeline.rs`、`src/calibration_fit.rs`、`src/raw_backend.rs`、`src/dng_writer.rs`、`src/db.rs`、`.github/workflows/release.yml`

---

### 二、本阶段核心交付任务

#### 任务 1：清理临时迁移旧文件，精编板块 ③「色彩科学与光学密度白皮书」（`03-color-science` · `inApp: true` 双端同步 · 中英双语共 8 篇）
1. **旧文件清理与路由向后兼容**：
   - 彻底删除 Phase 3A 临时迁移生成的 2 个旧文件名：`docs/handbook/{zh,en}/03-color-science/color-science.md` 与 `hardware-calibration.md`，确保 `03-color-science/` 下中英各自严格只保留以下 **4 个规范章节文件**。
   - 在 `three-mathematical-domains.md` 的 Frontmatter 中声明 `aliases: ["color-science"]`，在 `hardware-calibration-guide.md` 中声明 `aliases: ["hardware-calibration"]`，确保全站既有深链（如 `features.astro`）与桌面端旧引用零死链平滑兼容。
2. **4 章白皮书核心规格与硬核内容要求**（必须忠实反映 NexFilm v1.1.0 真实实现，明确区分 v1.1 已落地能力与后续路线图，绝不夸大未实现的物理 Status M 滤镜积分或胶片 H-D 特性曲线反演）：
   - **`three-mathematical-domains.md`（负片反相的三大数学域）**：
     - 严谨推导透射率域（`log` 前：$T = I_{\text{transmitted}} / I_{\text{open}}$）、光学密度域（`log` 后：$D_{\text{raw}} = -\log_{10}(\max(T, \epsilon)),\; D_{\text{net}} = D_{\text{raw}} - D_{\text{base}}$）与正片工作/输出域（Linear ProPhoto RGB → 影调/相纸 LUT/ICC）。
     - 用完整数学证明与**内联暗房风 SVG 信号流图**阐明核心铁律 $-\log_{10}(Ax) \neq A[-\log_{10}(x)]$：为什么相机/灯板分离矩阵必须严格作用在透射率域，而绝不能挪到密度域，也绝不能拿普通显示器色彩空间矩阵冒充密度校准。
   - **`input-domain-and-unified-pipeline.md`（五级输入域解析与统一密度管线）**：
     - 详解“输入格式只决定输入域，密度数学全局唯一”的统一架构：`resolve_input_domain` 的 5 级判定链（内嵌 ICC → FFF/扫描仪容器记录 → RAW/LinearRaw 相机元数据 → 扫描仪机型默认 → sRGB 兜底及 `estimated` 置信标记）。
     - 详解片基采样三级优先级（片边橙色色罩窄带 → Film Area 内低密度尾部 → 最亮分位降权及饱和度/均匀度门限）与**有界通道跨度响应（Bounded Channel Span）**：共享窗口原点保证片基中性、死区 `1.15×` 保护正常底片零干扰、常规上限 `1.35×` 与上游截断通道恢复上限 `3.0×` 如何拯救宽色域拼接 TIFF 塌陷，以及为何坚决摒弃破坏真实影调的灰世界均值拉齐。
   - **`hardware-calibration-guide.md`（硬件校准原理与实验性工作流指南）**：
     - 对比三种工作模式：`Smart Auto (ProPhoto Estimate)`、`Capture Corrected`（利用 `dark` 暗场帧扣除传感器热噪/杂散光、利用 `open gate` 平场帧消除背光板边缘照度衰减与色温漂移）与 `Capture Characterized`（透射色卡采样拟合 `CaptureSeparation3x3` 及其岭回归正则化与病态矩阵自动回退机制）。
     - 提供实拍采集 Dark Frame / Open Gate / 透射标定靶的标准暗房操作规程（SOP）与当前实验性边界诚实披露。
   - **`cinema-and-aces-workflow.md`（16-bit 线性交付与 DaVinci Resolve / ACEScg 专业后期衔接）**：
     - 对比展示级成品导出与三种母带级导出（16-bit Linear TIFF、Linear DNG、Raw Mosaic DNG）在数据管线中的精确截断节点。
     - 给出将 NexFilm 导出的 16-bit Linear ProPhoto RGB 母带导入 DaVinci Resolve（Color Managed / ACEScct / ACEScg 节点树）进行无损二级院线调色的标准配置表与逐步操作指南。

#### 任务 2：精编板块 ④「架构设计与开源贡献指南」（`04-architecture` · `inApp: false` 官网专属 · 中英双语共 6 篇）
面向希望审计光学算法、本地编译或参与贡献的开发者（Frontmatter 严格设置 `inApp: false`，确保不打入桌面端离线包）：
1. **`system-architecture.md`（系统总体架构与前后端渲染等价契约）**：
   - 剖析 Tauri 2 宿主 + Rust 多线程图像处理后端 + 原生 HTML/CSS/WebGL2 前端 + SQLite 本地持久化的协作模型（含**内联暗房风 SVG 系统架构图**）。
   - 详解异步两阶段性能架构（导入期仅提取嵌入式缩略图实现秒级入库，切入 Develop 按需解码并缓存 16-bit 线性代理）以及 WebGL2 实时着色器与 Rust CPU 全分辨率按行流式导出器的 **WYSIWYG 数学等价契约**。
2. **`core-modules-walkthrough.md`（核心源码模块导览）**：
   - 基于真实源码逐模块导读 `src/`（如 `pipeline.rs`、`color_science.rs`、`raw_backend.rs`、`calibration_fit.rs`、`dng_writer.rs`、`app_state.rs`）与 `ui/` 关键模块职责、流式 TIFF/FFF 分行解码器如何以低内存峰值吞吐数百兆扫描件，以及 SQLite 工程数据库迁移与旧版配方平滑升级机制。
3. **`building-and-contributing.md`（跨平台源码构建、自动化契约测试与贡献指南）**：
   - 完整整理 macOS（Homebrew `pkg-config` + `libraw` 0.22+）、Windows（MSVC Build Tools + Rust）与 Linux（`webkit2gtk-4.1` + `libraw`）从零克隆到 `cargo tauri dev` / `cargo tauri build` 的可复现命令与排错表。
   - 逐项解读 `.github/workflows/release.yml` 与 `scripts/verify-*.cjs` 自动化契约防线，说明 Issue 报告规范与 GPL-3.0 开源贡献准则。

> **写作与内联 SVG 强制规范（扩展 `scripts/compile-handbook.mjs` Gate 2 覆盖全部 4 大板块）**：
> - 将 `scripts/compile-handbook.mjs` 的 Gate 2 扩充至覆盖 `color-science`（4 章：每篇中文纯正文 `>= 2,200` 字、英文 `>= 1,100` 词）与 `architecture`（3 章：每篇中文纯正文 `>= 1,800` 字、英文 `>= 950` 词），同样强制每篇包含 `4–7` 个 `## {#id}` 二级小节、至少 1 张 Markdown 表格与至少 1 个 GitHub Alert，且通过 Gate 3（零感叹号、零浮夸营销词）与 Gate 4（HTML 标签平衡）。
> - **内联 SVG 铁律**：在 Markdown 中嵌入 `<svg ...>...</svg>` 图表时，**`<svg>` 内部严禁出现空行**（防止 CommonMark HTML 块解析提前闭合而混入 `<p>` 标签），并在 `website/src/styles/docs-prose.css` 与桌面端 `ui/style.css`（`.documentation-section-body svg`）中确保 `max-width: 100%; height: auto;` 自适应缩放且不破坏 Gate 4 标签平衡统计。

#### 任务 3：接入 Pagefind 毫秒级全文搜索（`⌘K` / `Ctrl+K` · 零别名重复索引 · 子路径兼容）
1. **构建流水线集成**：
   - 在 `website/` 中安装 `pagefind` 至 `devDependencies`（离线可复现构建，不依赖联网临时拉取 `npx`），将 `website/package.json` 的 `build` 脚本更新为 `"astro build && pagefind --site dist"`。
2. **精准索引与零别名重复（关键架构细节）**：
   - 在 `website/src/pages/{zh,en}/docs/[...slug].astro` 中向 `DocsLayout.astro` 传入 `isAlias: boolean`（当路由为 `aliases` 别名路径时 `isAlias = true`）。
   - 在 `DocsLayout.astro` 中：仅当 `!isAlias`（规范主路由）时在正文 `<article>` 上标记 `data-pagefind-body` 与 `data-pagefind-filter={`lang:${lang}`}`；当 `isAlias === true` 时**不标记** `data-pagefind-body`（并在 `<head>` 输出指向规范 URL 的 `<link rel="canonical" ... />`），彻底杜绝同一章节因 `aliases` 在搜索结果中重复出现 2–3 次！
3. **搜索交互模态框 `DocsSearchModal.astro`**：
   - 挂载于文档布局与文档门户页（激活 `DocsSidebar.astro` 中已预留的 `#doc-search-placeholder-btn`、移动端文档导航栏搜索按钮，并支持全局快捷键 `⌘K` / `Ctrl+K` 呼出、`Esc` 或点击遮罩关闭）。
   - 客户端通过 `withBase('/pagefind/pagefind.js')` 动态加载 Pagefind 检索引擎（完美兼容根路径与子路径部署），默认按当前页面 `lang`（`zh` 或 `en`）过滤检索结果，实时展示匹配章节标题、小节锚点与高亮摘要（`<mark>`），支持键盘上下方向键选定与 `Enter` 秒级跳转至对应 `#anchor`。

#### 任务 4：确定性编译与全链路自动化验收脚本
1. **消除编译器时间戳抖动**：
   - 移除 `scripts/compile-handbook.mjs` 生成头部中的动态时间戳（`new Date().toISOString()`），使 `ui/documentation-data.js` 的编译输出 100% 确定性幂等（包含板块 ① 4 章 + 板块 ② 7 章 + 板块 ③ 4 章 = **15 篇双语同步章节**；板块 ④ 3 章因 `inApp: false` 严格排除在桌面端包外）。
2. **新增双端同源校验脚本 `scripts/verify-handbook-sync.cjs`**：
   - 校验 `docs/handbook/zh` 与 `docs/handbook/en` 各自精确包含 **18 篇规范 Markdown 文件**（`4 + 7 + 4 + 3`），零残留旧迁移文件，且文件名、`id`、`track`、`inApp`、二级小节 `id` 序列 100% 对称；
   - 校验重新编译输出与磁盘上的 `ui/documentation-data.js` **字节级完全一致**（防止未来修改 Markdown 漏跑编译或手改生成产物）。
3. **新增官网构建产物全量体检脚本 `website/scripts/verify-site.mjs`**：
   - 扫描 `website/dist/` 下生成的全部 HTML 文件，程序化断言：
     1. **全站内部链接与 `#anchor` 片段零死链**（所有站内 `<a href="...">` 指向的路由目录/文件及 `#id` 在目标 HTML DOM 中真实存在）；
     2. **`/zh/` 与 `/en/` 生成路由 100% 对称**；
     3. **零裸露未渲染 LaTeX 公式源码**（无残留 `$$`、`\frac`、`\log_{10}` 纯文本泄漏）；
     4. **零遗留错误资产包名或镜像**（无 `NexFilm_1.1.0_`、无 `ghproxy`）；
     5. **Pagefind 索引产物完备**（`website/dist/pagefind/pagefind.js` 存在且非空，且别名路由未被重复建入索引）。
4. **升级全真浏览器集成测试 `website/scripts/test-browser-integration.cjs`**：
   - 更新 Test 7 断言首页内嵌沙箱 `Documentation` 视图准确加载 **15 篇**同步章节（不含 `04-architecture`）；
   - 新增 **Test 10（Pagefind 全文搜索端到端测试）**：在 `/zh/docs/` 与 `/en/docs/` 实测点击 `#doc-search-placeholder-btn` 及按下 `Ctrl+K`/`⌘K` 呼出搜索框，输入中英文关键词（如 `"印片光"` / `"Printer Lights"` / `"LibRaw"`），断言毫秒级返回当前语言匹配条目、零别名重复项，且点击结果能精准跳转至目标章节与小节锚点。

---

### 三、最终收官验收门槛（全部 100% 绿灯方可交付）
1. **严守边界**：`git status` 确认 `src/` 与 `Cargo.toml` 零改动；`docs/handbook/{zh,en}/` 严格为 `18 × 2 = 36` 篇规范文件。
2. **SSOT 编译与同源一致性门禁**：
   - `node scripts/compile-handbook.mjs && node scripts/verify-handbook-sync.cjs` 100% 通过。
3. **桌面端全套契约回归零破坏**：
   - `node --check ui/documentation-data.js && node --check ui/documentation.js`
   - `node scripts/verify-runtime-assets.cjs && node scripts/verify-i18n-contract.cjs && node scripts/verify-ui-startup-order.cjs && node scripts/verify-develop-detail-ui.cjs && node scripts/verify-contact-sheet.cjs && node scripts/verify-density-contract.cjs` 全部通过。
4. **官网静态检查、构建体检与 10 项全真浏览器集成测试 100% 绿灯**：
   - `cd website && npm run verify:static && npm run test:browser && node scripts/verify-site.mjs` 全部零警告、零报错通过。
```
