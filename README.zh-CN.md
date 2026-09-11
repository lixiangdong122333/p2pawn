# p2pawn

[English](README.md) | **简体中文**

> 终端里的点对点国际象棋 —— 无服务器、无账号，只有你和局域网里的同事。

Pawn to pawn，没有中间商。一个轻量的纯 TUI 国际象棋客户端，专为办公室局域网里的快速对局而生。

```
打开终端 → 运行 p2pawn → 选一位同事 → 开下
```

## 功能特性

- **纯 TUI**：基于 [ratatui](https://ratatui.rs) + crossterm，包含主菜单、
  大厅、棋盘、棋谱、对话框、设置、帮助、对局结束界面。
- **局域网自动发现**：UDP 广播自动找到其他在线玩家 —— 不用输 IP、不用
  部署服务器。每个客户端既是广播者也是监听者。
- **完整国际象棋规则**（合法走法、将军、将死、逼和、三次重复、50 步规则、
  王车易位、吃过路兵、升变、FEN、SAN/UCI）全部由
  [`chess`](https://crates.io/crates/chess) crate 提供 —— 没有手搓引擎。
- **键盘优先**：方向键移动光标，`Enter` 选子/走子，`Esc` 取消/菜单，
  `Tab` 翻转棋盘。不需要输入 `e2e4` 这种文本命令。
- **合法走法提示**：选中棋子后显示所有可走目标格（`◎`）。
- **国际象棋计时**：内置 1+0 / 3+2 / 5+0 / 10+0 预设，支持加秒，
  超时判负，每步与对手同步剩余时间。
- **和棋提议 / 接受 / 拒绝、认输**，以及对手掉线处理（判你获胜）。
- **PGN 全流程**：对局结束自动保存为标准 PGN 文件；历史界面逐条列出，
  支持逐着回放（`←`/`→`/`Home`/`End`）。
- **简单的 TOML 配置**：玩家名、棋子风格（Unicode/ASCII）、坐标显示、
  提示开关、默认计时模式、棋盘翻转。

## 构建

```bash
cargo build --release
```

产物是单个自包含的可执行文件（`target/release/p2pawn[.exe]`，约 9 MB），
只需要一个终端就能运行。

### Windows 无 MSVC 环境的说明

项目用 MSVC 工具链可以直接构建。如果你只有 GNU 工具链，请确保
`gcc`/`ld`/`dlltool`（可从 [WinLibs](https://winlibs.com/) 获取）在
`PATH` 中：

```bash
rustup toolchain install stable-x86_64-pc-windows-gnu
export PATH="/path/to/mingw64/bin:$PATH"
cargo +stable-x86_64-pc-windows-gnu build --release
```

## 使用

```
p2pawn
```

| 界面         | 按键                                                     |
|--------------|----------------------------------------------------------|
| 主菜单       | `↑↓` 选择，`⏎` 确认，`q`/`Esc` 退出                      |
| 大厅         | `↑↓` 选玩家，`⏎` 邀请，`r` 刷新，`Esc` 返回               |
| 对局         | `↑↓←→` 光标，`⏎` 选子/走子，`Esc` 菜单，`Tab` 翻转，`D` 提和 |
| 升变         | `←→` 选棋子，`⏎` 确认                                    |
| 历史         | `⏎` 回放，`d` 删除，`Esc` 返回                            |
| 回放         | `←`/`→` 单步，`Home`/`End` 跳到首/末，`Esc` 返回           |
| 设置         | `⏎`/`←→` 修改值，`Esc` 返回（自动保存）                    |

## 局域网对局原理

```
电脑 A                              电脑 B
  UDP 信标 :47610  ────广播──────▶  监听
  监听             ◀────广播────────  UDP 信标 :47610
       └─────────── TCP :47611 ────────┘
             邀请 → 接受 → 开始 → 走子…
```

- 发现：UDP 广播 JSON 信标（全局广播 + 各网卡子网定向广播），约 1.2 秒
  一次；超过 5 秒没收到信标的玩家会从列表中消失。
- 对局：两个客户端之间一条 TCP 连接（NDJSON，一行一条消息）。发起方发送
  `Invite`，对方 `Accept`/`Decline`，随后 `Start` 随机分配执棋颜色，
  双方开始对局，每步同步棋钟。

## 文件位置

- 配置：`<config_dir>/p2pawn/config.toml`
  （Windows 为 `~/AppData/Roaming/p2pawn/config.toml`，
  Linux 为 `~/.config/p2pawn/config.toml`）
- 棋谱：`<config_dir>/p2pawn/games/*.pgn`

## 项目结构

```
src/
  main.rs        终端初始化 + 事件循环
  app.rs         应用状态机、局域网对局编排
  input.rs       全部界面与弹窗的键盘路由
  ui.rs          全部渲染（各界面、棋盘、弹窗）
  config.rs      TOML 配置
  util.rs        时间戳工具
  game/
    session.rs   对局状态：走子、棋钟、和棋/认输/超时、终局判定
    clock.rs     双方棋钟（支持加秒）
    san.rs       SAN 格式化 + UCI 解析（规则来自 `chess` crate）
    pgn.rs       PGN 生成/解析 + 回放
    history.rs   本地 .pgn 存储
  net/
    proto.rs     线路协议（信标 + 对局消息）
    discovery.rs UDP 信标/监听线程
    conn.rs      TCP 对局连接 + 监听器
tests/
  integration.rs 发现、完整 TCP 对局、渲染冒烟测试
```

## 测试

```bash
cargo test
```

共 56 个测试（debug + release 双档）：SAN/PGN/UCI 往返、将死/逼和/三次
重复/升变/超时判定、棋钟语义、配置往返、历史存取、双实例 UDP 互相发现、
一条完整的 TCP 对局（邀请 → 接受 → 开始 → 走子 → 提和 → 拒和 → 认输）、
忙时拒绝，以及所有界面的 TestBackend 渲染冒烟测试。

### 多进程端到端测试

`examples/lan_pair.rs` 以无头模式驱动**真实的**应用状态机
（`App` + 按键处理 + 网络），让两个独立进程通过真实的 UDP 发现和 TCP
完成一整局对局：

```bash
cargo build --release --examples

# 终端 1：等待邀请并接受
P2PAWN_NAME=Alice ./target/release/examples/lan_pair host Alice

# 终端 2：发现玩家、发起邀请，下出愚者之母
P2PAWN_NAME=Bob ./target/release/examples/lan_pair client Bob
```

双方通过 UDP 广播互相发现，经 TCP 交换邀请/接受/开始消息，走出
1. f3 e5 2. g4 Qh4# 直到将死，各自保存 PGN。`client-quit` 模拟对局中
崩溃（对手将因掉线被判胜）；`P2PAWN_SLOW_MS=<ms>` 让每步棋延迟指定
毫秒，便于第三个实例介入观察（它会看到忙碌玩家的 "Playing" 状态并被
拒绝邀请）。

`P2PAWN_NAME` 同样作用于主程序 `p2pawn`，可以在不改配置的情况下覆盖
玩家名 —— 单机跑多个实例时非常实用。

## 贡献

欢迎提交 PR！`main` 只接受 squash 合并的 Pull Request，PR 标题遵循
[Conventional Commits](https://www.conventionalcommits.org) 规范 ——
标题前缀（`feat:`/`fix:` 等）决定自动版本号变化。CI 会检查格式化、
clippy、测试和依赖许可证。

完整指南见 [CONTRIBUTING.zh-CN.md](CONTRIBUTING.zh-CN.md)
（[English](CONTRIBUTING.md)）。

## 许可证

Copyright 2026 Xiangdong Li

在以下许可证中任选其一：

- Apache License, Version 2.0（[LICENSE-APACHE](LICENSE-APACHE)）
- MIT 许可证（[LICENSE-MIT](LICENSE-MIT)）

除非你明确声明，否则依据 Apache-2.0 许可证定义，你主动提交并包含在本
项目中的任何贡献，均按上述双许可证授权，不附加任何额外条款或条件。
