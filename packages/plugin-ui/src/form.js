// Native controls and business values remain owned by the caller.
export function createForm({ fields }) {
  let records = new Map(), disposed = false
  const tokens = (control) => (control.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(Boolean)
  function clearField(record) {
    if (!record.ownsError) return
    const { control, error } = record.field
    error.textContent = ''
    if (record.previousInvalid === null) control.removeAttribute('aria-invalid')
    else control.setAttribute('aria-invalid', record.previousInvalid)
    if (record.addedDescription) {
      const remaining = tokens(control).filter((id) => id !== error.id)
      if (remaining.length) control.setAttribute('aria-describedby', remaining.join(' '))
      else control.removeAttribute('aria-describedby')
    }
    record.ownsError = false
  }
  function setError(record, message) {
    clearField(record)
    if (!message) return
    const { control, error } = record.field
    record.previousInvalid = control.getAttribute('aria-invalid')
    const describedBy = tokens(control)
    record.addedDescription = !describedBy.includes(error.id)
    if (record.addedDescription) control.setAttribute('aria-describedby', [...describedBy, error.id].join(' '))
    control.setAttribute('aria-invalid', 'true')
    error.textContent = message
    record.ownsError = true
  }
  function release(record) {
    clearField(record)
    record.field.control.removeEventListener('input', record.edit)
    record.field.control.removeEventListener('change', record.edit)
  }
  function setFields(nextFields) {
    if (disposed) return
    const next = new Map()
    for (const field of nextFields) {
      let record = records.get(field.id)
      if (record && (record.field.control !== field.control || record.field.error !== field.error)) { release(record); record = null }
      if (!record) {
        record = { field, ownsError: false }
        record.edit = () => { if (!disposed) clearField(record) }
        field.control.addEventListener('input', record.edit)
        field.control.addEventListener('change', record.edit)
      }
      record.field = field
      next.set(field.id, record)
    }
    for (const [id, record] of records) if (!next.has(id)) release(record)
    records = next
  }
  function selected(ids) { return ids === undefined ? [...records.values()] : [...ids].map((id) => records.get(id)).filter(Boolean) }
  setFields(fields)
  return {
    setFields,
    validate(ids) {
      if (disposed) return true
      let firstInvalid = null
      for (const record of selected(ids)) {
        const message = record.field.validate?.(record.field.control) ?? null
        setError(record, message)
        if (message && !firstInvalid) firstInvalid = record.field.control
      }
      if (firstInvalid?.ownerDocument.hasFocus()) firstInvalid.focus()
      return firstInvalid === null
    },
    setErrors(errors) {
      if (disposed) return
      for (const [id, message] of errors) { const record = records.get(id); if (record) setError(record, message) }
    },
    clear(ids) { if (!disposed) for (const record of selected(ids)) clearField(record) },
    dispose() {
      if (disposed) return
      for (const record of records.values()) release(record)
      records.clear()
      disposed = true
    },
  }
}
