import { invoke } from "@tauri-apps/api/core";
import { closeContextMenu, showContextMenu } from "../ui/context-menu";
import { showImagePreview } from "../render/media";
import { captureWithSnippingTool, pixelsToFile } from "./clipboard-image";


export function initComposer({ appendMessage, onStop, onThinkingChange }) {
  const input = document.querySelector("#message-input");
  const picker = document.querySelector("#attachment-input");
  const folderPicker = document.querySelector("#folder-input");
  const attachButton = document.querySelector("#add-attachment");
  const stopButton = document.querySelector("#stop-generation");
  const thinkingToggle = document.querySelector("#thinking-toggle");
  const list = document.querySelector("#attachment-list");
  const composer = document.querySelector("#composer");
  const objectUrls = new Set();
  const previewUrls = new Map();
  const sentTexts = [];
  let selectedFiles = [];
  let historyIndex = -1;
  let draft = "";
  const observer = new ResizeObserver(() => {
    document.querySelector("#conversation").style.paddingBottom = `${composer.offsetHeight + 24}px`;
  });
  observer.observe(composer);

  function renderAttachments() {
    for (const url of previewUrls.values()) URL.revokeObjectURL(url);
    previewUrls.clear();
    list.replaceChildren();
    list.hidden = !selectedFiles.length;
    composer.classList.toggle("has-attachments", Boolean(selectedFiles.length));

    for (const [index, file] of selectedFiles.entries()) {
      const chip = document.createElement("span");
      const isImage = file.type.startsWith("image/");
      chip.className = isImage ? "attachment-chip attachment-chip--image" : "attachment-chip";

      if (isImage) {
        const thumb = document.createElement("img");
        const url = URL.createObjectURL(file);
        previewUrls.set(file, url);
        thumb.src = url;
        thumb.alt = file.name;
        thumb.title = file.name;
        thumb.addEventListener("click", () => showImagePreview(url, file.name, file.name));
        chip.append(thumb);
      } else {
        const name = document.createElement("span");
        name.textContent = file.name;
        name.title = file.name;
        chip.append(name);
      }

      const remove = document.createElement("button");
      remove.type = "button";
      remove.setAttribute("aria-label", `移除 ${file.name}`);
      remove.textContent = "×";
      remove.addEventListener("click", () => {
        selectedFiles.splice(index, 1);
        renderAttachments();
      });
      chip.append(remove);
      list.append(chip);
    }
  }

  function reset() {
    selectedFiles = [];
    picker.value = "";
    input.value = "";
    historyIndex = -1;
    draft = "";
    for (const url of objectUrls) URL.revokeObjectURL(url);
    objectUrls.clear();
    renderAttachments();
  }

  function recall(index) {
    historyIndex = index;
    input.value = index >= sentTexts.length ? draft : sentTexts[index];
    input.setSelectionRange(input.value.length, input.value.length);
  }

  function navigateHistory(direction) {
    if (input.value.includes("\n")) {
      const atEdge = direction === -1
        ? input.selectionStart === 0 && input.selectionEnd === 0
        : input.selectionStart === input.value.length && input.selectionEnd === input.value.length;
      if (!atEdge) return false;
    }
    if (historyIndex === -1) {
      if (direction === -1) {
        if (!sentTexts.length) return false;
        draft = input.value;
        recall(sentTexts.length - 1);
        return true;
      }
      return false;
    }
    if (direction === -1) {
      if (historyIndex === 0) return true;
      recall(historyIndex - 1);
      return true;
    }
    recall(historyIndex + 1);
    return true;
  }

  function send() {
    if (composer.classList.contains("is-busy")) return;
    const text = input.value.trim();
    const files = [...selectedFiles];
    if (!text && !files.length) return;
    historyIndex = -1;
    draft = "";
    const parts = [];
    if (text) parts.push({ type: "text", text });
    for (const file of files) {
      const url = URL.createObjectURL(file);
      objectUrls.add(url);
      parts.push({
        type: file.type.startsWith("image/") ? "image" : "file",
        url,
        name: file.name,
        mime: file.type,
        size: file.size,
      });
    }
    if (text) {
      sentTexts.push(text);
      if (sentTexts.length > 100) sentTexts.shift();
    }
    const message = { role: "user", parts };
    appendMessage(message);
    window.dispatchEvent(new CustomEvent("litechat:send", { detail: { text, files, message } }));
    selectedFiles = [];
    picker.value = "";
    input.value = "";
    historyIndex = -1;
    draft = "";
    renderAttachments();
    input.focus();
  }

  function addFiles(files) {
    if (!files?.length) return;
    selectedFiles.push(...files);
    renderAttachments();
    input.focus();
  }

  async function takeScreenshot() {
    try {
      const file = await captureWithSnippingTool();
      if (file) addFiles([file]);
    } catch {
      // 取消或失败时静默处理
    }
  }

  function openMenu() {
    const rect = attachButton.getBoundingClientRect();
    attachButton.setAttribute("aria-expanded", "true");
    showContextMenu(rect.left, rect.top - 8, [
      {
        label: "上传文件或图片",
        action: () => {
          attachButton.setAttribute("aria-expanded", "false");
          picker.click();
        },
      },
      {
        label: "上传文件夹",
        action: () => {
          attachButton.setAttribute("aria-expanded", "false");
          folderPicker.click();
        },
      },
      { separator: true },
      {
        label: "截图",
        action: () => {
          attachButton.setAttribute("aria-expanded", "false");
          void takeScreenshot();
        },
      },
    ], { placement: "above" });
    window.addEventListener(
      "pointerdown",
      () => attachButton.setAttribute("aria-expanded", "false"),
      { once: true },
    );
  }

  attachButton.addEventListener("click", () => {
    if (attachButton.getAttribute("aria-expanded") === "true") {
      attachButton.setAttribute("aria-expanded", "false");
      closeContextMenu();
      return;
    }
    openMenu();
  });
  picker.addEventListener("change", () => {
    addFiles(picker.files);
    picker.value = "";
  });
  folderPicker.addEventListener("change", () => {
    addFiles(folderPicker.files);
    folderPicker.value = "";
  });
  input.addEventListener("input", (event) => {
    if (event.isComposing) return;
    if (historyIndex !== -1) {
      historyIndex = -1;
      draft = "";
    }
  });

  input.addEventListener("paste", async (event) => {
    const items = [...(event.clipboardData?.items ?? [])];
    const files = items
      .filter((item) => item.kind === "file")
      .map((item) => item.getAsFile())
      .filter(Boolean);

    if (files.length) {
      event.preventDefault();
      addFiles(files.map((file, index) =>
        file.name && file.name !== "image.png"
          ? file
          : new File([file], `pasted-${Date.now()}-${index}.png`, { type: file.type || "image/png" }),
      ));
      return;
    }

    if (!window.__TAURI_INTERNALS__) return;
    event.preventDefault();
    try {
      const shot = await invoke("read_clipboard_image");
      const file = await pixelsToFile(shot, `pasted-${Date.now()}.png`);
      if (file) addFiles([file]);
    } catch {
      // 剪贴板没有图片，交回默认粘贴行为
      document.execCommand("paste");
    }
  });
  input.addEventListener("keydown", (event) => {
    if (event.isComposing) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
      return;
    }
    if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      if (event.altKey || event.ctrlKey || event.metaKey) return;
      const direction = event.key === "ArrowUp" ? -1 : 1;
      if (navigateHistory(direction)) event.preventDefault();
    }
  });
  function setText(value) {
    input.value = value;
    historyIndex = -1;
    draft = value;
    input.focus();
    input.setSelectionRange(value.length, value.length);
  }

  function setBusy(busy) {
    composer.classList.toggle("is-busy", busy);
    attachButton.disabled = busy;
    thinkingToggle.disabled = busy;
    input.setAttribute("aria-busy", String(busy));
    stopButton.hidden = !busy;
    if (!busy) input.focus();
  }

  function setThinking(enabled) {
    thinkingToggle.setAttribute("aria-pressed", String(Boolean(enabled)));
  }

  function getThinking() {
    return thinkingToggle.getAttribute("aria-pressed") === "true";
  }

  stopButton.addEventListener("click", () => onStop?.());
  thinkingToggle.addEventListener("click", () => {
    const next = !getThinking();
    setThinking(next);
    void onThinkingChange?.(next);
  });

  return {
    reset,
    focus: () => input.focus(),
    setText,
    setBusy,
    setThinking,
    getThinking,
    getText: () => input.value,
  };
}
