# Android 分支交接（2026-11-22）

## 当前状态与用户约束

- 工作分支：`feat/android-support`；基于桌面 RC8 主分支提交 `80abd7d6d7de73499138e1fed7460b0a822ea528`。
- 用户因额度不足要求立即收尾，写交接文档并 Git commit。本次提交保存当前源码进度，不表示已完成 Android 编译或真机验收。
- 没有安装 Android Studio、SDK、NDK；没有执行 Cargo/Gradle/Flutter 编译，没有生成 APK 或原生 `.so`，没有真实 VPN／凭据操作。
- `flutter/android/app/build.gradle` 保持不变，继续使用原来的 `flutter.minSdkVersion`、`flutter.targetSdkVersion` 和 `flutter.compileSdkVersion`，不要擅改 SDK 标准。
- 不合并到 `easytier-embedded`，本次只本地提交，不推送。不要使用之前有问题的 DSH MCP。

## 已写入的实现

### Android 服务层

`flutter/android/app/src/main/kotlin/com/carriez/flutter_hbb/`：

- `EasyTierNative.kt`：独立 JNI 库五个接口，缺组件时明确不可用。
- `EasyTierProfileStore.kt`：应用私有原子方案存储、选择状态、仅 EasyTier 标记。
- `EasyTierVpnService.kt`：独立 `:easytier` 进程、Messenger 控制、物理网络绑定、TUN 配置、后台状态发布、通知停止及撤销清理。
- `EasyTierClient.kt`：跨进程 Messenger 请求／响应。
- `EasyTierActivityBridge.kt`：MethodChannel、系统 VPN 授权和 SAF 文档导入导出。
- `EasyTierStateReceiver.kt`：应用级受签名权限保护的状态接收器，将快照交给根 Rust。
- `MainApplication.kt` 避免在组网子进程加载原远控库；主进程注册状态接收器和应用级凭据存储。
- `MainActivity.kt` 注册通道并接回授权／文档结果；`ffi.kt` 增加 `onEasyTierState` JNI 声明。
- Manifest 声明 VPN 服务、系统绑定权限、内部签名权限及前台服务权限；增加通知图标和混淆保留规则。
- `LanCredentialStore.kt` 增加独立的本机 LAN 账户安全存储方法。

### 原生组网库

`libs/easytier_android/` 是独立 Rust cdylib 工程，不加入根 workspace。

- EasyTier 固定提交：`8428a89d2dabc94c97d370ec607c6ca142473626`，使用独立 Rust 1.95 工具链。
- JNI 库名 `libsubnetdesk_easytier_android.so`，对象为 `com.carriez.flutter_hbb.EasyTierNative`。
- 使用上游 mobile 外部 TUN fd 接口；JNI 复制 fd，等待实际 TunDeviceReady 才报告运行成功，停止后释放副本。
- 复用 RC8 `src/easytier/network_view.rs` 输出设备和路由展示数据；启用 AES-GCM，未将失败伪装成运行成功。
- `profile.rs` 用结构化 TOML 编辑保留未知字段。
- `build_android.py` 仅提供将来有 NDK 后的显式构建入口，本轮没有执行。
- **尚无该独立包的 Cargo.lock**。首次未来构建需解析依赖、生成并审阅锁文件；固定上游提交不等于锁定全部传递依赖。

### Flutter 与根 Rust

- 新增 `flutter/lib/common/widgets/easytier_android.dart`；Android 控制器走 MethodChannel，桌面继续走原 FFI。
- 现有组网页、手机工作区、快捷双开关和 EasyTier 设备列表共用同一控制器；节点只填 IP，不自动远控。
- Android TOML 导入导出走 SAF；`readProfile` 已改为 Future，编辑调用已加 await。
- 手机本机名继续复用原 LanServerInfoPanel，启动 hostname 读取同一 `device_name`；运行中改名提示重启方案。
- Android 隐藏桌面专用服务、自启动和备份入口。
- 新增 `src/android_easytier.rs`：主进程 JNI 快照、来源 IP 白名单及 15 秒过期拒绝；早于 APP_DIR 初始化的回调不提前加载配置缓存。
- `src/lan_server.rs` 为 Android 接入来源限制，桌面路径保留。
- `src/android_credentials.rs`、`src/lan_password.rs` 为本机 LAN 密码接入 Keystore 回显及失败回滚；与远端 fingerprint 凭据分开。

