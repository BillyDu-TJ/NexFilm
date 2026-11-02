# NexFilm 官网 — 需求/设计文档（修订版）

> 状态：选型已确认 · 设计细化中
> 初版：2026-10-18 · 修订：2026-10-18

---

## 1. 背景与目标

为 NexFilm Engine 建设官方网站，承担以下职责：

1. **产品展示** — 让没听过的人 30 秒内明白"这是干什么的、凭什么用它"；通过处理前后对比与样片画廊提供视觉说服力
2. **下载与更新** — 各平台安装包入口 + 版本历史 + 历史版本回退 + 新版本感知
3. **反馈** — bug 报告、功能建议、留言
4. **文档** — 从应用内 9 章文档迁移为在线版，补充 FAQ 与常见问题
5. **资源聚合** — 开源信息、赞赏入口、贡献指南

硬约束：**长期运行成本趋近于零；学生认证到期后网站完全不受影响。**

## 2. 可行性结论

**完全可行，且核心链路不依赖学生认证。**

| 环节 | 方案 | 成本 |
|---|---|---|
| 站点托管 | 静态站 + Cloudflare Pages（或 GitHub Pages） | 永久免费（公开仓库） |
| 安装包分发 | 直接链接 GitHub Releases（`/releases/latest`） | 永久免费，无带宽顾虑 |
| 反馈 | GitHub Issues + Discussions（giscus 组件） | 永久免费，零后端 |
| 域名 | 学生包免费一年 | 一年后约 $10–20/年，或退回免费二级域名 |
| 匿名统计 | GoatCounter（免费开源档） | 永久免费 |

仓库现状对建站非常有利：

- `BillyDu-TJ/NexFilm` 为公开仓库，已有 v1.0.0 → v1.1.0 共 4 个 Release，每个含 Windows x64 NSIS + macOS Apple Silicon / Intel DMG
- README 中英双语；`ui/documentation-data.js` 内置 9 章中英文档（quick-start、interface-basics、import-and-rolls、develop-workspace、export-and-contact-sheets、film-emulation、troubleshooting、color-science、hardware-calibration + appendix），可直接迁移为在线文档
- 现成视觉素材：`ui/assets/design-reference/nexfilm-logo.svg`（明/暗两版）、`docs/assets/nexfilm-v1.0-develop.png` 截图、赞赏码两张、导入界面参考图三张
- 无 Tauri updater（未配置 pubkey / endpoint），"更新"即版本页 + 最新版号展示；未来若启用 updater，站点再挂 `latest.json` 即可

### 学生包（2026-10 现状）能提供什么

| 资源 | 内容 | 对本项目的用处 | 注意 |
|---|---|---|---|
| Name.com | 免费域名一年，25+ 后缀（`.app` `.dev` `.software` `.live`…） | 官网独立域名 | 仅一年；`.app`/`.dev` 强制 HTTPS（各家托管免费支持，无额外成本） |
| Namecheap | 免费 `.me` 域名一年 + 1 张 SSL 证书 | 备选域名 | 托管商本就送 HTTPS，证书用处不大 |
| Azure | $100 额度 | 可选：自建反馈后端 / 统计 | 需信用卡注册、需维护，不推荐首选 |
| GitHub Pro | 认证期间免费 | 仓库已公开，基本用不上 | 毕业失效 |
| DigitalOcean | ~~$200 额度~~ | **已退出学生包**（额度 2026-07-31 全部过期） | 不要基于它规划 |

## 3. 选型决策（已确认）

| 决策点 | 默认选型 | 理由 | 备选 |
|---|---|---|---|
| 受众 | 大陆 + 海外并重，中英双语站点 | README 与应用本身均双语；作者受众两地都有 | 单语可砍一半工作量 |
| 托管 | **Cloudflare Pages** | 自定义域名在大陆可达性通常优于 github.io；免费额度极大（无限带宽） | GitHub Pages（更简单，纯海外受众时选它） |
| 域名 | **学生包 Name.com 免费一年**，建议 `nexfilm.app`（备选 `nexfilm.dev` / `nexfilm.studio` / `getnexfilm.app`） | 免费、后缀贴开发者产品 | Namecheap `.me`；或先不绑域名直接用 `*.pages.dev` |
| 技术栈 | **Astro + Tailwind CSS** | 营销页 + 内容页 + 双语路由一体；产物纯静态、构建快、零 JS 默认 | VitePress（偏文档站）；纯 HTML（多页双语难维护） |
| 反馈 | **Issues（模板化）+ giscus 评论 + 无后端表单** | Issues/giscus 零后端零维护；表单（Formspree/Tally 免费档，提交转邮箱）照顾不用 GitHub 的用户 | 只保留 Issues + giscus（若不想维护表单账号） |
| 代码位置 | **独立仓库 `BillyDu-TJ/nexfilm-web`** | 主仓库保持干净；Cloudflare Pages 直连该仓库 push 即部署 | 本仓库 `website/` 目录 + Actions 部署 |

