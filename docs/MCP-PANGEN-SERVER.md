# MCP Pangen Server 设计文档

## 1. 概述

MCP Pangen Server 是一个 MCP（Model Context Protocol）服务器，用于将 Pangen（光刻 OPC 软件）与 AI Agent（如 Claude Code、Lattice）连接起来，支持**手动调参**和**自动化迭代优化**两种模式。

### 1.1 整体架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Claude Code (AI Agent)                        │
│                                                                     │
│  · 调用 MCP Server 工具（job_submit, analyze_result 等）             │
│  · 调用 RAG 知识库（search_pangen_docs）                             │
│  · 结合两者结果做分析和决策                                          │
└─────────────────────────────────────────────────────────────────────┘
                │                                    │
                │ MCP 工具调用                       │ RAG 检索
                ↓                                    ↓
┌─────────────────────────────┐          ┌─────────────────────────────┐
│     MCP Pangen Server      │          │      RAG Server             │
│                             │          │    (向量知识库)            │
│  ┌───────────────────────┐ │          └─────────────────────────────┘
│  │    迭代优化层          │ │
│  │  Task Mgr             │ │
│  │  Parameter Tuner      │ │
│  │  Result Analyzer       │ │
│  └───────────────────────┘ │
│              ↓              │
│  ┌───────────────────────┐ │
│  │    基础工具层          │ │
│  │  Job Mgr / Config Mgr │ │
│  └───────────────────────┘ │
│              ↓              │
│  ┌───────────────────────┐ │
│  │  Pangen Interface     │ │ (SSH)
│  │  (SSH 执行器)         │ │
│  └───────────────────────┘ │
└─────────────────────────────┘
                │
                │ SSH
                ↓
┌─────────────────────────────────────────────────────────────────────┐
│                          GPU Cluster                                 │
│  · Pangen 可执行文件 · Log 文件 · 结果文件 (GB级)                    │
└─────────────────────────────────────────────────────────────────────┘
```

**职责分工**：
- **MCP Server**：Job 管理、配置管理、执行 Pangen 命令、返回数值指标
- **GPU Cluster**：运行 Pangen 的服务器集群，MCP Server 通过 SSH 访问
- **RAG Server**：Claude Code 直接访问，获取 Pangen 文档和历史调参经验
- **Claude Code**：协调者，结合 MCP 结果和 RAG 知识做决策

### 1.2 组件职责

| 组件 | 职责 |
|------|------|
| **Claude Code / Lattice** | AI Agent，通过 MCP 协议调用工具 |
| **RAG Server** | 向量知识库，Claude Code 直接连接，MCP Server 不感知 |
| **MCP Pangen Server** | 工具执行、Job 管理、配置管理、确认网关 |
| **GPU Cluster** | 运行 Pangen 的服务器集群，通过 SSH 访问 |

---

## 2. 模块详解

### 2.1 MCP Protocol Layer

MCP 协议层，处理 JSON-RPC 通信：

- 接收来自 Claude Code 的 `tools/list`、`tools/call` 请求
- 返回工具定义和执行结果
- 暴露 Resources 供 Claude Code 查询

### 2.2 Job Manager（Job 管理模块）

核心模块，负责 Pangen 任务的完整生命周期管理。

**功能**：
- 生成 job_id（UUID v4）
- 通过 SSH 在 GPU 服务器上启动/取消 Pangen 进程
- 轮询 job 状态（从状态文件读取）
- 管理 job 工作目录

**核心接口**：

| 接口 | 说明 |
|------|------|
| `submit(config_name)` | 创建 job，SSH 启动 Pangen，返回 job_id |
| `status(job_id)` | 查询 job 当前状态 |
| `list()` | 列出所有 job |
| `cancel(job_id)` | 发送 SIGTERM 取消 job |
| `result(job_id)` | 获取结果文件元信息（路径/大小/格式） |
| `logs(job_id)` | 读取 job 的 log 文件内容 |

**状态文件**（保存在本地 `~/.pangen-mcp/jobs/`）：
```json
{
  "id": "uuid-v4",
  "gpu_host": "gpu-01",
  "pid": 12345,
  "work_dir": "/data/job-xxx",
  "config_file": "/path/to/config.json",
  "status": "pending|running|completed|failed|cancelled",
  "progress": "45%",
  "start_time": "...",
  "end_time": null,
  "exit_code": null
}
```

### 2.3 Config Manager（配置管理模块）

管理 Pangen 的配置文件。配置文件存放在 MCP 服务器本地，包含 JSON 格式的仿真参数。

**功能**：
- 读取/写入配置文件的指定字段
- 配置验证（参数类型、范围检查）
- 管理多个配置模板

**使用场景**：
```
用户: "把迭代次数改成 100"
Claude 调用 config_set(key="simulation.iterations", value="100")

