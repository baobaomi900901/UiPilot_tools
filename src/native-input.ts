import type { ClassifiedTextRecord, ControlKey } from './protocol'

export function bindNativeTextInput(
  input: HTMLInputElement,
  control: ControlKey,
  emit: (record: ClassifiedTextRecord) => void,
): () => void {
  const composition = (kind: 'compositionStart' | 'compositionUpdate' | 'compositionEnd') => (event: Event) => {
    if (!event.isTrusted || !(event instanceof CompositionEvent) || event.target !== input) return
    emit({ kind, control, value: input.value })
  }
  const onStart = composition('compositionStart')
  const onUpdate = composition('compositionUpdate')
  const onEnd = composition('compositionEnd')
  const onInput = (event: Event) => {
    if (!event.isTrusted || !(event instanceof InputEvent) || event.target !== input) return
    if (event.inputType === 'insertCompositionText') {
      emit({ kind: 'compositionInput', control, value: input.value, inputType: 'insertCompositionText' })
    } else {
      emit({ kind: 'ordinaryInput', control, value: input.value, inputType: event.inputType })
    }
  }

  input.addEventListener('compositionstart', onStart)
  input.addEventListener('compositionupdate', onUpdate)
  input.addEventListener('compositionend', onEnd)
  input.addEventListener('input', onInput)

  let active = true
  return () => {
    if (!active) return
    active = false
    input.removeEventListener('compositionstart', onStart)
    input.removeEventListener('compositionupdate', onUpdate)
    input.removeEventListener('compositionend', onEnd)
    input.removeEventListener('input', onInput)
  }
}
