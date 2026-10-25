# 开发恢复演示：独立离线夹具

`com.uipilot.demo-recovery` 用一条稳定命令 `recovery` 演示更新与私有数据的独立生命周期。采用 `demo-multi` 的 package 布局、一项 `commands` 和 `submit/mainResult`；最低 Host 0.5.0，manifest/API v1，无权限、复制动作、网络、凭据、Panel、窗口或后台任务。普通设置只有 manifest 默认值的只读展示，不新增设置编辑器。

## 命令与数据约定

| 输入 | 行为 |
| --- | --- |
| `/recovery` 或 `/recovery status` | 显示构建标记、存储 schema/计数、普通默认设置；严格只读，缺失数据不初始化 |
| `/recovery inc` | 校验完整 `{schemaVersion:1,count}`，构造新副本，恰好一次 `storage.set('recovery-snapshot', snapshot)` |
| `/recovery throw` | 显式抛出带合成错误哨兵的受控异常；不写数据 |
| V2 专用 `/recovery seed-v2` | 一次写入 `{schemaVersion:2,count,sentinel:'RECOVERY_PRIVATE_SENTINEL_V2'}`，保留已有计数；只供此独立测试命名空间，主动把合法 schema1 转成一次性合成 schema2 |
| V2 专用 `/recovery inspect-v2` | 只读显示 schema2 的准确 sentinel；没有 schema2 时如实显示当前数据 |

计数为 0–1,000,000 的安全整数。只有 `null` 表示缺失；任何其他格式错误、未知字段、未知/较新版本都禁止写入，绝不 remove/reset。schema1 读取器对 schema2 返回 `write-blocked`，不覆盖数据。V2 的 seed 只接受缺失、合法 schema1 或自身准确 schema2；对未知数据仍禁止写入。V2 的 `inc` 不修改 schema2。

保存异常返回 `save-uncertain`（保存结果待确认），不声称回滚/未保存，不自动重试。先主动运行 status 读取，再决定是否重试。取消、过期、失败不能证明此前已经获准的写入没有完成。插件版本、业务数据 schema、Host 版本、manifest/API 版本是四件独立的事；Host 不自动迁移或恢复业务数据。

## 生成和静态验证

在仓库根目录使用现有 Node.js 与本地 CLI，无需安装依赖：

```powershell
node --test examples/public-plugins/com.uipilot.demo-recovery/tests/runtime.test.js
node --check examples/public-plugins/com.uipilot.demo-recovery/package/dist/runtime.js
node examples/public-plugins/com.uipilot.demo-recovery/scripts/create-fixtures.mjs .superpowers/deliveries/uipilot-0.7.0-acceptance/fixtures
npm run build --workspace @uipilot/plugin-cli
```

输出目录必须不存在；生成器拒绝覆盖。每个变体的 `package/` 中只有 plugin.json 和 dist/runtime.js。文档/测试/生成器留在包外。对下表每个目录分别运行，archive 输出父目录须已存在且文件不得存在：

```powershell
node packages/plugin-cli/dist/cli.mjs validate <package目录> --platform windows --json
node packages/plugin-cli/dist/cli.mjs pack <package目录> --output <新归档.uipilot-plugin> --platform windows --json
node packages/plugin-cli/dist/cli.mjs validate <新归档.uipilot-plugin> --platform windows --json
```

静态 CLI 不执行 Runtime，因此 C 静态有效；只有 Host 的 readiness 检查会拒绝缺少 onCommand。所有变体保留相同 pluginId、commandId 与启动名。

| 变体 | 插件版本 | 用途 |
| --- | --- | --- |
| A | 1.0.0 | 基线，构建标记 A |
| B | 1.0.0 | 同版本重新载入，构建标记 B |
| C-readiness-broken | 1.0.0 | 静态有效，缺少 onCommand |
| T-controlled-throw | 1.0.0 | 显式 throw 故障演示；status/inc 仍正常 |
| D-upgrade | 1.0.1 | 通用更新，仍读写 schema1 |
| V2-disposable | 1.0.0 | 显式 schema2 种子与只读核对 |

## Host 0.7.0 人工验收（全部待测试）

新交付根目录：`C:\Users\moby\.codex\worktrees\local-plugin-foundation\UiPilot_tools\.superpowers\deliveries\uipilot-0.7.0-acceptance`。完整中文操作表、每次选择器对应的完整目录/文件路径、exe/resource/archive hashes 由交付目录 README.zh-CN.md 和 delivery-evidence.json 提供。安装开发目录选择 package 文件夹；安装插件包选择 .uipilot-plugin 文件。二者不可混选。

初始组仅三步：安装 A 并运行 status；运行 inc、改名/收藏、完整退出重启核对（改名时用实际别名，核对后恢复 recovery）；同版本定向重新载入 B，检查标记变化与数据/收藏保留。单纯刷新列表不重新读取开发目录。

完整组随后检查：取消与错误目录/版本选择；C readiness 失败后原 B/数据仍可用；同版本修复；连续三次 throw（中间不运行成功命令）使插件故障停用；修正刷新后仍停用，必须主动重新启用；D 通用升级保留 schema1；从 D 通用降级到 V2 1.0.0（明确数据兼容提示），显式 seed-v2 后同版本切换 A，inc 被拒，再切换 V2 inspect-v2 核对 sentinel。最后可保留 V2 测试数据，不要求破坏数据恢复 A 的写入能力。

诊断在健康、readiness 失败和故障状态分别预览，用户显式复制。检查 `RECOVERY_CONTENT_SENTINEL`、`RECOVERY_SETTING_SENTINEL`、`RECOVERY_ERROR_SENTINEL`、`RECOVERY_SOURCE_PATH_SENTINEL`、`RECOVERY_PRIVATE_SENTINEL_V2` 均未出现在诊断 JSON。允许的 pluginId/packageVersion 不能当成泄漏。只检查预览/复制内容，不向外发送。Host 可保留最近管理操作历史，不能用旧失败记录推断当前状态。

Light/Dark、键盘/焦点/取消反馈、旧 Notes/templates/multi 的简短兼容检查均由用户完成。自动测试覆盖缺失只读、完整单写、未知数据不变、未写失败与已写丢确认的不确定性、变体稳定身份和 schema2 经旧版读取器后的保留；它们不代表真实 Host 手工验收通过。