## 4. 视觉设计规范

> 独立出此章节，确保开发时有统一的视觉锚点。

### 4.1 配色

| 用途 | 深色模式 | 浅色模式（预留） |
|---|---|---|
| 背景 | `#0F1117`（近黑） | `#F8F9FB` |
| 表面/卡片 | `#1A1D27` | `#FFFFFF` |
| 主色 | `#0A5AA8`（取自 logo SVG fill） | `#0A5AA8` |
| 主色亮版 | `#3B8EE0` | `#0A5AA8` |
| 强调 | `#F59E0B`（琥珀，下载按钮高亮） | `#D97706` |
| 正文 | `#E2E8F0` | `#1E293B` |
| 次文 | `#94A3B8` | `#64748B` |
| 边框 | `#2D3348` | `#E2E8F0` |

### 4.2 字体

- 标题：`Inter`（西文）+ `Noto Sans SC`（中文），font-weight 600/700
- 正文：同字体族，font-weight 400
- 代码/文件名：`JetBrains Mono` 或系统等宽
- 加载策略：`font-display: swap`，通过 Google Fonts 或自托管 woff2 子集

### 4.3 响应式断点

| 名称 | 宽度 | 布局 |
|---|---|---|
| mobile | < 640px | 单列，汉堡菜单 |
| tablet | 640–1024px | 双列卡片 |
| desktop | 1024–1440px | 标准三列/内容+侧栏 |
| wide | > 1440px | 内容居中 max-width 1280px |

### 4.4 关键视觉元素

- **深色为主**，与 NexFilm Develop 工作台的气质一致
- 右上角预留浅色/深色切换按钮（初版可仅实现深色）
- Logo 使用现有 `nexfilm-logo.svg`（深色背景）/ `nexfilm-logo-dark.svg`（浅色背景）
- 页面过渡：Astro View Transitions（同站导航不刷新）
- 图片加载：skeleton placeholder → 渐显

## 5. 需求规格

### 5.1 站点地图

```
/                    → 按浏览器语言重写到 /zh/ 或 /en/
/zh/  /en/
├─ index             首页（落地页）
├─ gallery/          样片画廊（处理前后对比）         [NEW]
├─ features/         功能详解（分章多页）
├─ docs/             在线文档（9 章迁移 + 搜索）      [NEW — 独立于 features]
├─ download          下载（含历史版本入口）
├─ changelog         更新日志
├─ feedback          反馈与留言
├─ faq               常见问题                        [NEW]
├─ about             关于与开源
├─ privacy           隐私与数据声明                   [NEW]
└─ 404               自定义 404                      [NEW]
```

### 5.2 页面需求

#### 首页

- **定位语**：中文"科学胶片反转引擎" / 英文 "Scientific Film Inversion Engine" + 一句话价值主张
- **Hero 区**：
  - 主视觉使用**深色主题截图**（需补拍）替代当前浅色截图，或制作动态 hero 视频（10 秒循环，展示"负片→正片"全过程）
  - 移动端降级为静态图 + 叠加文案
  - 无 JS 时降级为纯文案 + 截图
- **核心卖点四格**：
  1. 密度域反转（基片扣除 → 光学密度 → 密度窗）
  2. Roll 校准帧与整卷批处理
  3. 胶片模拟 LUT（Kodak / Ilford）
  4. 全离线本地处理，零云端依赖
