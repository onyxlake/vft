# VFT — Visual File Transfer

用户请求的后续增补：[人工补包与 Windows TUI 实施计划](manual-repair-tui-plan.md)
（2026-10-02，已实现核心和 Windows TUI；本地验证已记录，真实远程人工操作验收待完成）。原始里程碑与验收记录保持不变。

## 1. 项目目标

VFT 是一个以 Rust 为核心实现的单向视觉文件传输协议与工具集。

主要传输链路：

```text
File
  ↓
Sender
  ↓
Visual Frames
  ↓
Remote Desktop / Screen / Video Codec
  ↓
Receiver Capture
  ↓
Decode + FEC
  ↓
Recovered File
```

首要使用场景：

- Sender 与 Receiver 通常无法直接网络通信。
- 默认不存在双向 ACK。
- 中间可能经过 RDP、远程桌面软件、H.264/H.265/AV1 等视频压缩链路。
- Receiver 可能面对重复帧、漏帧、压缩失真和缩放。
- 某些环境可能存在可选的 remote-file-system 反馈，但协议不得依赖该反馈。

核心原则：

> Simplex first, feedback optional.

---

# 2. 第一阶段支持范围

当前开发阶段只实现：

```text
Shared Rust Libraries
+
Windows Sender
+
Windows Receiver
```

目标平台：

```text
Windows 10
Windows 11
x64 first
```

暂不实现：

```text
Linux Sender
Linux Receiver
Android Receiver
WASM Sender
WASM Receiver
```

但公共库 API 必须避免做出阻碍这些平台的设计。

---

# 3. 最终仓库结构

初始 workspace：

```text
vft/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── .gitignore
├── rustfmt.toml
├── clippy.toml
│
├── crates/
│   ├── vft-protocol/
│   ├── vft-fec/
│   ├── vft-visual/
│   ├── vft-sender/
│   ├── vft-receiver/
│   └── vft-testkit/
│
├── apps/
│   ├── vft-send-win/
│   └── vft-recv-win/
│
├── tests/
│   ├── golden/
│   └── fixtures/
│
└── docs/
    ├── protocol.md
    ├── visual-format.md
    ├── architecture.md
    └── future-platforms.md
```

后期再加入：

```text
apps/
├── vft-send-linux/
├── vft-recv-linux/
└── vft-web/

mobile/
└── android/
```

---

# 4. Library 分层

## `vft-protocol`

职责：

```text
Protocol version
Session ID
Object ID
Manifest
Visual packet header
Symbol ID
Encoding profile
CRC
serialization/deserialization
```

要求：

- 不依赖 UI。
- 不依赖文件系统。
- 不依赖 Windows API。
- 尽量保持 `no_std + alloc` 可行性。
- 明确定义 wire format。
- 所有整数明确 endian。
- 协议结构必须有 golden test vectors。

初期协议数据：

```text
ProtocolVersion
SessionId
ObjectId
PacketType
SymbolId
ProfileId
PayloadLength
Payload
CRC32C
```

Packet 类型至少：

```text
Manifest
SourceSymbol
RepairSymbol
Control
```

---

## `vft-fec`

职责：

```text
file → source blocks
source blocks → repair symbols

received symbols
→ recovery
→ original blocks
```

第一版本不追求完整 RaptorQ。

优先实现简单、确定性的 systematic fountain-like codec：

```text
source symbols
+
deterministic repair symbols
```

约束：

```text
same manifest
+
same symbol ID
=
same repair symbol
```

必须跨平台、跨编译保持 bit-identical。

未来可替换为：

```text
RaptorQ
Raptor
更高级 fountain/FEC
```

但 `vft-sender` / `vft-receiver` 不应该依赖具体算法实现细节。

建议抽象：

```rust
trait FecEncoder
trait FecDecoder
```

---

## `vft-visual`

职责：

```text
protocol packet
↓
logical visual grid
```

以及：

```text
normalized sampled grid
↓
protocol packet
```

不负责：

```text
Win32 drawing
screen capture
camera capture
filesystem
```

初始 Profile：

```text
Profile 0:
binary
large cells
maximum robustness

Profile 1:
4-level luminance
8x8 / configurable cell
balanced

Profile 2:
future experimental
```

