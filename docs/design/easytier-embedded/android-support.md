# Android 适配实现与后续构建交接

## 分支和范围

分支为 `feat/android-support`，从 RC8 主分支创建。本轮只实现代码，不安装 Android Studio、SDK 或 NDK，不编译 Rust/Android/Flutter，不产出 APK，也不合并桌面主分支。

保持原仓库的 SDK 声明：

```groovy
compileSdkVersion flutter.compileSdkVersion
minSdkVersion flutter.minSdkVersion
targetSdkVersion flutter.targetSdkVersion
```

这些值仍由后续实际使用的 Flutter SDK 决定。本轮不擅自固定、提高或降低 SDK 版本，也不修改已有 Android 应用 ID 和 Kotlin namespace。

## 组件边界

- Flutter 复用 RC8 组网页、设备卡片和手机工作区。桌面继续走原 Rust FFI，Android 通过 `com.zibochen.subnetdesk/easytier` MethodChannel 与 VPN 服务通信。
- Kotlin 的 EasyTier VPN 服务运行在独立 `:easytier` 进程，主进程不加载组网核心。系统授权、前台通知、网络切换、TUN、方案文件与文档选择由 Android 层处理。
- `libs/easytier_android` 是独立 Rust cdylib 工程，固定 EasyTier 提交 `8428a89d2dabc94c97d370ec607c6ca142473626`，不加入原 RustDesk workspace。使用独立 Rust 工具链，避免改变桌面宿主依赖。
- 原生核心不是 CLI 可执行文件。通过上游 mobile 外部 TUN fd 接口运行，使用 RC8 的纯数据关联算法输出节点、直连／中继、下一跳与延迟。
- 根 Rust 的 `android_easytier.rs` 只接收主进程中的 VPN 状态快照，用于入站来源限制，不运行另一套 EasyTier。

## Android 控制链路

1. 用户选择方案并启动，必要时通过系统 `VpnService.prepare()` 授权。
2. Activity 经受保护的 Messenger 控制 VPN 服务，服务只接受本应用 UID 的控制请求；系统 VPN 的绑定仍走标准 VpnService 接口。
3. 服务选择非 VPN 底层网络，仅将 `:easytier` 进程绑定到该网络。主进程继续使用系统路由，以便远控流量进入 EasyTier TUN。
4. 核心取得静态或 DHCP 虚拟地址后，服务建立 TUN；原生层复制 fd，并等待真实的 TunDeviceReady 再报告运行成功。
5. 后台服务持续发布节点状态；主进程 Application 的内部接收器通过 JNI 更新来源快照，不依赖 Flutter 页面保持打开。

禁止通过 `addDisallowedApplication(packageName)` 排除整个应用来绕开 VPN，否则本应用自己的远控流量也会被排除。不得借用原媒体投影服务来请求组网权限，也不得为了普通组网自动开启屏幕分享。

VPN 服务使用 `BIND_VPN_SERVICE` 保护。内部状态广播采用本应用签名权限；较新 Android 的动态接收器使用非导出标记，旧版本仍保留签名权限保护。前台服务按 API 级别处理类型及权限。

参考：[Android VPN 开发文档](https://developer.android.com/develop/connectivity/vpn)、[VpnService API](https://developer.android.com/reference/android/net/VpnService)、[前台服务类型](https://developer.android.com/develop/background-work/services/fgs/service-types)。

## 配置、状态与来源限制

- Android 方案保存在应用私有目录，使用原子文件写入；支持多个可切换方案、结构化字段编辑及 TOML 导入导出。
- Android 的 TOML 文档通过 SAF 读写，不把 `content://` URI 当作桌面文件路径。
- 组网 hostname 取本机 LAN 信息中的设备名；改名后提示重新启动当前方案。
- 快捷启动使用上次选择的方案。开启仅 EasyTier 会启动组网；主动停止会关闭仅 EasyTier，意外断线或权限撤销不自动放行 LAN。
- 来源白名单只接受当前有效节点的虚拟 IPv4，兼容 IPv4-mapped IPv6；代理子网不自动加入白名单。快照超过 15 秒、状态错误或服务中断时拒绝新的入站连接。
- Application 的状态回调可能先于 Flutter 初始化；在 `APP_DIR` 确定前不加载旧配置缓存，避免将安全模式写入错误目录。
- LAN 本机密码使用现有 Android Keystore 存储，账号与远端设备条目分开；保存认证配置失败时回滚安全存储。回显前核对实际用户名及认证哈希。
- 桌面专用的系统服务、自启动、OS-keyring 配置备份入口不在 Android 显示；不能把不可用平台能力伪装成已完成。

## 网络及平台边界

- 仅安装有效的虚拟网段及代理路由。默认路由须有明确的出口配置，不因其他节点广播 0/0 就接管全机流量。
- 主进程的组网和远控不相互排除；核心的物理网络绑定只作用于隔离进程。
- 没有接管 IPv6 地址时保留系统 IPv6 路径，不额外设置公共 DNS。
- 本实现不自动接管系统 DNS；不支持的 `accept_dns`、系统内核转发、禁用 TUN 和物理网卡 UDP 捕获配置明确报错，不静默降级。
- 缺少原生 `.so` 时显示组件缺失，不能报告运行成功。JNI/TUN/后台生命周期的实际兼容性仍需要后续 SDK 编译和真机验证。

## 本轮检查与后续验证

本轮只进行源码审查、Dart 语法解析、清单/配置文件解析及 Git 差异检查；这些检查不等同于 Android 类型检查、链接、安装或功能实测。

后续具备 SDK/NDK 后，按 `libs/easytier_android/README.md` 生成与 RustDesk 主库 ABI 一致的原生库，再构建 APK。Android Studio 不是代码本身的依赖，也可使用相应命令行工具。

最小实测清单：

1. 系统授权同意、取消与撤销，缺原生库时的错误反馈。
2. 静态 IP 与 DHCP、切换方案、Wi-Fi/蜂窝网络切换、通知停止、Activity 重建与后台运行。
3. 手机自身的远控流量能经过 TUN；仅 EasyTier 模式拒绝普通 LAN 来源，服务失联时保持拒绝。
4. TOML 导入导出、多方案选择、手机设备卡片与详情同步刷新。
5. LAN 密码保存、显示/隐藏、重启后回显及错误回滚。

SDK 编译及真机发现的问题应继续在本分支修复，通过后再讨论合并和新的 Android 发布版本。
