# 启动器应用图标设计

## 状态

- 日期：2026-07-21
- 状态：产品边界已批准，等待规格复核
- 实现基线：本地 `main` at `291705ff4793cb4e56a878aa099b50cc26d5d231`
- 基线说明：规格提交保持上述基线；实施前必须在用户批准后 fail-closed 校准到最新干净本地 `main`，不得直接在本规格分支开始实现
- 影响范围：Windows 应用扫描、内存应用快照、搜索结果投影与启动器结果行渲染

## 目标与边界

启动器搜索结果在每一行左侧显示 Windows Shell 提供的真实应用图标，同时覆盖 Start Menu `.lnk` 桌面快捷方式与 AppsFolder 打包应用。真实图标不可用时保留该应用，并显示统一的 CSS 默认应用符号。图标容器固定为 `28 x 28`，在结果项内垂直居中；标题、副标题、选中、键盘导航、滚动和启动行为保持不变。

本功能复用现有 `Application.icon -> ResultItem.icon -> protocol icon?: string` 字段，不新增前后端字段、命令或状态库。图标只存在于现有内存 `Application` 快照和由此产生的 DTO 中，不写入磁盘，不建立独立缓存，不按可见结果懒加载。

## 现状链路

协议和领域模型已经预留图标，但当前链路没有实际传递：

1. `src-tauri/src/apps/mod.rs` 的 `Application` 已有 `icon: Option<String>`。
2. `src-tauri/src/model.rs` 的 `ResultItem` 和 `src/protocol.ts` 的 `ResultItem`、`AppAliasTarget` 已有可选 `icon`。
3. `src-tauri/src/apps/discovery.rs` 的 `.lnk` 发现和 `src-tauri/src/apps/appsfolder.rs` 的打包应用发现都固定填入 `icon: None`。
4. `src-tauri/src/apps/discovery.rs::registry_entry` 又固定生成 `ResultItem.icon = None`，即使未来 `Application` 有值也会丢失。
5. `src/launcher-core.ts` 的搜索响应投影不读取 `item.icon`，公开快照的 `ViewResult` 也没有图标字段。
6. `src/launcher-view.tsx` 对每条搜索结果只画空方框 `.app-mark`；该样式当前是 `22 x 22`、顶部对齐并带顶部间距。
7. 设置加载 DTO 已自然投影 `Application.icon`，但设置页 core 和视图不使用它。本需求允许该安全值继续随既有字段传递，但不在设置页渲染真实图标。

规格编写期间只读复核了后继主线 `ba61fba75a00f1aa59e6157a9e3ff0de7376ac97`：该提交新增文件搜索域、`OpenIndexedPath` 动作和应用/文件共用的结果注册表，但未改变上述应用图标链路。图标只属于应用搜索结果，不进入文件搜索 DTO，也不改变共享注册表的 query domain、序列或动作解析规则。

因此完整修正点是“发现时提取、Rust 结果投影保留、前端 core 验证后保留、视图安全渲染”，而不是新增协议或查询图标的旁路。

## 方案选择

### 采用：扫描期 Shell image factory + WIC + 内存 data URL

在现有后台应用扫描期间，从入口本身的 `IShellItem` 获取图标：

- 桌面快捷方式：从 `.lnk` 路径创建 Shell item，再提取该入口的图标，不改为读取目标可执行文件图标。
- 打包应用：复用 AppsFolder 枚举当前项的 `IShellItem`，在该 COM 对象仍位于扫描线程和当前 apartment 时提取。

两个来源统一转换为带 alpha 的 32 x 32 PNG，再编码成内存 data URL，写入对应 `Application.icon`。微软说明 `GetImage` 的图标提取可能耗时且不应在 UI 线程执行；扫描期提取与现有后台刷新线程边界一致。[IShellItemImageFactory::GetImage](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishellitemimagefactory-getimage)

### 拒绝：搜索结果逐项调用图标命令

