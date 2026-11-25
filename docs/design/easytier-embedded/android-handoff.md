# Android 适配与工具准备：新会话交接（2026-11-24）

## 1. 新会话先做什么

本文件最初用于 Android 工具准备交接。用户随后明确授权“编译一个试试”，现已完成 arm64-v8a 测试签名 Release APK，并通过签名、完整性和原生库检查。**第 14 节是最新分支整合状态**；首轮构建记录见第 11 节。用户已反馈首轮真机问题，本代理未安装或进行真机验证，不自动合并、推送或发布。

建议新会话读本文件后再按需读 `android-support.md` 和 `libs/easytier_android/README.md`。不要重新从头调查全部代码，也不要把“源码已实现”当作“Android 已编译通过”。

## 2. 仓库、提交与操作边界

- 当前工作树：`C:\Users\FocusLab-XL\.codex\worktrees\easytier-wfp\SubnetDesk`。
- 当前分支：`feat/android-support`。
- 最近提交：`c111e40bf`，Android 首轮交叉构建修复与 EasyTier 依赖锁定。
- 前一提交：`2de90a301`，修复 Android 组网网络绑定与页面生命周期清理；更早 `f8791ac85` 保存适配初版。
- 分支基于主分支 RC8：`80abd7d6d7de73499138e1fed7460b0a822ea528`。
- Android 分支尚未推送、尚未合并。主分支是 `easytier-embedded`；`master` 保留原 v2.0.0 基础。
- 主工作树：`E:\GitHub\SubnetDesk`；共享 Git 索引实际位于 `E:\GitHub\SubnetDesk\.git\worktrees\SubnetDesk3`。本工作树里的 Git 写操作可能需要沙箱提权。
- 私有远程：`https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded.git`，与原作者仓库无关；Actions 之前已关闭。
- 开始此次交接前，已提交源码无未提交修改；新增 `.tmp/` 未跟踪，内含本轮下载脚本。当前文档重写也尚未提交。不要把 `.tmp/` 整目录随手加入提交。

用户要求：

- 简体中文交流、文档和提交说明；PowerShell 7 优先，UTF-8 文件读写。
- 不再使用 DSH MCP / dsh-flash，之前该 MCP 有问题。
- 只做必要验证。工具准备不等于授权编译项目。
- 不安装 Android Studio 或模拟器；先只准备 arm64-v8a 真机版本所需组件。
- 保留原仓库 SDK 标准，不擅改 Gradle SDK 声明。
- 不真实建立 VPN、远控、操作用户凭据做验证。
- 不擅自合并、推送或发布 Android 版本。

## 3. 工具目录与已有环境

用户同意工具统一放 `D:\toolchains`。核对时 D 盘约剩余 341 GB，C 盘约剩余 38 GB。尽量把缓存放 D 盘，不改全局 Java 或 PATH。

| 工具 | 目录／说明 |
| --- | --- |
| Flutter 3.44.1 | `D:\toolchains\flutter`，已有 Android arm/arm64/x64 引擎缓存，不需要重复下载 |
| Pub 缓存 | `D:\toolchains\pub-cache` |
| Rust/Cargo | `RUSTUP_HOME=D:\toolchains\rustup`，`CARGO_HOME=D:\toolchains\cargo` |
| Rust 工具链 | 已有 1.82、1.82.0、1.95；主项目用 1.82，独立 EasyTier 用 1.95 |
| Android SDK | `D:\toolchains\android-sdk`，尚未完成组件安装 |
| JDK 17 | `D:\toolchains\jdk-17`，已下载校验并解压 |
| 系统原有 Java 21 | `C:\Program Files\Eclipse Adoptium\jdk-21.0.9.10-hotspot`，未替换 |
| Gradle 缓存 | `D:\toolchains\gradle-cache`，目录已建，未下载 Gradle 分发包 |
| 官方下载与日志 | `D:\toolchains\android-downloads` |
| Protobuf | `D:\toolchains\protoc-29.3\bin\protoc.exe`，已有 |
| CMake | `D:\toolchains\vs\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe`，已有 |
| Ninja | `D:\toolchains\vs\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja\ninja.exe`，已有 |
| LLVM / vcpkg / VS | `D:\toolchains\llvm`、`D:\toolchains\vcpkg`、`D:\toolchains\vs`，已有；Android 目标依赖尚未构建 |
| 拟用 Android 构建副本 | `D:\toolchains\subnetdesk-android-build`，尚未创建，先不执行项目构建 |

