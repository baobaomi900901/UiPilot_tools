import type { ClassifiedTextRecord, ControlKey } from './protocol'

export function bindNativeTextInput(
  input: HTMLInputElement,
  control: ControlKey,
  emit: (record: ClassifiedTextRecord) => void,
): () => void {
  let compositionActive = false
  const onStart = (event: Event) => {
    if (!event.isTrusted || !(event instanceof CompositionEvent) || event.target !== input) return
    compositionActive = true
    emit({ kind: 'compositionStart', control })
  }
  const onEnd = (event: Event) => {
    if (!(event instanceof CompositionEvent) || event.target !== input || !compositionActive) return
    compositionActive = false
    emit({ kind: 'compositionBoundary', control })
  }
  const onInput = (event: Event) => {
    if (!event.isTrusted || !(event instanceof InputEvent) || event.target !== input) return
    if (event.isComposing) {
      if (compositionActive) emit({ kind: 'compositionInput', control, value: input.value, inputType: event.inputType })
      return
    }
    compositionActive = false
    emit({ kind: 'ordinaryInput', control, value: input.value, inputType: event.inputType })
  }

  input.addEventListener('compositionstart', onStart)
  input.addEventListener('input', onInput)
  input.addEventListener('compositionend', onEnd)

  let bound = true
  return () => {
    if (!bound) return
    bound = false
    compositionActive = false
    input.removeEventListener('compositionstart', onStart)
    input.removeEventListener('input', onInput)
    input.removeEventListener('compositionend', onEnd)
  }
}