Logical visual frame 示例：

```text
+-----------------------------+
| sync | metadata | sync      |
+-----------------------------+
| calibration / timing        |
+-----------------------------+
|                             |
|         DATA GRID           |
|                             |
+-----------------------------+
| sync / timing / frame mark  |
+-----------------------------+
```

Visual frame 必须：

- 自描述。
- 不依赖前一帧。
- 可以检测重复帧。
- 可以跳过损坏帧。
- 可以在中途加入传输。

---

## `vft-sender`

纯 Rust sender state machine。

职责：

```text
input object metadata
input data chunks
FEC
packet scheduling
manifest repetition
visual frame generation
```

不得知道：

```text
Win32
GDI
monitor
window
keyboard
```

主要 API 目标：

```rust
SenderSession::new(...)
SenderSession::next_frame(...)
SenderSession::progress(...)
```

Sender 调度初始策略：

```text
Manifest
Source Symbols
Repair Symbols
Repair Symbols
...
Periodic Manifest
...
```

没有 ACK 时：

```text
持续生成新的 repair symbols
```

以后 optional feedback 可以：

```text
DONE
progress
profile request
```

但不属于第一阶段必要功能。

---

## `vft-receiver`

纯 Rust receiver state machine。

职责：

```text
decoded visual packets
duplicate elimination
CRC validation
manifest management
FEC recovery
progress tracking
object verification
```

不负责：

```text
desktop capture
Win32 window
saving dialog
camera APIs
```

核心接口目标：

```rust
ReceiverSession::push_packet(...)
ReceiverSession::progress(...)
ReceiverSession::is_complete(...)
ReceiverSession::reconstruct(...)
```

后期再加入图像层：

```text
raw pixels
→ locator
→ sampling
→ visual symbols
```

第一阶段允许先假定 Windows receiver 已经知道 sender 窗口区域，以减少 vision complexity。

---

## `vft-testkit`

用于：

```text
packet corruption
frame drop
frame duplication
symbol errors
video-like blur
scaling
noise
```

目标是能够模拟：

```text
Sender
↓
bad transport
↓
Receiver
```

而不启动真实 GUI。

这是第一阶段的重要组件，而不是后期附加功能。

---

# 5. Windows Sender

位置：

```text
apps/vft-send-win
```

技术：

```text
Rust
windows crate
Win32 API
GDI / DIB framebuffer
```

避免：

```text
Qt
Electron
Tauri
wgpu
SDL
OpenCV
大型 GUI framework
```

第一版 CLI：

```text
vft-send-win.exe file.bin
```

参数未来支持：

```text
--profile balanced
--fps 15
--cell-size 8
--fullscreen
--monitor 1
```

交互：

```text
ESC    exit
SPACE  pause
F11    fullscreen
```

显示逻辑：

```text
VisualFrame
↓
software RGB framebuffer
↓
DIB
↓
StretchDIBits
```

必须避免逐 cell 调 GDI drawing API。

---

# 6. Windows Receiver

位置：

```text
apps/vft-recv-win
```

Phase 1 capture 可以从简单方案开始。

优先顺序：

```text
1. 已知矩形区域截图
2. 指定窗口捕获
3. Desktop Duplication / Windows Graphics Capture
```

Receiver pipeline：

```text
desktop frame
↓
crop
↓
visual frame normalization
↓
cell sampling
↓
vft-visual
↓
vft-protocol
↓
vft-receiver
↓
FEC
↓
file
```

输出：

```text
filename
session ID
received symbols
valid symbols
duplicate symbols
invalid frames
FEC progress
recovered bytes
hash status
```

第一阶段允许 CLI + 简单 Win32 preview，不要求完整 GUI。

---

# 7. 第一版 Visual Profile

## Robust

```text
binary luminance
16x16 cells
5-10 FPS
high FEC redundancy
```

用途：

```text
low bitrate remote desktop
heavy compression
bad scaling
```

---

## Balanced

第一版主要目标：

```text
4-level luminance
8x8 cells
10-15 FPS
moderate FEC
```

目标环境：

```text
1080p remote desktop
5-15 Mbps video stream
```

