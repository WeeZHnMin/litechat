const BOTTOM_THRESHOLD = 80;
const SETTLE_DELAYS = [60, 200, 600];
const SETTLE_WINDOW = 800;

function storageKey(conversationId) {
  return `litechat:scroll:${conversationId}`;
}

export function createScrollMemory(element) {
  let activeId = null;
  let pending = 0;
  let programmatic = false;
  let settling = false;
  let userScrolled = false;

  function read(conversationId) {
    try {
      const raw = localStorage.getItem(storageKey(conversationId));
      if (!raw) return null;
      const value = JSON.parse(raw);
      return Number.isFinite(value?.top) ? value : null;
    } catch {
      return null;
    }
  }

  function write(conversationId, value) {
    try {
      localStorage.setItem(storageKey(conversationId), JSON.stringify(value));
    } catch {
      // storage may be unavailable or full; losing a scroll position is harmless
    }
  }

  function isVisible() {
    return !element.hidden && element.offsetParent !== null;
  }

  function snapshot() {
    const top = element.scrollTop;
    const atBottom = element.scrollHeight - top - element.clientHeight <= BOTTOM_THRESHOLD;
    return { top, atBottom };
  }

  function capture(conversationId = activeId) {
    if (!conversationId) return;
    if (!isVisible()) return;
    write(conversationId, snapshot());
  }

  function apply(saved) {
    programmatic = true;
    element.scrollTo({
      top: saved.atBottom ? element.scrollHeight : saved.top,
      behavior: "instant",
    });
    requestAnimationFrame(() => {
      programmatic = false;
    });
  }

  element.addEventListener(
    "scroll",
    () => {
      if (programmatic) return;
      if (settling) userScrolled = true;
      if (pending) return;
      pending = requestAnimationFrame(() => {
        pending = 0;
        if (!settling) capture();
      });
    },
    { passive: true },
  );

  function restore(conversationId) {
    activeId = conversationId;
    if (!conversationId) return;
    const saved = read(conversationId);
    if (!saved) {
      element.scrollTop = element.scrollHeight;
      return;
    }
    userScrolled = false;
    settling = true;
    const reapply = () => {
      if (userScrolled) return;
      apply(saved);
    };
    requestAnimationFrame(() => {
      reapply();
      for (const delay of SETTLE_DELAYS) {
        window.setTimeout(reapply, delay);
      }
      window.setTimeout(() => {
        settling = false;
        if (!userScrolled) capture(conversationId);
      }, SETTLE_WINDOW);
    });
  }

  function reset() {
    capture();
    activeId = null;
    settling = false;
    userScrolled = false;
  }

  window.addEventListener("beforeunload", () => capture());

  return { capture, restore, reset };
}
