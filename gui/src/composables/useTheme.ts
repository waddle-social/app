import { watch } from 'vue';

import { useSettingsStore, type ThemeChoice } from '../stores/settings';
import { useWaddle } from './useWaddle';

interface ThemeColors {
  background: string;
  foreground: string;
  surface: string;
  accent: string;
  border: string;
  success: string;
  warning: string;
  error: string;
  muted: string;
}

type BuiltinThemeName = 'light' | 'dark' | 'high-contrast';

const BUILTIN_THEMES: Record<BuiltinThemeName, ThemeColors> = {
  light: {
    background: '#ffffff',
    foreground: '#1a1a1a',
    surface: '#f5f5f5',
    accent: '#0066cc',
    border: '#d4d4d4',
    success: '#22863a',
    warning: '#b08800',
    error: '#cb2431',
    muted: '#6a737d',
  },
  dark: {
    background: '#1e1e2e',
    foreground: '#cdd6f4',
    surface: '#313244',
    accent: '#89b4fa',
    border: '#45475a',
    success: '#a6e3a1',
    warning: '#f9e2af',
    error: '#f38ba8',
    muted: '#6c7086',
  },
  'high-contrast': {
    background: '#000000',
    foreground: '#ffffff',
    surface: '#1a1a1a',
    accent: '#ffff00',
    border: '#ffffff',
    success: '#00ff00',
    warning: '#ffff00',
    error: '#ff0000',
    muted: '#aaaaaa',
  },
};

function isBuiltinThemeName(name: string): name is BuiltinThemeName {
  return Object.prototype.hasOwnProperty.call(BUILTIN_THEMES, name);
}

function resolveSystemTheme(): 'light' | 'dark' {
  if (typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches) {
    return 'dark';
  }
  return 'light';
}

function applyThemeColors(colors: ThemeColors): void {
  const root = document.documentElement;
  root.style.setProperty('--waddle-bg', colors.background);
  root.style.setProperty('--waddle-fg', colors.foreground);
  root.style.setProperty('--waddle-surface', colors.surface);
  root.style.setProperty('--waddle-accent', colors.accent);
  root.style.setProperty('--waddle-border', colors.border);
  root.style.setProperty('--waddle-success', colors.success);
  root.style.setProperty('--waddle-warning', colors.warning);
  root.style.setProperty('--waddle-error', colors.error);
  root.style.setProperty('--waddle-muted', colors.muted);
}

function applyThemeChoice(choice: ThemeChoice): void {
  const resolved = choice === 'system' ? resolveSystemTheme() : choice;
  const colors = BUILTIN_THEMES[resolved];
  if (colors) {
    applyThemeColors(colors);
  }
}

export function applyPluginColors(pluginId: string, tokens: Record<string, string>): void {
  const root = document.documentElement;
  for (const [token, value] of Object.entries(tokens)) {
    root.style.setProperty(`--waddle-plugin-${pluginId}-${token}`, value);
  }
}

export function useTheme(): void {
  const settings = useSettingsStore();
  const waddle = useWaddle();

  applyThemeChoice(settings.theme);

  watch(() => settings.theme, applyThemeChoice);

  if (typeof window !== 'undefined') {
    const mql = window.matchMedia('(prefers-color-scheme: dark)');
    mql.addEventListener('change', () => {
      if (settings.theme === 'system') {
        applyThemeChoice('system');
      }
    });
  }

  waddle.listen<{ name: string }>('ui.theme.changed', (event) => {
    const name = event.payload.name;
    if (isBuiltinThemeName(name)) {
      applyThemeColors(BUILTIN_THEMES[name]);
    }
  });
}
