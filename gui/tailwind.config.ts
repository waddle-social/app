import type { Config } from 'tailwindcss';

export default {
  content: ['./index.html', './src/**/*.{vue,ts,tsx}'],
  theme: {
    extend: {
      colors: {
        background: 'var(--waddle-bg)',
        foreground: 'var(--waddle-fg)',
        accent: 'var(--waddle-accent)',
        surface: 'var(--waddle-surface)',
        border: 'var(--waddle-border)',
        success: 'var(--waddle-success)',
        warning: 'var(--waddle-warning)',
        danger: 'var(--waddle-error)',
        muted: 'var(--waddle-muted)',
      },
    },
  },
  plugins: [],
} satisfies Config;
