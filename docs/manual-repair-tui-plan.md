# 人工补包与 Windows TUI 实施计划

状态：P1–P5 已实现；P6 自动化、ConPTY 与本地屏幕验证已有证据，完整双窗口及真实 RDP 人工操作验收待完成。日期：2026-10-02（Asia/Shanghai）。

实施记录：[本地验证与剩余验收](benchmarks/manual-repair-tui-20261002.md)。

本计划落实用户提出的人工补包与 send/recv TUI 偏好，是原有
[实施计划](plan.md)的增补，不代表功能或远程性能已经验收。

## 1. 目标和范围

接收端展示未完成组及秩缺口，用户手动将组号或请求码输入发送端。
发送端在当前会话内优先发送这些组的新修复包，然后回到自动调度。
无人操作时仍可通过原有单向通道恢复；不依赖 ACK、剪贴板共享、共享文件或网络。

首期交付两个 Windows 应用默认启用的英文 TUI、共享补包请求模型、定向调度及验证。
浏览器发送端保持兼容，人工补包网页交互留作后续；不更换 FEC、捕获 API、
视觉 profile，不顺带修改默认 64 包自动修复批次或加入压缩。

TUI 是控制界面。发送画面继续使用独立 Win32/GDI 窗口，不能用终端字符替代视觉帧。
用户须将控制终端放在视觉窗口之外；双窗口在 1280×720 和单屏远程桌面上都要验收。

## 2. 用户流程

### 启动及兼容性

交互终端默认启用 TUI，无需添加参数。stdin/stdout 任一不是交互终端时，默认使用
普通 CLI，保留原日志和自动退出行为。增加互斥的 `--tui`（显式要求）与 `--no-tui`
（强制普通 CLI）；自动化脚本显式使用 `--no-tui`，避免附带终端时意外等待交互。
模式在启动时确定，运行期间不自动切换。不增加文件选择向导；帮助与参数错误仍采用普通文本输出。

已实现命令：

```powershell
vft-send-win input.bin --profile balanced --fps 15 --topmost
vft-recv-win --auto --output recovered.bin
# Explicit CLI mode for scripts:
vft-recv-win --auto --output recovered.bin --no-tui
```

显式要求 TUI 但 stdin/stdout 不是交互终端时，清楚报错并提示去掉 `--tui`；
不向重定向输出写入终端控制序列。统计文件仍采用 create-new。

### 界面语言

两端所有应用生成的 TUI 标题、标签、状态、快捷键说明、帮助、预览、验证错误、
日志与完成摘要统一使用英文，不随系统区域设置切换。计划说明文档继续使用中文。
文件名、路径和用户输入保持原文并安全显示，不翻译或强制转为 ASCII。
OS 错误提供英文上下文及错误码；本地化的系统原始错误仅作为诊断详情，不能成为唯一提示。

### 接收端

```text
VFT Receive   session: 0123456789ABCDEF
Recovered: 332 / 336 groups    SHA: Pending
Captures: 60/s   Adjacent unique frames: 12.6/s   Rank gain: 6.1/s

Group      Rank / Total    Deficit
18         61 / 64         3
42         63 / 64         1
90         57 / 64         7
91         62 / 64         2

[R] Generate request  [Up/Down] Navigate  [Space] Select
[A] Select page  [PgUp/PgDn] Change page  [Q] Quit
```

- 默认按组号排列，只列出未完成组；未见任何包的组显示秩 0，不创建 FEC decoder。
- 展示全部组恢复进度和秩进度，但 SHA 验证前不能显示文件已成功或 verified goodput。
- R 默认生成全部未完成组的请求，超过单次限额则分页；已有选择时只使用选择项。
- 请求面板冻结本次文本，捕获和恢复继续运行；显示生成时间，明确这是进度快照。
- 可手抄简易组列表，也可复制完整请求码。首期依靠终端手动选择/粘贴能力，不要求
  应用访问系统剪贴板，更不假定远程剪贴板重定向可用。
- 新进度不改写正在抄录的请求；用户按 R 刷新并生成新请求。
- 已实现的 P 复制视图把完整码作为单一逻辑行打印，终端负责换行，不附加逐行边框；
  捕获继续，Tab 返回 TUI，Left/Right 切换请求页。完成/失败另行显示，不改写请求码。
- 全部组完成后显示 `Verifying SHA-256...`，校验和 create-new 写入成功后显示 `Complete` 及完成路径。
  TUI 保留结果等待退出；原 CLI 继续自动退出。校验失败或输出失败分别显示，保留失败退出码。

### 发送端

