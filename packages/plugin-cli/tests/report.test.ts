import { describe, expect, it } from 'vitest'

import { IssueCollector } from '../src/report.js'

describe('IssueCollector', () => {
  it('sorts by the frozen total key and deduplicates identical keys', () => {
    const issues = new IssueCollector()
    issues.add({ phaseRank: 40, ruleRank: 2, code: 'MANIFEST_SEMANTIC_INVALID', path: 'z.js', message: 'later' })
    issues.add({ phaseRank: 20, ruleRank: 1, code: 'MANIFEST_JSON_INVALID', message: 'first' })
    issues.add({ phaseRank: 40, ruleRank: 1, code: 'MANIFEST_SEMANTIC_INVALID', path: 'a.js', message: 'middle' })
    issues.add({ phaseRank: 40, ruleRank: 1, code: 'MANIFEST_SEMANTIC_INVALID', path: 'a.js', message: 'ignored duplicate' })

    expect(issues.finish()).toEqual({
      truncated: false,
      issues: [
        { code: 'MANIFEST_JSON_INVALID', message: 'first' },
        { code: 'MANIFEST_SEMANTIC_INVALID', path: 'a.js', message: 'middle' },
        { code: 'MANIFEST_SEMANTIC_INVALID', path: 'z.js', message: 'later' },
      ],
    })
  })

  it('retains only the smallest 100 unique issues and marks truncation', () => {
    const issues = new IssueCollector()
    for (let index = 149; index >= 0; index -= 1) {
      issues.add({
        phaseRank: 80,
        ruleRank: index,
        code: 'CSS_REFERENCE_INVALID',
        path: `dist/${String(index).padStart(3, '0')}.css`,
        message: `issue ${index}`,
      })
    }

    const result = issues.finish()
    expect(result.truncated).toBe(true)
    expect(result.issues).toHaveLength(100)
    expect(result.issues[0]?.path).toBe('dist/000.css')
    expect(result.issues.at(-1)?.path).toBe('dist/099.css')
  })

  it('orders discriminated locations deterministically', () => {
    const issues = new IssueCollector()
    issues.add({ phaseRank: 30, ruleRank: 1, code: 'MANIFEST_SCHEMA_INVALID', location: { kind: 'name', value: 'required' }, message: 'name' })
    issues.add({ phaseRank: 30, ruleRank: 1, code: 'MANIFEST_SCHEMA_INVALID', location: { kind: 'byteOffset', value: '9' }, message: 'byte' })
    issues.add({ phaseRank: 30, ruleRank: 1, code: 'MANIFEST_SCHEMA_INVALID', location: { kind: 'jsonPointer', value: '/a' }, message: 'pointer' })

    expect(issues.finish().issues.map((issue) => issue.message)).toEqual(['pointer', 'byte', 'name'])
  })

  it('orders canonical paths before issue and rule ranks within a phase', () => {
    const issues = new IssueCollector()
    issues.add({ phaseRank: 50, ruleRank: 1, code: 'PLATFORM_INCOMPATIBLE', path: 'z.js', message: 'z' })
    issues.add({ phaseRank: 50, ruleRank: 99, code: 'API_INCOMPATIBLE', path: 'a.js', message: 'a' })

    expect(issues.finish().issues.map((issue) => issue.path)).toEqual(['a.js', 'z.js'])
  })
})
