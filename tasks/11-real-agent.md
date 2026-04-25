# 任务 11：实现 real-agent example

## 目标

创建一个使用真实 LLM 的 example，端到端验证 Lattice 对接真实模型的能力。

## 分支

`feat/real-agent`

## 依赖

- 任务 9（Anthropic 后端）或 任务 10（OpenAI 后端），至少完成一个

## 具体内容

### 实现要点

1. 从环境变量读取配置：
   - `LATTICE_LLM_PROVIDER`：`anthropic` 或 `openai`（默认 `openai`）
   - `LATTICE_API_KEY`：LLM API key
   - `LATTICE_API_BASE`：可选，自定义 base URL
   - `LATTICE_MODEL`：可选，模型名称
2. 根据 provider 创建对应的 LLMClient 实现
3. 注册 bash 工具，描述清晰让 LLM 能正确调用
4. 从命令行参数或 stdin 读取用户任务
5. 运行 ControlLoop，打印最终答案和事件日志

### 使用方式

```bash
# OpenAI 兼容（包括本地部署）
LATTICE_API_KEY=sk-xxx LATTICE_API_BASE=http://localhost:8000/v1 \
  cargo run -p real-agent -- "列出当前目录的文件"

# Anthropic
LATTICE_LLM_PROVIDER=anthropic LATTICE_API_KEY=sk-ant-xxx \
  cargo run -p real-agent -- "用 curl 查一下天气"
```

### 预期效果

- LLM 理解用户任务
- 自主决定调用 bash 工具
- 沙箱执行命令并返回结果
- LLM 基于结果给出最终答案

## 验收标准

- [ ] `cargo build -p real-agent` 通过
- [ ] 使用真实 LLM 端到端跑通（手动验证）
- [ ] 支持 Anthropic 和 OpenAI 两种 provider
- [ ] 支持自定义 base URL
- [ ] 所有代码注释使用英文
- [ ] `cargo clippy` 零警告