已通过本机 FlutterExtension.kt 读取原表达式的实际值：compile SDK 36、target SDK 36、min SDK 24，默认 NDK `28.2.13676358`。原项目工作流固定 NDK r28c、cargo-ndk 3.1.2、Java 17、Gradle 8.14.3-all。**不是要把这些数字硬编码回项目。**

`flutter/android/app/build.gradle` 的三项仍完全未改：

```groovy
compileSdkVersion flutter.compileSdkVersion
minSdkVersion flutter.minSdkVersion
targetSdkVersion flutter.targetSdkVersion
```

## 4. 下载／安装的真实状态

### 已完成

1. Temurin JDK 17.0.21.1+2 已解压到 `D:\toolchains\jdk-17`，压缩包已与 Adoptium API 的 SHA256 核对。
2. Android 官方命令行工具 `commandlinetools-win-16111833_latest.zip` 已与官方清单 SHA1 核对，解压到 `android-sdk\cmdline-tools\latest`。
3. Rust 1.82、1.95 的 `aarch64-linux-android` 标准库均已安装。最后用 `rustup target list --installed --toolchain 1.82` / `1.95` 确认两者均列出该 target。
4. Flutter 的 Android 引擎、Protobuf、CMake、Ninja 原先就有，未重复安装。

### 未完成

- NDK r28c 尚未合并完整压缩包，也未解压安装。
- SDK Platform 36、Build Tools 36.0.0、Platform Tools/ADB 均未安装，已按目标文件不存在核对。
- cargo-ndk 3.1.2 尚未安装：`cargo +1.82 install cargo-ndk --version 3.1.2 --locked` 在 crates.io 索引超时，缺 `windows_aarch64_gnullvm` 等索引条目。已有下载缓存可以复用。
- Gradle 8.14.3-all 尚未下载，仅查询了官方 SHA256 与长度。
- Android vcpkg 库没有下载构建；主远控库和新增 EasyTier 库均没有编译。
- 没有创建 Android 签名密钥、APK 或 `.so`。原 release 构建依赖 `android/key.properties`；以后实测包可另行选择签名，不复用原项目私钥。

## 5. 网络问题与续传要点（避免重复踩坑）

### 大文件停顿

普通 curl 和 rustup 下载多次停在中途。`curl --range` 下载 4 MiB 小分段可正常工作，因此写了两个临时脚本：

- `.tmp/codex-range-download.py`：4 路并发分段，每段验证 Content-Range 和长度，全部合并后验证官方 SHA1/SHA256。
- `.tmp/codex-download-android-tools.ps1`：CLI/JDK/NDK 的官方校验与解压；下载时支持断点续传。

**重要限制：Python 分段脚本目前不会自动复用上一次已完成的 `.part`，直接重跑会重下分段。** 新会话应先检查已有 `.headers` 的 Content-Range 和 `.part` 长度，只补缺失／无效段，再顺序合并、核对最终官方哈希，不能为了省下载跳过校验。

NDK 缓存现状：

- 官方总长度 `748118221` 字节。
- 主文件 `D:\toolchains\android-downloads\android-ndk-r28c-windows.zip` 目前只有前 `109620608` 字节，不能解压。
- 另有 153 个 `.part`，合计 `636927741` 字节（包含失败的不完整段）；对应 `.headers` 也保留。
- 此次失败段：`227061120-231255423`，预期 4194304 字节，最后只收到 2624432 字节后超时。
- 其他段仍应逐个按响应头和实际长度验证，不能仅凭数量认定完整。合并从主文件长度开始，每步 4194304 字节；最后一段到 `748118220`。
- 官方 SHA1：`086bba43ff2f5eb0e387b15c8278bb4e0d89ba1d`，以缓存的 Google 清单再核对。
- 全部合并并通过哈希后，才可运行解压脚本 `-Package NDK`；该脚本遇到完整且校验一致的 zip 会直接解压，不再下载。

### 最新 SDK 管理器的额外引导下载

本轮拿到的 latest 命令行工具已把 sdkmanager 改成 Android CLI 包装器。日志显示：

```text
WARNING: The SDK Manager CLI tool (sdkmanager) is deprecated. Android CLI will be used instead.
Downloading Android CLI...
```

额外 Android CLI 引导下载卡住，Platform/Build Tools/ADB 尚未真正安装。`--licenses` 此版本提示不再需要，**不能把这当成所有所需 SDK 许可已完成**。

准备过的应对方案是改用 Google 官方固定版命令行工具 19.0（仍提供传统 sdkmanager），但**尚未执行下载或切换**。它与新 Android CLI 的安装方式可择一，不要反复重启卡住的 latest 包装器。

