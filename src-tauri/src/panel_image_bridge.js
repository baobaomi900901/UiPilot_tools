// Host-owned image notices have a separate bounded channel from business JSON.
function createPanelImageBridge({ enabled, epoch, active, registered, invoke, json }) {
  let sessionEpoch = epoch(), receiver = null, pending = null, revision = null, ready = false, generation = 0;
  const u64 = value => typeof value === 'string' && /^(0|[1-9][0-9]*)$/.test(value)
    && (value.length < 20 || (value.length === 20 && value <= '18446744073709551615'));
  const newer = (left, right) => right === null || left.length > right.length || (left.length === right.length && left > right);
  const validImage = value => {
    if (value === null) return true;
    if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).sort().join(',') !== 'byteLength,height,id,mimeType,previewDataUrl,width'
      || typeof value.id !== 'string' || !value.id || new TextEncoder().encode(value.id).length > 512 || value.id.includes('\0')
      || value.mimeType !== 'image/png' || !Number.isInteger(value.width) || value.width < 1 || value.width > 4096
      || !Number.isInteger(value.height) || value.height < 1 || value.height > 4096
      || value.width * value.height > 16000000 || !Number.isInteger(value.byteLength) || value.byteLength < 1 || value.byteLength > 3 * 1024 * 1024
      || typeof value.previewDataUrl !== 'string' || !value.previewDataUrl.startsWith('data:image/png;base64,')) return false;
    const encoded = value.previewDataUrl.slice(22);
    return encoded.length > 0 && encoded.length % 4 === 0 && /^[A-Za-z0-9+/]*={0,2}$/.test(encoded)
      && encoded.length / 4 * 3 - (encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0) <= 40 * 1024;
  };
  const deliver = () => {
    if (!ready || !receiver || !pending || !active()) return;
    const event = pending, target = receiver, version = generation;
    queueMicrotask(() => {
      if (!active() || !ready || version !== generation || receiver !== target || pending !== event || epoch() !== event.sessionEpoch) return;
      pending = null;
      try { Promise.resolve(target(event)).catch(() => undefined); } catch (_) {}
    });
  };
  return {
    register(next) {
      if (!enabled || !active() || registered() || receiver || typeof next !== 'function') throw new TypeError('register one onImageChanged handler before onUpdate');
      receiver = next;
      return () => { if (receiver === next) receiver = null; };
    },
    receive(input) {
      if (!enabled || !active()) return;
      let event;
      try { event = json.snapshot(input, ['sessionEpoch', 'revision', 'image']); } catch (_) { return; }
      if (!u64(event.sessionEpoch) || event.sessionEpoch === '0' || event.sessionEpoch !== sessionEpoch || event.sessionEpoch !== epoch()
        || !u64(event.revision) || !newer(event.revision, revision) || !validImage(event.image)) return;
      revision = event.revision; pending = event; deliver();
    },
    prepare(nextEpoch) {
      ready = false; generation++;
      if (nextEpoch !== sessionEpoch) { sessionEpoch = nextEpoch; receiver = null; pending = null; revision = null; }
    },
    markReady() { ready = true; deliver(); },
    close() { ready = false; generation++; receiver = null; pending = null; },
    clear() {
      const captured = epoch();
      return async () => {
        const current = () => enabled && active() && captured === epoch();
        if (!current()) throw json.error('ExpiredPanelSession');
        try { await invoke('plugin_panel_clear_image', { sessionEpoch: captured }); }
        catch (error) {
          if (!current()) throw json.error('ExpiredPanelSession');
          throw json.error(json.errorName(error));
        }
        if (!current()) throw json.error('ExpiredPanelSession');
      };
    },
  };
}