这会新增协议、请求状态和竞态；搜索结果先出现默认图标再异步替换还会产生闪烁与行重绘。它也会把 Shell 提取时延转移到用户每次搜索，而不是现有低频刷新事务。

### 拒绝：向前端暴露图标路径

`.lnk`、可执行文件和包安装路径属于后端私有启动信息，不应进入 WebView DTO。打包应用也没有与桌面入口统一且稳定的公开图标路径；`file:` 或任意 URL 会扩大 CSP、路径隐私和资源加载边界。

## 模块边界与数据流

新增最小 Windows 图标模块 `src-tauri/src/apps/icon.rs`，只负责把一个可信 Shell item 转成受限的 `Option<String>` 图标值。它不拥有应用排序、身份、缓存或 DTO 逻辑。模块提供两个窄入口：从 `.lnk` 路径创建 Shell item后提取，以及从调用者已有的 AppsFolder `IShellItem` 提取；二者共享相同的 HBITMAP、WIC PNG 和 Base64 转换路径。

数据流固定为：

```text
后台 app discovery
  -> .lnk 路径创建 IShellItem / AppsFolder 复用 IShellItem
  -> IShellItemImageFactory 请求 32 x 32、SIIGBF_ICONONLY
  -> HBITMAP 所有权守卫
  -> WIC 生成带 alpha PNG 到定长内存流
  -> Windows CryptBinaryToStringW 生成无换行 Base64
  -> Application.icon
  -> registry_entry 复制到 ResultItem.icon
  -> launcher-core 严格验证并复制到 ViewResult.icon
  -> launcher-view 装饰性 img；失败时露出 CSS 默认图标
```

具体责任如下：

- `src-tauri/src/apps/icon.rs`：Shell item 获取、位图所有权、WIC 编码、Base64 和 Rust 侧输出上限。
- `src-tauri/src/apps/discovery.rs`：桌面候选仍按现有规则解析和去重；每个有效应用额外尽力提取 `.lnk` Shell item 图标，并让 `registry_entry` 复制安全图标。
- `src-tauri/src/apps/appsfolder.rs`：在 `raw_entry` 尚持有枚举 item 时提取图标；`RawPackagedEntry` 和最终 `Application` 保留该 `Option<String>`。名称或 AUMID 有效性仍独立决定应用是否进入快照。
- `src-tauri/src/apps/cache.rs`：不改结构；继续以完整 `Vec<Application>` 作为唯一内存快照，刷新失败时保留上次完整快照。
- `src-tauri/src/result_registry.rs`：不改共享注册表、`QueryDomain` 或 `ResultAction`；图标只是应用域 `ResultItem` 的公开展示值，文件域结果和 `OpenIndexedPath` 保持原样。
- `src/launcher-core.ts` 与 `src/protocol.ts`：协议字段不变；core 在进入私有模型前做运行时验证，`ViewResult` 增加同名可选字段并在公开快照中保留。
- `src/launcher-view.tsx` 与 `src/styles.css`：结果行只增加真实图标与默认符号的固定容器；不改变 listbox/option 层级、焦点或结果行文本结构。

## 后台线程与 COM 模型

初始刷新已经由名为 `app-discovery` 的专用后台扫描线程执行；手动重新扫描已经通过 `spawn_blocking` 执行。两条路径最终都进入同一 `discover()`，并在当前扫描线程以 `COINIT_APARTMENTTHREADED` 初始化 COM，随后串行执行 Start Menu 和 AppsFolder 发现。

图标提取必须留在该扫描调用栈：