初始 goodput 目标：

```text
40-100 KB/s
```

重点不是达到理论最大速度，而是：

```text
stable
recoverable
measurable
```

---

# 8. Codec-Friendly 原则

Visual encoding 必须针对视频编码器设计。

避免：

```text
random pixel noise
1px patterns
high-frequency checkerboards
heavy color dependence
```

优先：

```text
large uniform blocks
luminance encoding
8x8 / 16x16 alignment
limited symbol levels
partial frame updates
stable borders
```

原因：

```text
H.264
H.265
AV1
Remote Desktop codec
```

都更喜欢低空间频率和时间连续性。

---

# 9. Manifest

Manifest 必须周期重复发送。

包含：

```text
protocol version
session ID
object ID
filename
file size
SHA-256
source block size
source block count
FEC type
visual profile
compression information
```

Receiver 必须支持：

```text
从传输中间开始捕获
↓
等待下一个 manifest
↓
开始恢复
```

---

# 10. 帧设计要求

每个 frame 必须独立拥有：

```text
magic
version
session
packet type
symbol ID
profile
payload length
CRC
```

禁止依赖：

```text
capture frame number
previous frame state
arrival order
```

Receiver 必须正确处理：

```text
A
A
B
B
D
D
```

这种远程桌面输出。

Duplicate：

```text
直接丢弃
```

Missing：

```text
由 FEC 恢复
```

---

# 11. 完整性

Packet：

```text
CRC32C
```

Object：

```text
SHA-256
```

最终文件只有：

```text
FEC reconstruction success
+
SHA-256 match
```

才标记为 COMPLETE。

---

# 12. Git 开发策略

从空 workspace 开始：

```bash
git init
```

开发过程中采用：

> 小步实现 + 自动 checkpoint commit

但只允许 commit 已经：

```text
cargo fmt
cargo check
cargo test
```

通过的状态。

---

# 13. Commit 规则

采用 Conventional Commits 风格。

例如：

```text
chore: initialize rust workspace

feat(protocol): define packet header format

test(protocol): add golden packet vectors

feat(fec): add deterministic source symbols

feat(fec): add repair symbol generation

feat(visual): add binary grid profile

feat(sender): add sender session state machine

feat(win-sender): add win32 framebuffer renderer

feat(receiver): add receiver session tracking

feat(win-receiver): add desktop crop capture

test(e2e): recover file with dropped frames

docs: document protocol v0
```

不要：

```text
update
fix stuff
wip
changes
```

---

# 14. 自动 Commit 策略

开发 agent 可以自动提交，但遵守：

```text
一个 logical milestone
=
一个 commit
```

推荐步骤：

```text
implement
↓
cargo fmt --all -- --check
↓
cargo clippy --workspace --all-targets -- -D warnings
↓
cargo test --workspace
↓
git diff review
↓
git commit
```

禁止自动：

```text
git push
git rebase remote branches
git reset --hard unrelated user work
git clean -fdx
force push
```

除非明确授权。

---

# 15. Development Milestones

## M0 — Workspace Bootstrap

目标：

```text
empty git repository
→ compiling Rust workspace
```

完成：

```text
Cargo workspace
README
.gitignore
format/lint setup
crate skeletons
```

Commit：

```text
chore: initialize vft rust workspace
```

---

## M1 — Protocol v0

实现：

```text
protocol version
packet header
manifest
serialization
CRC32C
golden vectors
```

验收：

```text
encode
→ bytes
→ decode
=
same structure
```

Commit：

```text
feat(protocol): implement protocol v0 packet format
```

---

## M2 — Block Model

实现：

```text
file segmentation
source symbol model
object metadata
```

支持：

```text
small file
large file streaming
```

不得要求整个文件永久载入内存。

Commit：

```text
feat(protocol): add source block model
```

---

## M3 — Fountain/FEC v0

实现：

```text
systematic source symbols
deterministic repair symbols
receiver recovery
```

模拟：

```text
drop 10%
drop 20%
duplicates
random reorder
```

验收：

```text
最终 SHA-256 一致
```

Commit 可拆：

