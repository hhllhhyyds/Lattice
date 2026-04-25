# 任务 18：Docker 化独立部署

> ⚠️ **STATUS: DRAFT — 未经人工 review，内容可能调整。**

## 目标

将 lattice-server 打包为 Docker 镜像，提供 docker-compose 配置，支持一键启动独立部署的 Lattice 平台服务。

## 分支

`feat/docker-deploy`

## 依赖

- 任务 17（配置管理 — 服务器配置完整可用）

## 具体内容

### 1. Dockerfile（多阶段构建）

```dockerfile
# Stage 1: Build
FROM rust:1.79-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p lattice-server

# Stage 2: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/lattice-server /usr/local/bin/
COPY --from=builder /app/lattice.toml.example /etc/lattice/lattice.toml

EXPOSE 3000
ENV LATTICE_HOST=0.0.0.0
ENV LATTICE_PORT=3000

ENTRYPOINT ["lattice-server", "--config", "/etc/lattice/lattice.toml"]
```

### 2. docker-compose.yml

```yaml
version: "3.8"
services:
  lattice:
    build: .
    ports:
      - "3000:3000"
    environment:
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
      - LATTICE_PORT=3000
      - RUST_LOG=info
    volumes:
      - ./lattice.toml:/etc/lattice/lattice.toml:ro
    restart: unless-stopped
```

### 3. 示例配置文件

创建 `lattice.toml.example`，包含完整的配置示例和注释。

### 4. .dockerignore

```
target/
.git/
*.md
!README.md
.github/
examples/
tasks/
docs/
```

### 5. 启动脚本

`scripts/start.sh`：
```bash
#!/bin/bash
# Quick start script for Lattice server
# Usage: ./scripts/start.sh [--build]

if [ "$1" = "--build" ]; then
    docker compose build
fi

docker compose up -d
echo "Lattice server is running at http://localhost:3000"
echo "Health check: curl http://localhost:3000/health"
```

### 6. CORS 收紧

配置文件中 production profile 支持：
- 限制 `allowed_origins`
- 限制 `allowed_methods`
- 限制 `allowed_headers`

### 7. README 更新

更新项目 README.md，新增：
- Quick Start 部分（Docker 一键启动）
- API 文档概览
- 配置说明

## 验收标准

- [ ] `docker build -t lattice .` 成功构建
- [ ] `docker compose up` 一键启动
- [ ] 容器内 `curl localhost:3000/health` 返回 200
- [ ] 从宿主机可访问所有 API 端点
- [ ] 配置文件通过 volume 挂载可更新
- [ ] API key 通过环境变量注入，不暴露在镜像中
- [ ] 镜像大小合理（< 100MB runtime stage）
- [ ] README 包含完整的部署说明
- [ ] 所有 pub 类型和方法有英文 doc comment

## 新增文件

```
Lattice/
├── Dockerfile
├── docker-compose.yml
├── .dockerignore
├── lattice.toml.example
└── scripts/
    └── start.sh
```

## 设计说明

- **为什么多阶段构建？** Rust 编译环境很大（~2GB），runtime 只需要二进制和基础系统库，最终镜像控制在 100MB 以下。
- **为什么 bookworm-slim 不是 alpine？** Rust 默认 glibc 链接。alpine 用 musl，需要额外配置 cross-compile，MVP 阶段不值得折腾。
- **为什么还没有 Kubernetes？** 先确保单机部署好用。K8s manifest 是后续的事。