- 不在 Tauri 主线程、WebView 线程或 React 渲染期间调用 Shell/WIC。
- 不跨线程传递或缓存 `IShellItem`、WIC COM 接口或 `HBITMAP`。
- WIC factory 在当前已初始化的扫描 apartment 内创建；COM 接口由 `windows` 的 RAII 引用计数管理，并在 `CoUninitialize` 前析构。
- 现有 `discover()` 的 COM 初始化失败仍使整个扫描失败；某个应用的图标 Shell/WIC 操作失败只返回 `None`。
- 托盘“打开主界面”、窗口显示和 WebView 首帧路径不得等待应用扫描、图标提取或刷新完成，也不得调用 `GetImage`/WIC。初始扫描尚未完成时可以先显示现有空快照；后续刷新期间继续读取上次完整快照。
- 慢或阻塞的图标提取只能占用现有扫描互斥，不得持有 `AppCache.applications` 的读锁或写锁。快照写锁只在完整发现成功后的最终替换期间短暂取得，因此 `snapshot()` 和已有搜索读取不等待图标提取。

此设计不增加图标专用线程、队列或并发提取。扫描缓存的互斥和最终快照替换语义保持唯一。

## Windows Shell 与 WIC 转换

每个入口通过 `IShellItemImageFactory::GetImage` 固定请求 `SIZE { 32, 32 }` 和 `SIIGBF_ICONONLY`。不请求缩略图、不使用 `SIIGBF_MEMORYONLY` 或 `SIIGBF_INCACHEONLY` 降级；耗时提取已经位于后台线程。成功返回的 HBITMAP 由调用者拥有，微软要求最终通过 `DeleteObject` 释放。[GetImage 的返回资源与线程说明](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishellitemimagefactory-getimage)

本地 `windows 0.61.3` 已核对以下能力，实施时以实际编译签名为准，不在本规格中伪造调用代码：