- **样片展示条**：横向滚动 3–5 组处理前后对比缩略图，点击进入 Gallery
- **下载按钮组**：自动识别访客平台并高亮对应安装包，显示当前版本号；无 JS 时降级为"前往最新版 Release"链接
- **导航**：通往 Gallery / Features / Docs / Download / Feedback

#### Gallery（样片画廊） [NEW]

- 展示 8–12 组处理前后对比（负片 → 正片）
  - 交互方式：滑动对比器（slider before/after），移动端改为点击切换
  - 标注所用胶片型号、扫描方式、使用的 LUT
- 用户投稿区（未来可选）：引导至 Discussions 投稿
- 素材来源：需在 M1 阶段前准备至少 6 组样片素材

#### 功能详解

素材来源：`ui/documentation-data.js` 9 章 + `docs/unified-density-pipeline.md` 等

- 按模块分章：
  1. **导入**（Roll / 散片 / 格式支持）
  2. **Develop 工作区**（反转 + 密度调整 + 曲线 + 细节）
  3. **校准**（白点 / 校准帧 / 硬件轮廓）
  4. **密度窗口与点曲线**
  5. **胶片模拟 LUT**
  6. **导出**（格式 / ICC / 接触印样 / 线性输出）
- 每章内容结构：
  - 一屏概述（50–100 字）
  - 配图：应用截图 + 处理效果图（尽量用深色主题截图）
  - 关键参数说明表
  - "在文档中详细了解 →" 链接至 Docs 对应章节
- 深链锚点，方便在 Issue 回复里直接引用某章

#### 在线文档 [NEW]

- 将 `documentation-data.js` 的 9 章内容迁移为 Markdown/MDX 静态页
- 章节列表：
  1. 快速开始
  2. 界面基础
  3. 导入与胶卷管理
  4. Develop 工作区
  5. 导出与接触印样
  6. 胶片模拟
  7. 疑难解答
  8. 色彩科学
  9. 硬件校准（实验性）
  10. 附录（快捷键 / 数据位置 / 已知限制 / 许可证）
- 左侧目录导航，右侧正文
- 站内搜索（Pagefind — Astro 生态零成本静态搜索）
- 版本标注：每章底部标注适用版本（如 "适用于 v1.1.0+"）

#### 下载页

- 三个安装包卡片（Windows x64 / macOS Apple Silicon / macOS Intel），各带文件名、大小、版本号（运行时从 Releases API 获取，构建时值兜底）
- 系统要求表（最低 OS 版本、内存建议、磁盘空间）
- **未签名 / 未公证安装教学**：Windows SmartScreen"仍要运行"、macOS 右键打开或系统设置放行（复用 README 文案），配步骤截图
- **历史版本入口**：页面底部"查看所有版本"链接至 GitHub Releases 列表页，或构建时生成最近 5 个版本的下载卡片
- Linux 现状说明（暂无验证过的发行包，指明自行构建路径）
- 大陆访问慢时的镜像/加速提示（占位，具体镜像后续定）

#### 更新日志

- 各版本 Release Notes（构建时从 Releases API 拉取生成静态内容），最新版置顶并标注
- 构建时注入 `GITHUB_TOKEN` 环境变量以避免 60 次/小时 API 限流（Cloudflare Pages 支持环境变量配置）
- "当前最新版"徽标可被首页/下载页复用

#### 反馈页

- Issue 引导卡片：bug 报告 / 功能建议两个模板按钮（主仓库 `.github/ISSUE_TEMPLATE/`）
- giscus 留言区配置：
  - Discussion 分类：`Website Feedback`（需在主仓库新建）
  - 映射方式：`pathname`（每个页面独立讨论串）
  - 主题：`dark`（匹配站点深色主题）
  - 大陆加载慢时自动降级显示 Issues 链接
- 无后端反馈表单：嵌入 Formspree/Tally 免费档，提交直接转邮箱，覆盖不使用 GitHub 的用户；表单加载失败时降级显示联系邮箱
- 联系邮箱（README 已有）；赞赏码（作者 + UI 设计师，素材已有）

#### FAQ（常见问题） [NEW]

