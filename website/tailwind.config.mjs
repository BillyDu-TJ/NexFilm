/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{astro,html,js,jsx,md,mdx,svelte,ts,tsx,vue}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Optical Darkroom Core Tokens (docs/website-plan.md v2.0)
        'darkroom': {
          canvas: '#121214',
          bg: '#161618',
          chrome: '#1d1d20',
          panel: '#222225',
          raised: '#2a2a2e',
          line: 'rgba(255, 255, 255, 0.08)',
          'line-strong': 'rgba(255, 255, 255, 0.16)',
        },
        // Copper Amber Accent System
        accent: {
          DEFAULT: '#c4814f',
          hover: '#d28e59',
          soft: 'rgba(196, 129, 79, 0.15)',
          muted: '#9b5e31',
        },
        // Printer Light Channels
        printer: {
          r: '#e06c65',
          g: '#6bc48d',
          b: '#62a6e5',
        },
        // Typography colors
        text: {
          primary: '#f0f0f1',
          secondary: '#aaaab0',
          muted: '#74747b',
        },
        // Semantic backwards-compatible surfaces mapped to darkroom
        surface: {
          bg: '#161618',
          canvas: '#121214',
          card: '#222225',
          panel: '#222225',
          hover: '#2a2a2e',
          raised: '#2a2a2e',
          border: 'rgba(255, 255, 255, 0.08)',
          'border-strong': 'rgba(255, 255, 255, 0.16)',
          muted: '#18181a',
        },
        brand: {
          DEFAULT: '#c4814f',
          light: '#d28e59',
          dark: '#9b5e31',
          glow: 'rgba(196, 129, 79, 0.18)',
        },
      },
      fontFamily: {
        sans: [
          '"Inter"',
          '"SF Pro Text"',
          '"SF Pro Display"',
          '"Segoe UI Variable Text"',
          '"PingFang SC"',
          '"Noto Sans SC"',
          '"Microsoft YaHei"',
          '-apple-system',
          'BlinkMacSystemFont',
          '"Segoe UI"',
          'sans-serif',
        ],
        mono: [
          '"JetBrains Mono"',
          '"SFMono-Regular"',
          '"Cascadia Mono"',
          'Consolas',
          'monospace',
        ],
        editorial: [
          '"Noto Serif SC"',
          '"Source Han Serif SC"',
          '"Songti SC"',
          'SimSun',
          'Georgia',
          'serif',
        ],
        serif: [
          '"Noto Serif SC"',
          '"Source Han Serif SC"',
          '"Songti SC"',
          'SimSun',
          'Georgia',
          'serif',
        ],
      },
      boxShadow: {
        'glow-accent': '0 0 25px -5px rgba(196, 129, 79, 0.35)',
        'darkroom-card': '0 12px 28px rgba(0, 0, 0, 0.22)',
      },
    },
  },
  plugins: [],
};
