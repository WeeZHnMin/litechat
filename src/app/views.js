export class View {
  constructor({ name }) {
    this.name = name;
  }

  get isOpen() {
    return false;
  }

  activate() {}

  deactivate() {}

  handleKeyDown() {
    return false;
  }

  handleEscape() {
    return false;
  }
}

export class PanelView extends View {
  constructor({ name, manager, element, toggleButton = null, initialFocus = null }) {
    super({ name });
    this.manager = manager;
    this.element = element;
    this.toggleButton = toggleButton;
    this.initialFocus = initialFocus;
  }

  get isOpen() {
    return !this.element.hidden;
  }

  activate() {
    this.element.hidden = false;
    this.toggleButton?.setAttribute("aria-expanded", "true");
    // 面板里多为表单：聚焦第一个可交互元素会立刻画出焦点环，也容易让人以为
    // 该控件已被选中。只在面板本身可聚焦时把焦点交给它，方向键/滚动仍然可用。
    this.element.focus({ preventScroll: true });
  }

  deactivate() {
    this.element.hidden = true;
    this.toggleButton?.setAttribute("aria-expanded", "false");
  }

  handleEscape() {
    this.manager.close(this);
    return true;
  }
}

export class ChatView extends View {
  constructor({ name, onEscape }) {
    super({ name });
    this.onEscape = onEscape;
  }

  get isOpen() {
    return true;
  }

  handleEscape() {
    return this.onEscape() !== false;
  }
}

export class ViewManager {
  constructor({ conversation, composer }) {
    this.conversation = conversation;
    this.composer = composer;
    this.views = new Map();
    this.baseline = null;
    this.active = null;
  }

  register(view) {
    this.views.set(view.name, view);
    return this;
  }

  setBaseline(view) {
    this.baseline = view;
    return this;
  }

  open(view) {
    if (!view || this.active === view) return;
    if (this.active) this.active.deactivate();
    this.active = view;
    this.setChatVisible(false);
    view.activate();
  }

  close(view = this.active) {
    if (!view || view !== this.active) return;
    view.deactivate();
    this.active = null;
    this.setChatVisible(true);
    (view.toggleButton ?? this.composer).focus?.();
  }

  closeActive() {
    this.close(this.active);
  }

  toggle(view) {
    if (this.active === view) this.close(view);
    else this.open(view);
  }

  setChatVisible(visible) {
    this.conversation.hidden = !visible;
    this.composer.hidden = !visible;
  }

  handleKeyDown(event) {
    if (event.isComposing) return;
    if (document.querySelector(".image-lightbox")) return;
    if (this.active) {
      if (this.active.handleKeyDown(event)) return;
      if (event.key === "Escape") {
        event.preventDefault();
        this.active.handleEscape();
      }
      return;
    }
    if (event.key === "Escape" && this.baseline?.handleEscape(event)) {
      event.preventDefault();
    }
  }
}