- 内容来源：README"已知限制"、`documentation-data.js` 的 troubleshooting 章、Issue 中的高频问题
- 分类组织：
  - **安装问题**：SmartScreen 拦截、macOS 公证、xattr 命令
  - **格式兼容**：支持哪些 RAW？Nikon Z8 HE/HE* 限制、JPEG 校色效果差的原因
  - **性能问题**：大文件内存需求、超大 JPEG/PNG 解码失败
  - **数据安全**：数据存哪里？会修改原片吗？如何备份？
  - **使用技巧**：胶片范围怎么设？批量反相最佳实践
- 使用 Astro 的 `<details>` 折叠组件，无 JS 可用

#### 关于

- GPL-3.0 许可说明、贡献方式（PR 规范）、作者与 UI 设计师致谢
- 技术栈介绍（Rust / Tauri / SQLite / WebGL）
- 项目故事 / 动机（可选，增加人情味）

#### 隐私与数据声明 [NEW]

- 声明站点不使用 Cookie（GoatCounter 无 Cookie）
- Formspree/Tally 表单数据处理说明
- giscus 通过 GitHub OAuth 的说明
- NexFilm 应用本身的本地数据政策（不联网、不上传）

#### 自定义 404 [NEW]

- 品牌化 404 页面（使用 NexFilm logo + 深色主题）
- "看起来这一帧曝光失败了"之类的趣味文案
- 提供首页、下载页、文档页的快捷链接
- 站内搜索框

### 5.3 非功能需求

- **双语**：`/zh/`、`/en/` 路由；首次访问按 `Accept-Language` 跳转，任意页可切换语言（保持当前页面位置）；使用 `hreflang` 标签告知搜索引擎双语对应关系
- **性能**：纯静态、默认零客户端 JS（下载按钮增强、Gallery 滑动器除外）；Lighthouse 性能/可访问性 ≥ 90
- **视觉**：深色为主（与 Develop 界面气质一致），使用现有 SVG logo 明暗两版；预留浅色主题切换
- **SEO**：
  - 每页独立 `<title>` 和 `<meta description>`
  - Open Graph 图片（用 hero 截图，1200×630px）
  - `sitemap.xml` 自动生成（Astro 内置 `@astrojs/sitemap`）
  - `robots.txt`
  - JSON-LD 结构化数据（`SoftwareApplication` schema）：名称、版本、OS、许可证、下载链接
  - Canonical URL 策略：每个页面指定 canonical，避免 `/zh/` `/en/` 重复索引
- **可访问性**：
  - 所有图片提供有意义的 `alt` 文本
  - 颜色对比度 ≥ 4.5:1（WCAG AA）
  - 完整键盘导航支持
  - Skip-to-content 链接
  - 语义化 HTML 结构（`<nav>`、`<main>`、`<article>`、`<aside>`）
- **移动端**：
  - 汉堡菜单替代顶部导航栏
  - Gallery 对比器改为点击切换（而非滑动器）
  - 下载按钮组改为纵向排列
  - 触控友好的 touch target ≥ 44px
- **可维护性**：新发一个 Release 时**网站零改动**（版本号、下载链接运行时拉取）；改文案即改即生效

## 6. 技术设计

### 6.1 架构

```
访客 → Cloudflare Pages（静态站，nexfilm.app）
        ├─ 下载按钮 → github.com/BillyDu-TJ/NexFilm/releases/latest（GitHub CDN）
        ├─ 版本号/文件大小 → 运行时 fetch api.github.com …/releases/latest（构建时值兜底）
        ├─ 更新日志 → 构建时 fetch + GITHUB_TOKEN → 静态 HTML
        ├─ 反馈 → 主仓库 Issues（模板）+ giscus（Discussions）+ 表单（Formspree/Tally 转邮箱）
        ├─ 站内搜索 → Pagefind（构建时生成索引，纯静态）
        ├─ 匿名统计 → GoatCounter（外部脚本，无 Cookie）
        └─ 内容 → 构建时从文档素材生成静态页
```

### 6.2 站点仓库结构（`nexfilm-web`）

