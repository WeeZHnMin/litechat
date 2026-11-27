import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { showContextMenu } from "../ui/context-menu";

export function safeResourceUrl(value, allowImageData = false) {
  if (typeof value !== "string" || !value.trim()) return "";
  if (allowImageData && /^data:image\/(?:png|jpe?g|gif|webp);base64,/i.test(value)) {
    return value;
  }
  try {
    const url = new URL(value, window.location.href);
    const allowed = ["https:", "http:", "blob:", "asset:"].includes(url.protocol);
    return allowed ? url.href : "";
  } catch {
    return "";
  }
}

export async function openResource(value) {
  const url = safeResourceUrl(value, true);
  if (!url) return;
  const protocol = new URL(url, window.location.href).protocol;
  if (protocol === "blob:" || protocol === "data:") {
    downloadResource(url, "attachment");
    return;
  }
  if (!["http:", "https:"].includes(protocol)) return;
  try {
    if (window.__TAURI_INTERNALS__) {
      await openUrl(url);
    } else {
      const opened = window.open(url, "_blank", "noopener,noreferrer");
      if (opened) opened.opener = null;
    }
  } catch (error) {
    console.error("Unable to open resource:", error);
  }
}

export function downloadResource(value, name = "download") {
  const url = safeResourceUrl(value, true);
  if (!url) return;
  const parsed = new URL(url, window.location.href);
  if (!["blob:", "data:"].includes(parsed.protocol) && parsed.origin !== window.location.origin) {
    void openResource(url);
    return;
  }
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
}

function actionButton(label, onClick) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "media-action";
  button.textContent = label;
  button.addEventListener("click", onClick);
  return button;
}

export function showImagePreview(value, alt = "图片预览", name = "image.png") {
  const url = safeResourceUrl(value, true);
  if (!url) return;
  const overlay = document.createElement("div");
  overlay.className = "image-lightbox";
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-modal", "true");
  overlay.setAttribute("aria-label", alt);
  const toolbar = document.createElement("div");
  toolbar.className = "lightbox-toolbar";
  toolbar.append(actionButton("关闭", () => overlay.remove()));
  const image = document.createElement("img");
  image.src = url;
  image.alt = alt;
  image.referrerPolicy = "no-referrer";

  image.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    event.stopPropagation();
    showContextMenu(event.clientX, event.clientY, [
      { label: "复制图片", action: () => copyImage(url) },
    ]);
  });

  overlay.append(toolbar, image);
  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) overlay.remove();
  });
  overlay.addEventListener("keydown", (event) => {
    if (event.key === "Escape") overlay.remove();
  });
  document.body.append(overlay);
  overlay.tabIndex = -1;
  overlay.focus();
}

async function copyImage(url) {
  try {
    const response = await fetch(url);
    const blob = await response.blob();
    if (navigator.clipboard?.write && window.ClipboardItem) {
      await navigator.clipboard.write([new ClipboardItem({ [blob.type]: blob })]);
      return;
    }
    await invoke("copy_image_to_clipboard", { png: await blobToBase64(blob) });
  } catch (error) {
    console.error("复制图片失败:", error);
  }
}

function blobToBase64(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result);
      resolve(result.slice(result.indexOf(",") + 1));
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

function partImageUrl(part) {
  const mime = part.mime || "image/png";
  if (part.data && typeof part.data === "string") {
    return `data:${mime};base64,${part.data}`;
  }
  return safeResourceUrl(part.url, true);
}

export function renderImagePart(part) {
  const figure = document.createElement("figure");
  figure.className = "generated-image";
  const url = partImageUrl(part);
  if (!url) {
    figure.classList.add("media-error");
    figure.textContent = part.alt || "图片无法显示";
    return figure;
  }
  const preview = document.createElement("button");
  preview.type = "button";
  preview.className = "image-preview-button";
  const image = document.createElement("img");
  image.src = url;
  image.alt = part.alt || part.name || "生成图片";
  image.loading = "lazy";
  image.referrerPolicy = "no-referrer";
  preview.append(image);
  preview.addEventListener("click", () => {
    showImagePreview(url, image.alt, part.name || "image.png");
  });
  const caption = document.createElement("figcaption");
  const label = document.createElement("span");
  label.textContent = part.name || part.alt || "图片";
  caption.append(label);
  figure.append(preview, caption);
  return figure;
}

export function formatFileSize(size) {
  if (!Number.isFinite(size) || size <= 0) return "";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(size) / Math.log(1024)), units.length - 1);
  return `${(size / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`;
}

export function renderFilePart(part) {
  const card = document.createElement("article");
  card.className = "file-card";
  const badge = document.createElement("div");
  badge.className = "file-badge";
  const extension = (part.name || "FILE").split(".").pop();
  badge.textContent = extension.slice(0, 4).toUpperCase();
  const meta = document.createElement("div");
  meta.className = "file-meta";
  const name = document.createElement("strong");
  name.textContent = part.name || "附件";
  const details = document.createElement("span");
  details.textContent = [part.mime, formatFileSize(part.size)].filter(Boolean).join(" · ");
  meta.append(name, details);

  // 上传的附件存在工作目录里，用 system 默认程序打开；远程 URL 走浏览器
  const savedPath = typeof part.path === "string" ? part.path : "";
  const url = safeResourceUrl(part.url, true);
  const action = actionButton("打开", () => {
    if (savedPath && window.__TAURI_INTERNALS__) {
      invoke("open_workspace_file", { path: savedPath }).catch((error) => {
        console.error("无法打开附件:", error);
      });
      return;
    }
    if (!url) return;
    const parsed = new URL(url, window.location.href);
    if (parsed.origin === window.location.origin) {
      downloadResource(url, part.name || "download");
    } else {
      void openResource(url);
    }
  });
  action.disabled = !savedPath && !url;
  card.append(badge, meta, action);
  return card;
}

export function renderCitationsPart(part) {
  const section = document.createElement("section");
  section.className = "citations";
  const title = document.createElement("h2");
  title.textContent = "参考来源";
  const list = document.createElement("div");
  list.className = "citation-list";
  for (const [index, source] of (part.sources || []).entries()) {
    const url = safeResourceUrl(source.url);
    if (!url) continue;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "citation-card";
    const number = document.createElement("span");
    number.className = "citation-number";
    number.textContent = String(source.id || index + 1);
    const content = document.createElement("span");
    content.className = "citation-content";
    const name = document.createElement("strong");
    name.textContent = source.title || new URL(url).hostname;
    const host = document.createElement("small");
    host.textContent = source.snippet || new URL(url).hostname;
    content.append(name, host);
    button.append(number, content);
    button.addEventListener("click", () => void openResource(url));
    list.append(button);
  }
  section.append(title, list);
  return section;
}