用户: "当前配置里的迭代次数是多少"
Claude 调用 config_get(key="simulation.iterations")
```

**注意**：这里管理的"配置"是 Pangen 运行参数的配置文件，不是 MCP 服务器本身的配置。

### 2.4 Confirmation Gate（确认网关）

在执行高风险操作前，要求用户手动确认。

**需要确认的场景**：

| 场景 | 说明 |
|------|------|
| 执行脚本 | `execute_pangen_script` 执行前，用户需审阅脚本内容 |
| 敏感操作 | 删除数据、修改集群配置等高风险操作 |
| 长时 Job | 启动预计运行超过 30 分钟的 job |

**流程**：
```
1. Claude 调用需要确认的工具
2. MCP 服务器返回确认请求，暂停执行
3. Claude Code 向用户展示确认对话框
4. 用户批准 → MCP 服务器继续执行
5. 用户拒绝 → 返回错误
```

### 2.5 Pangen Interface（Pangen 接口层）

封装与 Pangen 程序交互的底层细节。

**功能**：
- SSH 命令执行器（每次命令新开一个 SSH 连接）
- Log 文件读取器（流式读取，支持 offset）
- 结果文件元信息读取器（路径/大小/格式）

**SSH 执行流程**：
```
1. 建立 SSH 连接
2. 创建工作目录
3. nohup 启动 Pangen，记录 PID
4. 关闭 SSH 连接
5. 后台轮询状态文件
```

### 2.6 Result Analyzer（结果分析模块）

Job 完成后，分析 Pangen 的输出结果，提取数值指标。

**功能**：
- 解析二进制结果文件，提取数值指标（如 CD 误差、良率等）
- 读取 log 文件，提取关键事件和警告
- 生成结构化的分析报告

**输出格式**：
```json
{
  "job_id": "xxx",
  "metrics": {
    "cd_error_mean": 0.23,
    "cd_error_max": 1.87,
    "yield_estimate": 94.5
  },
  "warnings": ["部分区域收敛较慢", "建议增加迭代次数"]
}
```

**注意**：模糊判断（如"布局复杂度较高"）由 Claude Code 结合 RAG 知识库完成，Result Analyzer 只负责精确的数值指标提取。

### 2.7 Parameter Tuner（参数调整模块）

自动化的参数调整策略引擎。根据分析结果，生成下一轮的配置调整建议。

**功能**：
- 加载预定义的调参策略（如"网格细化策略"、"权重调整策略"）
- 根据历史 job 结果，选择合适的调整方向
- 生成新的配置参数（不直接写入，需要确认）
- 记录调参历史，用于策略学习

**调参策略示例**：

| 策略名 | 触发条件 | 调整方式 |
|--------|----------|----------|
| `refine_grid` | 收敛慢 | 网格间距 × 0.8 |
| `increase_iterations` | 迭代未收敛 | 迭代次数 × 1.5 |
| `adjust_weight` | 局部误差大 | 调整权重参数 |
| `restart` | 多次失败 | 重置到初始配置 |

**工作流程**：
```
1. 用户发起"调参任务"，设定迭代上限（如 10 次）
2. MCP 服务器创建 Task，内部维护 job 链
3. 循环：
   - 运行 job
   - 分析结果
   - 生成调参建议
   - 到达上限或 AI 决定停止 → 退出
