function showToast(message, type = 'info') {
    const container = getDOMElement('toast-container');
    if (!container) return;
    const toast = document.createElement('div');
    toast.className = `toast ${type}`;
    toast.textContent = message;
    container.appendChild(toast);
    setTimeout(() => {
        toast.style.opacity = '0';
        toast.style.transform = 'translateY(-100%) scale(0.9)';
        toast.style.transition = 'all 0.3s ease';
        setTimeout(() => {
            toast.remove();
            if (container.children.length === 0) domCache.delete('toast-container');
        }, 300);
    }, 3000);
}

function showModal(id) {
    var modal = getDOMElement(id);
    if (!modal) {
        console.error('Modal not found:', id);
        return;
    }

    if (modal.parentElement !== document.body) {
        document.body.appendChild(modal);
    }

    modal.setAttribute('role', 'dialog');
    modal.setAttribute('aria-modal', 'true');
    modal.setAttribute('data-state', 'open');

    modal.dataset.previouslyFocused = document.activeElement ? (document.activeElement.id || '') : '';

    modal.style.display = 'flex';
    requestAnimationFrame(function () {
        modal.classList.add('modal-visible');
        modal.classList.remove('modal-exiting');
    });

    requestAnimationFrame(function () {
        var closeBtn = modal.querySelector('.modal-close');
        if (closeBtn) {
            closeBtn.focus();
        }
    });

    var onKeyDown = function (e) {
        if (e.key === 'Escape') {
            hideModal(id);
        }
    };
    modal.addEventListener('keydown', onKeyDown);
    modal._escCleanup = function () { modal.removeEventListener('keydown', onKeyDown); };

    if (!modal.dataset.noCloseOnBackdrop) {
        var onBackdrop = function (e) {
            if (e.target === modal) {
                hideModal(id);
            }
        };
        modal.addEventListener('click', onBackdrop);
        modal._backdropCleanup = function () { modal.removeEventListener('click', onBackdrop); };
    }
}

function hideModal(id) {
    var modal = getDOMElement(id);
    if (!modal) return;

    modal.setAttribute('data-state', 'closed');
    modal.classList.add('modal-exiting');
    modal.classList.remove('modal-visible');

    if (typeof modal._escCleanup === 'function') {
        modal._escCleanup();
        modal._escCleanup = null;
    }
    if (typeof modal._backdropCleanup === 'function') {
        modal._backdropCleanup();
        modal._backdropCleanup = null;
    }

    setTimeout(function () {
        var prevId = modal.dataset.previouslyFocused;
        if (prevId) {
            var prevEl = document.getElementById(prevId);
            if (prevEl) {
                try { prevEl.focus(); } catch (e) {}
            }
        }
        modal.classList.remove('modal-exiting');
        modal.style.display = 'none';
    }, 200);
}

function showConfirmDialog(title, message, confirmText, cancelText) {
    return new Promise((resolve) => {
        const overlay = document.createElement('div');
        overlay.className = 'modal';
        overlay.setAttribute('role', 'dialog');
        overlay.setAttribute('aria-modal', 'true');
        overlay.setAttribute('aria-labelledby', 'confirm-dialog-title');

        overlay.innerHTML = `
            <div class="modal-content" style="width:440px;min-height:auto;">
                <div class="modal-header">
                    <h3 id="confirm-dialog-title">${escapeHtml(title || '确认')}</h3>
                    <button class="modal-close confirm-cancel" aria-label="关闭对话框">&times;</button>
                </div>
                <div class="modal-body">
                    <p style="margin:0;color:var(--text-secondary);font-size:14px;line-height:1.6;">${message || ''}</p>
                </div>
                <div class="modal-footer">
                    <button class="modal-btn modal-btn--secondary confirm-cancel">${cancelText || '取消'}</button>
                    <button class="modal-btn modal-btn--danger confirm-ok">${confirmText || '确定'}</button>
                </div>
            </div>
        `;

        document.body.appendChild(overlay);
        
        // Show modal with animation
        requestAnimationFrame(() => overlay.classList.add('modal-visible'));

        var close = function (result) {
            overlay.setAttribute('data-state', 'closed');
            overlay.classList.add('modal-exiting');
            overlay.classList.remove('modal-visible');

            setTimeout(function () {
                if (overlay.parentElement) {
                    overlay.parentElement.removeChild(overlay);
                }
                resolve(result);
            }, 200);
        };

        // Close on cancel buttons
        overlay.querySelectorAll('.confirm-cancel').forEach(btn => {
            btn.addEventListener('click', () => close(false));
        });
        
        // Confirm action
        overlay.querySelector('.confirm-ok').addEventListener('click', () => close(true));
        
        // Close on backdrop click
        overlay.addEventListener('click', (e) => {
            if (e.target === overlay) close(false);
        });
        
        // Close on ESC key
        overlay.addEventListener('keydown', (e) => {
            if (e.key === 'Escape') close(false);
        });
    });
}

function formatNumber(num) {
    if (num >= 1000000) return (num / 1000000).toFixed(1) + 'M';
    if (num >= 1000) return (num / 1000).toFixed(1) + 'K';
    return String(num);
}




function escapeOnclick(str) {
    return str
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/\\/g, '\\\\')
        .replace(/'/g, "\\'");
}



function generateColorAvatar(username, size) {
    size = size || 64;
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    let hash = 0;
    for (let i = 0; i < username.length; i++) {
        hash = username.charCodeAt(i) + ((hash << 5) - hash);
    }
    const hue = Math.abs(hash) % 360;
    ctx.fillStyle = 'hsl(' + hue + ', 55%, 50%)';
    ctx.beginPath();
    ctx.arc(size / 2, size / 2, size / 2, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = '#ffffff';
    ctx.font = 'bold ' + Math.floor(size * 0.45) + 'px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(username.charAt(0).toUpperCase(), size / 2, size / 2);
    return canvas.toDataURL('image/png');
}

const VERSION_TYPE_LABELS = { release: '正式版', snapshot: '快照版', special: '愚人节版', old_beta: '旧测试版', old_alpha: '旧内测版', '(old)': '旧版' };
function getVersionTypeLabel(v) {
    const type = v.type || 'release';
    let label = VERSION_TYPE_LABELS[type] || type;
    if (v.complianceLevel === 0) label = '未混淆';
    return label;
}

const DL_FOLDER_KEY = 'versepc_dl_folders';
function getRememberedFolder(key) {
    try { const d = JSON.parse(localStorage.getItem(DL_FOLDER_KEY) || '{}'); return d[key] || ''; } catch (e) { return ''; }
}
function saveRememberedFolder(key, folderPath) {
    try { const d = JSON.parse(localStorage.getItem(DL_FOLDER_KEY) || '{}'); d[key] = folderPath; localStorage.setItem(DL_FOLDER_KEY, JSON.stringify(d)); } catch (e) {}
}
