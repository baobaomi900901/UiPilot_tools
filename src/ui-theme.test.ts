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
      background: '#f7f7f8',
      foreground: '#171719',
      surface: '#ffffff',
      primary: '#18191a',
      muted: '#f1f1f2',
      mutedForeground: '#6f6f74',
      border: '#d9d9dc',
      ring: '#8e8e93',
    })
    expect(uiSemanticTokens.dark).toMatchObject({
      background: '#07080a',
      foreground: '#f4f4f6',
      surface: '#0d0d0d',
      primary: '#ffffff',
      muted: '#101111',
      mutedForeground: '#9c9c9d',
      ring: '#434345',
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
      borderRadius: 8,
      borderRadiusSM: 6,
      controlHeight: 36,
      controlHeightSM: 28,
      fontFamily: 'Inter, "Inter Fallback", -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei UI", sans-serif',
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
      colorPrimary: '#18191a',
      colorPrimaryHover: '#2e2e30',
    })
    expect(dark.components?.Switch).toMatchObject({
      colorPrimary: '#ffffff',
      colorPrimaryHover: '#e8e8e8',
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
    expect(themeCssSource).toContain('--uipilot-ui-surface-elevated: #101111;')
    expect(themeCssSource).toContain('--uipilot-ui-surface-card: #121212;')
  })

  it('loads semantic tokens through the primary stylesheet entry', () => {
    expect(applicationCssSource).toMatch(
      /^@import '\.\/ui-theme\.css';\r?\n@import 'overlayscrollbars\/styles\/overlayscrollbars\.css';/,
    )
    expect(indexHtmlSource).toContain('<link rel="stylesheet" href="/src/styles.css" />')
    expect(indexHtmlSource).not.toContain('<link rel="stylesheet" href="/src/ui-theme.css" />')
  })

  it('applies Raycast chrome to launcher results, settings tabs, and plugin details', () => {
    expect(applicationCssSource).toMatch(
      /\.launcher-view \.result-row\.is-selected\s*\{[^}]*background:\s*var\(--uipilot-ui-surface-card\);[^}]*box-shadow:\s*none;/s,
    )
    expect(applicationCssSource).toMatch(
      /\.settings-tabs \.ant-tabs-tab-active\s*\{[^}]*background:\s*var\(--uipilot-ui-accent\);/s,
    )
    expect(applicationCssSource).toMatch(
      /\.settings-view \.ant-btn:not\([^}]*background:\s*var\(--uipilot-ui-surface-elevated\);/s,
    )
    expect(applicationCssSource).toMatch(
      /\.public-plugin-detail-list\s*\{[^}]*grid-template-columns:\s*92px minmax\(0, 1fr\);[^}]*padding:\s*8px 0 0;/s,
    )
  })

  it('adds only Lucide and no Tailwind or Radix dependency', () => {
    const dependencies = JSON.parse(packageJsonSource).dependencies as Record<string, string>
    expect(dependencies['lucide-react']).toBe('^1.21.0')
    expect(Object.keys(dependencies).filter((name) => /tailwind|radix/i.test(name))).toEqual([])
  })
})