4. 返回最终配置和调参报告
```

### 2.8 Task Manager（任务管理模块）

管理迭代优化任务，包含多个相关的 Job。

**功能**：
- 创建调参任务（Task），包含目标配置和迭代上限
- 追踪任务进度（已完成轮次、当前状态）
- 管理任务内的 job 链
- 支持暂停/恢复/终止任务

**Task 状态文件**（保存在本地 `~/.pangen-mcp/tasks/`）：
```json
{
  "id": "task-uuid",
  "goal": "优化参数达到 CD_error < 0.5",
  "max_iterations": 10,
  "current_iteration": 3,
  "status": "running|completed|failed|paused",
  "jobs": ["job-1", "job-2", "job-3"],
  "best_result": {
    "job_id": "job-2",
    "cd_error": 0.67
  },
  "start_time": "...",
  "history": [
    {"iteration": 1, "config": {...}, "result": {...}},
    {"iteration": 2, "config": {...}, "result": {...}}
  ]
}
```

### 2.9 Local Storage（本地存储）

MCP 服务器本地的文件存储。

| 路径 | 内容 |
|------|------|
| `~/.pangen-mcp/jobs/{job_id}.json` | Job 状态文件 |
| `~/.pangen-mcp/tasks/{task_id}.json` | Task 状态文件 |
| `~/.pangen-mcp/configs/` | Pangen 配置模板 |
| `~/.pangen-mcp/strategies/` | 调参策略定义 |
| `~/.pangen-mcp/logs/` | MCP 服务器自身日志 |

---

## 3. MCP 服务器内部架构图

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                            MCP Pangen Server                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│  MCP Protocol Layer (JSON-RPC, tools/list, tools/call)                      │
├─────────────────────────────────────────────────────────────────────────────┤
│                              迭代优化层                                     │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        Task Manager                                  │   │
│  │   · 管理迭代任务生命周期（create/progress/pause/resume/stop）        │   │
│  │   · 维护 job 链和调参历史                                            │   │
│  │   · 控制迭代循环（到达上限则停止）                                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    ↓                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      Parameter Tuner                                 │   │
│  │   · 加载调参策略                                                    │   │
│  │   · 根据分析结果生成调参建议                                        │   │
│  │   · 返回新的配置参数                                                │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    ↓                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      Result Analyzer                                 │   │
│  │   · 解析二进制结果，提取数值指标（CD 误差等）                       │   │
│  │   · 生成结构化分析报告                                              │   │
│  │   · 注意：不访问 RAG，模糊判断由 Claude Code 结合 RAG 完成           │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    ↑                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│                              基础工具层                                     │
│  ┌─────────────┬─────────────┬─────────────┬──────────────────────────┐   │
│  │  Job Manager│ Config Mgr  │Confirmation│      Pangen Interface    │   │
│  │             │             │   Gate     │                           │   │
│  │ · submit()  │ · get()    │            │ · ssh_exec()              │   │
│  │ · status()  │ · set()    │ · pending()│ · read_log()              │   │
│  │ · cancel()  │ · validate()│ · approve()│                           │   │
│  │ · result()  │             │ · reject() │                           │   │
│  │ · logs()    │             │            │                           │   │
│  └─────────────┴─────────────┴─────────────┴──────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────────┤
│                           Local Storage                                    │
│   jobs/{job_id}.json  │  tasks/{task_id}.json  │  configs/  │ strategies/│
└─────────────────────────────────────────────────────────────────────────────┘
                                        │
                                        │ SSH
                                        ↓
                               ┌──────────────────────┐
                               │    GPU Cluster        │
                               │  · Pangen 可执行文件 │
                               │  · Log 文件 (文本)    │
                               │  · 结果文件 (GB级)   │
                               └──────────────────────┘
```

### 3.1 迭代优化数据流

