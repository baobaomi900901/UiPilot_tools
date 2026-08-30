const bridge = window.uipilotPluginPanel;
const status = document.querySelector('#status');
const entries = document.querySelector('#entries');
let latestSnapshot = null;

function render(snapshot) {
  latestSnapshot = snapshot;
  status.textContent = `revision=${snapshot.revision}; entries=${snapshot.entries.length}`;
  entries.replaceChildren(...snapshot.entries.map((entry) => {
    const item = document.createElement('li');
    item.dataset.kind = entry.kind;
    item.dataset.id = entry.id;
    item.textContent = entry.kind === 'text'
      ? entry.textPreview
      : entry.kind === 'image'
        ? `${entry.width}x${entry.height}`
        : `${entry.firstFileName} (${entry.fileCount}) ${entry.available ? 'available' : 'missing'}`;
    return item;
  }));
}

bridge.onUpdate(() => undefined);
bridge.onHostKey(async (event) => {
  if (event.key !== 'Enter' || !latestSnapshot?.entries.length) return;
  try {
    await bridge.clipboardHistory.paste({
      id: latestSnapshot.entries[0].id,
      routeSequence: event.routeSequence,
    });
  } catch (error) {
    status.textContent = error instanceof Error ? error.name : 'UnknownPasteError';
  }
});

bridge.clipboardHistory.onChanged(render);
render(await bridge.clipboardHistory.list());
