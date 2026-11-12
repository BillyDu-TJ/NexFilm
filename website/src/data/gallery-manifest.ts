import { withBase } from '../utils/urls';

export interface GallerySample {
  slug: string;
  filmStock: string;
  format: '135' | '120';
  gear: string;
  inputDomain: string;
  emulationLut: string;
  negativeSrc: string;
  positiveSrc: string;
  aspect: string;
  title: {
    zh: string;
    en: string;
  };
  desc: {
    zh: string;
    en: string;
  };
}

export const galleryManifest: GallerySample[] = [
  {
    slug: 'portra-400-35mm',
    filmStock: 'Kodak Portra 400',
    format: '135',
    gear: 'Nikon Z6 II + Nikkor Micro 105mm f/2.8',
    inputDomain: 'CameraNative / Linear Bayer RAW',
    emulationLut: 'Kodak Portra Endura',
    negativeSrc: withBase('/gallery/portra-400-neg.webp'),
    positiveSrc: withBase('/gallery/portra-400-pos.webp'),
    aspect: 'aspect-[16/10]',
    title: {
      zh: 'Kodak Portra 400 (135 格式 · 35mm)',
      en: 'Kodak Portra 400 (135 Format · 35mm)',
    },
    desc: {
      zh: '经典人像彩色负片，温润肤色还原与细腻的高光过渡。NexFilm 统一密度管线在红黄通道的高光抑制非常自然，搭配内置 Kodak Portra Endura 相纸响应曲线，真实再现经典冲印质感。',
      en: 'Legendary color negative portrait stock with subtle skin tones and forgiving highlight roll-off. Unified density inversion keeps the red/yellow highlight roll-off natural, finished with built-in Kodak Portra Endura paper tonality.',
    },
  },
  {
    slug: 'superia-400-35mm',
    filmStock: 'Fujifilm Superia X-TRA 400',
    format: '135',
    gear: 'Sony A7R IV + FE 90mm f/2.8 Macro G',
    inputDomain: 'CameraNative / Linear Bayer RAW',
    emulationLut: 'Kodak Endura Premier',
    negativeSrc: withBase('/gallery/superia-400-neg.webp'),
    positiveSrc: withBase('/gallery/superia-400-pos.webp'),
    aspect: 'aspect-[16/10]',
    title: {
      zh: 'Fujifilm Superia X-TRA 400 (135 格式 · 35mm)',
      en: 'Fujifilm Superia X-TRA 400 (135 Format · 35mm)',
    },
    desc: {
      zh: '特有的第四感色层，绿色与青色阶调生动，微反差强烈。逐通道片基自动扣除后彻底消除橙褐色环境色干扰，有效还原街头阴影部的丰富细节。',
      en: 'Fujifilm daylight emulsion featuring the proprietary 4th color layer. Crisp emerald greens, rich micro-contrast, and pristine shadow depth unlocked through per-channel film base subtraction.',
    },
  },
  {
    slug: 'hp5-plus-400-120',
    filmStock: 'Ilford HP5 Plus 400',
    format: '120',
    gear: 'Hasselblad 500C/M + CF 80mm f/2.8 翻拍',
    inputDomain: 'CameraNative / Linear Bayer RAW',
    emulationLut: 'Ilford MGFB 2',
    negativeSrc: withBase('/gallery/hp5-plus-400-neg.webp'),
    positiveSrc: withBase('/gallery/hp5-plus-400-pos.webp'),
    aspect: 'aspect-[16/10]',
    title: {
      zh: 'Ilford HP5 Plus 400 黑白负片 (120 中画幅 6x6)',
      en: 'Ilford HP5 Plus 400 B&W (120 Medium Format 6x6)',
    },
    desc: {
      zh: '厚重饱满的银盐颗粒与开阔的动态范围。单通道对数光学密度映射忠实还原乳剂厚度，搭配内置 Ilford Multigrade FB 2 号相纸特性曲线，暗部深邃有层次，高光如纯银般流淌。',
      en: 'Iconic medium-format silver halide emulsion with rich grain and immense latitude. Single-channel logarithmic density inversion paired with built-in Ilford Multigrade FB Grade 2 silver gelatin paper response.',
    },
  },
  {
    slug: 'vision3-500t-35mm',
    filmStock: 'Kodak Vision3 500T (5219)',
    format: '135',
    gear: 'Canon EOS R5 + RF 100mm f/2.8L Macro IS USM',
    inputDomain: 'CameraNative / Linear Bayer RAW',
    emulationLut: 'Kodak 2383',
    negativeSrc: withBase('/gallery/vision3-500t-neg.webp'),
    positiveSrc: withBase('/gallery/vision3-500t-pos.webp'),
    aspect: 'aspect-[16/10]',
    title: {
      zh: 'Kodak Vision3 500T 电影胶卷 (5219 · 35mm)',
      en: 'Kodak Vision3 500T Motion Picture Film (5219 · 35mm)',
    },
    desc: {
      zh: '去碳后的钨丝灯电影负片，具备极高的高光宽容度与柔和光晕。通过暗房印片光调节与内置 Kodak 2383 电影印片正片 LUT，瞬间呈现经典好莱坞大银幕影调。',
      en: 'Remjet-removed tungsten motion picture stock with extreme dynamic latitude. Controlled halation glow balanced through darkroom printer lights and iconic Kodak 2383 print film emulation.',
    },
  },
];
