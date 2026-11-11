export const languages = {
  zh: '简体中文',
  en: 'English',
};

export const defaultLang = 'zh';

export const ui = {
  zh: {
    'nav.home': '首页',
    'nav.gallery': '效果画廊',
    'nav.features': '功能详解',
    'nav.docs': '使用文档',
    'nav.download': '下载',
    'nav.changelog': '更新日志',
    'nav.faq': '常见问题',
    'nav.feedback': '反馈交流',
    'nav.about': '关于',

    'hero.badge': 'NexFilm Engine v1.1.0 稳定版已发布',
    'hero.title_pre': '科学级',
    'hero.title_highlight': '胶片密度域反转',
    'hero.title_post': '工作台',
    'hero.subtitle': '专为翻拍与扫描负片打造。基于物理光学密度模型反相，杜绝数码曲线硬拉导致的交叉偏色；支持整卷一致性校准与暗房印片光微调，全程本地离线运行。',
    'hero.download_btn': '立即下载',
    'hero.docs_btn': '查看功能文档',
    'hero.github_btn': 'GitHub 仓库',

    'features.title': '核心特性与技术优势',
    'features.subtitle': '抛弃黑盒 LUT 猜解，基于光学密度物理定律构建的负片反转管线',
    'features.density_title': '统一光学密度管线',
    'features.density_desc': '按通道真实扣除片基，计算对数透射率光学密度。无论输入是相机 RAW、扫描仪 TIFF 还是 FFF，均获得严谨一致的结果。',
    'features.calibration_title': '整卷基准标定与批处理',
    'features.calibration_desc': '采样真实未曝光片基与全曝光片头，锁定物理色彩锚点。按整卷最亮画面标定共享白点，一键自动批量反转全卷。',
    'features.develop_title': '实时 WebGL 显色工作台',
    'features.develop_desc': '集成暗房印片光、四通道控制点曲线、局部细节锐化与纹理去朦胧，搭配直方图与波形图，实时 60FPS 调色。',
    'features.privacy_title': '100% 本地离线与隐私保护',
    'features.privacy_desc': '原生 Rust + Tauri + SQLite 构建。不上传任何原片，不依赖云端 API，零数据收集，随时随地专注胶片创作。',

    'gallery.title': '真实样片反转效果',
    'gallery.subtitle': '拖动滑块直观对比负片原始扫描与 NexFilm 科学反转正片',

    'download.latest': '最新版本',
    'download.desc': '支持 Windows 10/11 与 macOS，全平台免配置开箱即用。',
    'download.win': 'Windows x64 安装包',
    'download.mac_arm': 'macOS Apple Silicon 芯片',
    'download.mac_intel': 'macOS Intel 处理器',
    'download.note': '安装包未进行商业代码签名，首次运行请参考下方安装指南放行。',

    'footer.copyright': 'NexFilm Engine · 采用 GNU General Public License v3.0 授权开源',
    'footer.author': '软件作者: Billy_Du · UI 设计: lonely-xmw',
    'footer.disclaimer': '本站纯静态运行，不跟踪用户隐私。',
  },
  en: {
    'nav.home': 'Home',
    'nav.gallery': 'Gallery',
    'nav.features': 'Features',
    'nav.docs': 'Docs',
    'nav.download': 'Download',
    'nav.changelog': 'Changelog',
    'nav.faq': 'FAQ',
    'nav.feedback': 'Feedback',
    'nav.about': 'About',

    'hero.badge': 'NexFilm Engine v1.1.0 Released',
    'hero.title_pre': 'Scientific',
    'hero.title_highlight': 'Film Density Inversion',
    'hero.title_post': 'Engine',
    'hero.subtitle': 'Engineered for camera scanning and film scanners. Built on physical optical density inversion to eliminate cross-channel distortion, with roll-wide calibration and darkroom printer lights — entirely offline.',
    'hero.download_btn': 'Download Now',
    'hero.docs_btn': 'Documentation',
    'hero.github_btn': 'GitHub Repo',

    'features.title': 'Engine Architecture & Core Features',
    'features.subtitle': 'Ditch arbitrary LUT guesswork. A transparent pipeline built on optical transmission physics.',
    'features.density_title': 'Unified Optical Density Pipeline',
    'features.density_desc': 'Per-channel base subtraction mapped to logarithmic optical density. Unifies camera RAW, scanner TIFF, and FFF into one consistent workflow.',
    'features.calibration_title': 'Roll Calibration & Batch Inversion',
    'features.calibration_desc': 'Sample unexposed base and full-exposure leader to lock physical anchors. Measure shared white point across the roll for seamless batch inversion.',
    'features.develop_title': 'Real-Time WebGL Workspace',
    'features.develop_desc': 'Printer lights, 4-channel parametric point curves, texture sharpening, and dehaze with real-time histogram and waveform at 60 FPS.',
    'features.privacy_title': '100% Local, Offline & Private',
    'features.privacy_desc': 'Built with Rust, Tauri, and SQLite. Your scans never touch any cloud server. Completely offline, zero tracking, total control.',

    'gallery.title': 'Film Inversion Gallery',
    'gallery.subtitle': 'Drag the slider to compare raw camera scans with NexFilm inverted positives.',

    'download.latest': 'Latest Release',
    'download.desc': 'Available for Windows 10/11 x64 and macOS (Apple Silicon & Intel).',
    'download.win': 'Download for Windows x64',
    'download.mac_arm': 'Download for macOS Apple Silicon',
    'download.mac_intel': 'Download for macOS Intel',
    'download.note': 'Binaries are not commercially code-signed yet. Please follow the instructions below on first launch.',

    'footer.copyright': 'NexFilm Engine · Open source under GNU GPL v3.0',
    'footer.author': 'Author: Billy_Du · UI Design: lonely-xmw',
    'footer.disclaimer': 'Static website with zero tracking cookies.',
  }
} as const;

export function useTranslations(lang: 'zh' | 'en') {
  return function t(key: keyof typeof ui['zh']) {
    return ui[lang][key] || ui['zh'][key];
  };
}