```
nexfilm-web/
├─ astro.config.mjs          # i18n: zh/en；site: https://nexfilm.app
├─ package.json
├─ tailwind.config.mjs        # 配色 token、字体、断点
├─ tsconfig.json
├─ src/
│  ├─ pages/
│  │  ├─ index.astro          # 语言检测重写
│  │  ├─ 404.astro            # [NEW] 自定义 404
│  │  ├─ zh/ 与 en/           # 两套路由，共用组件与内容集合
│  │  │  ├─ index.astro       # 首页
│  │  │  ├─ gallery.astro     # [NEW] 样片画廊
│  │  │  ├─ features/[...slug].astro
│  │  │  ├─ docs/[...slug].astro   # [NEW] 在线文档
│  │  │  ├─ download.astro
│  │  │  ├─ changelog.astro
│  │  │  ├─ feedback.astro
│  │  │  ├─ faq.astro         # [NEW] FAQ
│  │  │  ├─ about.astro
│  │  │  └─ privacy.astro     # [NEW] 隐私声明
│  ├─ components/
│  │  ├─ Header.astro         # 含响应式汉堡菜单
│  │  ├─ Footer.astro
│  │  ├─ DownloadButtons.astro
│  │  ├─ LangSwitch.astro
│  │  ├─ ThemeToggle.astro    # [NEW] 深色/浅色切换
│  │  ├─ BeforeAfter.astro    # [NEW] 处理前后对比滑动器
│  │  ├─ Giscus.astro
│  │  ├─ DocSidebar.astro     # [NEW] 文档侧栏目录
│  │  ├─ SearchWidget.astro   # [NEW] Pagefind 搜索
│  │  └─ SEOHead.astro        # [NEW] 统一 SEO meta + JSON-LD
│  ├─ content/
│  │  ├─ features/            # 功能详解各章（md/mdx，双语成对）
│  │  ├─ docs/                # [NEW] 9 章在线文档（从 documentation-data.js 迁移）
│  │  │  ├─ zh/
│  │  │  │  ├─ 01-quick-start.md
│  │  │  │  ├─ 02-interface-basics.md
│  │  │  │  ├─ ...
│  │  │  │  └─ 10-appendix.md
│  │  │  └─ en/
│  │  │     └─ (同上英文版)
│  │  ├─ faq/                 # [NEW] FAQ 条目
│  │  └─ gallery/             # [NEW] 样片元数据（胶片型号、设备、LUT）
│  ├─ i18n/
│  │  ├─ zh.ts                # UI 字典
│  │  └─ en.ts
│  ├─ styles/
│  │  ├─ global.css           # Tailwind + 自定义属性
│  │  └─ typography.css       # [NEW] 文档页排版样式
│  └─ utils/
│     ├─ github-releases.ts   # [NEW] 构建时拉取 Release 数据
│     └─ platform-detect.ts   # 浏览器端平台检测
├─ public/
│  ├─ favicon.svg
│  ├─ og-image.png            # 1200×630px OG 图
│  ├─ logo.svg
│  ├─ logo-dark.svg
│  ├─ robots.txt
│  └─ gallery/                # [NEW] 样片图片
├─ scripts/
│  ├─ migrate-docs.mjs        # [NEW] 从 documentation-data.js 提取内容为 Markdown
│  └─ check-links.mjs         # [NEW] 构建后链接检查
└─ .github/
   └─ workflows/
      └─ deploy.yml           # [NEW] CI：lint → build → Lighthouse → link check → deploy
```

### 6.3 关键组件行为

#### DownloadButtons

1. 服务端渲染三个平台的 `<a href="…/releases/latest">`（无 JS 可用）
2. 客户端脚本 fetch `https://api.github.com/repos/BillyDu-TJ/NexFilm/releases/latest`（约 3KB），按 `navigator.userAgent` / `navigator.userAgentData` 判定平台，高亮对应按钮并补上精确的资产直链、版本号、文件大小
3. API 限流（60 次/小时/IP）失败时保持降级形态，不影响使用

#### BeforeAfter（样片对比器） [NEW]

1. 纯 CSS + 少量 JS 的滑动器：两张图叠放，`input[type=range]` 控制裁切
2. 移动端 < 640px 改为点击切换（避免滑动与页面滚动冲突）
3. 图片使用 `<picture>` + WebP/AVIF + fallback JPEG，lazy loading

#### 双语

- 文案集中在 `src/i18n/` 字典；长文（功能详解、文档）走 content collections，`zh/`、`en/` 同 slug 成对，缺翻译时回退另一语言并标注
- 每个页面输出 `<link rel="alternate" hreflang="zh" href="…">` 和 `<link rel="alternate" hreflang="en" href="…">`
- 语言切换按钮保持当前路径，仅替换 `/zh/` ↔ `/en/` 前缀

