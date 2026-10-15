// Native file paths come from the WebView's OS drop target, not DOM File objects.
export async function bindNativeDrop(tauri, { onPaths, onHover, onError = () => {} }) {
  const webview = tauri.webview?.getCurrentWebview();
  if (!webview) throw new Error('无法获取当前 WebView');
  return webview.onDragDropEvent(async ({ payload }) => {
    onHover(payload.type === 'enter' || payload.type === 'over');
    if (payload.type === 'drop') {
      try { await onPaths(payload.paths); }
      catch (error) { onError(error); }
    }
  });
}
