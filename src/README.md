# src 目录结构

```
src/
├── main.js                  应用入口：装配各模块、注册视图、暴露 window.liteChat
│
├── app/                     应用外壳与页面框架
│   ├── shell.js             渲染整个 DOM 骨架（标题栏 / 会话区 / 各面板 / 输入区）
│   ├── views.js             视图基类：View / PanelView / ChatView / ViewManager
│   ├── window-controls.js   标题栏按钮：最小化、置顶、新对话
│   └── demo.js              ?demo=1 时的富消息演示数据
│
├── chat/                    对话核心
│   ├── chat.js              消息渲染、流式助手回复（含思考块）
│   ├── conversation.js      一回合的编排：建会话 → 发请求 → 收事件 → 落库
│   ├── composer.js          输入框：发送、附件、上下键回溯、忙碌状态
│   ├── history.js           会话列表与切换、消息持久化、阅读位置协调
│   └── scroll-memory.js     按会话记忆滚动位置
│
├── render/                  内容渲染
│   ├── markdown.js          marked + KaTeX + DOMPurify + 代码高亮 + 链接/图片增强
│   └── media.js             URL 校验、图片灯箱、文件卡片、引用卡片、下载
│
├── settings/                设置
│   ├── settings.js          设置面板：快捷键配置
│   └── model-settings.js    模型 API 配置：服务商、地址、模型、密钥、深度思考
│
├── ui/                      通用可复用组件
│   ├── context-menu.js      自定义右键菜单 + 全局默认菜单拦截
│   ├── clipboard.js         复制文本（含降级方案）
│   └── shortcut-recorder.js 快捷键录制：ShortcutField / ShortcutGroup
│
└── styles/
    ├── styles.css           外壳、标题栏、面板、输入区、滚动条
    └── messages.css         消息气泡、Markdown 排版、媒体卡片、思考块
```

## 依赖方向

```
main.js
  ├── app/        （外壳与视图框架，不依赖 chat）
  ├── chat/       → render/, ui/, app/views
  ├── render/     → ui/（context-menu）
  ├── settings/   → ui/, app/views
  └── ui/         （无本地依赖，最底层）
```

`ui/` 和 `render/` 是叶子模块，`app/views.js` 只定义契约，`chat/` 负责业务编排。
