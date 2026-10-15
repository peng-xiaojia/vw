export class WorkspaceTree {
  constructor({ invoke, onChange = () => {} }) {
    this.invoke = invoke;
    this.onChange = onChange;
    this.generation = 0;
    this.state = { root: null, parent: null, nodes: new Map(), expanded: new Set(), busy: false, error: '' };
  }
  emit() { this.onChange(this.state); }
  async setRoot(path) {
    const generation = ++this.generation;
    this.state.busy = true;
    this.state.error = '';
    this.emit();
    try {
      const listing = await this.invoke('vw_list_dir', { path });
      if (generation !== this.generation) return false;
      Object.assign(this.state, {
        root: listing.path, parent: listing.parent ?? null,
        nodes: new Map([[listing.path, listing]]), expanded: new Set([listing.path]),
      });
      return true;
    } catch (error) {
      if (generation === this.generation) this.state.error = `无法打开文件夹：${error}`;
      return false;
    } finally {
      if (generation === this.generation) { this.state.busy = false; this.emit(); }
    }
  }
  async toggle(path) {
    if (this.state.expanded.has(path)) { this.state.expanded.delete(path); this.emit(); return true; }
    const generation = this.generation;
    const cached = this.state.nodes.get(path);
    if (cached?.loading) return false;
    if (!cached) {
      this.state.nodes.set(path, { path, entries: [], loading: true });
      this.emit();
      try {
        const listing = await this.invoke('vw_list_dir', { path });
        if (generation !== this.generation) return false;
        this.state.nodes.set(path, listing);
      } catch (error) {
        if (generation === this.generation) {
          this.state.nodes.delete(path);
          this.state.error = `无法读取文件夹：${error}`;
          this.emit();
        }
        return false;
      }
    }
    if (generation !== this.generation) return false;
    this.state.expanded.add(path);
    this.state.error = '';
    this.emit();
    return true;
  }
  refresh() { return this.state.root ? this.setRoot(this.state.root) : Promise.resolve(false); }
  parent() { return this.state.parent ? this.setRoot(this.state.parent) : Promise.resolve(false); }
}

export function displayPath(path) {
  const value = String(path ?? '');
  if (/^\\\\\?\\UNC\\/iu.test(value)) return `\\\\${value.slice(8)}`;
  if (/^\\\\\?\\/u.test(value)) return value.slice(4);
  return value;
}

export function parentPath(path) {
  const value = displayPath(path).replace(/[\\/]+$/u, '');
  const index = Math.max(value.lastIndexOf('/'), value.lastIndexOf('\\'));
  if (index < 0) return null;
  if (index === 0) return value.slice(0, 1);
  if (index === 2 && /^[a-z]:/iu.test(value)) return value.slice(0, 3);
  if (value.startsWith('\\\\') && index <= value.indexOf('\\', 2)) return null;
  return value.slice(0, index);
}

export function pathWithin(root, path) {
  const normalized = value => {
    const raw = displayPath(value).replace(/\//gu, '\\').toLowerCase();
    return raw === '\\' || /^[a-z]:\\$/u.test(raw) ? raw : raw.replace(/\\+$/u, '');
  };
  const base = normalized(root);
  const target = normalized(path);
  return Boolean(base && (target === base || target.startsWith(base.endsWith('\\') ? base : `${base}\\`)));
}
