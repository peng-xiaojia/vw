// Common dispatch knows providers, never the syntax or formatting rules of a file.
const providers = new Map();
export function registerFormatter(name, formatter) {
  if (providers.has(name)) throw new Error(`重复的格式化器：${name}`);
  providers.set(name, formatter);
}
export async function formatDocument(doc, text, invoke) {
  const support = doc.format;
  if (!support) throw new Error('该格式插件没有提供格式化功能');
  if (support.provider === 'rpc') return invoke('vw_format', { handle: doc.handle, text });
  const format = providers.get(support.provider);
  if (!format) throw new Error('格式化器尚未安装');
  return format(text, support);
}