## 接口约定

- MethodChannel：`com.zibochen.subnetdesk/easytier`。
- 方法：`status`、`command`、`readProfile`、`importProfile`、`exportProfile`，返回 JSON 字符串。
- `command` 延续桌面动作，包括 `set_remote_only` + `enabled`；完成状态使用 `completed_request_id` / `command_error`，不得空对象假报成功。
- 内部广播快照至少有 `state`、`remote_only`、`nodes[].address`、`queried_at`；根 JNI 符号为 `Java_ffi_FFI_onEasyTierState`。
- Native 接口：`start(config)`、`status()`、`setTunFd(fd)`、`stop()`、`profile(toml, fieldsJson)`，返回 `ok` / `error` 等 JSON。
- Kotlin 保留原 PFD，Native 持有 dup。停止超时不能仅关闭原 PFD 就声称核心已停止。

## 收尾时已知待复核项

1. **没有 Android 类型检查、链接或实际运行验证**。先核对 Kotlin/JNI/Rust 真实签名，再在 SDK 到位后编译，不能把源码解析结果当作功能通过。
2. `EasyTierVpnService.configureTunnel()` 当前没有在 Builder 上设置底层网络；`onAvailable()` 中的 `setUnderlyingNetworks()` 可能发生在 VPN 尚未建立时而失败。后续应在 API 22+ 对 Builder 设置当前底层网络，并核对返回值和切网行为。
3. `releaseFailedCore()` 已包含仅 `:easytier` 进程可执行的延迟自终止兜底。需重点检查停止超时、回复送达、generation 判定、onDestroy/onRevoke 重入，绝不能终止主应用。
4. 核对主进程状态广播与 `remote_only` 持久化：主动停止可以关限制，撤销／崩溃／超时必须保留拒绝策略；无效快照不能长期延长旧白名单。
5. 核对 SAF 授权取消、Activity 重建、组件缺失、超时等结果均能到达 Flutter；不得将进入 starting 当作 TUN 已就绪。
6. Native 当前明确不支持系统 DNS 接管 `accept_dns`、系统转发、`no_tun`、物理网卡 UDP 广播捕获，启动会明确报错。当前虚拟网卡模型以 IPv4 为主；未接管的 IPv6 保留系统路径。
7. 默认路由只在明确出口配置且有效核心路由提供 0/0 时接管；仍需真机验证。不要排除整个应用包，否则远控主进程也会绕过 TUN。

## 已做的轻量检查

- Flutter 代理做了 Dart 格式器的纯语法解析（`--output=none`），未编译／运行测试；未初始化完整 package 环境时存在既有环境提示。
- Native 代理核对了固定上游源码中的 manager、TUN 事件及 proto API，并解析 Cargo TOML 和 Python AST；未做类型检查。
- 根代理检查 Git 空白错误、清单 XML、独立 Cargo TOML、Python 脚本 AST，并确认原 Gradle SDK 声明未改。
- 未读取或修改用户实际的 Android 配置、系统凭据或 VPN 状态。

## 下一步

先读本文件及 [Android 实现说明](android-support.md)、[原生库说明](../../../libs/easytier_android/README.md)。优先处理上述待复核项，不扩大到桌面重构。

用户允许并具备 SDK/NDK 后，再按原 SDK 标准生成与原 RustDesk 库 ABI 一致的 EasyTier `.so`，随后进行 APK 编译和最小真机验证。当前源版本保留 RC8，不擅自发布 Android RC、安装工具链、合并主分支或推送。