- 官方包：`https://dl.google.com/android/repository/commandlinetools-win-13114758_latest.zip`
- 官方清单 package：`cmdline-tools;19.0`
- 长度：`143040480` 字节。
- SHA1：`54a582f3bf73e04253602f2d1c80bd5868aac115`。
- 安装时避免覆盖已有目录；可先并列解压到 `cmdline-tools\19.0`，显式调用其 sdkmanager。
- SDK 目标包：`platforms;android-36`、`build-tools;36.0.0`、`platform-tools`。

曾卡住的进程是 PID 32728：

```text
D:\toolchains\android-sdk\cmdline-tools\latest\bin\android.exe --sdk="D:\toolchains\android-sdk" sdk install platforms android-36 build-tools 36.0.0 platform-tools
```

收尾时再次在沙箱外枚举本轮命令对应的安装器，匹配数量为 0；它已经不在运行。NDK 分段下载和 cargo-ndk 安装进程也已失败退出。新会话仍应先核对实际进程；**不要凭旧 PID 杀进程**，也不要把旧会话 exec session ID 当成新会话能继续使用的任务句柄。

日志：`sdk-install.log`、`sdk-licenses.log`。官方清单缓存：`repository2-3.xml`，JDK/CLI 元数据：`sources.json`，均位于 `D:\toolchains\android-downloads`。

### 高校镜像站

用户打开 `https://mirrors.cernet.edu.cn/list`，问是否有适用镜像。该目录需要 JavaScript；网页读取只返回启用 JavaScript 提示，浏览器控制读取后来被中止。

**尚未实际核对该站的 Rust/Gradle/Android 镜像目录，也没有切换镜像。** 新会话可继续查目录，优先考虑 Rust crates/标准库及 Gradle 镜像；确认确实含所需文件与版本，再复用官方哈希核验。不要声称已经找到可用镜像，不要关闭 TLS 校验。

## 6. 已取得的官方校验信息

| 包 | 长度（字节） | 官方校验 |
| --- | ---: | --- |
| CLI 16111833 | 154957218 | SHA1 `57d04f2d75eb8e8fffc5000a987e5de4b5a63e9d` |
| Temurin JDK17 zip | 191150540 | SHA256 `eafba0944f777abdc74d5aad16049c9b5a1275b6ce99e2939ba9f7f3e6f08c87` |
| NDK r28c Windows | 748118221 | SHA1 `086bba43ff2f5eb0e387b15c8278bb4e0d89ba1d` |
| Gradle 8.14.3-all | 224584249 | SHA256 `ed1a8d686605fd7c23bdf62c7fc7add1c5b23b2bbc3721e661934ef4a4911d7c` |

Gradle 官方地址：`https://services.gradle.org/distributions/gradle-8.14.3-all.zip`，校验地址为同 URL 后加 `.sha256`。

## 7. Android 代码实现概况

初版交接时这些代码仅完成源码实现；本轮 Kotlin/Rust Android 编译和链接已通过，完整 APK 结果见第 11 节。**仍未安装或真机验证**。

- 复用 RC8 Flutter 组网页和设备圆角卡片；Android 通过 `com.zibochen.subnetdesk/easytier` MethodChannel，桌面仍走原 FFI。
- Kotlin `EasyTierVpnService` 运行独立 `:easytier` 进程；跨进程 Messenger 校验应用 UID，系统 VPN 授权由 Activity 发起。
- 只有独立核心进程绑定物理非 VPN 网络；主远控进程继续走系统 TUN 路由。**禁止 addDisallowedApplication(packageName) 排除整个应用。**
- 多方案应用私有原子存储；SAF 导入导出 TOML；hostname 与本机页面 device_name 一致。
- 独立 Rust cdylib `libs/easytier_android` 不加入根 workspace，固定 EasyTier `8428a89d2dabc94c97d370ec607c6ca142473626`（2.6.4），工具链 1.95；主项目仍用 1.82。
- JNI 库名 `libsubnetdesk_easytier_android.so`；对象 `com.carriez.flutter_hbb.EasyTierNative`。五接口 start/status/setTunFd/stop/profile。
- Kotlin 持原 PFD，JNI 持 CLOEXEC dup；真实 TunDeviceReady 后才 running，停止 join 后释放副本。
- 复用 `src/easytier/network_view.rs` 做节点、直连／中继、路由下一跳及延迟展示；启用 AES-GCM。
- 根 Rust `src/android_easytier.rs` 只接收受保护快照，用于来源 IP 白名单，15 秒过期拒绝；异常不自动放开 LAN。
- Android LAN 本机密码接现有 Keystore，支持回显，和远端 fingerprint 凭据独立。
- 手机快捷双开关、设备页与首页 EasyTier 列表共用控制器；节点操作只填入 IP。
- 手机隐藏桌面专用系统服务、自启及桌面备份入口。

