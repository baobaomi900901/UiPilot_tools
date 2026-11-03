import {
  CANONICAL_COMBINING_CLASSES,
  CANONICAL_COMPOSITIONS,
  CANONICAL_DECOMPOSITIONS,
  FULL_CASE_FOLDS,
} from './generated/unicode-data.js'

export const FOLD_ALGORITHM_ID = 'uipilot-unicode-15.1-full-fold-nfc-v1'

const decompositions = new Map(CANONICAL_DECOMPOSITIONS)
const combiningClasses = new Map(CANONICAL_COMBINING_CLASSES)
const folds = new Map(FULL_CASE_FOLDS)
const compositions = new Map(
  CANONICAL_COMPOSITIONS.map(([first, second, result]) => [first * 0x110000 + second, result]),
)

const S_BASE = 0xac00
const L_BASE = 0x1100
const V_BASE = 0x1161
const T_BASE = 0x11a7
const L_COUNT = 19
const V_COUNT = 21
const T_COUNT = 28
const N_COUNT = V_COUNT * T_COUNT
const S_COUNT = L_COUNT * N_COUNT

function decomposeHangul(codePoint: number): readonly number[] | undefined {
  const syllableIndex = codePoint - S_BASE
  if (syllableIndex < 0 || syllableIndex >= S_COUNT) return undefined
  const leading = L_BASE + Math.floor(syllableIndex / N_COUNT)
  const vowel = V_BASE + Math.floor((syllableIndex % N_COUNT) / T_COUNT)
  const trailingIndex = syllableIndex % T_COUNT
  return trailingIndex === 0 ? [leading, vowel] : [leading, vowel, T_BASE + trailingIndex]
}

function appendDecomposition(codePoint: number, output: number[]): void {
  const mapping = decomposeHangul(codePoint) ?? decompositions.get(codePoint)
  if (!mapping) {
    output.push(codePoint)
    return
  }
  for (const mapped of mapping) appendDecomposition(mapped, output)
}

function canonicalOrder(codePoints: readonly number[]): number[] {
  const output: number[] = []
  for (const codePoint of codePoints) {
    const currentClass = combiningClasses.get(codePoint) ?? 0
    let insertion = output.length
    if (currentClass !== 0) {
      while (insertion > 0) {
        const previousClass = combiningClasses.get(output[insertion - 1]) ?? 0
        if (previousClass === 0 || previousClass <= currentClass) break
        insertion -= 1
      }
    }
    output.splice(insertion, 0, codePoint)
  }
  return output
}

function composeHangul(first: number, second: number): number | undefined {
  const leadingIndex = first - L_BASE
  if (leadingIndex >= 0 && leadingIndex < L_COUNT) {
    const vowelIndex = second - V_BASE
    if (vowelIndex >= 0 && vowelIndex < V_COUNT) {
      return S_BASE + (leadingIndex * V_COUNT + vowelIndex) * T_COUNT
    }
  }
  const syllableIndex = first - S_BASE
  const trailingIndex = second - T_BASE
  if (
    syllableIndex >= 0 &&
    syllableIndex < S_COUNT &&
    syllableIndex % T_COUNT === 0 &&
    trailingIndex > 0 &&
    trailingIndex < T_COUNT
  ) {
    return first + trailingIndex
  }
  return undefined
}

function compose(first: number, second: number): number | undefined {
  return composeHangul(first, second) ?? compositions.get(first * 0x110000 + second)
}

export function normalizeNfc(value: string): string {
  const decomposed: number[] = []
  for (const character of value) appendDecomposition(character.codePointAt(0)!, decomposed)
  const ordered = canonicalOrder(decomposed)
  if (ordered.length === 0) return ''

  const output = [ordered[0]]
  let starterIndex = 0
  let starter = ordered[0]
  let lastClass = 0
  for (const codePoint of ordered.slice(1)) {
    const currentClass = combiningClasses.get(codePoint) ?? 0
    const composite = compose(starter, codePoint)
    if (composite !== undefined && (lastClass < currentClass || lastClass === 0)) {
      output[starterIndex] = composite
      starter = composite
      continue
    }
    if (currentClass === 0) {
      starterIndex = output.length
      starter = codePoint
    }
    lastClass = currentClass
    output.push(codePoint)
  }
  return String.fromCodePoint(...output)
}

export function foldName(value: string): string {
  const folded: number[] = []
  for (const character of normalizeNfc(value)) {
    const codePoint = character.codePointAt(0)!
    folded.push(...(folds.get(codePoint) ?? [codePoint]))
  }
  return normalizeNfc(String.fromCodePoint(...folded))
}