```text
VFT Send   input.bin   session: 0123456789ABCDEF
Phase: Automatic repair    Requested FPS: 15    Presented frames: ...
Source packets: ...    Automatic repairs: ...    Manual repairs: ...

[R] Enter repair request  [Space] Pause/Resume
[X] Cancel manual repair  [Q] Quit

Request: 18:3,42:1,90:7,91:2
Preview: 4 groups, 40 new repair packets
[Enter] Apply  [Esc] Cancel editing
```

- 编辑输入不自动暂停发送；需要静止画面时显式 Space。编辑框获得焦点时快捷键
  作为文本处理，Esc 只关闭编辑框。现有视觉窗口的 Space/ESC/F11 行为保留。
- Enter 校验后显示规范化目标和预算，再次 Enter 应用；这是本地操作预览，不涉及额外授权。
- 请求提交到传输线程成功后才显示 `Request applied`；校验失败、队列繁忙或会话已结束不清空输入。
- X 清空尚未发出的人工预算，不回滚已发送包编号；自动调度从自己的位置继续。
- 人工批次结束仅显示 `Manual repair batch sent`，发送端不能推断接收端已完成。
- Pause、Stop、窗口关闭都作用于同一个会话状态；不重新打开文件或生成新 session。

## 3. 请求语法与边界

界面和文本请求中的组号统一从 1 开始；解析后仅转换一次到内部从 0 开始的 generation。
冒号右侧永远表示秩缺口，不是修复包编号或实际发送数量。

| 输入 | 含义 |
| --- | --- |
| `18,42,90-91` | 每个目标组默认发送 16 个新修复包 |
| `18:3,42:1,90:7,91:2` | 按缺口计算预算 |
| `VFR1/<tag>/<request-id>/<entries>/<crc>` | 带会话绑定与校验的完整请求 |

完整请求示例中的 tag/id/crc 使用占位说明，不能将示意文字作为有效黄金向量。
P1 必须冻结如下文本契约并给出独立计算的真实向量：

- tag：对完整规范 manifest 编码计算 SHA-256，取前 8 字节，编码为 16 位十六进制；
  它覆盖 session、object 和元数据。作为误操作检测，不提供身份认证。
- request-id：接收适配器为每个新请求生成 64 位标识，使用操作系统随机源；核心仅接收值，
  不引入 OS API。16 位十六进制；重复显示同一快照保持同一 ID，重新生成使用新 ID。
- entries：按组号递增的十进制 `group:deficit` 列表；完整码只用这种唯一规范形式。
- crc：对 `VFR1/<tag>/<request-id>/<entries>` 的规范 ASCII 字节计算现有 CRC32C，
  输出 8 位十六进制。允许外层空白和十六进制大小写归一化；其他字符按严格语法验证。
- 简易输入允许逗号周围空白、范围以及带缺口单项；重复/重叠目标拒绝，避免预算含义不清。
  完整码不能在忽略校验的情况下退化为简易输入。
- 最多 4096 字节、256 个展开后的目标；先检查范围长度再展开。
  组号必须存在，缺口在 `1..=该组 k`，拒绝 0、反向范围、溢出、未知版本和空请求。
- 缺口预算首期为 `max(8, ceil(1.5 × deficit) + 4)`，用整数计算；缺 3、1、7、2
  的预算分别为 9、8、15、8，总计 40。预算是启发式，不保证恢复，差链路可以生成新请求。
- 每组最多 128 个、每请求最多 32768 个修复包，所有计算 checked。
- 完整码校验当前 manifest tag；简易输入预览醒目标注当前文件与 tag，用户自行核对。
- 保存最近 64 个已接受的完整请求 ID；同一 ID/内容再次提交为 no-op，ID 相同而内容不同拒绝。
  保留窗口外再次提交可能产生额外修复，但不能复用修复编号。简易输入每次明确应用可再次补包。
- 首期只允许一个活动人工请求；已有请求时不静默替换，提示先等待或取消。
  请求覆盖的组后来已经恢复不会造成错误，只会多发一些包。

## 4. 核心实现

### 接收进度

在 `vft-receiver` 增加只读、可分页/迭代的 generation progress API：
`generation, source_count, rank, complete`。不得暴露内部行数据或复制完整文件。
完整组的秩为 k，未分配 decoder 的组为 0，最后一组使用实际 k；无 manifest 和空文件
都返回明确的空状态。TUI 从该 API 构建最多 256 项的快照。

### 定向发送

在 `vft-sender` 分离三个概念：自动调度位置、每组下一个修复编号、累计统计。
不能继续用累计 `repair_symbols` 同时推导自动位置和包编号，否则插入人工包会改变轮次或复用编号。

- 每组从 k 开始分配修复索引；自动和人工路径共用单调分配器，checked overflow，永不回绕。
- 无人工请求时保持现有 manifest/source/64-repair 序列；加入请求后自动位置独立前进，
  自动修复使用该组尚未分配的下一个索引。
