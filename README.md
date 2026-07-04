# DuskTerm

DuskTerm 是一个基于 Tauri 2、Rust 和 Vue 3 的跨平台桌面终端工具，面向 SSH 运维、远程文件管理、端口隧道、Telnet 和串口设备管理等场景。

- 当前版本：`1.0.0`
- 仓库地址：<https://github.com/dusk-song/duskterm>
- 许可证：MIT

## 功能特性

- 多协议会话：支持 SSH、Telnet、Serial 会话的创建、编辑、保存、连接和重连。
- 多终端工作区：支持多标签、水平/垂直分屏、焦点面板切换和状态栏快捷入口。
- SFTP 文件管理：支持目录浏览、上传、下载、重命名、删除、权限修改和远程文件编辑。
- 命令知识库：统一维护常用命令、快捷触发词、标签、说明、安全级别和执行策略。
- 终端快捷匹配：按知识库触发词提供候选命令，支持键盘选择并默认插入命令。
- 安全拦截：敏感/高危命令规则来自命令知识库，覆盖手动输入、快捷插入、知识库执行和历史命令重放。
- 端口隧道：支持本地、远程和动态隧道配置、启动、停止和批量管理。
- 本地安全：会话敏感字段使用 AES-256-GCM 加密存储，私钥文件在 Unix 系统下自动校正为 `0600` 权限。
- 状态监控：提供 CPU、内存、磁盘和网络等运行状态展示。
- 主题与图标：内置紧凑的桌面工具界面、xterm.js 终端主题和按文件类型加载的 Material 图标。

## 技术栈

| 层级 | 技术 |
| --- | --- |
| 桌面框架 | Tauri 2 |
| 后端 | Rust, Tokio |
| 前端 | Vue 3, Composition API |
| 状态管理 | Pinia |
| 终端模拟 | xterm.js |
| 编辑器 | Ace |
| UI 基础 | shadcn-vue / reka-ui |
| SSH/SFTP | russh, russh-keys, russh-sftp |
| 串口通信 | serialport |
| 存储加密 | AES-256-GCM |
| 构建工具 | Vite 6, pnpm |

## 开发环境

需要安装：

- Node.js 18 或更高版本
- pnpm 8 或更高版本
- Rust stable
- Tauri 2 所需系统依赖：<https://tauri.app/start/prerequisites/>

安装依赖：

```bash
pnpm install
```

启动桌面开发模式：

```bash
pnpm tauri dev
```

只启动前端开发服务：

```bash
pnpm dev
```

## 测试与构建

运行前端/结构回归测试：

```bash
pnpm test
```

构建前端产物：

```bash
pnpm build
```

检查 Rust/Tauri 后端：

```bash
cd src-tauri
cargo check
```

生成桌面安装包：

```bash
pnpm desktop:build
```

Windows 当前打包目标为 NSIS exe 安装包，产物位于 `src-tauri/target/release/bundle/nsis/`。

## 项目结构

```text
src/
  components/          Vue 业务组件
    app-shell/         状态栏、应用外壳组件
    knowledge/         命令知识库面板与维护弹窗
    terminal/          终端视图与交互逻辑
    sftp/              SFTP 文件面板
  composables/         组合式逻辑
  stores/              Pinia 状态
  utils/               命令索引、安全匹配、主题等工具
src-tauri/
  src/
    session/           会话监督与运行时
    storage/           加密存储、导入导出、知识库持久化
    sftp/              SFTP 后端能力
    tunnel/            端口隧道
    terminal/          终端写入队列和传输探测
  tauri.conf.json      Tauri 应用与打包配置
docs/
  packaging-and-updates.md
tests/
  *.test.js            Node test 回归测试
```

## 命令知识库与安全策略

快捷命令和敏感命令已经统一为命令知识库条目：

- 快捷触发词用于终端候选匹配。
- 安全级别用于生成敏感/高危命令拦截规则。
- 执行策略控制从知识库发起动作时的行为。
- 高危命令可以复制和插入，但不能从知识库一键直接执行。
- 插入后的命令在用户手动回车时仍会经过终端敏感命令拦截。

## 打包与更新

常规发布使用：

```bash
pnpm desktop:build
```

关于 NSIS 安装包、原地升级和后续 Tauri updater 接入方式，见 [docs/packaging-and-updates.md](docs/packaging-and-updates.md)。

## 贡献

提交前建议至少运行：

```bash
pnpm test
pnpm build
cd src-tauri && cargo check
```

欢迎通过 Issue 和 Pull Request 改进项目。

## 许可证

本项目基于 [MIT License](LICENSE) 开源。
