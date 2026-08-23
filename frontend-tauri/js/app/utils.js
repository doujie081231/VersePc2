const domCache = new Map();
function getDOMElement(id) {
    if (domCache.has(id)) {
        const el = domCache.get(id);
        if (el.isConnected) return el;
        domCache.delete(id);
    }
    const el = document.getElementById(id);
    if (el) domCache.set(id, el);
    return el;
}
function clearDOMCache() { domCache.clear(); }

// 缓存常用 DOM 元素（在 init 结束时调用）
const commonElements = {};
function cacheCommonElements() {
    const ids = [
        'mod-filter-version', 'mod-filter-loader', 'mod-filter-search',
        'msauth-status-text', 'acc-start-btn', 'launch-error-msg',
        'status-indicator', 'status-text', 'launch-btn',
        'mod-multiselect-toggle', 'mod-filter-sort', 'mod-list'
    ];
    ids.forEach(id => {
        const el = document.getElementById(id);
        if (el) commonElements[id] = el;
    });
}
// 刷新单个缓存元素
function refreshElementCache(id) {
    const el = document.getElementById(id);
    if (el) commonElements[id] = el;
    else delete commonElements[id];
}

// 防抖函数
function debounce(fn, delay = 300) {
    let timer = null;
    return function(...args) {
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => {
            fn.apply(this, args);
            timer = null;
        }, delay);
    };
}

// 节流函数
function throttle(fn, limit = 100) {
    let inThrottle = false;
    return function(...args) {
        if (!inThrottle) {
            fn.apply(this, args);
            inThrottle = true;
            setTimeout(() => { inThrottle = false; }, limit);
        }
    };
}

// 定时器管理
const managedTimers = { intervals: new Map(), timeouts: new Map() };
function setManagedInterval(fn, delay, key) {
    if (managedTimers.intervals.has(key)) clearInterval(managedTimers.intervals.get(key));
    const id = setInterval(fn, delay);
    managedTimers.intervals.set(key, id);
    return id;
}
function clearManagedInterval(key) {
    if (managedTimers.intervals.has(key)) {
        clearInterval(managedTimers.intervals.get(key));
        managedTimers.intervals.delete(key);
    }
}
function clearAllManagedIntervals() {
    managedTimers.intervals.forEach(id => clearInterval(id));
    managedTimers.intervals.clear();
}