#### SEOHead 组件 [NEW]

```astro
---
// 统一注入：title, description, OG, JSON-LD, canonical, hreflang
const { title, description, ogImage, type = 'website' } = Astro.props;
---
<title>{title}</title>
<meta name="description" content={description} />
<link rel="canonical" href={Astro.url.href} />
<link rel="alternate" hreflang="zh" href={zhUrl} />
<link rel="alternate" hreflang="en" href={enUrl} />
<meta property="og:title" content={title} />
<meta property="og:description" content={description} />
<meta property="og:image" content={ogImage ?? '/og-image.png'} />
<meta property="og:type" content={type} />
<script type="application/ld+json" set:html={jsonLd} />
```

#### JSON-LD 结构化数据（下载页） [NEW]

```json
{
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  "name": "NexFilm Engine",
  "applicationCategory": "MultimediaApplication",
  "operatingSystem": "Windows 10+, macOS 12+",
  "softwareVersion": "1.1.0",
  "license": "https://www.gnu.org/licenses/gpl-3.0.html",
  "downloadUrl": "https://github.com/BillyDu-TJ/NexFilm/releases/latest",
  "offers": { "@type": "Offer", "price": "0", "priceCurrency": "USD" }
}
```

#### 站内搜索 [NEW]

- 使用 [Pagefind](https://pagefind.app/)：构建后自动索引所有静态页
- 零运行时成本（索引文件静态托管在 `_pagefind/`）
- 搜索 UI 内嵌于 Header 或 Docs 侧栏

#### 部署

- Cloudflare Pages 连接 GitHub 仓库：构建命令 `npm run build && npx pagefind --site dist`，输出目录 `dist`，push `main` 自动上线，预览分支自动生成预览 URL
- 环境变量：`GITHUB_TOKEN`（构建时拉取 Release 数据，避免限流）
- 绑定域名：学生包兑换 Name.com 域名后，DNS 指给 Cloudflare（域名在 Name.com、DNS 托管 Cloudflare，或直接转入 Cloudflare Registrar）

### 6.4 CI/CD 管线 [NEW]

```yaml
# .github/workflows/deploy.yml
name: Build & Deploy
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 20 }
      - run: npm ci
      - run: npm run build
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      - run: npx pagefind --site dist

      # 链接检查
      - name: Check links
        run: node scripts/check-links.mjs dist

      # i18n 完整性：检查 zh/en 页面是否成对
      - name: i18n parity check
        run: |
          zh_count=$(find dist/zh -name '*.html' | wc -l)
          en_count=$(find dist/en -name '*.html' | wc -l)
          if [ "$zh_count" -ne "$en_count" ]; then
            echo "::error::zh ($zh_count) and en ($en_count) page counts differ"
            exit 1
          fi

      # Lighthouse CI
      - name: Lighthouse
        uses: treosh/lighthouse-ci-action@v12
        with:
          urls: |
            http://localhost:4321/zh/
            http://localhost:4321/en/
            http://localhost:4321/zh/download
          budgetPath: .lighthouserc.json

      # 部署到 Cloudflare Pages（仅 main 分支）
      - name: Deploy
        if: github.ref == 'refs/heads/main'
        uses: cloudflare/wrangler-action@v3
        with:
          command: pages deploy dist --project-name=nexfilm-web
```

### 6.5 内容迁移脚本 [NEW]

`scripts/migrate-docs.mjs` 的职责：

1. 读取 `ui/documentation-data.js`，解析 `CHAPTERS['zh-CN']` 和 `CHAPTERS['en']` 数组
2. 将每章的 HTML `content` 转换为 Markdown（使用 `turndown`）
3. 输出到 `src/content/docs/zh/` 和 `src/content/docs/en/`，带 frontmatter（title、chapter number、reading time）
4. 脚本为一次性迁移工具；后续文档维护在网站仓库内进行

### 6.6 主仓库配合改动（少量）

- 新增 `.github/ISSUE_TEMPLATE/bug-report.yml`（系统、版本、复现步骤、报错信息）— 使用 YAML 表单格式
- 新增 `.github/ISSUE_TEMPLATE/feature-request.yml`
- 新增 `.github/ISSUE_TEMPLATE/config.yml`（引导讨论类话题去 Discussions；**注意：此文件必须为 `.yml` 而非 `.md`**）
- 开启 Discussions 并新建 `Website Feedback` 分类（供 giscus 使用）

## 7. 里程碑与时间估算

| 阶段 | 内容 | 估算工期 | 验收标准 |
|---|---|---|---|
| **M0 素材准备** | 补拍深色主题截图、准备 6+ 组样片对比素材、核实域名可用性 | 3–5 天 | 素材清单完备，域名已确认可注册 |
| **M1 骨架** | Astro 双语脚手架、首页（含 Hero + 卖点）、下载页、404、CI 管线、部署到 `*.pages.dev` | 5–7 天 | 手机/桌面可访问，下载按钮可点，Lighthouse ≥ 85 |
| **M2 内容** | Gallery 页、功能详解迁移、在线文档（migrate-docs 脚本 + Pagefind）、FAQ、更新日志、反馈页 + Issue 模板 + giscus + 表单 | 7–10 天 | 全部页面填充完毕，站内搜索可用 |
| **M3 上线** | 兑换学生包域名并绑定、SEO（JSON-LD / OG / sitemap）、隐私声明页、大陆可达性验证、Lighthouse ≥ 90 | 3–5 天 | `nexfilm.app`（暂名）正式可访问，搜索引擎可索引 |
| **M4 可选** | Tauri updater `latest.json`、下载镜像、GoatCounter 匿名统计、浅色主题、用户样片投稿流程 | 按需 | 按需验收 |

## 8. 风险与对策

| 风险 | 对策 |
|---|---|
| 域名免费期仅一年 | 域名是可替换层（DNS 随时可换托管），到期要么续费（约 $10–20/年）要么退回免费二级域名 `*.pages.dev`，站点内容零改动 |
| 大陆访问 pages.dev / GitHub 不稳定 | 绑定自定义域名后通常好转；下载页留镜像提示位；反馈页 giscus 慢时降级为 Issues 链接 |
| GitHub API 限流（运行时） | 下载按钮降级形态仍完整可用；构建时有 `GITHUB_TOKEN` 不受限 |
| 安装包未签名 | 下载页显著位置放安装教学，配步骤截图 |
| 表单 / giscus 等第三方组件大陆加载不稳 | 均设降级：giscus → Issues 链接，表单 → 联系邮箱 |
| 学生认证失效 | 全链路无付费依赖，唯一影响是域名续费原价 |
| 样片素材版权问题 | 仅使用作者本人拍摄的胶片素材，或取得授权的用户投稿 |
| `documentation-data.js` 更新后网站内容过时 | M2 迁移后文档维护转移至网站仓库；主仓库应用内文档独立于网站，两者可异步更新 |
| Pagefind 索引体积过大 | 9 章文档 + 功能页预估索引 < 500KB，在可接受范围 |

## 9. 待确认事项

> 以下事项需在开工前确认，避免返工。

- [ ] 域名选择：`nexfilm.app` / `nexfilm.dev` / 其他？需先在 Name.com 查询实际可用性
- [ ] Hero 视觉方案：补拍深色主题截图即可，还是制作短视频循环？
- [ ] 样片素材：是否已有足够的处理前后对比素材？需要额外准备多少组？
- [ ] 在线文档是否需要与应用内文档保持同步更新？还是允许异步迭代？
- [ ] GoatCounter 是否在 M3 就接入，还是推迟到 M4？
- [ ] 是否需要在首页添加"社区/用户评价"区块？

## 10. 参考

- 学生包官方页：<https://education.github.com/pack>
- Namecheap 学生域名：<https://nc.me/landing/github>
- DigitalOcean 退出学生包公告：<https://github.com/orgs/community/discussions/201240>
- giscus：<https://giscus.app>
- Pagefind（静态搜索）：<https://pagefind.app>
- GoatCounter（隐私友好统计）：<https://www.goatcounter.com>
- Astro i18n 文档：<https://docs.astro.build/en/guides/internationalization/>
- JSON-LD SoftwareApplication：<https://schema.org/SoftwareApplication>
- Lighthouse CI：<https://github.com/GoogleChrome/lighthouse-ci>
