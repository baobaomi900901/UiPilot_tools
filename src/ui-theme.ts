import { theme, type ThemeConfig } from 'antd'

import type { ThemePreference } from './protocol'

export type UiColorScheme = 'light' | 'dark'

export interface UiSemanticTokens {
  accent: string
  accentForeground: string
  background: string
  border: string
  destructive: string
  foreground: string
  input: string
  muted: string
  mutedForeground: string
  primary: string
  primaryForeground: string
  ring: string
  secondary: string
  secondaryForeground: string
  surface: string
  surfaceRaised: string
}

const light = Object.freeze<UiSemanticTokens>({
  accent: '#f5f5f5',
  accentForeground: '#171717',
  background: '#ffffff',
  border: '#e5e5e5',
  destructive: '#e7000b',
  foreground: '#0a0a0a',
  input: '#e5e5e5',
  muted: '#f5f5f5',
  mutedForeground: '#737373',
  primary: '#171717',
  primaryForeground: '#fafafa',
  ring: '#a3a3a3',
  secondary: '#f5f5f5',
  secondaryForeground: '#171717',
  surface: '#ffffff',
  surfaceRaised: '#ffffff',
})

const dark = Object.freeze<UiSemanticTokens>({
  accent: '#262626',
  accentForeground: '#fafafa',
  background: '#0a0a0a',
  border: 'rgba(255, 255, 255, 0.1)',
  destructive: '#ff6467',
  foreground: '#fafafa',
  input: 'rgba(255, 255, 255, 0.15)',
  muted: '#262626',
  mutedForeground: '#a3a3a3',
  primary: '#e5e5e5',
  primaryForeground: '#171717',
  ring: '#737373',
  secondary: '#262626',
  secondaryForeground: '#fafafa',
  surface: '#171717',
  surfaceRaised: '#171717',
})

export const uiSemanticTokens = Object.freeze({ light, dark })

export function resolveUiColorScheme(
  preference: ThemePreference,
  systemDark: boolean,
): UiColorScheme {
  if (preference === 'dark') return 'dark'
  if (preference === 'light') return 'light'
  return systemDark ? 'dark' : 'light'
}

export function uiThemeConfig(scheme: UiColorScheme): ThemeConfig {
  const tokens = uiSemanticTokens[scheme]
  return {
    algorithm: scheme === 'dark' ? theme.darkAlgorithm : theme.defaultAlgorithm,
    token: {
      colorPrimary: tokens.primary,
      colorInfo: tokens.primary,
      colorError: tokens.destructive,
      colorBgBase: tokens.background,
      colorBgLayout: tokens.background,
      colorBgContainer: tokens.surface,
      colorBgElevated: tokens.surfaceRaised,
      colorText: tokens.foreground,
      colorTextBase: tokens.foreground,
      colorTextSecondary: tokens.mutedForeground,
      colorTextTertiary: tokens.mutedForeground,
      colorBorder: tokens.border,
      colorBorderSecondary: tokens.border,
      colorFillSecondary: tokens.secondary,
      colorFillTertiary: tokens.muted,
      borderRadius: 10,
      borderRadiusLG: 10,
      borderRadiusSM: 6,
      controlHeight: 32,
      controlHeightSM: 24,
      fontFamily: 'Inter, Avenir, Helvetica, Arial, "Microsoft YaHei UI", sans-serif',
      motion: false,
    },
    components: {
      Button: {
        defaultBg: tokens.surface,
        defaultBorderColor: tokens.border,
        defaultColor: tokens.foreground,
        primaryColor: tokens.primaryForeground,
        textHoverBg: tokens.accent,
        defaultShadow: 'none',
        primaryShadow: 'none',
        dangerShadow: 'none',
      },
      Input: {
        activeBorderColor: tokens.ring,
        hoverBorderColor: tokens.ring,
        activeShadow: `0 0 0 2px ${scheme === 'dark' ? 'rgba(115, 115, 115, 0.3)' : 'rgba(163, 163, 163, 0.25)'}`,
      },
      Select: {
        activeBorderColor: tokens.ring,
        hoverBorderColor: tokens.ring,
        activeOutlineColor: 'transparent',
        optionSelectedBg: tokens.accent,
      },
      Switch: {
        colorPrimary: '#16a34a',
        colorPrimaryHover: '#15803d',
      },
      Tabs: {
        itemColor: tokens.mutedForeground,
        itemHoverColor: tokens.foreground,
        itemSelectedColor: tokens.foreground,
        inkBarColor: tokens.primary,
      },
      Tooltip: {
        colorBgSpotlight: tokens.surfaceRaised,
        colorTextLightSolid: tokens.foreground,
      },
    },
  }
}
