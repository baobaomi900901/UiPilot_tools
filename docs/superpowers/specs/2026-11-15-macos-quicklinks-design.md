# macOS Quicklinks 宿主接线

复用 [Quicklinks 合同](2026-08-30-quicklinks-design.md)和 [UI 规范](../../ui-guidelines.md)，开放独立于插件 Runtime 的 `/quicklinks` 管理与自定义命令。Windows 路径不改调度；Mac `builtinFeatures` 仍不整体开放，单独公布 quicklinks 能力。默认图标、可选 PNG、创建/编辑/删除、模板查询和管理入口收藏形成完整单元。

## 命名空间与持久化

Quicklinks 使用共享数据格式和存储逻辑，Mac 原子读写使用现有不跟随符号链接的目录句柄。所有 Mac quicklink 写入与插件安装/改名由同一 ManagementState 互斥锁串行化。Quicklink 保存检查所有已安装插件（包括停用项）的有效命令名和全部宿主保留名；插件预览、最终提交和改名反向检查现存 quicklinks。预览后新增冲突 quicklink，最终安装必须拒绝；拒绝不覆盖旧配置。主查询只惰性读取 quicklink 文件，不为了列出命令创建插件安装目录。

Mac 写入包装器拒绝未知字段、过长输入、非现存更新 ID 和不合法的 URL 模板。仅 http/https、无用户信息、有 host、包含 `{Query}`；模板不超过 4096 字节，查询不超过 4096 字节，扩展后 URL 不超过原生 16 KiB 上限。查询作为 URL component 编码，不接受客户端执行 URL。图标仍是 128×128 PNG 且不超过 256 KiB；文件读取有界，只接受原生选择器返回的路径，不开放任意路径 IPC。

## 查询与执行所有权

`/quicklinks` 可收藏，Enter 打开共享管理界面。自定义命令的空参数只补全输入，带参数产生明确执行行；输入阶段不访问网络、不打开浏览器。普通文本不触发 quicklinks。查询 action 保存服务端记录身份、命令名、模板与参数；执行前和最终准入分别核验记录仍存在且未改变。准备 URL 后，在 management → settings → registry 的短锁内核验功能启用并消费一次性票据，然后锁外调用系统默认浏览器。编辑/删除先于最终准入则零打开；准入先于编辑则允许已授权调用完成。旧结果、新查询、隐藏重开和迟到完成不能覆盖当前界面。同一结果不能重复调度。

## IPC 与选择器

list/save/delete/icon 四个命令仅授予 Mac main，并在读取 State 或路径前验证真实来源。Find、插件和远程页面无权限。图标选择器复用现有 AppKit sheet 与 dialog owner，选择期间抑制主窗口失焦隐藏；取消不改数据，关闭/重开后的迟到回调不能保存候选或抢焦点。成功取得的 security-scoped URL 保留到有界复制结束并配对释放。没有 GUI 自动化，真实选择/浏览器由用户验收。

## 验证边界

自动覆盖存储重开、双向冲突、预览和提交间冲突、严格输入、URL 编码、编辑/删除与最终准入的两种顺序、一次性执行、晚到完成、窄能力过滤、管理与收藏 UI 链路及 ACL。fake opener 和临时合成文件不替代真实浏览器、图标选择、键盘或安装后重启验收。证书、生产插件隔离、Panel/Window 运行均未由本功能开放。
