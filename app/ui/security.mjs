export function externalUrl(value) {
  if (typeof value !== 'string' || /[\u0000-\u0020\\]/u.test(value)) return null;
  if (!/^(https?:\/\/|mailto:)/iu.test(value)) return null;
  try {
    const url = new URL(value);
    if (!['http:', 'https:', 'mailto:'].includes(url.protocol)) return null;
    if (url.protocol !== 'mailto:' && !url.hostname) return null;
    return url.href;
  } catch { return null; }
}

export function blobUrl(handle, id, platform = '') {
  if (typeof handle !== 'string' || !handle || typeof id !== 'string' || !id) return null;
  const origin = /win/iu.test(platform) ? 'http://vw-blob.localhost' : 'vw-blob://localhost';
  return `${origin}/${encodeURIComponent(handle)}/${encodeURIComponent(id)}`;
}
