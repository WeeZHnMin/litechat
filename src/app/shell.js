import { icon } from "./icons";
import { settingsViewHtml } from "../settings/layout";

export function renderShell() {
  document.querySelector("#app").innerHTML = `
    <main class="window-shell">
      <header class="titlebar" data-tauri-drag-region>
        <div class="titlebar-actions">
          <button class="icon-button" id="minimize" type="button" aria-label="最小化" title="最小化">${icon("minimize")}</button>
          <button class="icon-button" id="pin" type="button" aria-label="钉在桌面" aria-pressed="false" title="钉在桌面">${icon("pin")}</button>
          <button class="icon-button" id="new-chat" type="button" aria-label="新对话" title="新对话">${icon("newChat")}</button>
        </div>
        <div class="titlebar-actions titlebar-actions--right">
          <button class="icon-button" id="history-button" type="button" aria-label="历史会话" aria-controls="history-view" aria-expanded="false" title="历史会话">${icon("history")}</button>
          <button class="icon-button" id="settings-button" type="button" aria-label="设置" aria-controls="settings-view" aria-expanded="false" title="设置">${icon("settings")}</button>
        </div>
      </header>
      <section class="conversation" id="conversation" aria-label="聊天内容"></section>
      <section class="history-view" id="history-view" aria-label="历史会话" tabindex="-1" hidden>
        <header class="settings-header">
          <button class="icon-button" id="history-back" type="button" aria-label="返回聊天" title="返回聊天">${icon("back")}</button>
          <h1>历史会话</h1>
        </header>
        <p class="history-hint">↑ ↓ 选择 · Enter 打开 · Esc 返回</p>
        <div class="history-list" id="history-list" role="listbox" aria-label="历史会话列表" tabindex="0"></div>
      </section>
      ${settingsViewHtml()}
      <section class="composer" id="composer" aria-label="消息输入区">
        <div class="attachment-list" id="attachment-list" hidden></div>
        <textarea id="message-input" rows="1" aria-label="消息" placeholder="发消息..."></textarea>
        <div class="composer-toolbar">
          <button class="icon-button plus-button" id="add-attachment" type="button" aria-label="添加附件" aria-haspopup="menu" aria-expanded="false" title="添加附件">${icon("plus")}</button>
          <button class="thinking-toggle" id="thinking-toggle" type="button" aria-pressed="false" title="开启后模型会先推理再回答">深度思考</button>
          <button class="thinking-toggle permission-toggle" id="permission-toggle" type="button" aria-pressed="false" aria-haspopup="menu" aria-expanded="false" title="工具执行权限">每次询问</button>
          <button class="stop-button" id="stop-generation" type="button" hidden>停止生成</button>
        </div>
      </section>
      <input id="attachment-input" type="file" multiple hidden />
      <input id="folder-input" type="file" webkitdirectory directory multiple hidden />
    </main>
  `;
}
