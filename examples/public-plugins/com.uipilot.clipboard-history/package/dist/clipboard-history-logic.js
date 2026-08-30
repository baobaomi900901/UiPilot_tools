export function compareRevisions(left, right) {
  if (left.length !== right.length) {
    return left.length < right.length ? -1 : 1
  }
  if (left === right) return 0
  return left < right ? -1 : 1
}

export const FILTERS = Object.freeze(['all', 'image', 'files', 'text'])

export function shouldApplySnapshot(currentRevision, nextRevision) {
  return currentRevision === null || compareRevisions(currentRevision, nextRevision) < 0
}

export function filterEntries(entries, filter) {
  if (filter === 'all') return [...entries]
  return entries.filter((entry) => entry.kind === filter)
}

export function cycleFilter(current, direction) {
  const currentIndex = Math.max(0, FILTERS.indexOf(current))
  const offset = direction < 0 ? -1 : 1
  return FILTERS[(currentIndex + offset + FILTERS.length) % FILTERS.length]
}

export function reconcileSelection(entries, selectedId) {
  if (entries.length === 0) return null
  return entries.some((entry) => entry.id === selectedId) ? selectedId : entries[0].id
}

export function moveSelection(entries, selectedId, direction) {
  if (entries.length === 0) return null
  const currentIndex = entries.findIndex((entry) => entry.id === selectedId)
  if (currentIndex < 0) return entries[0].id
  const nextIndex = Math.max(0, Math.min(entries.length - 1, currentIndex + Math.sign(direction)))
  return entries[nextIndex].id
}
