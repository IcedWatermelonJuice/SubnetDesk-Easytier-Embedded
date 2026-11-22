# Android EasyTier 原生桥接

这是独立于桌面 RustDesk 工作空间的 Android `cdylib`。EasyTier 固定在提交
`8428a89d2dabc94c97d370ec607c6ca142473626`（v2.6.4），使用单独的 Rust 1.95
工具链，库名为 `libsubnetdesk_easytier_android.so`。不要将其加入根 workspace，
也不要把桌面 core/CLI 可执行程序复制到 Android 并执行。

本次只实现源代码，没有运行 Cargo、Gradle、NDK 或 APK 编译。没有生成或虚构 `.so`。
首次编译需要正常解析依赖并生成本包的 `Cargo.lock`，审阅后提交；后续构建使用
`--locked`。固定上游提交不等于已经锁定所有传递依赖。

## 与 Kotlin 的协议

对象 `com.carriez.flutter_hbb.EasyTierNative` 提供 `start(String)`、`status()`、
`setTunFd(Int)`、`stop()`、`profile(String,String)`，均返回 JSON 字符串。
JNI 错误返回 `ok:false` 和不包含原始配置密钥的错误消息，不将失败伪报成运行中。

- `start` 接收运行配置，使用新的实例 ID，不改动保存的方案。
- `profile` 的第二参数是 JSON patch，支持 network_name、secret、peers、dhcp、ipv4；
  `{}` 仅校验并读取字段。空 TOML 创建默认方案；hostname 只用于运行副本注入本机名称。
  TOML 通过结构化编辑保留未修改字段，不使用正则改写。
- `status` 包含 state、instance_id、ipv4（CIDR）、nodes、routes、allowed_ips、
  tun_routes、mtu、tun_ready、allow_default_route、queried_at、error。
  节点展示通过共享的 RC8 `network_view.rs` 数据关联算法生成。
- `running` 要求核心存活、虚拟 IPv4 已取得，并收到核心实际 `TunDeviceReady` 事件；
  仅把 fd 放入消息队列不代表网卡可用。未运行状态的 allowed_ips 为空。
- 状态 JSON 有 240 KiB 上限，防止 Binder 单次传输溢出；超限明确失败，不悄悄截断授权信息。

## 线程、网络及描述符所有权

桥接在专用线程上显式创建一个 Tokio runtime；上游 EasyTier launcher 自己拥有其核心线程。
JNI 不在调用线程中创建 runtime，异步代码不持有 Mutex 进入 await。上游实例销毁会 join，
因此停止操作放到 blocking pool，完成后才关闭 TUN 的副本描述符。

VpnService 必须运行于独立 `:easytier` 进程，启动核心之前把该进程绑定到真实的非 VPN
底层网络，并通过 `setUnderlyingNetworks` 标明底层连接。这样只让 EasyTier 的传输套接字
绕过自身 VPN；不能用 `addDisallowedApplication(packageName)` 把 SubnetDesk 的远控流量也排除。
物理网络改变时由服务停止并重建核心/TUN，不依赖提升权限的 SO_BINDTODEVICE。

Kotlin 持有原始 ParcelFileDescriptor，传 `getFd()`。JNI 用 CLOEXEC 的 dup 创建自己的
OwnedFd。固定上游 mobile TUN 设置 `close_fd_on_drop(false)`，故桥接必须保留并最终关闭
此副本。重建 TUN 时保留旧副本，收到新 TUN ready 后再释放；停止核心后释放全部副本。

只把虚拟网段、节点和已发布代理子网放入 tun_routes。只有当前方案显式配置出口节点，且该
出口的有效核心路由确实发布 `0.0.0.0/0` 时，才允许默认路由。普通方案不自动接管全网。
不设置第三方公共 DNS；`accept_dns` 目前明确拒绝。系统转发、no_tun 和物理广播捕获也
明确拒绝，不静默承诺 Android 无法实现的能力。传输 IPv6 沿用核心支持；本桥接状态和 TUN
路由输出以当前 SubnetDesk 的 IPv4 组网模型为准，未接管 IPv6 由 Kotlin 保留系统处理。

## 以后具备 SDK/NDK 时

1. 保留原项目 `flutter.minSdkVersion`、`flutter.targetSdkVersion`、compile SDK 表达式，
   根据实际 Flutter SDK 读取 min SDK 数值，不为了此库提高它们。
2. 安装对应 Rust Android targets、cargo-ndk、NDK、Protobuf 构建依赖。
3. 显式执行 `python libs/easytier_android/build_android.py --ndk-home <NDK目录> --min-sdk <原minSDK>`。
4. 产物输出到 `flutter/android/app/src/main/jniLibs/<ABI>/`，然后再正常构建 APK。
5. 实机检查 VPN 授权、DHCP 与静态 IP、TUN ready/error、切网、重建/断开/revoke、后台通知，
   以及仅 EasyTier 来源策略失败关闭。源码核对不等于这些运行验证已经通过。
