export class AssociationDialog {
  constructor({ invoke }) { this.invoke = invoke; this.busy = false; this.registration = 'none'; }
  get(id) { return document.getElementById(id); }
  status(message) { this.get('association-status').textContent = message; }
  setBusy(busy) {
    this.busy = busy;
    for (const button of this.get('association-dialog').querySelectorAll('button')) button.disabled = busy;
    for (const id of ['btn-association-save', 'btn-association-all', 'btn-association-none']) {
      this.get(id).disabled = busy || this.registration === 'other' || this.registration === 'error';
    }
  }
  render(state) {
    this.registration = state.registration;
    const container = this.get('association-formats'); container.replaceChildren();
    const groups = new Map();
    for (const format of state.formats) {
      let fieldset = groups.get(format.group);
      if (!fieldset) {
        fieldset = document.createElement('fieldset');
        const legend = document.createElement('legend'); legend.textContent = format.group;
        fieldset.append(legend); container.append(fieldset); groups.set(format.group, fieldset);
      }
      const label = document.createElement('label');
      const checkbox = document.createElement('input'); checkbox.type = 'checkbox'; checkbox.value = format.extension;
      checkbox.checked = state.selected.includes(format.extension); checkbox.disabled = state.registration === 'other';
      label.append(checkbox, document.createTextNode(`.${format.extension}`)); fieldset.append(label);
    }
    this.get('btn-association-save').disabled = state.registration === 'other';
    for (const id of ['btn-association-all', 'btn-association-none']) this.get(id).disabled = state.registration === 'other';
    this.status(state.registration === 'other' ? '另一份 vw 已注册“打开方式”。请先移除旧版本的关联。' : state.selected.length ? `当前已注册 ${state.selected.length} 种格式。` : '当前未注册文件格式。');
  }
  async show() {
    const dialog = this.get('association-dialog'); if (dialog.open) return;
    dialog.showModal(); this.setBusy(true); this.get('association-formats').replaceChildren(); this.status('正在读取关联格式…');
    try { this.render(await this.invoke('vw_association_state')); }
    catch (error) { this.registration = 'error'; this.status(`读取失败：${error.message || error}`); }
    finally { this.setBusy(false); }
  }
  async save() {
    if (this.busy || this.registration === 'other') return;
    const selected = [...this.get('association-formats').querySelectorAll('input:checked')].map(input => input.value);
    this.setBusy(true); this.status('正在保存…');
    try { this.render(await this.invoke('vw_association_apply', { selected })); this.status(`已保存 ${selected.length} 种格式。默认打开程序请在 Windows 中指定。`); }
    catch (error) { this.status(`保存失败：${error.message || error}`); }
    finally { this.setBusy(false); }
  }
  wire() {
    this.get('btn-association-cancel').addEventListener('click', () => this.get('association-dialog').close());
    this.get('btn-association-save').addEventListener('click', () => this.save());
    this.get('btn-association-all').addEventListener('click', () => { for (const input of this.get('association-formats').querySelectorAll('input')) input.checked = true; });
    this.get('btn-association-none').addEventListener('click', () => { for (const input of this.get('association-formats').querySelectorAll('input')) input.checked = false; });
    this.get('btn-association-defaults').addEventListener('click', async () => {
      try { await this.invoke('vw_association_defaults'); }
      catch (error) { this.status(`无法打开系统设置：${error.message || error}`); }
    });
  }
}