```
用户: "优化参数，目标 CD_error < 0.5，最多 10 次"

Claude Code:
  1. 调用 task_create() 创建任务
  2. 调用 job_submit() 提交 job
  3. 轮询 job_status() 直到完成
  4. 调用 analyze_result() 获取分析报告（数值指标）
  5. 调用 RAG 知识库查询类似案例
  6. 结合两者决定下一步：
     - 如果未达标且未达上限：调用 tune_params() 获取调参建议
     - 如果达标或已达上限：停止迭代
  7. 调用 config_set() 应用新配置
  8. 重复 2-7 直到结束
  9. 调用 task_result() 获取最终报告
```

```
用户: "优化参数，目标 CD_error < 0.5，最多 10 次"

    ┌─────────────────────────────────────────────────────────────┐
    │                      Task: iteration = 1                    │
    │                                                             │
    │  1. Task Manager → Job Manager.submit()                    │
    │                           ↓                                 │
    │  2. Pangen 运行 → Job 结束                                 │
    │                           ↓                                 │
    │  3. Result Analyzer.analyze() → 分析报告                   │
    │                           ↓                                 │
    │  4. CD_error = 0.8 > 0.5, 未达标                          │
    │  5. Parameter Tuner.tune() → 生成新配置建议                 │
    │                           ↓                                 │
    │  6. Task: iteration = 2                                   │
    │  ... (循环直到 iteration = 10 或达标)                       │
    │                                                             │
    └─────────────────────────────────────────────────────────────┘
```

---

## 4. 工具列表

### 4.1 Task 工具（Task Management）

| 工具名 | 参数 | 返回 | 说明 |
|--------|------|------|------|
| `task_create` | `goal: string, max_iterations: number, initial_config: string` | `{task_id: string}` | 创建调参任务 |
| `task_progress` | `task_id: string` | `TaskProgress` | 获取任务进度 |
| `task_pause` | `task_id: string` | `boolean` | 暂停任务 |
| `task_resume` | `task_id: string` | `boolean` | 恢复任务 |
| `task_stop` | `task_id: string` | `boolean` | 停止任务 |
| `task_list` | 无 | `TaskInfo[]` | 列出所有任务 |
| `task_result` | `task_id: string` | `TaskResult` | 获取任务最终结果和报告 |

### 4.2 Job 工具（Job Management）

| 工具名 | 参数 | 返回 | 说明 |
|--------|------|------|------|
| `job_submit` | `config_name: string` | `{job_id: string}` | 提交 job |
| `job_status` | `job_id: string` | `JobStatus` | 查询状态 |
| `job_list` | 无 | `JobInfo[]` | 列出所有 job |
| `job_cancel` | `job_id: string` | `boolean` | 取消 job |
| `job_result` | `job_id: string` | `ResultMetadata` | 获取结果元信息 |
| `job_logs` | `job_id: string, offset?: number` | `string` | 读取 log |

### 4.3 Config 工具（Configuration Management）

| 工具名 | 参数 | 返回 | 说明 |
|--------|------|------|------|
| `config_get` | `key: string` | `string` | 读取配置字段 |
| `config_set` | `key: string, value: string` | `boolean` | 写配置字段 |
| `config_list` | 无 | `ConfigInfo[]` | 列出所有配置模板 |
| `config_validate` | `config_name: string` | `ValidationResult` | 验证配置合法性 |

### 4.4 Result Analyzer 工具（Result Analysis）

| 工具名 | 参数 | 返回 | 说明 |
|--------|------|------|------|
| `analyze_result` | `job_id: string` | `AnalysisReport` | 分析 job 结果，生成报告 |
| `get_metrics` | `job_id: string` | `Metrics` | 获取数值指标 |
| `compare_results` | `job_id_1: string, job_id_2: string` | `ComparisonResult` | 对比两个 job 的结果 |

### 4.5 Parameter Tuner 工具（Parameter Tuning）

| 工具名 | 参数 | 返回 | 说明 |
|--------|------|------|------|
| `tune_params` | `job_id: string, strategy?: string` | `TuningSuggestion` | 根据结果生成调参建议 |
| `list_strategies` | 无 | `StrategyInfo[]` | 列出可用调参策略 |
| `apply_tuning` | `task_id: string, suggestion: TuningSuggestion` | `boolean` | 应用调参建议到配置 |

### 4.6 代码执行工具（Code Execution）

