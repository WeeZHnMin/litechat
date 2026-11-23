export function eventAccelerator(event) {
  if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) return "";
  let key = "";
  if (/^Key[A-Z]$/.test(event.code)) key = event.code.slice(3);
  if (/^Digit[0-9]$/.test(event.code)) key = event.code.slice(5);
  if (/^F(?:[1-9]|1[0-9]|2[0-4])$/.test(event.code)) key = event.code;
  if (event.code === "Space") key = "Space";
  if (!key) return "";
  const modifiers = [];
  if (event.ctrlKey) modifiers.push("CmdOrControl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Super");
  if (!modifiers.length) return "";
  return [...modifiers, key].join("+");
}

export class ShortcutField {
  constructor({ action, element, accelerator, group }) {
    this.action = action;
    this.element = element;
    this.accelerator = accelerator;
    this.group = group;
    this.recording = false;
    this.rejected = false;
    element.addEventListener("click", () => this.start());
    element.addEventListener("keydown", (event) => this.handleKeyDown(event));
    element.addEventListener("blur", () => this.cancel());
    this.render();
  }

  start() {
    if (this.recording) return;
    this.group.beginRecording(this);
    this.recording = true;
    this.rejected = false;
    this.element.focus();
    this.render();
  }

  cancel() {
    if (!this.recording) return;
    this.recording = false;
    this.render();
    this.group.endRecording();
  }

  handleKeyDown(event) {
    if (!this.recording) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      this.cancel();
      return;
    }
    if (event.repeat) return;
    const accelerator = eventAccelerator(event);
    if (!accelerator) return;
    if (this.group.isDuplicate(this.action, accelerator)) {
      this.rejected = true;
      this.render();
      window.setTimeout(() => {
        if (this.rejected) {
          this.rejected = false;
          this.render();
        }
      }, 1500);
      return;
    }
    const previous = this.accelerator;
    this.accelerator = accelerator;
    this.recording = false;
    this.rejected = false;
    this.render();
    this.group.endRecording();
    this.group.commit(this.action, accelerator, previous);
  }

  setValue(accelerator) {
    this.accelerator = accelerator;
    this.rejected = false;
    this.render();
  }

  render() {
    this.element.textContent = this.rejected
      ? "与已有快捷键重复"
      : this.recording
        ? "请按快捷键…"
        : this.accelerator;
    this.element.classList.toggle("is-recording", this.recording && !this.rejected);
    this.element.classList.toggle("is-rejected", this.rejected);
  }
}

export class ShortcutGroup {
  constructor({ elements, config, onApply, onRecordingChange }) {
    this.values = {};
    this.fields = new Map();
    this.onApply = onApply;
    this.onRecordingChange = onRecordingChange;
    for (const [action, element] of Object.entries(elements)) {
      this.values[action] = config[action];
      this.fields.set(
        action,
        new ShortcutField({ action, element, accelerator: config[action], group: this }),
      );
    }
  }

  get isRecording() {
    return [...this.fields.values()].some((field) => field.recording);
  }

  cancelRecording() {
    for (const field of this.fields.values()) {
      if (field.recording) field.cancel();
    }
  }

  handleEvent(event) {
    const active = [...this.fields.values()].find((field) => field.recording);
    if (!active) return false;
    active.handleKeyDown(event);
    return true;
  }

  isDuplicate(action, accelerator) {
    return Object.entries(this.values).some(
      ([key, value]) => key !== action && value === accelerator,
    );
  }

  beginRecording(field) {
    for (const other of this.fields.values()) {
      if (other !== field && other.recording) other.cancel();
    }
    this.onRecordingChange?.(true);
  }

  endRecording() {
    if (this.isRecording) return;
    this.onRecordingChange?.(false);
  }

  async commit(action, accelerator, previous) {
    const previousValues = { ...this.values };
    this.values[action] = accelerator;
    try {
      const saved = await this.onApply({ ...this.values });
      this.apply(saved);
    } catch (error) {
      this.values = previousValues;
      this.fields.get(action)?.setValue(previous);
      throw error;
    }
  }

  apply(config) {
    for (const [action, value] of Object.entries(config)) {
      this.values[action] = value;
      this.fields.get(action)?.setValue(value);
    }
  }
}
