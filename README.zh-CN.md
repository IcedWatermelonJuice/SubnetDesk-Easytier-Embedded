<p align="center"><img src="res/subnetdesk-mark.png" alt="SubnetDesk" width="120"></p>

# SubnetDesk · EasyTier 内嵌版

把 EasyTier 组网集成到远程桌面应用中。本仓库是我们独立维护的 SubnetDesk 分支，基于 [SubnetDesk](https://github.com/zibo-chen/SubnetDesk) 和 [RustDesk](https://github.com/rustdesk/rustdesk)。

**当前版本：2.0.0-rc9 · build 78** · [English](README.md) · [下载 RC9](https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded/releases/tag/rc9) · [反馈问题](https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded/issues)

## 本分支新增的功能

- 内嵌 EasyTier 方案管理：新建、编辑、复制、TOML 导入导出、切换方案及启动组网。
- 组网设备卡片和详情：按当前快照展示直连／中继、下一跳、路径及延迟信息。
- 快速启动组网和“仅允许 EasyTier 远控”：按当前节点虚拟 IPv4 限制入站来源，同时保留远控鉴权与权限检查。
- 方案编辑中的“延迟优先”开关，对应 `latency_first`，保存后重启当前组网生效。
- 桌面配置备份恢复和访问凭据管理，具体能力受平台支持限制。
- Android 通过 `VpnService` 组网；手机远控画面可选择 0°、90°、180°、270° 旋转。

SubnetDesk 使用可达的 IP／主机名直接连接，默认远控 TCP 端口为 **21118**，不依赖 RustDesk 信令服务器。EasyTier 负责建立私有网络，可使用其自身的节点／中继；不保证所有网络环境都能直连。

## RC9 下载

| 平台 | 本次产物 | 说明 |
| --- | --- | --- |
| Windows x64 | 便携 ZIP | 未签名测试版。完整解压后启动顶层 `SubnetDesk.exe`。 |
| Android arm64-v8a | APK | Android 7.0／API 24 及以上；沿用上一版 Android 测试 APK 的证书。 |

安装包及 `SHA256SUMS` 见 [RC9 Release](https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded/releases/tag/rc9)。RC9 是预发布版本。本次没有提供 macOS、Linux、Windows ARM64 或其他 Android ABI 的安装包；继承的源码支持不代表这些变体已经在本分支构建或实测。

Windows 升级时先从托盘退出应用，再替换程序文件，保留 `userdata/`。Android 可覆盖安装上一版同证书测试包；VPN 授权与被控所需的屏幕共享／输入权限分别管理。

## 快速使用

1. 两端安装 SubnetDesk。在被控端设置访问用户名和密码，开启“可被远程”，完成操作系统要求的权限。
2. 打开“组网”，在两端新建或导入兼容的 EasyTier 方案。配置自己的网络身份和对等节点地址，不要公开网络密钥。
3. 启动方案；Android 需要同意系统 VPN 授权。确认两端取得虚拟 IP，并且网络互通。
4. 使用被控端的 EasyTier IP 连接，例如 `10.88.0.2:21118`。核对设备指纹后，使用访问凭据完成鉴权。
5. 按需在被控端开启“仅允许 EasyTier 远控”。这项开关限制入站来源，不代替“可被远程”、用户名密码或系统权限。

Android 同一时间只能运行一个系统 VPN；接收远控还受屏幕共享和输入授权等系统限制。“可被发现”和“可被远程”是两个独立开关。

## 配置覆盖规则

Windows 和 Android 在生成／保存配置及启动组网时，统一强制以下四项；旧存储方案启动时也会覆盖：

```toml
[flags]
disable_quic_input = true
disable_kcp_input = true
enable_quic_proxy = false
enable_kcp_proxy = false
```

QUIC／KCP TCP 加速代理会在被控端重新建立 TCP 连接，导致当前远控来源白名单看不到原始节点 IP。因此这里禁用的是 TCP 加速代理，**普通 QUIC 隧道仍可使用**。无论是否开启“仅允许 EasyTier 远控”，上述四项都会覆盖；`latency_first` 由用户开关控制。

Android 另外强制 `proxy_forward_by_system = false`、`no_tun = false`、`enable_udp_broadcast_relay = false`，运行时 `bind_device = false`。`accept_dns = true` 仍明确报错，不会静默改成关闭。普通方案不会自动接管全机默认路由；出口路由必须有明确配置和实际发布。

“仅 EasyTier”只允许当前节点的虚拟 IPv4，不将节点发布的整个代理子网加入白名单。查询失败或快照过期时拒绝新入站连接；主动停止组网会关闭这项限制。详见 [访问控制说明](docs/design/easytier-embedded/access-controls.md) 和 [Android 适配说明](docs/design/easytier-embedded/android-support.md)。

## 从源码构建

```powershell
git clone --recurse-submodules --branch easytier-embedded https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded.git
cd SubnetDesk-Easytier-Embedded
```

RC9 使用 Flutter 3.44.1；主应用使用 Rust 1.82，独立 Android EasyTier 库使用 Rust 1.95。Android 使用 JDK 17、NDK r28c、cargo-ndk 3.1.2，min SDK 24、compile／target SDK 36。保留工程中的 Flutter SDK 表达式，不要为了组网库擅自提高最低系统版本。

准备对应平台的原生依赖，并生成工程要求的 Flutter／Rust bridge 文件后，Windows 构建入口为：

```powershell
python build.py --portable --flutter --skip-portable-pack --hwcodec
```

Android 打包前必须同时准备主远控库、独立 EasyTier JNI 库和 NDK C++ 运行库。依赖及生成步骤见 [Android 库构建说明](libs/easytier_android/README.md)、[Android 实现说明](docs/design/easytier-embedded/android-support.md)，以及既有 [构建工作流](.github/workflows/flutter-build.yml)／[bridge 工作流](.github/workflows/bridge.yml)。仅编译组网库不能得到完整应用。

## 验证与限制

RC9 已通过定向配置／界面测试，以及 Windows、Android 完整 Release 构建；已检查 Windows ZIP 完整性和文件／架构、Android 签名、原生库依赖完整性及 16 KiB 对齐。本版本尚未完成完整的真机远控／VPN 验收。

请在 [本仓库 Issues](https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded/issues) 提供可复现问题，附平台、应用版本和脱敏日志；不要提交密码、私钥或 EasyTier 网络密钥。版本变更见 [RC9 发布说明](docs/releases/2.0.0-rc9.md)。

## 致谢与许可证

感谢 [SubnetDesk](https://github.com/zibo-chen/SubnetDesk)、[RustDesk](https://github.com/rustdesk/rustdesk) 和 [EasyTier](https://github.com/EasyTier/EasyTier)。本仓库是独立维护的分支，不代表上述上游项目的官方发布。

SubnetDesk 源码沿用 [GNU Affero General Public License v3.0](LICENCE)。捆绑的 EasyTier、Wintun 等组件保留各自许可证和声明；Windows 包含对应 EasyTier 核心源码归档及许可证。仅在已获授权的设备上使用远程控制。