- 人工请求可打断原始包阶段；原始包游标保持，恢复时不能遗漏或重发原始包。
- Manifest 仍具有最高优先级，按全局帧计数每 30 帧发送，不能被人工请求饿死。
- 人工目标每组连续最多 8 个包，再轮到下一个目标；每 32 个人工数据包插入
  1 个普通调度数据包，保证持续人工操作也不会永久阻断普通传输。manifest 不计入该配额。
- 每组游标元数据按 manifest 组数计费，人工模式相关元数据预算上限 8 MiB，
  checked allocation；可以在第一次请求时延迟构建并从自动进度推导各组已用编号。
  超限时拒绝人工模式，保留现有自动发送能力，不加载全文件。
- 仍只缓存一组源数据。记录组切换和读取字节数，衡量短批次代价；不引入全文件缓存。
- 请求解析、范围/预算检查及资源预留全部成功后再原子应用。
  读取或编码失败不得消耗人工预算、修复索引或自动游标。
- 排队的是组号/数量等小命令，不是大量预生成帧。暂停保留队列，停止释放队列。

共享请求解析/格式化放在新的 `vft-protocol::repair_request` 模块，维持 `no_std + alloc`。
这是独立的人工输入文本契约，不能借用线上 Control 包暗增自动反馈。
现有 packet、manifest、FEC、视觉格式及黄金字节不变，旧接收器能接收新调度产生的包。

## 5. TUI 技术与线程边界

优先使用 Ratatui + Crossterm，具体版本在 P3 以 Rust 1.85 实测后锁定；
不直接假定最新版本满足 MSRV，也不为 TUI 升级仓库最低编译器。
只在 Windows 适配层加入依赖，不流入五个共享库或 WASM 构建。

