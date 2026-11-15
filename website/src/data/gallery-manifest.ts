import { withBase } from '../utils/urls';

export interface HeroComparisonSample {
  slug: string;
  filmStock: string;
  format: '135' | '120';
  gear: {
    zh: string;
    en: string;
  };
  inputDomain: string;
  negativeSrc: string;
  positiveSrc: string;
  aspect: string;
  title: {
    zh: string;
    en: string;
  };
}

export interface GallerySample {
  slug: string;
  filmStock: string;
  filmType: {
    zh: string;
    en: string;
  };
  format: '135' | '120';
  camera: string;
  scanner: string;
  gear: {
    zh: string;
    en: string;
  };
  inputDomain: string;
  positiveSrc: string;
  width: number;
  height: number;
  title: {
    zh: string;
    en: string;
  };
  desc: {
    zh: string;
    en: string;
  };
}

export const heroSample: HeroComparisonSample = {
  slug: 'fuji-400-35mm',
  filmStock: 'Fuji 400',
  format: '135',
  gear: {
    zh: 'Contax RTS II · Nikon D810 翻拍',
    en: 'Contax RTS II · Nikon D810 Scan',
  },
  inputDomain: 'Camera Native RGB',
  negativeSrc: withBase('/gallery/fuji-400-neg.webp'),
  positiveSrc: withBase('/gallery/fuji-400-pos.webp'),
  aspect: 'aspect-[1920/1701]',
  title: {
    zh: 'Fuji 400 (135 彩色负片)',
    en: 'Fuji 400 (35mm Color Negative)',
  },
};

export const galleryManifest: GallerySample[] = [
  {
    slug: 'kodak-gold-200-35mm',
    filmStock: 'Kodak Gold 200',
    filmType: {
      zh: '彩色负片 (C-41)',
      en: 'Color Negative (C-41)',
    },
    format: '135',
    camera: 'Nikon F3',
    scanner: 'Nikon D810',
    gear: {
      zh: 'Nikon F3 拍摄 · Nikon D810 翻拍',
      en: 'Shot on Nikon F3 · Scanned with Nikon D810',
    },
    inputDomain: 'Camera Native RGB',
    positiveSrc: withBase('/gallery/kodak-gold-200-pos.webp'),
    width: 1920,
    height: 1741,
    title: {
      zh: 'Kodak Gold 200 · 135 彩色负片',
      en: 'Kodak Gold 200 · 35mm Color Negative',
    },
    desc: {
      zh: '实拍高原晴空下的经幡与草甸（KODAK GB 200-7）。Camera Native RGB 原生色域直接扣除片基反相，蓝天通透干净不发青，高光云层与经幡色彩过渡自然，完整保留片边编号与齿孔边框。',
      en: 'Highland prayer flags and sunlit alpine grass shot on Kodak Gold 200 (KODAK GB 200-7). Inverted directly in Camera Native RGB without premapped gamut compression, keeping deep blue skies clean without cyan shift and preserving film rebate markings.',
    },
  },
  {
    slug: 'kodak-5222-35mm',
    filmStock: 'Kodak Eastman Double-X 5222',
    filmType: {
      zh: '黑白电影负片',
      en: 'B&W Motion Picture Negative',
    },
    format: '135',
    camera: 'Contax RTS II',
    scanner: 'Nikon D810',
    gear: {
      zh: 'Contax RTS II 拍摄 · Nikon D810 翻拍',
      en: 'Shot on Contax RTS II · Scanned with Nikon D810',
    },
    inputDomain: 'Camera Native RGB',
    positiveSrc: withBase('/gallery/kodak-5222-pos.webp'),
    width: 1920,
    height: 1692,
    title: {
      zh: 'Kodak Eastman Double-X 5222 · 135 黑白电影卷',
      en: 'Kodak Eastman Double-X 5222 · 35mm B&W Cine Film',
    },
    desc: {
      zh: '河岸林荫下的黑白电影底片实拍（5222+35）。基于对数光学密度映射还原银盐阶调，树冠暗部纹理扎实，水面强反光与建筑受光面高光滚降柔和，片边编码与齿孔锐利清晰。',
      en: 'Riverside shade and sunlit architecture captured on Eastman Double-X 5222 (5222+35). Logarithmic density inversion retains crisp silver grain across deep foliage shadows while gently rolling off bright water reflections.',
    },
  },
  {
    slug: 'kodak-5219-35mm',
    filmStock: 'Kodak Vision3 500T (5219)',
    filmType: {
      zh: '灯光型电影负片 (ECN-2)',
      en: 'Tungsten Cinema Negative (ECN-2)',
    },
    format: '135',
    camera: 'Contax RTS II',
    scanner: 'Nikon D810',
    gear: {
      zh: 'Contax RTS II 拍摄 · Nikon D810 翻拍',
      en: 'Shot on Contax RTS II · Scanned with Nikon D810',
    },
    inputDomain: 'Camera Native RGB',
    positiveSrc: withBase('/gallery/kodak-5219-pos.webp'),
    width: 1920,
    height: 1705,
    title: {
      zh: 'Kodak Vision3 500T (5219) · 135 电影负片',
      en: 'Kodak Vision3 500T (5219) · 35mm Cinema Film',
    },
    desc: {
      zh: '地下通道混合光源实拍（荧光灯顶光与红黄墙面照明）。密度反相完整保留了 5219 特有的青黄绿冷暖层次与红灯柔和晕光，顶灯高光与暗部通道过渡自然，保留完整片边与齿孔。',
      en: 'Underground transit corridor under mixed fluorescent and tungsten illumination. Density inversion preserves the signature teal-green shadows, warm tile midtones, gentle highlight bloom around lamps, and natural 5219 grain with full film borders.',
    },
  },
];
