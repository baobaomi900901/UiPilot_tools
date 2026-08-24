import {
  PLUGIN_VALIDATION_ISSUE_CODES,
  type PluginIssueLocation,
  type PluginValidationIssue,
  type PluginValidationIssueCode,
} from './contracts.js'

export interface OrderedIssue extends PluginValidationIssue {
  phaseRank: number
  ruleRank: number
}

interface StoredIssue {
  issue: PluginValidationIssue
  key: readonly [number, string, number, number, number, string]
}

const codeRanks = new Map<PluginValidationIssueCode, number>(
  PLUGIN_VALIDATION_ISSUE_CODES.map((code, index) => [code, index]),
)

function locationRank(location: PluginIssueLocation | undefined): number {
  if (!location) return 0
  if (location.kind === 'jsonPointer') return 1
  if (location.kind === 'byteOffset') return 2
  return 3
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, 'utf8'), Buffer.from(right, 'utf8'))
}

function compareStored(left: StoredIssue, right: StoredIssue): number {
  const phaseOrder = left.key[0] - right.key[0]
  if (phaseOrder !== 0) return phaseOrder
  const pathOrder = compareUtf8(left.key[1], right.key[1])
  if (pathOrder !== 0) return pathOrder
  for (const index of [2, 3, 4] as const) {
    const difference = left.key[index] - right.key[index]
    if (difference !== 0) return difference
  }
  return compareUtf8(left.key[5], right.key[5])
}

function sameKey(left: StoredIssue, right: StoredIssue): boolean {
  return left.key.every((value, index) => value === right.key[index])
}

function store(issue: OrderedIssue): StoredIssue {
  const { phaseRank, ruleRank, ...publicIssue } = issue
  return {
    issue: publicIssue,
    key: [
      phaseRank,
      issue.path ?? '',
      codeRanks.get(issue.code) ?? Number.MAX_SAFE_INTEGER,
      ruleRank,
      locationRank(issue.location),
      issue.location?.value ?? '',
    ],
  }
}

export class IssueCollector {
  readonly #limit: number
  readonly #items: StoredIssue[] = []
  #truncated = false

  constructor(limit = 100) {
    this.#limit = limit
  }

  add(issue: OrderedIssue): void {
    const candidate = store(issue)
    if (this.#items.some((item) => sameKey(item, candidate))) return

    this.#items.push(candidate)
    this.#items.sort(compareStored)
    if (this.#items.length > this.#limit) {
      this.#items.pop()
      this.#truncated = true
    }
  }

  finish(): { truncated: boolean; issues: PluginValidationIssue[] } {
    return {
      truncated: this.#truncated,
      issues: this.#items.map(({ issue }) => issue),
    }
  }
}