新增 `apps/vft-tui` 工作区辅助库，复用终端 RAII、输入编辑、布局、日志和共享视图模型；
终端依赖设为 Windows target-specific，其他目标可构建无终端后端的公共模型。
不新增异步运行时。官方资料：
[Ratatui 后端](https://ratatui.rs/concepts/backends/)、
[Crossterm event](https://docs.rs/crossterm/latest/crossterm/event/index.html)。
Crossterm 的 poll/read 必须由同一个输入线程调用。

- Sender：现有 Win32 主线程继续独占 HWND、GDI 和 SenderSession；TUI 线程独占终端，
  通过有界命令通道和私有唤醒消息通知主线程，在安全消息边界执行命令。
  不跨线程移动 Win32/GDI 资源，不在回调中阻塞等待终端。
- Receiver：捕获/恢复工作线程独占 Capture 和 ReceiverSession，TUI 主线程处理输入/渲染；
  所有 GDI 资源在其所属线程创建和销毁。控制命令在捕获迭代边界消费。
- 命令队列建议上限 32，满时 UI 显示繁忙，不丢弃 Stop 或应用结果；停止用独立取消信号。
  进度采用覆盖式最新快照，不积压旧快照、不传输图像。普通刷新 5 Hz，日志环形缓冲 200 条。
- 明确 Starting/Running/Paused/Stopping/Complete/Failed 生命周期。
  退出时通知工作线程、停止计时器/窗口、等待线程、恢复终端，再打印最终摘要。
- RAII 恢复 raw mode、alternate screen、光标及启用过的粘贴模式；覆盖正常退出、Ctrl+C、
  错误和可展开 panic。强制终止进程不承诺清理。保留已有 FFI panic containment 和 reentry guard。
- 编辑器支持退格、方向键、Home/End、粘贴和长度限制；单行输入拒绝控制字符，
  文件名/错误文本中的控制字符转义，避免破坏终端。兼容无 bracketed paste 的逐键输入。
- 100×28 使用完整布局，80×24 使用紧凑布局，更小终端提示扩容但仍可退出/暂停。
  不依赖 emoji、颜色或中文宽度表达关键状态；测试 Windows Terminal 与传统控制台。
- 终端日志只经 UI 写出，不让后台 println/eprintln 打断界面；CLI 模式保留原输出。

## 6. 分阶段检查点

| 阶段 | 工作与主要路径 | 验收与本地提交 |
| --- | --- | --- |
| P1 | `vft-receiver` 组进度；`vft-protocol` 请求契约、解析/格式化 | 最后一组、未见组、完成组；格式黄金向量、CRC/tag、范围/上限/溢出；`feat(protocol): add manual repair requests` |
| P2 | `vft-sender` 定向队列、独立自动游标、共用修复编号 | 无请求序列不变，混合请求编号不重复，暂停/取消/失败不丢状态，manifest 公平性；`feat(sender): prioritize requested repair generations` |
| P3 | `apps/vft-tui`、两应用生命周期接入、默认英文 TUI、`--tui/--no-tui` | Rust 1.85 和依赖锁定；终端检测/模式优先级、英文文案、终端恢复、resize、粘贴、关闭/错误、日志隔离；`feat(tui): add shared Windows terminal controls` |
| P4 | Receiver TUI 缺失表、分页/选择、请求快照 | 捕获不因编辑/展示阻塞，无 manifest/完成无请求，超限分页；`feat(win-receiver): show manual repair requests` |
| P5 | Sender TUI 输入、预览、应用/取消/进度 | 两种语法、跨会话拒绝、重复码 no-op、队列繁忙、输入不中断画面；`feat(win-sender): accept manual repair requests` |
| P6 | 测试工具显式指定 `--no-tui`、桌面验证、使用文档与性能记录 | SHA 独立验证、CLI 回归、远程人工操作计时；`test(e2e): verify TUI assisted repair` |

每个逻辑单元验证并审查 diff 后本地 checkpoint，不自动 push。
实现时更新 README、architecture、protocol、status、future-platforms，明确人工反馈已支持的范围。
P1–P5 已落实；仅按已执行的检查更新状态，剩余人工/远程验收不标记完成。

## 7. 测试及性能验收

### 自动化

- 请求模型：真实独立黄金向量、roundtrip、错 session/CRC、重复项/请求、巨大范围、
  Unicode/控制字符、截断、上限边界、空文件和最后短组。
- 调度：请求在 source/repair/manifest 边界插入；重复、取消再申请、同组跨路径、
  `u32` 索引耗尽、source 读取失败重试。编号唯一不等于方程必然独立，测试不得混淆。
- FEC/E2E：种子化随机丢包、突发丢包、重复/乱序、修复包中途加入；两种 profile。
  选择早期与末尾缺失组，比较相同信道轨迹下有/无人工请求的 SHA 完成包数和尾部延迟。
- 公平性：持续请求时原始包最终发完、所有组仍能得到普通修复，manifest 间隔保持。
- TUI：输入状态机、渲染缓冲测试覆盖关键数据与截断，避免只测试布局实现细节；
  Windows 交互测试覆盖 raw mode 清理、Ctrl+C、视觉窗口关闭和接收输出失败。
- 模式矩阵：交互终端无参数默认 TUI、交互终端 `--no-tui` 使用 CLI、重定向默认 CLI、
  非交互环境显式 `--tui` 报错、两个开关同时出现报错；脚本成功后不等待 TUI 退出。
- 语言验收：英文/中文系统区域设置下应用 TUI 文案均为英文，中文文件名和路径仍正确显示；
  帮助、请求错误、队列忙、SHA 失败、输出失败和终端过小提示全部纳入检查。
- 共享发送器修改后执行 web headless 兼容测试；浏览器人工输入不在首期交付范围。

实现检查命令：

```powershell
cargo fmt --all
python scripts/check.py
git diff --check
cargo check -p vft-protocol -p vft-fec -p vft-visual -p vft-sender -p vft-receiver --no-default-features
cargo +1.85.0 check --workspace --locked
cargo build --release --workspace
python scripts/local-e2e.py --sizes 1024 1048576 --fps 60 --cell-size 4
python scripts/check-web.py
```

### 桌面和真实远程

1. 两应用在交互终端不带 `--tui` 启动，确认默认英文界面；分别在传输期间 resize、输入、
   粘贴、暂停/恢复；确认终端不遮挡视觉帧。
2. 本地制造可控漏采，在未完成组列表生成请求；通过真正的发送端 TUI 输入完成恢复，
   单独的核心 API 注入测试不能替代这一交互验证。
3. 验证自动定位、移动后重捕获、两种 profile、CLI 模式、错误输出路径、取消请求后自动恢复。
4. 真实 RDP 对照自动基线与人工定向补包，覆盖 1 MiB 和 100 MiB；关键配置至少三次，
   保留失败诊断。把“最后 10% 到 SHA 完成”作为重点，但也记录完整传输时间。
5. 分别记录人工抄录/输入时间、请求接受到首个定向包时间、接受到完成时间、CPU/内存、
   实际相邻不同有效帧率、增秩、自动/人工发送数、组读取开销；不把请求 FPS 当实际帧率。
   最终 goodput 包含人工操作造成的墙钟等待，失败为零；输出 SHA 独立比较。

发布验收要求功能正确、无反馈仍工作、终端稳定恢复、现有 CLI/WASM 兼容。
性能提升只有真实测量后才能填写；用户手工操作时间可能抵消小文件收益。
本功能完成不自动等同 M14 完成，也不保证达到远程 40–100 KB/s 目标。