| 工具名 | 参数 | 返回 | 说明 |
|--------|------|------|------|
| `execute_pangen_script` | `script: string` | `ScriptResult` | 执行 Pangen Python 脚本 |

### 4.7 资源（Resources）

| URI | 说明 |
|-----|------|
| `pangen://jobs/{job_id}` | Job 状态资源 |
| `pangen://jobs` | 所有 Job 列表 |

---

## 4. Job 管理机制

### 4.1 job_id 生成策略

MCP 服务器本地生成 UUID v4 作为 job_id，通过 SSH 执行时记录映射信息。

### 4.2 Job 状态文件

路径：`~/.pangen-mcp/jobs/{job_id}.json`

```json
{
  "id": "uuid-v4",
  "gpu_host": "gpu-01",
  "pid": 12345,
  "work_dir": "/data/job-xxx",
  "config_file": "/path/to/config.json",
  "status": "running|completed|failed|cancelled",
  "start_time": "2026-05-01T10:00:00Z",
  "end_time": null,
  "exit_code": null,
  "progress": "45%"
}
```

### 4.3 SSH 执行流程

1. 用户调用 `job_submit`
2. MCP server 生成 `uuid-v4` 作为 job_id
3. 创建 `jobs/{job_id}.json` 状态文件
4. SSH 到 GPU 服务器，创建工作目录
5. nohup 执行 Pangen，记录 PID
6. 写入 PID 到状态文件
7. 返回 `job_id` 给 Claude Code

### 4.4 取消机制

通过 `ssh_session` 或 `gpu_host + pid` 发送 SIGTERM。

---

## 5. 通知机制

Claude Code **不支持** MCP 服务器主动推送通知。采用轮询模式：

### 5.1 Job 状态查询

Claude Code 通过 `job_status` 或 `resources/read pangen://jobs/{id}` 轮询状态。

### 5.2 典型工作流

```
用户: "启动 pangen job，用配置 A"

Claude 调用 job_submit → 返回 job_id="xxx"
Claude 调用 job_status(job_id="xxx") → status="running"
Claude 调用 job_logs(job_id="xxx") → 读取最新 log
Claude 继续轮询 job_status 直到 status="completed"
Claude 调用 job_result(job_id="xxx") → 返回结果路径/大小/格式
```

---

## 6. 确认机制

三种场景需要用户确认：

| 场景 | 说明 |
|------|------|
| 代码执行前 | Claude 执行 `execute_pangen_script` 前需用户批准代码 |
| 敏感操作 | 删除数据、修改集群配置等高风险操作 |
| 长时间 Job | 启动 1 小时以上的 Job 前确认 |

### 6.1 实现方式

MCP 服务器返回确认消息，Claude Code 暂停等待用户回复：

```json
// MCP 返回
{
  "content": [{
    "type": "text",
    "text": "此操作将启动一个 2 小时的 job，是否确认？\n\n操作: job_submit\n配置: config_A\nGPU: gpu-01"
  }]
}
```

---

## 7. 目录结构

```
pangen-mcp-server/
├── src/
│   ├── main.rs              # 入口
│   ├── mcp/
│   │   ├── mod.rs           # MCP 协议处理
│   │   ├── tools.rs         # 工具定义
│   │   └── resources.rs     # 资源定义
│   ├── task/
│   │   ├── mod.rs           # Task 管理
│   │   ├── manager.rs       # Task 生命周期
│   │   └── state.rs         # Task 状态持久化
│   ├── job/
│   │   ├── mod.rs           # Job 管理
│   │   ├── manager.rs       # Job 生命周期
│   │   └── state.rs         # Job 状态持久化
│   ├── config/
│   │   ├── mod.rs           # 配置管理
│   │   └── validator.rs     # 配置验证
│   ├── analyzer/
│   │   ├── mod.rs           # 结果分析
│   │   ├── metrics.rs       # 数值指标提取
│   │   └── rag_client.rs    # RAG 知识库交互
│   ├── tuner/
│   │   ├── mod.rs           # 参数调整
│   │   ├── strategy.rs      # 调参策略
│   │   └── engine.rs        # 调参引擎
│   ├── ssh/
│   │   ├── mod.rs           # SSH 接口
│   │   └── executor.rs      # 命令执行
│   └── pangen/
│       ├── mod.rs           # Pangen 接口
│       └── parser.rs        # Log/结果解析
├── jobs/                    # Job 状态文件目录
├── tasks/                   # Task 状态文件目录
├── configs/                 # 配置模板目录
├── strategies/              # 调参策略定义
└── Cargo.toml
```