```text
feat(fec): add deterministic repair symbol encoder

feat(fec): add decoder recovery

test(fec): cover loss duplicate and reorder scenarios
```

---

## M4 — Logical Visual Codec

实现：

```text
VisualFrame
sync area
metadata area
data cells
binary profile
4-level profile
```

此时不需要 GUI。

测试：

```text
packet
→ grid
→ sampled grid
→ packet
```

Commit：

```text
feat(visual): implement visual frame codec
```

---

## M5 — Distortion Simulator

实现：

```text
resize
blur
brightness change
contrast change
frame drop
frame duplication
block corruption
```

目的：

在真实 Remote Desktop 测试之前发现问题。

Commit：

```text
feat(testkit): add visual transport distortion simulator
```

---

## M6 — Sender Core

实现：

```text
manifest scheduling
source scheduling
repair scheduling
frame sequence
```

测试：

```text
sender → synthetic transport → receiver
```

Commit：

```text
feat(sender): implement simplex sender session
```

---

## M7 — Receiver Core

实现：

```text
session discovery
manifest handling
duplicate suppression
CRC validation
FEC integration
progress
reconstruction
```

Commit：

```text
feat(receiver): implement receiver session
```

---

## M8 — Headless End-to-End

这是第一个重要 milestone。

运行：

```text
input.bin
↓
sender-core
↓
visual frames
↓
simulated drop/duplicate/distortion
↓
receiver-core
↓
output.bin
```

验证：

```text
SHA256(input) == SHA256(output)
```

Commit：

```text
test(e2e): recover files through simulated visual transport
```

---

# 16. Windows Application Milestones

## M9 — Windows Sender Skeleton

实现：

```text
WinMain
Win32 window
fullscreen
keyboard handling
software framebuffer
DIB
```

先显示 test pattern。

Commit：

```text
feat(win-sender): add win32 framebuffer application
```

---

## M10 — Windows Sender Integration

连接：

```text
vft-sender
+
vft-visual
+
Win32 renderer
```

支持：

```text
vft-send-win.exe file.bin
```

UI 显示：

```text
filename
size
FPS
profile
symbol ID
```

Commit：

```text
feat(win-sender): stream files as visual frames
```

---

## M11 — Windows Receiver Capture

第一版：

```text
manual screen rectangle
```

以后再自动窗口识别。

实现：

```text
capture
crop
preview
```

Commit：

```text
feat(win-receiver): add desktop region capture
```

---

## M12 — Windows Visual Detection

实现：

```text
sync detection
grid alignment
luminance calibration
cell sampling
```

Commit：

```text
feat(win-receiver): decode captured visual frames
```

---

## M13 — Real Windows E2E

测试：

```text
sender window
↓
local screen
↓
receiver capture
↓
output file
```

必须成功恢复：

```text
1 KB
1 MB
100 MB
```

Commit：

```text
test(win): verify local screen file transfer
```

---

# 17. Remote Desktop Validation

## M14 — RDP / Remote Desktop Tests

开始真实测试：

```text
5 FPS
10 FPS
15 FPS
30 FPS
```

Profiles：

```text
binary 16x16
binary 8x8
4-level 16x16
4-level 8x8
```

收集：

```text
video bitrate
visual FPS
unique frame rate
packet error rate
symbol error rate
FEC overhead
payload goodput
CPU usage
```

不要只记录理论 bitrate。

核心指标：

```text
verified payload goodput
```

即：

```text
最终成功恢复的数据量 / 实际传输时间
```

Commit：

```text
test(remote): add remote desktop transport benchmarks
```

---

# 18. Adaptive Profile

只有完成实际 benchmark 后再做。

Receiver 统计：

```text
frame loss
packet CRC failure
symbol confidence
```

未来可通过：

```text
optional filesystem side channel
```

请求 sender：

```text
robust
balanced
fast
```

但第一版本：

```text
manual profile selection
```

即可。

---

# 19. 第一阶段 Done Definition

第一阶段只有满足以下条件才算完成：

