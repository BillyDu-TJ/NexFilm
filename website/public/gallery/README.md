# NexFilm Gallery Assets Specification (样片素材接入规范)

本文档定义 NexFilm 官网画廊与首页样片对比器的真实双源素材规范。

## 1. 核心架构原则

- **严格双源（Strict Dual-Source）**：对比器左侧为未经反相的原始负片扫描文件（Negative），右侧为在 NexFilm 中经过密度反相与印片光校正的正片成片（Positive）。
- **零滤镜穿帮（Zero Filter Invert）**：严禁使用 CSS `filter: invert()` 伪造负片，必须使用真实的独立文件。
- **无 UI 干扰（No UI Screenshots）**：展示内容为纯粹的摄影胶片画幅，包含未曝光片基边框、真实齿孔或纯净片门，不包含任何桌面软件白色窗口或界面按钮。

## 2. 文件命名与格式规范

将样片放置在本目录（`website/public/gallery/`）下，遵循以下命名约定：

```text
website/public/gallery/
├── <slug>-neg.webp    # 翻拍负片原貌（含 C-41 橙色色罩或银盐黑白负像）
└── <slug>-pos.webp    # NexFilm 对数密度反相正片（带相纸/印片 LUT 或纯净反相）
```

支持格式：
- `WebP`（推荐，质量 85~92，带无损或高质量有损压缩，单张体积 300KB ~ 1MB）
- `PNG` 或 `SVG`（高质量矢量或预渲染兜底格式）

建议分辨率与比例：
- 135 格式（3:2）：推荐 `1800 x 1200` 或 `2400 x 1600`
- 120 格式（6x6 / 6x7）：推荐 `1600 x 1600` 或 `2100 x 1800`
- 宽屏容器展示比例推荐：`aspect-[16/10]` 或 `aspect-[3/2]`

## 3. 在 `src/data/gallery-manifest.ts` 中注册

当放入新实拍图后，在 `website/src/data/gallery-manifest.ts` 中注册或替换现有 `slug` 即可：

```typescript
{
  slug: 'portra-400-35mm',
  filmStock: 'Kodak Portra 400',
  format: '135',
  gear: 'Nikon Z6 II + Nikkor Micro 105mm f/2.8',
  inputDomain: 'CameraNative / Linear Bayer RAW',
  emulationLut: 'Kodak Portra Endura', // 必须是 assets/luts/ 下存在的真实 .cube 文件名
  negativeSrc: '/gallery/portra-400-neg.webp',
  positiveSrc: '/gallery/portra-400-pos.webp',
  aspect: 'aspect-[16/10]',
  title: {
    zh: 'Kodak Portra 400 (135 画幅)',
    en: 'Kodak Portra 400 (135 Format)'
  },
  desc: {
    zh: '经典人像负片，温润肤色还原与细腻的高光过渡...',
    en: 'Legendary portrait color negative featuring subtle warm skin tones...'
  }
}
```

重新构建站点（`npm run build`）后即可生效。
