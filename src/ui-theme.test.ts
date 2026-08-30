import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

// @ts-expect-error Vite supplies the raw source module in Vitest.
import packageJsonSource from '../package.json?raw'
import {
  resolveUiColorScheme,
  uiSemanticTokens,
  uiThemeConfig,
} from './ui-theme'

const themeCssSource = readFileSync('src/ui-theme.css', 'utf8')
const applicationCssSource = readFileSync('src/styles.css', 'utf8')
const indexHtmlSource = readFileSync('index.html', 'utf8')

const semanticKeys = [
  'accent',
  'accentForeground',
  'background',
  'border',
  'destructive',
  'foreground',
  'input',
  'muted',
  'mutedForeground',
  'primary',
  'primaryForeground',
  'ring',
  'secondary',
  'secondaryForeground',
  'surface',
  'surfaceRaised',
] as const

describe('shared UiPilot visual theme', () => {
  it('resolves every persisted preference to one final color scheme', () => {
    expect(resolveUiColorScheme('system', false)).toBe('light')
    expect(resolveUiColorScheme('system', true)).toBe('dark')
    expect(resolveUiColorScheme('light', true)).toBe('light')
    expect(resolveUiColorScheme('dark', false)).toBe('dark')
  })

  it('keeps complete matching semantic token records for light and dark', () => {
    expect(Object.keys(uiSemanticTokens.light).sort()).toEqual([...semanticKeys])
    expect(Object.keys(uiSemanticTokens.dark).sort()).toEqual([...semanticKeys])
    expect(uiSemanticTokens.light).toMatchObject({
      background: '#ffffff',
      foreground: '#0a0a0a',
      surface: '#ffffff',
      primary: '#171717',
      muted: '#f5f5f5',
      mutedForeground: '#737373',
      border: '#e5e5e5',
      ring: '#a3a3a3',
    })
    expect(uiSemanticTokens.dark).toMatchObject({
      background: '#0a0a0a',
      foreground: '#fafafa',
      surface: '#171717',
      primary: '#e5e5e5',
      muted: '#262626',
      mutedForeground: '#a3a3a3',
      ring: '#737373',
    })
  })

  it('projects stable compact sdk-test geometry into Ant Design', () => {
    const light = uiThemeConfig('light')
    const dark = uiThemeConfig('dark')

    expect(light.token).toMatchObject({
      colorBgBase: uiSemanticTokens.light.background,
      colorBgContainer: uiSemanticTokens.light.surface,
      colorText: uiSemanticTokens.light.foreground,
      colorPrimary: uiSemanticTokens.light.primary,
      colorBorder: uiSemanticTokens.light.border,
      borderRadius: 10,
      borderRadiusSM: 6,
      controlHeight: 32,
      controlHeightSM: 24,
      fontFamily: 'Inter, Avenir, Helvetica, Arial, "Microsoft YaHei UI", sans-serif',
      motion: false,
    })
    expect(dark.token).toMatchObject({
      colorBgBase: uiSemanticTokens.dark.background,
      colorBgContainer: uiSemanticTokens.dark.surface,
      colorText: uiSemanticTokens.dark.foreground,
      colorPrimary: uiSemanticTokens.dark.primary,
      colorBorder: uiSemanticTokens.dark.border,
    })
    expect(light.components?.Switch).toMatchObject({
      colorPrimary: '#16a34a',
      colorPrimaryHover: '#15803d',
    })
    expect(dark.components?.Switch).toMatchObject({
      colorPrimary: '#16a34a',
      colorPrimaryHover: '#15803d',
    })
    expect(light.algorithm).not.toBe(dark.algorithm)
  })

  it('maps stable plugin variables from the shared css token layer', () => {
    expect(themeCssSource).toContain(':root,')
    expect(themeCssSource).toMatch(/:root\[data-color-scheme=['"]light['"]\]/)
    expect(themeCssSource).toMatch(/:root\[data-color-scheme=['"]dark['"]\]/)
    expect(themeCssSource).toContain('--uipilot-color-surface: var(--uipilot-ui-surface);')
    expect(themeCssSource).toContain('--uipilot-color-text: var(--uipilot-ui-foreground);')
    expect(themeCssSource).toContain('--uipilot-color-border: var(--uipilot-ui-border);')
    expect(themeCssSource).toContain('--uipilot-color-accent: var(--uipilot-ui-primary);')
  })

  it('loads semantic tokens through the primary stylesheet entry', () => {
    expect(applicationCssSource).toMatch(
      /^@import '\.\/ui-theme\.css';\r?\n@import 'overlayscrollbars\/styles\/overlayscrollbars\.css';/,
    )
    expect(indexHtmlSource).toContain('<link rel="stylesheet" href="/src/styles.css" />')
    expect(indexHtmlSource).not.toContain('<link rel="stylesheet" href="/src/ui-theme.css" />')
  })

  it('adds only Lucide and no Tailwind or Radix dependency', () => {
    const dependencies = JSON.parse(packageJsonSource).dependencies as Record<string, string>
    expect(dependencies['lucide-react']).toBe('^1.21.0')
    expect(Object.keys(dependencies).filter((name) => /tailwind|radix/i.test(name))).toEqual([])
  })
})
