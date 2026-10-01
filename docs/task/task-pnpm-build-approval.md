# Task：修复 pnpm 构建脚本审批导致无法启动

## 任务信息

- ID：`task-pnpm-build-approval`
- 状态：`done`
- 创建日期：2026-10-01
- 最后更新：2026-10-01

## 任务关系

- 前置依赖：无
- 后续依赖：无
- 关联任务：无
- 替代任务：无

## 背景与目标

### 当前问题

- pnpm 12 不再读取 `package.json` 中的 `pnpm` 字段，且未批准 `esbuild` 与 `vue-demi` 的安装构建脚本，导致 `pnpm install` 和 `pnpm tauri dev` 以 `ERR_PNPM_IGNORED_BUILDS` 失败。

### 期望结果

- 项目级 pnpm 配置由 `pnpm-workspace.yaml` 生效。
- 依赖安装完成后可继续启动 Tauri 开发环境。

## 范围

### 本次需要

- 迁移现有 pnpm 的架构与依赖覆盖配置。
- 显式批准项目所需的构建脚本。

### 本次不需要

- 升级依赖、pnpm 或 Tauri。
- 修改应用源码、IPC、构建或发布配置。

## 相关代码与上下文

优先阅读：

- `package.json`
- `pnpm-workspace.yaml`
- `pnpm-lock.yaml`

关联模块：

- 前端：无
- 后端：无
- 数据或配置：pnpm 项目级配置与锁文件

## 实现约束

- 保持现有依赖版本与 lockfile 解析结果。
- 平台要求：Windows x64。
- 不新增依赖，不进行无关重构或格式化。

## 关键规则与边界

- 仅批准锁文件中项目启动所需的 `esbuild` 与 `vue-demi` 构建脚本。
- 依赖覆盖配置必须继续在 pnpm 12 中生效。

## 验收标准

- [x] `pnpm install --frozen-lockfile` 不再出现 `ERR_PNPM_IGNORED_BUILDS`。
- [x] `pnpm tauri dev` 可通过前端开发服务器启动阶段。
- [x] 依赖解析与锁文件中的 Vue 覆盖配置保持一致。

## 验证要求

建议执行：

- `pnpm install --frozen-lockfile`
- `pnpm tauri dev`（以短时启动验证）

不要求执行：

- 完整安装包构建。

## 实际完成情况

- 实际修改文件：`package.json`、`pnpm-workspace.yaml`、本任务文档。
- 核心改动：将 pnpm 12 不再读取的 `package.json` 配置迁移至 `pnpm-workspace.yaml`，并批准 `esbuild` 与 `vue-demi` 的构建脚本；锁文件已由 pnpm 同步并恢复原有 Vue 覆盖记录。
- 验证命令与结果：`pnpm install --no-frozen-lockfile` 成功执行构建脚本；`pnpm install --frozen-lockfile` 成功；`pnpm tauri dev` 成功启动 Vite、完成 Rust 开发构建并运行 `duskterm.exe`，随后主动停止。
- 未验证项及原因：未进行完整安装包构建；不属于本次启动修复范围。
- 配置、数据格式、IPC 或兼容性影响：仅迁移 pnpm 项目级配置，无应用数据或 IPC 变更。