---

## 8. 技术选型

| 组件 | 选择 | 理由 |
|------|------|------|
| 语言 | Rust | 高性能、易维护、与 Lattice 一致 |
| MCP SDK | `mcp-sdk` / `rust-mcp` | 官方或社区实现 |
| SSH | `ssh2` crate | 纯 Rust 实现 |
| 状态存储 | JSON 文件 | 简单够用、便于调试 |
| 序列化 | `serde` + `serde_json` | Rust 标准 |

---

## 9. RAG 知识库

RAG 作为独立服务，Claude Code 直接访问，MCP Server 不感知。

| 组件 | 说明 |
|------|------|
| RAG Server | 独立部署，向量数据库（ChromaDB/Milvus） |
| RAG Client | Claude Code 内置，MCP Server 不涉及 |
| 知识库内容 | Pangen 官方文档、API 手册、历史调参经验 |

**Claude Code 使用 RAG 的场景**：
- 根据 `analyze_result()` 返回的数值指标，结合 RAG 进行模糊判断
- 查询类似问题的历史调参经验
- 获取调参策略建议

---

## 10. 待确认事项

### Pangen 相关
1. Pangen CLI 命令具体格式和参数
2. 配置文件的字段定义和验证规则
3. Log 文件的格式和解析方式
4. 结果文件的具体格式（OPC 相关二进制格式）
5. 结果分析中数值指标的具体定义（如 CD 误差的计算方式）

### 调参相关
6. 调参策略的具体定义格式（JSON/YAML/TOML）
7. 初始调参策略由谁提供（用户提供 / MCP 内置 / 从 RAG 学习）
8. 策略不奏效时的回退机制

### 系统集成
9. SSH 连接的具体认证方式（密钥/密码）
10. GPU 集群的调度方式（SLURM/SGE/直接执行）
11. Task 内部的循环控制由谁驱动（MCP 服务器 / Claude Code）

---

## 11. 典型工作流示例

### 手动调参工作流（用户单次 job）

```
用户: "用配置 A 运行一次 pangen"

Claude 调用 job_submit(config_name="A")
→ 返回 job_id="xxx"

Claude 轮询 job_status 直到 completed

Claude 调用 job_logs(job_id="xxx")
→ 读取 log，分析结果

Claude 调用 job_result(job_id="xxx")
→ 返回结果路径/大小/格式
```

### 自动化迭代优化工作流（Task）

```
用户: "帮我优化参数，目标 CD 误差 < 0.5，最多跑 10 次"

Claude 调用 task_create(goal="CD_error < 0.5", max_iterations=10, initial_config="default")

# MCP 服务器自动执行循环：
# 1. 运行 job
# 2. 分析结果
# 3. 生成调参建议
# 4. 应用建议，继续下一轮
# （直到达到上限或满足目标）

Claude 调用 task_progress(task_id="xxx")
→ 返回 {"current_iteration": 5, "best_result": {...}, "status": "running"}

Claude 调用 task_result(task_id="xxx")
→ 返回最终配置、调参报告、最佳结果
```

---

## 12. 后续计划

1. **Phase 1**: 核心 Job 管理（submit/status/cancel/logs）
2. **Phase 2**: 配置管理（config_get/config_set/validate）
3. **Phase 3**: 结果分析（analyze_result/metrics）
4. **Phase 4**: 调参策略（tune_params/list_strategies）
5. **Phase 5**: Task 管理（task_create/progress/stop）
6. **Phase 6**: 确认机制（confirmation gate）
7. **Phase 7**: 与 Claude Code/Lattice 集成测试
