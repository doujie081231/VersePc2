/**
 * @file console-log.js
 * @description 游戏日志导出（启动失败提示与反馈页共用）
 */
async function exportGameLog() {
  try {
    const versionId = typeof currentSettingsVersionId !== 'undefined' ? currentSettingsVersionId
      : (typeof launchVersionCustomSelect !== 'undefined' && launchVersionCustomSelect ? launchVersionCustomSelect.getValue() : '');
    const url = `/api/game/log/export${versionId ? '?versionId=' + encodeURIComponent(versionId) : ''}`;
    const a = document.createElement('a');
    a.href = url;
    a.download = '';
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    if (typeof showToast === 'function') showToast('日志导出成功', 'success');
  } catch (e) {
    console.error('[ExportLog] 导出失败:', e);
    if (typeof showToast === 'function') showToast('导出日志失败: ' + e.message, 'error');
  }
}
