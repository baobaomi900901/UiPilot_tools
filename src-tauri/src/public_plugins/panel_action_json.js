// Captured before importing plugin code. Never call getters or toJSON.
const panelJson = (() => {
  const descriptorsOf = Object.getOwnPropertyDescriptors;
  const prototypeOf = Object.getPrototypeOf;
  const setPrototype = Object.setPrototypeOf;
  const prototype = Object.prototype;
  const keysOf = Reflect.ownKeys;
  const has = Function.prototype.call.bind(Object.prototype.hasOwnProperty);
  const freeze = Object.freeze;
  const create = Object.create;
  const isArray = Array.isArray;
  const finite = Number.isFinite;
  const stringify = JSON.stringify;
  const ErrorValue = Error;
  const define = Object.defineProperty;
  const actionPattern = /^[a-z][a-z0-9.-]{0,63}$/;
  const test = Function.prototype.call.bind(RegExp.prototype.test);
  const StringValue = String;
  const WeakSetValue = WeakSet;
  const call = (method) => Function.prototype.call.bind(method);
  const setHasValue = call(WeakSet.prototype.has);
  const setAddValue = call(WeakSet.prototype.add);
  const setDeleteValue = call(WeakSet.prototype.delete);
  const fieldsEvery = call(Array.prototype.every);
  const encode = Function.prototype.call.bind(TextEncoder.prototype.encode, new TextEncoder());
  const bad = () => { throw new ErrorValue('invalid panel JSON'); };
  const snapshot = (input, fields, serialize = true) => {
    let count = 0;
    const ancestors = new WeakSetValue();
    const copy = (value, depth) => {
      if (++count > 4096 || depth > 32) bad();
      if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
      if (typeof value === 'number') { if (!finite(value)) bad(); return value; }
      if (typeof value !== 'object' || setHasValue(ancestors, value)) bad();
      setAddValue(ancestors, value);
      try {
        const array = isArray(value);
        if (!array && prototypeOf(value) !== prototype && prototypeOf(value) !== null) bad();
        const descriptors = descriptorsOf(value);
        const output = array ? [] : create(null);
        if (array && serialize) setPrototype(output, null);
        const keys = keysOf(descriptors);
        if (array) {
          const length = descriptors.length.value;
          if (keys.length !== length + 1 || length > 4096) bad();
          for (let i = 0; i < length; i++) {
            const d = descriptors[StringValue(i)];
            if (!d || !d.enumerable || !has(d, 'value')) bad();
            output[i] = copy(d.value, depth + 1);
          }
        } else {
          for (const key of keys) {
            const d = descriptors[key];
            if (typeof key !== 'string' || key === '__proto__' || key === 'prototype' || key === 'constructor' || !d.enumerable || !has(d, 'value')) bad();
            output[key] = copy(d.value, depth + 1);
          }
        }
        return freeze(output);
      } finally { setDeleteValue(ancestors, value); }
    };
    const result = copy(input, 0);
    if (!result || isArray(result) || typeof result !== 'object' || keysOf(result).length !== fields.length || !fieldsEvery(fields, k => has(result, k))) bad();
    if (serialize && encode(stringify(result)).byteLength > 65536) bad();
    return result;
  };
  const validAction = value => typeof value === 'string' && test(actionPattern, value)
    && value !== '__proto__' && value !== 'prototype' && value !== 'constructor';
  const error = name => {
    const value = new ErrorValue(name);
    define(value, 'name', { value: name });
    return value;
  };
  const errorName = value => {
    switch (value) {
      case 'InvalidPanelRequestError': case 'PanelRequestBusyError': case 'ExpiredPanelSession':
      case 'PanelRequestCancelledError': case 'PanelRequestTimeoutError': case 'InvalidPanelResponseError':
      case 'PanelRequestFailedError': case 'PanelRequestUnavailableError': return value;
      default: return 'PanelRequestUnavailableError';
    }
  };
  const windowErrorName = value => {
    switch (value) {
      case 'InvalidWindowRequestError': case 'WindowRequestBusyError': case 'ExpiredWindowSessionError':
      case 'WindowRequestCancelledError': case 'WindowRequestTimeoutError': case 'InvalidWindowResponseError':
      case 'WindowRequestFailedError': case 'WindowRequestUnavailableError': return value;
      default: return 'WindowRequestUnavailableError';
    }
  };
  const reply = data => {
    const envelope = create(null);
    envelope.requestId = 'reply';
    envelope.data = data;
    return snapshot(envelope, ['requestId', 'data'], false).data;
  };
  return freeze({ snapshot, reply, validAction, error, errorName, windowErrorName });
})();
