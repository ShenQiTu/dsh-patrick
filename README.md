# DSH Patrick

面向 DSH 的 Windows 桌面模型网关，使用 Rust、Tauri 2、React、TypeScript 和 Vite 构建。当前版本 **0.1.5**。

本项目不是 DSH 或 DeepSeek 官方产品。界面布局和部分图标参考 [cursor-byok](https://github.com/leookun/cursor-byok)，不包含其 Cursor 专用接管、证书安装、广告或更新器。

## 功能

- 在桌面界面管理多个模型、API 地址、API Key、自定义请求头和额外 JSON 参数。
- 支持 OpenAI Chat Completions、OpenAI Responses 和 Anthropic Messages；按配置协议转发，不做协议互转。
- 获取上游模型列表、单模型测试、批量测试（最多并发 3）、取消和测试结果详情。
- 每个模型单独启用注入提示词；新模型默认关闭。推理强度留空时保留客户端设置。
- DSH 自动接入：新增本项目自己的 provider，同步启用模型，不替换原 provider 或强制切换会话默认模型。
- 断开网关时撤销本项目写入的配置；接入前备份，遇到外部冲突会报错而不是覆盖。
- Token 趋势、活动热力图、调用记录和按来源筛选的统计。
- 浅色、深色和跟随系统主题；关闭窗口收起到托盘，真正退出使用托盘菜单。

## 初次使用

1. 启动 Windows 桌面程序，在模型页添加自己的服务器地址、模型 ID 和 Key。仓库不附带可用的账户或个人模型配置。
2. 在「DSH 接入」中确认自己的 DSH profile 目录（通常为 DSH 数据目录下的 `profiles/web`）。
3. 在 DSH 模型页打开网关开关。默认本机地址为 `http://127.0.0.1:8787/v1`，端口可调整。
4. 回到 DSH，在 **DSH Patrick** provider 下选择模型。上游 Key 只保存在 Patrick；写入 DSH 的是独立本机令牌。
5. 使用「断开」或托盘退出恢复本项目管理的配置。窗口右上角的 × 只是隐藏窗口。

DSH 接入需要本机标准安装位置的 Node.js，以及目标 profile 已安装的 `yaml` 依赖。可以设置 `DSH_HOME` 辅助查找，但应以接入页确认的路径为准。获取模型列表和点击测试会访问你填写的服务商，并可能产生服务商用量。

### Zen 请求头

- 模型编辑中的 Zen 开关仅用于网关自己的测试按钮。
- 一次独立测试生成一个会话 ID；同次重试复用，新的独立测试重新生成。
- 正常代理流量不使用测试 ID；客户端的有效 `x-opencode-session` 优先于模型静态配置。
- DSH 长期会话头由 DSH 及其插件处理，测试按钮不会替换 DSH 用户会话。
- HTTP 200 不直接视为成功；测试会核对实际文本输出、流终止和上游错误。

## 参数与路由

- 模型列表：`GET /v1/models`。
- Chat：`/v1/chat/completions`；Responses：`/v1/responses`；Anthropic：`/v1/messages`。
- 默认填写 API 根地址；勾选「完整请求地址」后使用指定端点。
- 自定义 Headers 和提示词分别受各模型开关控制。关闭开关保留编辑内容，但不发送。
- 额外参数为 JSON 对象；模型、消息、地址、鉴权等保留字段受到校验。
- 模型设置留空不等于强制覆盖客户端值。显式模型设置可能覆盖同名客户端参数。
- 本机入口需要独立网关令牌，不接受把上游 Key 当成本机令牌使用。

## 数据与隐私

- 默认数据位于 EXE 同级 `app-data`，可通过 `DSH_PATRICK_DATA` 指定独立数据目录。
- Windows 当前用户 DPAPI 加密配置和统计历史。上游 Key、自定义头等仍属于敏感数据，加密文件同样不应提交或随发布包分发。
- DSH profile 内的 `.dsh-patrick` 保存私有备份，可能包含旧凭据，只留在本机。
- 正式转发不记录消息正文；测试结果保留有限预览，并对已配置凭据进行遮盖。
- 启用模型测试时仅发送合成测试内容，不重放用户对话。
- 便携包升级时数据按目录隔离；不要以为复制新 EXE 到另一个目录会自动迁移原配置。
- 上游 HTTPS 验证证书；请求不跟随重定向，也不自动读取系统代理。

## 开发与构建

当前实现针对 Windows。需要 Node.js 24、Rust stable 的 MSVC 工具链、Visual Studio C++ Build Tools（含 Windows SDK）和 WebView2 Runtime。

在项目根目录运行：

```powershell
npm ci
npm run build
cargo test --locked --manifest-path .\src-tauri\Cargo.toml
node .\tools\statistics.test.mjs
```

开发桌面窗口使用独立数据目录：

```powershell
$env:DSH_PATRICK_DATA = Join-Path (Get-Location) 'app-data'
npm run tauri -- dev
```

生成 release EXE：

```powershell
npm run tauri -- build
```

未设置 `CARGO_TARGET_DIR` 时，EXE 位于 `src-tauri/target/release/dsh-patrick.exe`。目前关闭安装包生成，发布时仅打包新构建的 EXE 和公开说明，不复制任何 `app-data`。

`npm run dev` 单独启动的是浏览器界面预览，不具有真正的桌面网关能力。缓存位置可由 `CARGO_HOME`、`CARGO_TARGET_DIR` 和 `npm_config_cache` 自行指定；公开构建流程不依赖开发者的内部打包脚本或测试报告。

### 可选 DSH 集成测试

默认 Rust 测试及统计测试使用本机模拟数据，不调用真实上游。DSH 集成测试需要额外依赖，默认标记为忽略：

- `DSH_PATRICK_YAML_ANCHOR`：已安装 YAML 依赖的 profile/package.json。
- `DSH_PATRICK_DSH_SOURCE`：已构建 DSH 源码目录。
- `DSH_PATRICK_TEST_ROOT`：尚不存在的隔离测试目录，仅用于独立 Node bridge 测试。

```powershell
node .\tools\dsh-bridge.test.cjs
node .\tools\dsh-runtime.test.mjs
cargo test --locked --manifest-path .\src-tauri\Cargo.toml -- --ignored --nocapture
```

只在设置完上述依赖与隔离目录后执行；不要把真实 profile 作为测试写入目标。

## 发布检查

- Git 仅加入审核过的源码、依赖锁文件、测试和公开文档。
- 排除用户配置、Key、Token、备份、日志、个人截图、编译缓存与内部交付材料。
- 每次提交前检查暂存区；忽略规则不会清除已有 Git 历史中的文件。
- 源码进入仓库，干净构建的便携包进入 GitHub Releases。
- 本项目整体许可证将在首次公开发布前由维护者确认；现有第三方文件仍遵守各自许可。

## 参考与第三方素材

- [cursor-byok](https://github.com/leookun/cursor-byok)，参考提交 `31e9f5cbc98a7d28cffcb5efbb9323cd3e143354`。相关许可见 `src/assets/LICENSE-cursor-byok.txt`。
- DSH 图标来自 DSH 的 favicon，相关许可见 `src/assets/LICENSE-DSH.txt`。
- 其他图标与应用图片来源见 `src/assets/SOURCE.md` 和 `src-tauri/icons/SOURCE.txt`。角色图、品牌名称和标识的权利不因代码开源而自动授予；公开分发前需单独核对。
