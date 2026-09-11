# 贡献指南（p2pawn）

感谢你有意为 p2pawn 做贡献！本文档说明开发流程、代码规范，以及版本发布如何自动完成。

**语言**：[English](CONTRIBUTING.md) | 简体中文

## 工作流：分支 → PR → squash 合并

`main` 分支受保护：只接受**通过全部必需检查并 squash 合并**的 Pull
Request。任何人（包括维护者）都不直接推送 —— 唯一例外是发布机器人的
版本号提交。

1. **从最新 `main` 切分支**：

   ```bash
   git switch main && git pull
   git switch -c feat/your-thing     # 或 fix/... docs/... 等
   ```

2. **在你的分支上随意提交** —— 分支上的提交信息属于你自己。
   只有 **PR 标题**会参与自动化（见下一节）。

3. **推送并向 `main` 发起 PR**。

4. 等 CI 通过（见下），然后 **Squash and merge**。
   仓库只允许 squash 这一种合并方式。

## Conventional Commits：PR 标题决定版本号

squash 合并会把你的 PR 标题变成 `main` 上唯一的提交主题，发布流水线
据此推导下一个版本号：

| PR 标题前缀 | 版本变化（0.x 阶段） | 示例 |
|---|---|---|
| `feat:` | **minor** | `feat: add LAN chat` → v0.2.0 |
| `fix:` | **patch** | `fix: clock drift` → v0.2.1 |
| `feat!:` 或描述里写 `BREAKING CHANGE:` | **minor**（1.0 后为 major） | `feat!:` 新协议 |
| `chore:`、`docs:`、`ci:`、`refactor:`、`test:` | **patch**，只要该变更会出现在 changelog 里（用户可见的文档、依赖升级）；changelog 略过的纯内部维护（CI 调整等）不发版 | `docs: typo` → 通常 v0.2.1 |

经验法则：

- PR 标题**必须**以 conventional-commit 类型开头：`feat`、`fix`、
  `chore`、`docs`、`refactor`、`test`、`ci`。不符合的 PR 会被 CI
  直接拒掉（`amannn/action-semantic-pull-request` 检查）。
- 带作用域的写法没问题：`feat(replay): ...`。
- 维护性变更只要进入 changelog（文档、依赖更新）就会以 **patch** 版本
  发布；changelog 略过的变更（解析器跳过的 CI 调整等）不触发发版。
  拿不准时，假设合并会产生一个 patch 版本。

## CI 检查什么（对你意味着什么）

每个 PR 运行 `.github/workflows/ci.yml`：

- **fmt + clippy** —— `cargo fmt --check` 和
  `cargo clippy --all-targets -- -D warnings`。警告即错误。推送前
  本地先跑：

  ```bash
  cargo fmt --all
  cargo clippy --all-targets -- -D warnings
  ```

- **test (debug) / test (release)** —— 双档全量测试。UDP 发现、TCP
  对局、TUI 渲染均有覆盖；如果改动涉及网络或 UI，请补充或扩展测试
  （`tests/integration.rs` 里有可运行的样例）。

- **license check** —— `cargo-deny` 校验全部依赖（包括你新增的）
  的许可证都在 `deny.toml` 的白名单内。如果你引入的依赖带了新的
  许可证表达式，请把 SPDX ID 加进白名单。copyleft 许可证会被拒绝。

- **PR 标题** —— 上一节的 conventional-commit 检查。

四项都是 `main` 的**必需检查**，任何一项红灯都无法合并。

## 代码规范

- **Rust 2024 edition，stable 工具链**（`rust-toolchain.toml` 锁定）。
- **不手搓国际象棋规则。** 棋盘合法性、将死/逼和、三次重复、吃过路
  兵、升变等全部来自 `chess` crate。`src/game/san.rs` 只负责展示层的
  SAN *格式化* —— 如果你发现自己在写走法生成逻辑，请停下来先查
  `chess` crate。
- **依赖树保持精简。** 这是一个局域网小工具，不是平台。加 crate 之前
  先想想约 50 行标准库代码能否解决。
- **错误处理**：I/O 和网络失败都是用户可见的 —— 用 toast/对话框呈现，
  而不是在运行路径上 `unwrap()`。测试和真正不可能失败的地方用
  `unwrap()` 没问题。
- **跟随现有代码风格** —— 注释密度和命名与现有代码保持一致。
- 用户可见的变更需**同时更新** `README.md` 和 `README.zh-CN.md`
  （两者保持同步），发布流水线会把它写进自动生成的 CHANGELOG。

## 测试说明

- 测试在回环地址上绑定真实的 UDP/TCP 端口；支持并行，本地和 CI 都
  安全。
- `examples/lan_pair.rs` 是无头端到端驱动，让两个进程通过真实 socket
  完整下一局 —— 无需第二台机器即可复现局域网行为：

  ```bash
  cargo build --release --examples
  P2PAWN_NAME=Alice ./target/release/examples/lan_pair host Alice   # 终端 1
  P2PAWN_NAME=Bob   ./target/release/examples/lan_pair client Bob   # 终端 2
  ```

## 发布如何运作（合并后会发生什么）

合并 PR 会触发 `.github/workflows/release.yml`：

1. squash 提交的标题决定版本变化（见前表）；机器人自己的
   `chore(release):` 提交会被跳过，避免重触发循环。
2. 机器人更新 `Cargo.toml` 版本号、用 git-cliff 重新生成
   `CHANGELOG.md`、推送 `chore(release): vX.Y.Z` 提交，并在该提交上
   创建 tag 和 GitHub Release。
3. 矩阵构建产出 Windows（MSVC）、Linux（musl 静态）、macOS（aarch64）
   三平台二进制及 `.sha256` 校验文件，附到 Release 上。

你永远不需要手动发版。如果发布运行失败，重跑失败的 job 通常就够了；
上传步骤是幂等的（`--clobber`）。

## 提交 Issue

请附上：操作系统 + 终端、p2pawn 版本（`gh release list` 或 commit
SHA）、你的操作步骤、实际现象 vs 预期。网络问题请说明两台机器是否在
同一子网，以及是否有防火墙/VPN 软件。