最近提交 `2de90a301` 修复：

- API22+ Builder 设置底层物理网络，建立后检查 setUnderlyingNetworks 返回值。
- TUN 缓存键纳入 MTU、物理网络和默认路由许可；失去虚拟地址退 starting。
- 失败取消待启动方案、清理网络请求及通知；销毁后不再向关闭的执行器提交网络事件。
- 停止失败走仅独立进程的延迟终止兜底；跨线程字段 Volatile、前台通知启停共享锁。
- 存储损坏保持限制开启；销毁不覆盖故障状态。
- Activity 退出统一完成 Flutter 请求、结果只回调一次，导出迟到回调不再拉起选择器。

## 8. 后续代码／验证边界

1. 独立 `libs/easytier_android` 已真实解析、审阅并提交 Cargo.lock（`c111e40bf`）；后续使用 --locked。
2. Kotlin/Rust/JNI 编译、原生链接及 APK 打包已通过；系统 API 与业务行为的实际兼容性仍须真机确认。
3. 重点真机核对 Wi-Fi/蜂窝切换、TUN重建、通知停止、onRevoke/onDestroy重入、后台状态及主远控进程经TUN连通。
4. 停止超时的 `releaseFailedCore()` 只能杀 `:easytier`，绝不能终止主进程。
5. 核对 remote_only 持久化、启动初始化顺序、过期快照拒绝新连接，主动停止与异常区别。
6. 检查授权取消、SAF取消、页面重建、组件缺失、超时反馈；不能把 starting 当成功。
7. 当前明确拒绝 accept_dns 系统 DNS 接管；系统转发、no_tun、物理 UDP 广播捕获三项按用户要求在保存和启动时强制设为 false。虚拟网卡状态以 IPv4 为主，未接管 IPv6 保留原路由。
8. 默认路由仅明确配置出口且收到有效出口 0/0 时接管；不得无条件接管全机网络。
9. 完整 APK 需要主远控库、EasyTier库与必要 C++共享库，不能只编译 EasyTier 就宣称完成。
10. 原工作流及 `flutter/ndk_arm64.sh` 是 Linux/Bash 流程；本轮是 Windows，不应直接照搬。该旧脚本写死 --platform 21，未来构建需按实际原 Flutter minSdk 24评估并显式传递，不能偷偷修改应用 SDK 标准。

## 9. 原工具准备推荐顺序（历史）

1. 读本文件，git status，核对实际进程与文件，避免重复下载。
2. 先从已下载 NDK 分段补缺、合并、官方哈希校验、解压。
3. 查用户提供的高校镜像目录，有匹配版本再用于 Rust crates/Gradle 等加速；只做必要源配置，不改全局已有工具链默认值。
4. 解决 SDK 管理器引导问题，安装 SDK Platform36 / BuildTools / ADB；安装 cargo-ndk3.1.2，预备 Gradle 分发包与构建环境脚本。
5. 用版本命令、文件存在性和 SDK 清单确认工具齐备；不做项目编译。
6. 向用户报告工具准备完成，再按用户新授权决定是否实际构建。不要仅因工具齐备自动编译。

## 10. 工具准备完成记录（2026-11-24，续接会话）

本节更新前文的未完成状态。**仅准备并检查工具，没有编译项目、构建 vcpkg Android 依赖、生成 APK／项目 .so、创建签名密钥、连接设备或运行 VPN。** 未合并、推送或发布。

