// Captured-session clipboard and native-speech facades. No native error text crosses this boundary.
const panelEffects = (() => {
  const errors = new Set(['PermissionDenied', 'ExpiredPanelSession', 'InvalidPanelText', 'ClipboardWriteFailed', 'SpeechUnavailable', 'SpeechBusy', 'SpeechVoiceUnavailable', 'SpeechFailed']);
  let session = null;
  const inputSnapshot = (input, keys) => {
    try {
      if (!input || typeof input !== 'object' || Array.isArray(input)) throw new Error();
      const proto = Object.getPrototypeOf(input);
      if (proto !== null && Object.getPrototypeOf(proto) !== null) throw new Error();
      const descriptors = Object.getOwnPropertyDescriptors(input);
      if (Reflect.ownKeys(descriptors).length !== keys.length) throw new Error();
      const copy = Object.create(null);
      for (const key of keys) {
        const descriptor = descriptors[key];
        if (!descriptor || !descriptor.enumerable || !Object.hasOwn(descriptor, 'value') || typeof descriptor.value !== 'string') throw new Error();
        copy[key] = descriptor.value;
      }
      return panelJson.snapshot(copy, keys);
    } catch (_) { throw namedError('InvalidPanelText'); }
  };
  const text = value => {
    if (typeof value !== 'string' || !value.length || value.includes('\u0000') || new TextEncoder().encode(value).length > 16384) throw namedError('InvalidPanelText');
    return value;
  };
  const language = value => {
    if (value !== 'en' && value !== 'zh-CHS') throw namedError('InvalidPanelText');
    return value;
  };
  const state = value => {
    if (!value || typeof value !== 'object' || Object.keys(value).some(key => !['playbackId','status','language','errorCode'].includes(key)) ||
        !['idle','playing','completed','stopped','error'].includes(value.status) ||
        !(value.playbackId === null || typeof value.playbackId === 'string' && value.playbackId.length > 0) ||
        !(value.language === null || ['en','zh-CHS'].includes(value.language)) ||
        (value.errorCode !== undefined && !errors.has(value.errorCode))) throw namedError('SpeechFailed');
    return deepFreeze({ playbackId: value.playbackId, status: value.status, language: value.language, ...(value.errorCode === undefined ? {} : { errorCode: value.errorCode }) });
  };
  const reset = epoch => {
    if (session && session.epoch === epoch) return;
    if (session) { session.active = false; session.handlers.clear(); }
    const owner = { epoch, active: true, handlers: new Set() };
    const valid = () => { if (!owner.active || session !== owner || !invoke) throw namedError('ExpiredPanelSession'); };
    const call = async (command, input) => {
      valid();
      try {
        const result = await invoke(command, { sessionEpoch: epoch, ...(input === undefined ? {} : { input }) });
        valid();
        return result;
      } catch (error) {
        valid();
        const raw = error && typeof error === 'object' ? error.code || error.name : null;
        throw namedError(errors.has(raw) ? raw : command.includes('clipboard') ? 'ClipboardWriteFailed' : 'SpeechFailed');
      }
    };
    owner.clipboard = deepFreeze({ async writeText(input) {
      valid(); const value = inputSnapshot(input, ['text']); text(value.text);
      await call('plugin_panel_clipboard_write_text', value);
    } });
    owner.speech = deepFreeze({
      async voices() {
        const voices = await call('plugin_panel_speech_voices');
        if (!Array.isArray(voices) || voices.some(voice => !voice || Object.keys(voice).sort().join(',') !== 'id,language,name' || typeof voice.id !== 'string' || typeof voice.name !== 'string' || !['en','zh-CHS'].includes(voice.language))) throw namedError('SpeechFailed');
        return deepFreeze(voices.map(voice => ({ id: voice.id, name: voice.name, language: voice.language })));
      },
      async play(input) { valid(); const value = inputSnapshot(input, ['text','language']); text(value.text); language(value.language); return state(await call('plugin_panel_speech_play', value)); },
      async stop(input) { valid(); const value = inputSnapshot(input, ['playbackId']); if (typeof value.playbackId !== 'string' || !value.playbackId.length || value.playbackId.length > 128) throw namedError('InvalidPanelText'); return state(await call('plugin_panel_speech_stop', value)); },
      async state() { return state(await call('plugin_panel_speech_state')); },
      onState(handler) { valid(); if (typeof handler !== 'function') throw new TypeError('speech handler required'); owner.handlers.add(handler); return () => owner.handlers.delete(handler); },
    });
    session = owner;
  };
  Object.defineProperty(window, '__UIPILOT_PLUGIN_PANEL_SPEECH_STATE__', { configurable: false, value: event => {
    const owner = session;
    if (!owner || !owner.active || event?.sessionEpoch !== owner.epoch) return;
    let next; try { next = state(event.state); } catch (_) { return; }
    for (const handler of owner.handlers) { try { Promise.resolve(handler(next)).catch(() => undefined); } catch (_) {} }
  } });
  return { reset, clipboard: () => session.clipboard, speech: () => session.speech };
})();
