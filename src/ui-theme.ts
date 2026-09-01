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
  accent: '#e9e9eb',
  accentForeground: '#171719',
  background: '#f7f7f8',
  border: '#d9d9dc',
  destructive: '#dc4343',
  foreground: '#171719',
  input: '#b9b9bd',
  muted: '#f1f1f2',
  mutedForeground: '#6f6f74',
  primary: '#18191a',
  primaryForeground: '#ffffff',
  ring: '#8e8e93',
  secondary: '#f1f1f2',
  secondaryForeground: '#171719',
  surface: '#ffffff',
  surfaceRaised: '#ffffff',
})

const dark = Object.freeze<UiSemanticTokens>({
  accent: '#121212',
  accentForeground: '#f4f4f6',
  background: '#07080a',
  border: '#242728',
  destructive: '#ff6161',
  foreground: '#f4f4f6',
  input: 'rgba(255, 255, 255, 0.16)',
  muted: '#101111',
  mutedForeground: '#9c9c9d',
  primary: '#ffffff',
  primaryForeground: '#000000',
  ring: '#434345',
  secondary: '#101111',
  secondaryForeground: '#cdcdcd',
  surface: '#0d0d0d',
  surfaceRaised: '#101111',
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
      borderRadius: 8,
      borderRadiusLG: 10,
      borderRadiusSM: 6,
      controlHeight: 36,
      controlHeightSM: 28,
      fontFamily: 'Inter, "Inter Fallback", -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei UI", sans-serif',
      motion: false,
    },
    components: {
      Button: {
        defaultBg: tokens.secondary,
        defaultBorderColor: tokens.border,
        defaultColor: tokens.foreground,
        defaultHoverBg: tokens.accent,
        defaultHoverBorderColor: tokens.input,
        defaultHoverColor: tokens.foreground,
        primaryColor: tokens.primaryForeground,
        textHoverBg: tokens.accent,
        defaultShadow: 'none',
        primaryShadow: 'none',
        dangerShadow: 'none',
      },
      Input: {
        activeBg: tokens.secondary,
        activeBorderColor: tokens.ring,
        hoverBg: tokens.secondary,
        hoverBorderColor: tokens.ring,
        activeShadow: 'none',
      },
      Select: {
        activeBorderColor: tokens.ring,
        hoverBorderColor: tokens.ring,
        activeOutlineColor: 'transparent',
        optionSelectedBg: tokens.accent,
        selectorBg: tokens.secondary,
      },
      Switch: {
        colorPrimary: tokens.primary,
        colorPrimaryHover: scheme === 'dark' ? '#e8e8e8' : '#2e2e30',
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