- 本次会话从主工作树 `E:\GitHub\SubnetDesk` 接手，交接文件实际位于本 Android 工作树；未切换或修改主工作树的分支。
- 修正临时分段下载器以校验并复用已有分段响应头和长度。NDK 仅补齐无效段，合并后通过官方 SHA1 `086bba43ff2f5eb0e387b15c8278bb4e0d89ba1d`，解压至 `D:\toolchains\android-sdk\ndk\28.2.13676358`。`source.properties` 为 r28c / 28.2.13676358，clang --version 为 19.0.1。NDK 为官方 ZIP 手动安装，sdkmanager --list_installed 不列出它，已单独核验。
- 官方命令行工具 19.0 并列安装于 `cmdline-tools\19.0`，未覆盖 latest；ZIP 长度与 SHA1 均符合第 5 节。显式调用该目录的 sdkmanager，成功绕过 latest 包装器。
- SDK 安装完成，所需 `android-sdk-license` 已接受：`platforms;android-36` revision 2、`build-tools;36.0.0`、`platform-tools` 37.0.3。`sdkmanager --list_installed`、android.jar 存在性、aapt2 version 和 adb version 均通过。只调用 adb version，没有启动设备连接。
- `cargo +1.82 install cargo-ndk --version 3.1.2 --locked` 成功，cargo ndk --version 为 3.1.2。仅编译安装这个工具本身。主项目与独立 EasyTier 的依赖均未构建；安装时保留原锁文件，Cargo 提示其中 hermit-abi 0.3.1 已 yanked。
- Gradle 8.14.3-all 官方 ZIP 完整下载并通过第 6 节 SHA256，保存于 android-downloads，已展开至 `D:\toolchains\gradle-cache\wrapper\dists\gradle-8.14.3-all\10utluxaxniiv4wxiphsi49nj`，可复用原 wrapper URL。只运行 gradle --version，确认版本 8.14.3、JVM 17.0.21.1；未运行 Gradle 项目任务。
- JDK17 与 Rust 1.82 / 1.95 的 aarch64-linux-android target 再次核验通过。
- 会话环境脚本：`D:\toolchains\android-env.ps1`。在 PowerShell 执行 `. 'D:\toolchains\android-env.ps1'` 后生效；不会构建、不会修改全局 PATH / Java / Rust 默认工具链，也不改应用 SDK 声明。
- 续传脚本：`D:\toolchains\android-downloads\android-range-resume.py`；SDK 日志：同目录 `sdk-install-19.log`；工具版本核验：同目录 `tools-verified-20261124.log`。原下载缓存保留，未删除用户文件。
- 本次官方下载已成功，没有切换高校镜像或修改全局源配置。没有安装 Android Studio 或模拟器，也没有创建 Android 构建副本。

上述为工具准备完成时的状态。用户随后授权首轮构建，工具与构建结果见第 11 节；真机验证仍按第 8 节处理。
## 11. Android 首轮构建完成（2026-11-24）

用户已在本会话明确授权“编译一个试试”，前文“不编译”的限制对这次本地试构建已解除。仍不安装到设备、不运行 VPN、不合并、不推送、不发布。

