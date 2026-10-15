export function menuStates(state) {
  const doc = state.doc;
  return Object.entries({
    file: !state.saving, folder: !state.saving,
    save: Boolean(doc?.editable && state.dirty && !state.saving && !state.opening),
    export: Boolean(doc && doc.viewerId !== 'dir' && !state.exporting && !state.saving && !state.opening),
    close: Boolean(doc && !state.saving), cancel: state.opening,
    edit: Boolean(doc?.editable && !state.editing && !state.saving && !state.opening),
    preview: Boolean(doc && state.editing && !state.saving), search: Boolean(doc && !state.saving),
    format: Boolean(doc?.editable && doc?.format && !state.saving && !state.opening && !state.formatting),
    replace: Boolean(doc?.editable && !state.saving && !state.opening),
    binary: Boolean(doc && doc.viewerId !== 'dir' && doc.viewerId !== 'hex' && !state.saving),
    normal: Boolean(doc && !state.saving), text: Boolean(doc && doc.viewerId !== 'dir' && !state.saving),
  });
}
