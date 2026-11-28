import { invoke } from "@tauri-apps/api/core";
import { closeContextMenu, showContextMenu } from "../ui/context-menu";
import { REASONING_LEVELS, reasoningLabel } from "../settings/model-settings";
import { showImagePreview } from "../render/media";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { captureWithSnippingTool, pixelsToFile } from "./clipboard-image";


export function initComposer({ appendMessage, onStop, onReasoningChange, onModelChange, onListModels }) {
  const input = document.querySelector("#message-input");
  const picker = document.querySelector("#attachment-input");
  const folderPicker = document.querySelector("#folder-input");
  const attachButton = document.querySelector("#add-attachment");
  const stopButton = document.querySelector("#stop-generation");
  const thinkingToggle = document.querySelector("#thinking-toggle");
  const list = document.querySelector("#attachment-list");
  const composer = document.querySelector("#composer");
  const previewUrls = new Map();
  const sentTexts = [];
  let selectedFiles = [];
  /** 待导入的文件夹：{ name, path } */
  let selectedFolder = null;
  let historyIndex = -1;
  let draft = "";
  const observer = new ResizeObserver(() => {
    document.querySelector("#conversation").style.paddingBottom = `${composer.offsetHeight + 24}px`;
  });
  observer.observe(composer);

  const FILE_KINDS = [
    { test: /^(pdf)$/i, kind: "PDF", tone: "pdf" },
    { test: /^(xlsx?|csv|ods)$/i, kind: "Excel", tone: "sheet" },
    { test: /^(docx?|odt|rtf)$/i, kind: "Word", tone: "doc" },
    { test: /^(pptx?|odp)$/i, kind: "PPT", tone: "slide" },
    { test: /^(zip|rar|7z|gz|tar)$/i, kind: "压缩包", tone: "archive" },
    { test: /^(png|jpe?g|gif|webp|bmp|svg)$/i, kind: "图片", tone: "image" },
    { test: /^(mp4|mov|avi|mkv|webm)$/i, kind: "视频", tone: "video" },
    { test: /^(mp3|wav|flac|m4a|ogg)$/i, kind: "音频", tone: "audio" },
    { test: /^(txt|md|log)$/i, kind: "文本", tone: "text" },
    { test: /^(cmd|bat|ps1|sh)$/i, kind: "脚本", tone: "script" },
  ];

  const CODE_EXT = /^(js|mjs|cjs|ts|tsx|jsx|py|rs|go|java|kt|c|h|cpp|hpp|cs|php|rb|lua|sql|json|ya?ml|toml|xml|html|css)$/i;

  /** 按扩展名判断文件类型，返回图标元素和类型名。 */
  function describeFile(file) {
    const extension = (file.name.split(".").pop() || "").toLowerCase();
    const entry =
      FILE_KINDS.find((item) => item.test.test(extension)) ??
      (CODE_EXT.test(extension)
        ? { kind: "代码", tone: "code" }
        : { kind: extension ? extension.toUpperCase() : "文件", tone: "plain" });
    const badge = document.createElement("span");
    badge.className = `attachment-badge attachment-badge--${entry.tone}`;
    badge.setAttribute("aria-hidden", "true");
    badge.textContent = extension.slice(0, 4).toUpperCase() || "?";
    return { badge, kind: entry.kind };
  }

  function formatFileSize(bytes) {
    if (!Number.isFinite(bytes) || bytes <= 0) return "";
    const units = ["B", "KB", "MB", "GB"];
    const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
    const value = bytes / 1024 ** index;
    return `${index === 0 ? value : value.toFixed(value < 10 ? 1 : 0)} ${units[index]}`;
  }

  function renderAttachments() {
    for (const url of previewUrls.values()) URL.revokeObjectURL(url);
    previewUrls.clear();
    list.replaceChildren();
    const hasAny = selectedFiles.length > 0 || Boolean(selectedFolder);
    list.hidden = !hasAny;
    composer.classList.toggle("has-attachments", hasAny);

    if (selectedFolder) {
      const { name, path } = selectedFolder;
      const chip = document.createElement("span");
      chip.className = "attachment-chip attachment-chip--folder";
      const badge = document.createElement("span");
      badge.className = "attachment-badge attachment-badge--folder";
      badge.textContent = "📁";
      badge.setAttribute("aria-hidden", "true");
      const meta = document.createElement("span");
      meta.className = "attachment-meta";
      const title = document.createElement("span");
      title.className = "attachment-name";
      title.textContent = name;
      const sub = document.createElement("span");
      sub.className = "attachment-sub";
      sub.textContent = path;
      meta.append(title, sub);
      const remove = document.createElement("button");
      remove.type = "button";
      remove.setAttribute("aria-label", `移除文件夹 ${name}`);
      remove.textContent = "×";
      remove.addEventListener("click", () => {
        selectedFolder = null;
        renderAttachments();
      });
      chip.append(badge, meta, remove);
      list.append(chip);
    }

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
        const { badge, kind } = describeFile(file);
        chip.classList.add("attachment-chip--file");
        chip.append(badge);
        const meta = document.createElement("span");
        meta.className = "attachment-meta";
        const name = document.createElement("span");
        name.className = "attachment-name";
        name.textContent = file.name;
        name.title = file.name;
        const sub = document.createElement("span");
        sub.className = "attachment-sub";
        sub.textContent = [kind, formatFileSize(file.size)].filter(Boolean).join(" · ");
        meta.append(name, sub);
        chip.append(meta);
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
    renderAttachments();
  }

  /** 清空文本和草稿，但保留已选的附件。 */
  function clearText() {
    input.value = "";
    historyIndex = -1;
    draft = "";
  }

  /** 有没有需要 Esc 取消的内容：文字或附件。 */
  function hasInput() {
    return Boolean(input.value.trim() || selectedFiles.length || selectedFolder);
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

  async function fileToBase64(file) {
    const buffer = await file.arrayBuffer();
    let binary = "";
    const bytes = new Uint8Array(buffer);
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) {
      binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
    }
    return btoa(binary);
  }

  const MAX_ATTACHMENT_BYTES = 32 * 1024 * 1024;

  async function buildParts(text, files) {
    const parts = [];
    if (text) parts.push({ type: "text", text });
    for (const file of files) {
      const isImage = file.type.startsWith("image/");
      const part = {
        type: isImage ? "image" : "file",
        name: file.name,
        mime: file.type,
        size: file.size,
      };
      if (file.size > MAX_ATTACHMENT_BYTES) {
        part.tooLarge = true;
        part.text = `【附件：${file.name}】\n（文件 ${(file.size / 1024 / 1024).toFixed(1)} MB，超过 ${
          MAX_ATTACHMENT_BYTES / 1024 / 1024
        } MB 上限，未保存）`;
        parts.push(part);
        continue;
      }
      const data = await fileToBase64(file);
      if (isImage) {
        part.data = data;
      } else {
        try {
          part.path = await invoke("save_attachment", { name: file.name, data });
        } catch (error) {
          console.error("[附件] 保存失败:", error);
          part.uploadError = String(error);
        }
      }
      parts.push(part);
    }
    return parts;
  }

  /** 文件夹由后端直接读取本地目录，前端不经过文件上传通道。 */
  async function buildFolderPart({ name, path }) {
    try {
      const saved = await invoke("import_folder", { path });
      return {
        type: "folder",
        name: saved.path?.split("/").pop() || name,
        path: saved.path,
        tree: saved.tree,
        fileCount: saved.fileCount,
        truncated: saved.total ?? null,
      };
    } catch (error) {
      console.error("[文件夹] 导入失败:", error);
      return { type: "folder", name, uploadError: String(error) };
    }
  }

  async function send({ interrupt = false } = {}) {
    const text = input.value.trim();
    const files = [...selectedFiles];
    const folder = selectedFolder;
    if (!text && !files.length && !folder) return;

    historyIndex = -1;
    draft = "";
    if (text) {
      sentTexts.push(text);
      if (sentTexts.length > 100) sentTexts.shift();
    }

    selectedFiles = [];
    selectedFolder = null;
    picker.value = "";
    input.value = "";
    renderAttachments();

    let parts;
    try {
      parts = await buildParts(text, files);
      if (folder) {
        const folderPart = await buildFolderPart(folder);
        if (folderPart) parts.push(folderPart);
      }
    } catch (error) {
      console.error("[send] 构建消息失败:", error);
      return;
    }
    const message = { role: "user", parts };
    const busy = composer.classList.contains("is-busy");
    const rendered = appendMessage(message, { pending: busy });
    window.dispatchEvent(
      new CustomEvent("litechat:send", { detail: { text, files, message, rendered } }),
    );
    input.focus();
    return rendered;
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
          void pickFolder();
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
  /** 用系统原生目录选择器，绕开 WebView 的文件上传确认弹窗。 */
  async function pickFolder() {
    if (!window.__TAURI_INTERNALS__) {
      folderPicker.click();
      return;
    }
    try {
      const chosen = await openDialog({
        directory: true,
        multiple: false,
        title: "选择要上传的文件夹",
      });
      if (!chosen) return;
      const path = typeof chosen === "string" ? chosen : chosen.path;
      if (!path) return;
      const name = path.split(/[\/]/).filter(Boolean).pop() || "folder";
      selectedFolder = { name, path };
      renderAttachments();
      input.focus();
    } catch (error) {
      console.error("[文件夹] 选择失败:", error);
    }
  }
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

    const hasImage = await invoke("clipboard_has_image").catch(() => false);
    if (!hasImage) return;

    event.preventDefault();
    try {
      const shot = await invoke("read_clipboard_image");
      const file = await pixelsToFile(shot, `pasted-${Date.now()}.png`);
      if (file) addFiles([file]);
    } catch {}
  });
  input.addEventListener("keydown", (event) => {
    if (event.isComposing) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
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
    input.setAttribute("aria-busy", String(busy));
    input.placeholder = busy ? "继续输入，模型会在下一步看到…" : "发消息...";
    stopButton.hidden = !busy;
    if (!busy) input.focus();
  }

  stopButton.addEventListener("click", () => onStop?.());

  let effort = "off";
  let modelName = "";
  let loadingModels = false;

  /** 模型名过长时中间省略，保住首尾可辨识部分。 */
  function shortModel(name) {
    if (name.length <= 18) return name;
    return `${name.slice(0, 9)}…${name.slice(-6)}`;
  }

  function renderEffort() {
    const label = modelName ? shortModel(modelName) : "选择模型";
    const suffix = effort === "off" ? "" : ` · ${reasoningLabel(effort)}`;
    thinkingToggle.textContent = `${label}${suffix}`;
    thinkingToggle.setAttribute("aria-pressed", String(effort !== "off"));
    thinkingToggle.setAttribute("aria-haspopup", "menu");
    thinkingToggle.setAttribute("aria-expanded", "false");
    thinkingToggle.title = modelName
      ? `模型 ${modelName}，推理强度 ${reasoningLabel(effort)}`
      : "选择模型和推理强度";
  }

  async function applyEffort(value) {
    if (value === effort) return;
    const previous = effort;
    effort = value;
    renderEffort();
    try {
      await onReasoningChange?.(value);
    } catch {
      effort = previous;
      renderEffort();
    }
  }

  async function applyModel(name) {
    if (!name || name === modelName) return;
    const previous = modelName;
    modelName = name;
    renderEffort();
    try {
      await onModelChange?.(name);
    } catch {
      modelName = previous;
      renderEffort();
    }
  }

  /** 第二级：选推理强度。 */
  function openEffortMenu() {
    const rect = thinkingToggle.getBoundingClientRect();
    thinkingToggle.setAttribute("aria-expanded", "true");
    showContextMenu(
      rect.left,
      rect.top - 8,
      [
        { label: `模型：${modelName || "未选择"}`, disabled: true },
        { separator: true },
        ...REASONING_LEVELS.map((level) => ({
          label:
            level.value === effort
              ? `✓ ${level.label} · ${level.hint}`
              : `${level.label} · ${level.hint}`,
          action: () => void applyEffort(level.value),
        })),
      ],
      { placement: "above" },
    );
    const reset = () => thinkingToggle.setAttribute("aria-expanded", "false");
    window.addEventListener("pointerdown", reset, { once: true });
  }

  /** 第一级：每次点击都重新拉模型列表。 */
  async function openModelMenu() {
    if (loadingModels) return;
    const rect = thinkingToggle.getBoundingClientRect();
    thinkingToggle.setAttribute("aria-expanded", "true");
    loadingModels = true;
    showContextMenu(rect.left, rect.top - 8, [{ label: "正在获取模型…", disabled: true }], {
      placement: "above",
    });

    let names = [];
    try {
      names = (await onListModels?.()) ?? [];
    } catch (error) {
      console.error("获取模型列表失败:", error);
      names = [];
    } finally {
      loadingModels = false;
    }

    if (!names.length) {
      showContextMenu(
        rect.left,
        rect.top - 8,
        [
          { label: "没有获取到模型", disabled: true },
          {
            label: modelName ? `仅调整思考强度（${modelName}）` : "仅调整思考强度",
            action: () => openEffortMenu(),
          },
        ],
        { placement: "above" },
      );
      return;
    }

    const reset = () => thinkingToggle.setAttribute("aria-expanded", "false");
    showContextMenu(
      rect.left,
      rect.top - 8,
      names.map((name) => ({
        label: name,
        checked: name === modelName,
        action: () => {
          reset();
          void applyModel(name).then(() => openEffortMenu());
        },
      })),
      { placement: "above", searchable: names.length > 8 },
    );
    window.addEventListener("pointerdown", reset, { once: true });
  }

  thinkingToggle.addEventListener("click", () => {
    if (thinkingToggle.getAttribute("aria-expanded") === "true") {
      thinkingToggle.setAttribute("aria-expanded", "false");
      closeContextMenu();
      return;
    }
    void openModelMenu();
  });

  /** 设置里保存或切换模型后，把输入栏的档位同步成保存值。 */
  function setReasoningEffort(value) {
    effort = value || "off";
    renderEffort();
  }

  /** 设置页或启动时同步当前模型名。 */
  function setModel(name) {
    modelName = name || "";
    renderEffort();
  }

  renderEffort();

  return {
    reset,
    clearText,
    hasInput,
    focus: () => input.focus(),
    setText,
    setBusy,
    setReasoningEffort,
    setModel,
    getText: () => input.value,
  };
}
