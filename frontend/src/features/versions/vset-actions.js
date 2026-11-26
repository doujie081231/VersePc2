// vset-actions.js — data-action 注册
import { registerActions as registerActions } from '../../core/actions.js';
import { previewMod, toggleModInManager, removeModFromManager, applyAllModUpdates } from './vset-mods.js';
import { openModTranslateDialog, switchTranslateTab, startQuickTranslate, startAutoTranslate, exportModLangFile, importModLangFile } from './vset-translate.js';
import { hideModal } from '../misc/ui-components.js';

registerActions({
  closeOverlay: (event, el) => el.closest('.modal-overlay')?.remove(),
  toggleModInManager: (event, el, fileName, disable) => toggleModInManager(fileName, disable),
  previewMod: (event, el, projectId) => previewMod(projectId),
  openModTranslateDialog: (event, el, fileName, modName) => openModTranslateDialog(fileName, modName),
  removeModFromManager: (event, el, fileName) => removeModFromManager(fileName),
  applyAllModUpdates: (event, el) => applyAllModUpdates(el),
  switchTranslateTab: (event, el, tab) => switchTranslateTab(tab),
  startQuickTranslate,
  startAutoTranslate,
  exportModLangFile,
  importModLangFile,
  hideModal: (event, el, id) => hideModal(id),
});