```text
✓ Rust workspace clean
✓ shared protocol library
✓ deterministic FEC
✓ logical visual codec
✓ simulation testkit
✓ sender core
✓ receiver core

✓ Windows native sender
✓ Windows native receiver

✓ no required two-way communication
✓ receiver handles dropped frames
✓ receiver handles duplicated frames
✓ receiver can join mid-stream
✓ final SHA-256 validation

✓ local Windows E2E passes
✓ real remote desktop E2E passes

✓ benchmark results documented
```

并且：

```text
cargo fmt --check
cargo clippy
cargo test
```

全部成功。

---

# 20. 暂不实现

本阶段明确不要因为“以后可能需要”提前实现：

```text
Linux UI
Wayland capture
X11 capture
Android UI
CameraX
JNI bindings
WASM UI
browser camera
WebCodecs
QR compatibility
JAB compatibility
RaptorQ full implementation
encryption
multi-file archive
bidirectional ARQ
network transport
plugin system
GPU acceleration
```

避免 scope creep。

---

# 21. Future Plan

## Linux

Sender：

```text
winit + softbuffer
```

Receiver：

```text
PipeWire / Wayland
X11 fallback
```

共享：

```text
vft-sender
vft-receiver
vft-visual
```

---

## WASM Sender

高优先级未来目标。

架构：

```text
Browser File API
↓
Rust/WASM sender core
↓
Canvas
↓
Fullscreen
```

用途：

```text
无需安装 exe
无需 native dependencies
```

---

## WASM Receiver

未来实验性功能：

```text
getUserMedia
VideoFrame
WASM decoder
```

低于 WASM sender 优先级。

---

## Android Receiver

架构：

```text
Kotlin
CameraX
↓
Y plane
↓
JNI
↓
Rust receiver-core
```

重点：

```text
direct luminance processing
```

避免不必要的 YUV→RGB 转换。

---

## Optional Feedback Channel

未来支持：

```text
remote filesystem
```

Receiver 可以写：

```text
session.done
session.status
```

Sender 可读取：

```text
DONE
progress
requested profile
```

但 visual protocol 永远保持：

```text
feedback optional
```

---

# 22. Agent 开发行为

自动开发过程中：

1. 阅读当前 Git 状态。
2. 不覆盖用户已有改动。
3. 每次只完成一个 milestone 或 logical unit。
4. 运行完整相关测试。
5. 通过后自动 commit。
6. Commit message 必须描述真实改动。
7. 不 push。
8. 不 force reset。
9. 不隐藏失败测试。
10. 遇到设计问题优先保持 protocol/core 简单。

如果某个 milestone 很大，则拆成多个可测试 commit。

---

# 23. 第一批建议 Commit

仓库从空目录开始时，可以按这个顺序：

```text
1  chore: initialize vft rust workspace

2  feat(protocol): define protocol v0 structures

3  feat(protocol): implement packet serialization and crc

4  test(protocol): add golden protocol vectors

5  feat(fec): add source block model

6  feat(fec): add deterministic repair symbols

7  feat(fec): implement receiver recovery

8  test(fec): cover loss duplicate and reorder cases

9  feat(visual): add logical visual frame format

10 feat(visual): add binary luminance profile

11 feat(visual): add four-level luminance profile

12 feat(testkit): add transport degradation simulator

13 feat(sender): implement simplex sender session

14 feat(receiver): implement receiver session

15 test(e2e): recover file through simulated transport

16 feat(win-sender): add native win32 renderer

17 feat(win-sender): integrate visual sender

18 feat(win-receiver): add desktop region capture

19 feat(win-receiver): add visual frame detection

20 feat(win-receiver): recover transferred files

21 test(win): verify local screen transfer

22 test(remote): benchmark remote desktop transport

23 docs: document protocol v0 and benchmark results
```

---

# 24. 当前开发主线

在 Windows Remote Desktop E2E 成功以前，只沿着：

```text
Protocol
   ↓
FEC
   ↓
Visual Codec
   ↓
Simulation
   ↓
Sender Core
   ↓
Receiver Core
   ↓
Windows Sender
   ↓
Windows Receiver
   ↓
Remote Desktop Benchmark
```

推进。

Linux / Android / WASM：

```text
只保证架构兼容
只写 future plan
当前不实现
```

这样可以避免在真正验证“视觉信道是否值得用”之前，把大量开发时间消耗在平台适配上。
