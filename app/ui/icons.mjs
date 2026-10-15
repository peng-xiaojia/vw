// Lucide SVG assets (ISC / MIT), see icons/LICENSE.txt.
export function icon(name) {
  const image = document.createElement('img');
  image.src = `icons/${name}.svg`; image.alt = ''; image.setAttribute('aria-hidden', 'true'); image.className = 'ui-icon';
  return image;
}
export function fileIcon(entry, expanded = false) {
  if (entry.isDir) return expanded ? 'folder-open' : 'folder';
  const ext = entry.name.split('.').pop().toLowerCase();
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'].includes(ext)) return 'image';
  if (['json', 'jsonl', 'toml', 'xml', 'js', 'ts', 'rs', 'py', 'html', 'css', 'sh', 'bash', 'bat', 'cmd', 'ps1', 'mjs', 'cjs', 'tsx', 'jsx', 'sql', 'yaml', 'yml'].includes(ext)) return 'file-code';
  if (['txt', 'md', 'markdown', 'log', 'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'csv', 'tsv', 'ini', 'cfg'].includes(ext)) return 'file-text';
  return 'file';
}
