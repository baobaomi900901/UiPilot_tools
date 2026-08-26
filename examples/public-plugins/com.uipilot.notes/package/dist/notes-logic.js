export const STORAGE_KEY = 'notes.entries'

export function createId() {
  if (globalThis.crypto?.randomUUID) {
    return globalThis.crypto.randomUUID()
  }
  return `note-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

export function sortNotes(items) {
  return [...items].sort((left, right) => right.createdAt.localeCompare(left.createdAt))
}

export function normalizeNotes(value) {
  if (!Array.isArray(value)) {
    return []
  }
  return sortNotes(
    value
      .filter((item) => item && typeof item === 'object')
      .map((item) => {
        const legacyCreatedAt =
          typeof item.createdAt === 'string'
            ? item.createdAt
            : typeof item.updatedAt === 'string'
              ? item.updatedAt
              : new Date().toISOString()
        return {
          id: typeof item.id === 'string' ? item.id : createId(),
          title: typeof item.title === 'string' ? item.title : '',
          content: typeof item.content === 'string' ? item.content : '',
          createdAt: legacyCreatedAt,
        }
      })
      .filter((item) => item.title.trim().length > 0),
  )
}

export function filterNotes(items, queryText) {
  const query = String(queryText ?? '')
    .trim()
    .toLowerCase()
  if (!query) {
    return items
  }
  return items.filter((note) => {
    return (
      note.title.toLowerCase().includes(query) || note.content.toLowerCase().includes(query)
    )
  })
}
