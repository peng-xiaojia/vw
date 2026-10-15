// Frontend half of the built-in text / HTML / Markdown format capability.
// app/build.rs copies this plugin asset into the frontend distribution.
export function prettierFormatter(text, { parser }) {
  if (text.length > 2 * 1024 * 1024) return Promise.reject(Error('格式化上限为 2 MiB，请缩小文件后重试'));
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('../format-worker.mjs', import.meta.url), { type: 'module' });
    const timeout = setTimeout(() => { worker.terminate(); reject(Error('格式化超时，原文未改变')); }, 15000);
    const finish = (error, result) => { clearTimeout(timeout); worker.terminate(); error ? reject(Error(error)) : resolve(result); };
    worker.onmessage = ({ data }) => finish(data.error, data.text);
    worker.onerror = event => finish(event.message || '格式化组件加载失败');
    worker.postMessage({ text, parser });
  });
}