- 源码工作树仍是 `C:\Users\FocusLab-XL\.codex\worktrees\easytier-wfp\SubnetDesk` / `feat/android-support`。独立构建副本为 `D:\toolchains\subnetdesk-android-build`，未复制旧目录 ACL、target 或 Flutter 生成缓存。
- 共享子模块复用主工作树干净源码，核对提交为 `b6e4a66177c1634555ad4812de7f38fddeb531f2`。
- vcpkg 原压缩包安装缺 Git 元数据，已补充固定基线 `120deac3062162151622ca4860575a33844ba10b` 的浅 Git 对象，未覆盖现有源码文件。Android 依赖单独安装于 `D:\toolchains\vcpkg-android-installed`，全部 20 项安装完成。因部分上游 build.rs 忽略 VCPKG_INSTALLED_ROOT，创建 `D:\toolchains\vcpkg\installed\arm64-android` 到该安装目录内 arm64-android 的 junction；原 Windows 依赖保留。
- 本轮必要源码修正：根 build.rs 依据 CARGO_CFG_TARGET_OS 保护 Windows 构建；AOM Windows→Android 使用 NDK clang 集成汇编器；vcpkg 为 Android 声明 libsodium；现有固定版本 tempfile 移入公共依赖以满足跨平台录屏预检；app Gradle 引用 flutter.ndkVersion，三项 SDK 声明保持原样。
- Rust 1.95 独立 EasyTier Release 构建成功，生成 arm64 `libsubnetdesk_easytier_android.so`（24,309,184 字节）。SHA256：`9e0cd0be28ffa9063269390d52f0d12c273fc8ea840c1ddc2fab5fb02578eca6`。五个 JNI 导出、AArch64、仅系统动态依赖及 16 KiB LOAD 对齐检查通过。生成并审阅了独立库 Cargo.lock，已复制回源码；后续使用 --locked。
- Rust 1.82 主远控库 Release 构建成功，日志 `android-rustdesk-build-8.log`。生成 `liblibrustdesk.so`（23,602,136 字节），SHA256：`e3499342cde466dac470e5ae93c0990444f0c8a39415bbfcce7c526c9c5acfb6`。AArch64、16 KiB LOAD 对齐和动态依赖检查通过，已按原工作流复制为 jniLibs/arm64-v8a/librustdesk.so，并放入 r28c 的 libc++_shared.so。
- 主库构建脚本保存于 `D:\toolchains\android-downloads\android-rustdesk-build.ps1`，只改进程环境。脚本暂时按真实目标修正固定 hwcodec 的 build.rs，清理本次 Android target 中该依赖的旧构建脚本，结束后恢复原始字节；已核对恢复成功。没有将上游缓存的原有 QSV 修改覆盖掉。
- Windows cargo-ndk/cc 会用无 API 后缀的 --target 覆盖包装器；NDK r28c 的 C++ 条件变量头文件因此报 pthread_cond_clockwait 未声明。最小复现确认，显式 CFLAGS/CXXFLAGS_aarch64_linux_android=--target=aarch64-linux-android24 后通过，没有提高 minSdk。
- OpenSSL 自动构建的 make 会吞掉 Windows 包装器路径反斜杠。复用同一锁文件的 OpenSSL 3.5.4 源码，用规范 clang/ar 路径及 API24 手动 make build_libs/install_dev，结果保存在 `D:\toolchains\android-openssl-3.5.4`；只为 Android 目标设置 OPENSSL_DIR/STATIC/NO_VENDOR，未更换版本。日志 `android-openssl-build.log`，过程脚本位于主工作树 `.tmp/android-openssl-build.sh`。
- Gradle 的 Java HTTPS 下载间歇失败。专用临时 Maven 缓存只监听 `127.0.0.1:18764`，上游仍是官方 HTTPS，并按仓库隔离；仅按官方 SHA1 复用已有 JAR，大文件用分段下载。当前脚本 `E:\GitHub\SubnetDesk\.tmp\android-maven-proxy-v3.py`，临时 init 为 `D:\toolchains\gradle-cache\init.d\android-local-maven-cache.gradle`。任务结束须移除本轮 init 并停止本轮缓存服务，保留下载结果。不要再用较早的 proxy.py 版本，早期跨仓库元数据复用有误。
- Flutter 为插件自动安装 SDK Platform 31、34 和 Build Tools 35.0.0；app 仍 compile/target 36、min 24。另按官方清单安装 SDK CMake 3.22.1（SHA1 `292778f32a7d5183e1c49c7897b870653f2d2c1b`）；VS CMake 不能替代 Flutter 自动要求的这个 SDK 版本。
- 仅构建副本将 release 的 signingConfig 改为 debug，使用专用本地测试签名 `D:\toolchains\android-test-debug.keystore`；原源码 release 签名配置未改，不用原项目私钥。
- 构建副本复用既有 FRB 1.80.1 生成文件；已核对当前构建副本与既有 Windows 构建副本的 flutter_ffi.rs 哈希一致，并由本轮 Rust/Dart 完整编译验证兼容性。所有下载/原生构建日志集中于 `D:\toolchains\android-downloads`。

### 最终 APK 与检查结果

- 完整 Flutter/Gradle Release 构建成功，包含主远控库与 EasyTier 库，日志 `D:\toolchains\android-downloads\android-flutter-build-7.log`。
- 产物：`D:\toolchains\subnetdesk-releases\SubnetDesk-EasyTier-2.0.0-rc8-android-arm64-test-20261124.apk`，81,939,223 字节（约 78.1 MiB）。
- SHA256：`73b5cf66d91af55923f3a0747f340eacbb85961a0020ac463a8e9eb30ceb94da`。
- applicationId `com.zibochen.subnetdesk`，应用名 SubnetDesk，versionName `2.0.0-rc8`，实际 arm64 分包 versionCode `2077`（工程 build 77）；compile/target SDK 36、min SDK 24。
- apksigner 验签通过，采用专用 Android Debug 测试证书，APK v2 签名有效；这是测试签名，未使用正式发布身份。
- ZIP CRC、zipalign 的 16 KiB 检查、全部 5 个原生库 AArch64/ELF64/16 KiB LOAD 对齐与动态依赖完整性检查通过。
- 包内主库经 AGP strip 后 SHA256 为 `ff51994ab26f5a6ba2176f2998f37c4d1189fbdcf562f0802b0b874a992847f7`，已核对与 stripReleaseDebugSymbols 产物一致；与前文原始主库哈希不同。
- 核验报告：`android-apk-content-check.json`、`android-apk-signature-check.log`、`android-apk-manifest-check.log`，位于 android-downloads。
- 本轮必要修复与独立 Cargo.lock 已本地提交为 `c111e40bf`；未合并、推送或发布。交接文档已有修改继续保留在工作树。
- 本轮临时 Gradle init 已移除，loopback Maven 缓存服务已停止；下载结果保留。当前可复用脚本已归档到 `D:\toolchains\android-downloads`：`android-maven-proxy-v3.py`、`android-maven-cache.init.gradle`、`android-openssl-build.sh`、`android-rustdesk-build.ps1` 和 `check-android-apk.py`。不要默认开启本机缓存服务；需要新增下载时再显式启动并临时安装 init。
- 保留编译器的未使用变量、旧 API、KGP 未来迁移和既有图标字体声明警告；未为消除警告修改无关代码。

