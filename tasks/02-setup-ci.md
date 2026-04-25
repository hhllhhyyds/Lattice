# 任务 2：搭建 CI/CD

## 目标

配置 GitHub Actions，从第一行代码开始就有自动化检查守护质量。

## 分支

`feat/setup-ci`

## 具体内容

1. 创建 `.github/workflows/ci.yml`
2. 触发条件：push 到 main、所有 PR
3. CI 流水线步骤：
   - `cargo fmt --check` — 格式检查
   - `cargo clippy -- -D warnings` — lint 检查，警告视为错误
   - `cargo test` — 运行所有测试
   - `cargo doc --no-deps` — 文档编译检查
4. 使用 `actions/checkout@v4` + `dtolnay/rust-toolchain@stable`
5. 启用 cargo 缓存（`Swatinem/rust-cache@v2`）加速 CI

## 验收标准

- [ ] PR 提交后 GitHub Actions 自动触发
- [ ] 四项检查（fmt、clippy、test、doc）全部 pass
- [ ] CI 配置文件清晰可维护
