# 开发门禁（Development Gate）

本文规定代码进入 `main` 分支前必须满足的验证要求。

## 一、背景：为什么需要这份文档

2026-07-31 发生过一次典型事故：

| commit | 内容 |
| --- | --- |
| `1992ae6` | 「增加图片缩略图悬浮放大预览，完善VV模式窗口布局」，14 文件 +110/-44 |
| `e2b5388` | 「修复 compile error: UiRect 转 RECT 类型不匹配」，4 行 |

`1992ae6` **带着编译错误直接进了主干**，只能由下一个 commit 紧急补救。

事后复盘出两个根因，二者缺一不可：

**根因 1：开发机上没有 Rust 工具链。**
`cargo` / `rustc` / `rustup` 均不存在，`~/.cargo`、`~/.rustup` 目录也不存在。
改动是**盲写直推**的，作者本地根本没有能力发现类型不匹配。

**根因 2：CI 从未编译过 Windows 代码。**
本仓库的 Windows 实现位于 `src/main.rs` 的

```rust
#[cfg(target_os = "windows")]
mod app;
```

之下，`src/app/**` 约 250 个文件（含出问题的 `vv_popup.rs`）全部受此门控。
而事故发生时的三个 workflow：

| workflow | 触发方式 | 实际执行 | 能否拦截本次事故 |
| --- | --- | --- | --- |
| `build-windows-exe.yml` | 仅 `workflow_dispatch` | Windows `cargo build --release` | ❌ PR 不触发，要人手动点 |
| `native-hosts.yml` | PR / push | **仅 macOS + Linux**，`cargo check --bin zsclip`，且 `RUSTFLAGS: -A warnings` | ❌ 平台不对，Windows 代码被 cfg 排除，压根不参与编译 |
| `release-packages.yml` | push 到 `main` / tag | Windows `cargo build --release` | ❌ 触发时坏代码**已经在主干上了** |

也就是说：**在合入之前，没有任何一个自动任务编译过 Windows 代码。**
同时，仓库里 44 个文件、753 个 `#[test]` **在任何 workflow 里都没有运行过**
（`--bin zsclip` 不含 `--all-targets`，测试代码不参与编译）。

`ci-check.yml` 就是用来堵这个洞的。

## 二、硬性规定

### 1. 本地没有 Rust 工具链时，禁止直推 `main`

这是**强制要求**，没有例外：

- 先执行 `rustc --version && cargo --version` 自检。
- **命令不存在 = 你无法验证自己的代码 = 不允许 `git push` 到 `main`。**
- 必须改走 Pull Request，由 `ci-check.yml` 编译，**等 CI 绿灯后再合入**。
- 即使改动"看起来只有几行"、"只是改个常量"也一样。事故那次是 4 行的类型
  转换问题，恰恰是人眼最容易漏掉、编译器一秒就能抓到的错误。

### 2. 有工具链时，推送前本地自查

```bash
cargo check --all-targets --locked          # 含测试代码
cargo test  --locked                        # debug 构建，debug_assert! 生效
cargo clippy --all-targets --all-features --locked
```

注意 `src/app/**` 是 Windows 专属代码，**在 macOS / Linux 上本地检查不会覆盖到它**。
非 Windows 开发机改动 `src/app/**` 后，必须依赖 PR 上的 CI 结果。

### 3. 依赖 `../zsui` 必须就位

`Cargo.toml` 中 `zsui = { path = "../zsui" }` 是 path 依赖，需要按
`zsui-revision.txt` 固定的 revision 检出到仓库同级目录，否则 cargo 无法解析依赖：

```bash
revision="$(tr -d '\r\n' < zsui-revision.txt)"
git init ../zsui
git -C ../zsui remote add origin https://github.com/qiu7824/zsui
git -C ../zsui fetch --depth 1 origin "$revision"
git -C ../zsui checkout --detach FETCH_HEAD
```

## 三、CI 门禁矩阵（`.github/workflows/ci-check.yml`）

触发：**Pull Request** + **push 到 `main`**。全部跑在 `windows-latest` 上。

| Job | 命令 | 类型 |
| --- | --- | --- |
| `windows-check` (default features) | `cargo check --all-targets --locked`<br>`cargo test --locked` | 🔴 **硬门禁** |
| `windows-check` (all features) | `cargo check --all-targets --locked --all-features`<br>`cargo test --locked --all-features` | 🔴 **硬门禁** |
| `windows-no-default-features` | `cargo check --all-targets --locked --no-default-features` | 🟡 软门禁 |
| `clippy` | `cargo clippy --all-targets --all-features --locked -- -D warnings` | 🟡 软门禁 |

### 关于 feature 覆盖

`Cargo.toml` 当前的 feature 列表：

```toml
default = ["vv-paste", "cloud-sync", "lan-sync", "mail-merge", "ai-actions", "sticker"]
```

6 个 feature **全部在 default 中**，因此今天 `--all-features` 与默认档编译产物相同。
保留两档是为了将来新增**非 default** feature 时矩阵已经就位，不必再改 workflow。

真正未被覆盖的组合是 `--no-default-features`，已作为软门禁纳入观察。

`--all-targets` + `cargo test` 这一组合尤其重要：它让 `debug_assert!` 生效。
例如 `src/app/vv_popup.rs` 中 `native_host_vv_select_specs` 的长度与 index 断言，
只在 debug 构建下有意义 —— `release-packages.yml` 的 `--release` 打包流程测不到。

### 为什么 clippy 是软门禁

存量代码有明确的告警债务：`native-hosts.yml` 全程设置 `RUSTFLAGS: -A warnings`
主动屏蔽告警，`src/` 下另有 115 处 `#[allow(...)]`。在这样的基线上直接
`-D warnings` 几乎必然大面积飘红。

**一个永远无法通过的门禁，最终一定会被人删掉或绕过，结果比没有更糟。**
所以这里的选择是「先亮灯、不拦路」，而不是为了让它绿而删掉检查。

现行约定：

> **存量告警待清理，新代码不得引入新告警。**

Reviewer 应当查看 clippy job 的输出，对比 PR 前后的告警数量；只要是本次改动引入的
clippy 告警，就要求在合入前修掉。存量告警清零后，删掉 job 上的
`continue-on-error: true` 即可升级为硬门禁。`windows-no-default-features` 同理。

## 四、建议的仓库设置

在 GitHub 仓库 Settings → Branches → `main` 的分支保护规则中：

1. 勾选 **Require a pull request before merging**（从机制上禁止直推）。
2. 勾选 **Require status checks to pass before merging**，并把以下两项设为必需：
   - `Windows default features`
   - `Windows all features`

软门禁 job 不要设为必需 —— 它们的作用是提供信息，不是拦路。

> `ci-check.yml` 刻意**没有配置 `paths` 过滤**。编译门禁应当对任何改动都重新验证；
> 而且一旦设为 required check，被 `paths` 跳过的运行会永远停在 pending，反而卡死 PR。

## 五、相关文档

- `docs/native-host-verification.md` —— macOS / Linux native host 的冒烟验证
- `docs/zsui-platform-matrix.md` —— 各平台 UI 能力矩阵