- `IShellItemImageFactory::GetImage` 返回 `HBITMAP`；`SHCreateItemFromParsingName` 可由 `.lnk` 路径创建 Shell item。
- `IWICImagingFactory` 可创建 WIC bitmap、PNG encoder、frame 和 stream；WIC 是 Windows 原生的编码/格式转换边界。[IWICImagingFactory](https://learn.microsoft.com/en-us/windows/win32/api/wincodec/nn-wincodec-iwicimagingfactory)
- `CreateBitmapFromHBITMAP` 接受 alpha 选项；使用 `WICBitmapUseAlpha`，必要的像素格式转换必须保持 alpha，转换失败则该图标回退为 `None`。[CreateBitmapFromHBITMAP](https://learn.microsoft.com/en-us/windows/win32/api/wincodec/nf-wincodec-iwicimagingfactory-createbitmapfromhbitmap) [WIC alpha 选项](https://learn.microsoft.com/en-us/windows/win32/api/wincodec/ne-wincodec-wicbitmapalphachanneloption)
- `CryptBinaryToStringW` 与 `CRYPT_STRING_BASE64 | CRYPT_STRING_NOCRLF` 已由现有 `Win32_Security_Cryptography` feature 提供，用于生成标准、无换行 Base64。

只扩充锁定的 `windows = 0.61.3` 必要 feature：`Win32_Graphics_Gdi`、`Win32_Graphics_Imaging` 和 WIC frame 创建所需的 `Win32_System_Com_StructuredStorage`。不增加 crate。

## 数据 URI 安全契约

Rust 唯一允许产生、前端唯一允许保留的图标格式为：

```text
data:image/png;base64,<payload>
```

契约同时满足：

1. 前缀必须逐字、区分大小写等于 `data:image/png;base64,`，不允许参数或其他 MIME。
2. 整个字符串按 ASCII 字节计不得超过 `65,536` 字节（64 KiB），且 payload 非空。
3. payload 只含 `A-Z a-z 0-9 + /`，尾部可有零个、一个或两个 `=`；`=` 只能出现在末尾，整体长度必须是 4 的倍数，并符合标准 Base64 最后一组长度。
4. 不允许空白、换行、URL-safe Base64、百分号转义、SVG、`file:`、`http:`、`https:` 或任意其他 data MIME。
5. 前端不能仅依赖 TypeScript 类型；`launcher-core` 在搜索响应进入模型时重新验证。无效值按缺失处理，不透传给 `img.src`。

前缀占 22 字节，因此 Base64 payload 最大为 65,512 个字符，对应 PNG 原始字节最大为 49,134。Rust 使用 49,134 字节定长内存缓冲区初始化 WIC stream；该 stream 无法增长，编码溢出会失败并回退为 `None`。[IWICStream::InitializeFromMemory](https://learn.microsoft.com/en-us/windows/win32/api/wincodec/nf-wincodec-iwicstream-initializefrommemory) encoder 和 frame 提交成功后，以 stream 的实际写入长度截取 PNG，再调用 Windows Base64 API，并对最终 data URL 重做长度检查。

现有 CSP 已是 `img-src 'self' data:`，无需放宽；严格 core 校验进一步把可到达 `<img>` 的 `data:` 内容收窄为 PNG Base64。

## 资源所有权与失败路径

从 `GetImage` 成功返回的每一个 `HBITMAP` 必须立即进入所有权守卫，再进行任何 WIC 操作。本地 `windows-core 0.61.2` 提供 `Owned<T: Free>`，而 `windows 0.61.3` 的 `HBITMAP: Free` 实现会调用 `DeleteObject`；实现应复用该 RAII 所有者，而不是手工在多处分支清理。

由此资源不变量为：

- Shell 获取失败时没有 HBITMAP 需要释放。
- Shell 获取成功后，无论 WIC factory、bitmap 转换、像素格式、stream、encoder、frame、commit、长度查询、Base64 或最终校验在哪一步失败，HBITMAP 都恰好释放一次。
- WIC 和其他 COM 接口依赖接口类型析构释放引用，不保存到 `Application`，也不越过当前 COM apartment。
- 固定 PNG 缓冲区的 Rust 所有权必须覆盖 WIC stream 的完整生命周期；stream 析构后才允许回收缓冲区。
- 错误不得把 HRESULT、`.lnk` 路径、可执行文件路径、AUMID、包路径或原始图像数据写入 DTO、状态文本或日志。

单项图标失败是可恢复的展示降级：对应 `Application.icon = None`，应用仍进入快照并可搜索、排序和启动。图标失败不增加现有“无效应用”计数，也不把 AppsFolder 枚举降级为整体失败。已有的入口名称/AUMID/快捷方式有效性、AppsFolder 绑定与枚举错误语义保持不变；真正的整体发现失败仍由 `AppCache::refresh_with` 保留上次完整快照。

## Rust 到前端的投影

`registry_entry` 从 `Application.icon` 克隆到 `ResultItem.icon`。只改变现有可选展示字段，不改变 `ResultAction`、`requestId + resultId` 动作能力模型或应用身份；AUMID 和任何路径仍只存在于 Rust 的可信动作注册表。

搜索最多返回现有上限数量的结果，图标只跟随这些现有 `ResultItem` 发送。设置加载路径可以继续通过既有 `AppAliasTarget.icon` 自然传递同一安全 data URL，但设置页 core 仍不保留或渲染它；本需求不借机改变设置页。

`launcher-core` 对每个搜索项独立处理：标题、副标题和结果 ID 沿用现有逻辑；图标通过上述 validator 才进入私有结果和只读 `LauncherSnapshot`。图标缺失或拒绝不会丢弃整条结果，也不会改变结果顺序、选择索引或状态文本。

## 视图与回退表现

每条搜索结果的第一列继续固定为 `28px`，其中放置一个 `28 x 28` 的图标容器。容器 `align-self: center`，没有顶部 margin，真实图标和默认符号共用这一个稳定占位，不能改变行高或文本列宽。

默认符号不再是空方框，而是纯 CSS 的 2 x 2 应用瓦片：四个小方块使用当前文本色/主题色，通过伪元素或容器内现有装饰元素绘制，不引入 SVG、图像资源、文字或 icon 依赖。默认瓦片和真实图标是互斥显示状态，而不是永久叠放：安全 icon 存在且当前 `<img>` 尚未报错时只显示真实图标，默认瓦片不可见；icon 缺失或当前 src 触发 `error` 时才显示默认瓦片，避免默认瓦片透过 PNG 透明区域形成叠影。

安全图标存在时，在同一容器叠放装饰性 `<img>`：

- `width`、`height` 固定为 `28px`，使用保持比例且不裁切的 `object-fit: contain`。
- `alt=""` 且 `aria-hidden="true"`，应用名称仍由相邻标题提供，不产生重复朗读。
- 不增加焦点、点击、拖拽或加载状态。
- `onError` 只把当前 src 标记为失败并切换到默认瓦片，不修改 core、移除结果或发起重试。
- 错误状态以当前 src 为身份隔离；后续安全 src 变化时必须通过 React `key` 重建 `<img>` 或显式重置该状态，不能沿用上一张图的隐藏/失败状态。

没有安全图标时只显示默认瓦片。当前 settings 页的 `.app-mark` 使用可以维持原有布局；实现应使用结果区专用容器/选择器，避免本需求意外改造设置页图标。

`role="listbox"`、`role="option"`、`aria-selected`、输入框的 `aria-activedescendant` 和 `scrollIntoView({ block: 'nearest' })` 全部保持原样。图标加载或失败不得移动焦点、滚动列表或改变 active descendant。

## 测试设计

实施阶段按 TDD 先建立失败测试，再完成最小实现。自动测试矩阵如下：

### Rust 单元与边界测试

1. 桌面 `.lnk` Shell item 和 AppsFolder 既有 Shell item 都能进入同一提取器，成功时分别写入应用图标。
2. Shell item 创建/cast、`GetImage`、WIC factory、HBITMAP 转换、像素格式转换、stream、encoder/frame、commit、长度读取和 Base64 的每一阶段失败都只返回 `None`，应用仍保留。
3. 用可注入的最小操作接缝或所有权 guard 测试 HBITMAP 在成功、每个早退和 panic-free 错误路径恰好释放一次；获取 HBITMAP 前失败不释放。
4. PNG data URL 具有精确前缀、无换行标准 Base64和非空 payload；49,134 字节边界可接受，超过编码容量或最终 65,536 字节上限拒绝。
5. `Application.icon` 正确投影到 `ResultItem.icon`，`None` 仍省略序列化字段。
6. 结果 DTO JSON 可包含安全 PNG data URL，但不包含 `.lnk`、可执行文件、AUMID、包路径、HRESULT 或动作目标。
7. 任一图标失败不改变 `.lnk`/AppsFolder 应用计数、排序、身份或启动目标；AppsFolder 整体枚举失败仍保留上次快照。
8. 应用域结果继续通过共享注册表发布并携带图标；文件域 DTO、`QueryDomain::File`、`OpenIndexedPath` 和跨域旧结果失效语义不受影响。
9. 用 barrier/channel 阻塞图标提取接缝，在提取尚未释放时确定性断言 `AppCache::snapshot()` 可读取上次快照；测试不得用 sleep，也不得把图标提取放到 applications 读写锁内。
10. 生命周期/源代码边界测试确认托盘显示、`request_show`、窗口 API 和 WebView 展示路径不调用或等待 `discover`、`GetImage`、WIC、初始 refresh handle；扫描进行中仍可发布主界面展示。

Windows API 本身用小而确定的真实集成测试覆盖 32 x 32 PNG 输出；大量失败组合通过窄函数接缝测试，不建立通用 trait/factory 框架。

### 前端 core 与视图测试

1. core 保留完全合法且总长不超过 64 KiB 的 `data:image/png;base64,` 值。
2. core 拒绝错误 MIME、空 payload、超长值、非法字符、空白/换行、错误长度、非尾部或超过两个 padding，以及 `file/http/https/SVG`。
3. 合法图标进入只读 `ViewResult`；无效图标仅被移除，结果标题、顺序和选中项不变。
4. 有真实图标时渲染空 alt、`aria-hidden` 的 `<img>`，并确认默认瓦片不可见；无图标时只显示 CSS 2 x 2 默认瓦片。
5. 当前 src 触发图片 `error` 后切换为默认瓦片，结果仍在列表中；同一结果后续收到不同的安全 src 时重建或重置图片错误状态，新图可正常显示且默认瓦片再次不可见。
6. 样式契约覆盖固定 `28 x 28`、`align-self: center`、无顶部 margin、`object-fit: contain` 和共享稳定占位。
7. 现有键盘上下选择、`aria-activedescendant`、Enter/Escape、输入法与 `scrollIntoView({ block: 'nearest' })` 回归继续通过。
8. 设置页不渲染真实应用图标，现有别名交互无变化。

### 验证 gate 与人工验收

自动 gate：

```powershell
cd D:\code\UiPilot_tools\.worktrees\launcher-application-icons\src-tauri
cargo fmt --all -- --check
cargo test --lib
cargo check --lib
cargo clippy --lib -- -D warnings

cd D:\code\UiPilot_tools\.worktrees\launcher-application-icons
npm.cmd test -- --run
npm.cmd run build
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check-security-config.ps1
git diff --check
```

Windows 人工验收：

```powershell
cd D:\code\UiPilot_tools\.worktrees\launcher-application-icons
npm.cmd run tauri dev
```

1. 冷启动后在托盘图标可操作时立即点击“打开主界面”，不要等待应用或图标扫描完成；确认窗口和 WebView 可先显示，托盘首次打开不等待扫描结束。
2. 扫描完成后分别搜索一个 Start Menu 桌面应用、`计算器` 和 `设置`。
3. 确认桌面 `.lnk` 和 AppsFolder 打包结果都显示与 Windows 对应的真实图标；同名结果仍由副标题区分，排名和各自启动行为不变。
4. 找一个 Shell 无法提供或 WebView 无法解码图标的入口，确认显示 2 x 2 默认瓦片且应用仍可启动。
5. 连续按上下方向键跨越可视范围，确认选中项、active descendant 和 nearest 滚动不受图片加载影响。
6. 在浅色、深色和 Windows 强制颜色模式检查真实图标容器与默认瓦片可辨识，行文本不偏移，图标在行内垂直居中。

## 非目标

- 不修改设置页 UI，不在设置页展示真实图标。
- 不改变应用搜索排名、同名入口规则、应用身份、启动动作或使用次数。
- 不增加磁盘图标缓存、缩略图缓存、图标版本字段或缓存失效机制。
- 不增加独立图标命令、逐结果异步请求、可见项懒加载、图标线程池或新状态管理。
- 不抓取网页、远程图片、SVG、包清单图标路径、注册表图标路径或目标 EXE 图标作为 fallback。
- 不向前端暴露 `.lnk`、可执行文件、AUMID、包安装目录或任意本地路径。
- 不增加图片、Base64 或 Windows 封装第三方依赖；只扩充现有 `windows 0.61.3` features。
- 不改变窗口大小、三段滚动布局、结果文本布局、键盘/鼠标行为、协议命令或 Rust 缓存事务。

## 完成标准

- 两种应用入口都在后台扫描期尽力生成受限 PNG data URL，任何单项图标错误只回退默认图标。
- HBITMAP 在所有成功/失败路径恰好释放一次，COM/WIC 生命周期不越过扫描 apartment。
- Rust 只发送安全图标和公开展示字段，路径/AUMID/原生错误仍不可见。
- 前端只保留精确 PNG Base64、64 KiB 内的图标；解码失败自动露出默认瓦片。
- 真实和默认图标共享固定、垂直居中的 `28 x 28` 占位，现有选择、滚动、无障碍和启动行为无回归。