**没有安装到设备、启动应用、建立 VPN、操作真实远控或验证实际用户凭据。** 下一步应由用户用此测试 APK 做第 8 节真机检查；不能把编译成功表述为 Android 功能已实测通过。
## 12. 桌面方案兼容与手机画面旋转（2026-11-25）

用户实际测试首轮 APK，从桌面导入方案后遇到“Android 方案不支持系统转发、no_tun 或物理网卡 UDP 广播捕获”。用户明确要求强制复写三项，并补齐手机远控电脑的画面旋转入口。

- Android 在新建、导入及结构化编辑生成 TOML 时，将 `proxy_forward_by_system`、`no_tun`、`enable_udp_broadcast_relay` 强制设为 `false`，保留其他字段。启动旧方案也会先归一化并在 Worker 中再次覆盖，旧方案无需重新导入；没有批量改写旧存储文件。`accept_dns` 仍明确拒绝，其他路由及来源限制不变。
- 手机远控底部工具栏新增旋转菜单，可选 0°、90°、180°、270°；复用桌面会话的角度持久化和单屏能力限制。画面、光标及手机相对鼠标方向使用对应旋转映射；仅旋转本地显示画面，不改变远端系统屏幕方向。工具栏在窄屏可横向滚动，收起按钮固定。
- 配置覆盖的两个 Rust 单元测试通过（普通/inline flags、其他字段保留、缺省 flags 与幂等）；独立临时 harness 直接加载生产模块，未运行 VPN。
- 两个 Flutter widget 测试通过（320 像素窄屏角度选择及禁用菜单）；针对改动文件静态分析没有 error 或 warning，保留 56 条既有旧 API info。
- EasyTier JNI Release 增量构建成功；保留既有 `network_view.rs` 未使用 `cidr` 警告。复用上一轮主远控库，无需重建该库。
- 本轮日志：`D:\toolchains\android-downloads\android-profile-flags-test.log`、`android-rotation-widget-test-final.log`、`android-easytier-forced-flags-build.log`、`android-flutter-rotation-flags-build.log`。
- 该轮构建被用户暂停；随后按 RC9 统一构建，结果见第 13 节。未安装设备、建立 VPN 或操作真实远控；仍不合并、推送或发布。
## 13. RC9：来源校验兼容、延迟优先开关与双端构建（2026-11-25）

- 用户确认 QUIC TCP 代理会让被控端的 TCP 来源变成被控本机地址，从而被“仅允许 EasyTier 远控”拒绝。按用户要求，Windows 和 Android 的配置保存与启动均强制 `disable_quic_input = true`、`disable_kcp_input = true`、`enable_quic_proxy = false`、`enable_kcp_proxy = false`；普通 QUIC 隧道保留，未放宽白名单。
- 两端共用方案编辑器新增“延迟优先”开关，读取及写入 `[flags].latency_first`；新建默认开启，已有方案按配置回显，保存后重启当前组网生效。该属性由用户控制，不属于强制覆盖。
- Android 同时包含先前已授权的三项配置兼容覆盖和手机画面旋转菜单；运行时仍强制 `bind_device = false`，`accept_dns = true` 仍拒绝。旧方案启动时归一化，不批量改写旧存储文件。
- 两工作树应用版本已统一为 `2.0.0-rc9`、Flutter build `78`，更新 Cargo.toml/Cargo.lock、pubspec.yaml、Windows Runner.rc 和 Android versionName fallback。Windows 主程序与启动器资源版本均为 `2.0.0-rc9+78`；Android arm64 分包 versionCode 为 `2078`。
- Windows 源码位于 `E:\GitHub\SubnetDesk`；Android 源码位于 `C:\Users\FocusLab-XL\.codex\worktrees\easytier-wfp\SubnetDesk`。本轮修改保留在各自工作树，未提交、合并、推送或发布；既有 `.tmp/` 和未跟踪文档未加入提交。
- 定向测试通过：Windows 生产方案模块 2 项、Android 生产 flags 模块 3 项；Windows UI 2 项、Android UI 4 项（含手机旋转菜单）。静态分析没有 error，保留原有 public/private 类型及空值操作提示。Windows/Android 完整 Release 构建均通过。

### Windows 产物

- `D:\toolchains\subnetdesk-releases\SubnetDesk-EasyTier-2.0.0-rc9-windows-x64-portable-20261125.zip`
- 大小：58,597,997 字节。SHA256：`1a6bd25efb4a42a2839fd1946b07c22263d44cb328fdbf6c31ab17a7f208a8ca`。
- ZIP CRC、必要文件、PE x64、包内文件与本轮构建产物逐字节一致及不夹带用户数据检查通过。包含固定 EasyTier core/CLI、Wintun、许可证和核心源码归档。Windows 为未签名测试包。
- 日志与核验：`D:\toolchains\tmp\codex-rc9-build.log`、`codex-package-rc9.log`、`rc9-windows-package-check.json`；可复用驱动脚本 `codex-rc9-build.ps1` 和 `codex-package-rc9.py` 在同目录。

### Android 产物

- `D:\toolchains\subnetdesk-releases\SubnetDesk-EasyTier-2.0.0-rc9-android-arm64-test-20261125.apk`
- 大小：81,945,711 字节。SHA256：`df2234cfd047aae9fe4c2b5b3fd29f1327895d5c1436aefdb9840b4515608db5`。
- applicationId `com.zibochen.subnetdesk`、versionName `2.0.0-rc9`、versionCode `2078`；min SDK 24、compile/target SDK 36、NDK r28c 均保持原标准。
- 专用测试证书与首轮 APK 一致，APK v2 验签通过。ZIP CRC、zipalign 16 KiB、5 个 arm64 ELF 的 LOAD 对齐及动态依赖完整性检查通过；包内主远控库和 EasyTier 库与本轮 AGP strip 产物逐字节一致，主库包含 RC9 版本。
- 日志位于 `D:\toolchains\android-downloads`：`android-easytier-rc9-build.log`、`android-rustdesk-rc9-build.log`、`android-flutter-rc9-build.log`、`rc9-android-apk-content-check.json`、`rc9-android-apk-signature-check.log`、`rc9-android-apk-manifest-check.log`、`rc9-android-native-build-match.json`。
- 本轮 Android Gradle 复用第 11 节的同仓库 loopback URL 缓存并开启离线解析，没有启动缓存服务或新增 Maven 下载。构建后已移除本轮临时 `gradle-cache/init.d/android-local-maven-cache.gradle`。主库构建时 hwcodec 缓存临时修正已恢复，恢复后摘要与原文备份一致。
- 构建副本仍使用专用测试签名；源码工作树未改正式签名配置。核验脚本 `D:\toolchains\android-downloads\verify-rc9-apk.ps1`；两个最终产物均附 SHA256 文件。

未安装设备、启动应用、建立 VPN 或执行真实远控／凭据验证。下一步由用户复测 RC9：仅 EasyTier 下的鉴权、延迟优先开关保存及重启、手机画面旋转和输入坐标。
## 14. RC9 统一源码与发布（2026-11-25）

用户明确决定采用单一主分支维护，授权将 Android 适配合入 `easytier-embedded`，并从同一提交重新构建 Windows 和 Android。此前第 13 节两个工作树分别构建的记录及摘要为历史记录，不能当作本次统一源码发布包的摘要。

- 主分支同时包含 Windows 与 Android；保留平台目录、条件编译和独立 Android EasyTier crate，继续使用两个独立构建目录及工具链，不将 Rust 1.95 核心依赖并入 Rust 1.82 主 workspace。
- 本次合并保留异步方案读取接口；开关测试按同一接口运行。维护中英文 README 和 `.gitignore`，排除临时目录、缓存、运行数据、安装包及本机签名配置，保留源码与 Cargo.lock。
- 原有独立构建包保留。本次统一源码的构建／发布包使用独立的 release 产物名称及核验记录；最终摘要与源提交以 GitHub `rc9` Release 为准。
- 远程仓库为 `IcedWatermelonJuice/SubnetDesk-Easytier-Embedded`；用户已授权推送和发布 RC9/build78。仓库 Actions 保持原关闭状态，构建在本机执行；不安装应用或运行真实 VPN／远控进行验收。