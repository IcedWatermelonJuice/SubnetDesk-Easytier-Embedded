# Changelog

All notable SubnetDesk changes are documented in this file.

## [2.0.0-rc10] - 2026-11-27

- 修复 Windows 交互宿主远程终端的令牌获取错误：终端直接通过 ConPTY 启动 Shell，继承 SubnetDesk 宿主身份与权限，移除强制获取 UAC 非提升令牌的流程。
- 修复 EasyTier 编辑方案时“延迟优先”开关缺少背景和圆角边框的问题，统一手机与 Windows 的表单样式，并适配深色主题。
- Windows 隐私屏由纯黑遮罩改为深色提示画面，显示正在远程控制和隐私模式说明；每块显示器分别居中，并按显示器缩放调整字号，保留采集排除和断线恢复机制。
- 统一 Windows 和 Android 应用版本为 `2.0.0-rc10`、Flutter build `79`，沿用 `rc10` 标签发布 Windows x64 便携包和 Android arm64 测试 APK。

## [2.0.0-rc8] - 2026-11-22

- 将组网页节点与路由整合为圆角设备卡片，支持手机单列、宽桌面双列、搜索和路径筛选。
- 按当前选路策略展示直连／中继、下一跳名称和延迟，区分备用直连与实际选中的中继路径。
- 路由详情随同一份设备快照更新，提供下一跳链路延迟、代理子网和累计统计，过期数据不再显示为已确认的当前路径。
- 统一 Rust、Flutter 和 Windows 程序显示版本为 `2.0.0-rc8`，Flutter 构建号递增至 77。

## [2.0.0-rc.6] - 2026-11-15

### Fixed

- Kept the Windows user-session capture agent alive through empty display enumeration and recreated invalid GDI capture contexts, allowing the existing connection to resume when the same desktop becomes capturable again.
- Enabled automatic headless virtual displays for portable/user-session launches and active monitor connections, with bounded retries and protection against repeatedly plugging invisible outputs.
- Preserved client connections while displays recover and disabled the obsolete OS-service session selector in the interactive runtime.

### Validation

- Added capture-loop, headless retry, and client display-recovery regression tests. Real Windows RDP and headless-device validation is still required; a virtual monitor does not reactivate a Windows-suspended RDP session.

## [2.0.0-rc.5] - 2026-11-14

### Changed

- Unified the desktop Nearby, Recent, Favorites, and All device views around one persistent layout, with scoped search, status filters, and 12-device pagination.
- Improved favorite groups with a searchable directory, cross-page selection, batch moves, count previews, and undo.
- Keyed saved devices, favorites, and group memberships by public-key fingerprint; addresses and names remain mutable attributes. Existing associations migrate only when a verified identity is available.

### Added

- Added manual editing of device IP addresses, hostnames, and ports, with undo and stale-update protection. mDNS automatic address updates remain enabled by default and can be disabled to pin an address.

### Fixed

- Refused connection candidates whose handshake fingerprint differs from the saved device, preventing a reassigned IP from redirecting a saved connection to another device.

## [2.0.0] - 2026-10-31

### Added

- Added the redesigned desktop and mobile LAN workspace, device artwork, local host controls, and remote session toolbar.
- Added a shared Flutter UI package for Windows 7 x64 with a pinned custom engine, portable ZIP, installer, and package compatibility checks.

### Changed

- Unified Rust, Flutter, portable launcher, CI, and native package versions at 2.0.0, with Flutter build number 76.
- Desktop sharing now runs in the signed-in user's application session, with native tray controls, login startup, and isolated portable user data.
- Updated the shared protocol and integrated the recent master, Windows startup, and Win7 Flutter branches.
- Paused the separate Linux DRM unattended package for 2.0.0 while the user-session runtime lacks its root service; regular Linux packages remain available.

### Fixed

- Removed false legacy-version restrictions on inherited screenshot, file clipboard, transfer resume, and remote input features by advertising the current 2.0.0 release in both connection directions.
- Included the relative mouse compatibility fix, Windows terminal text input fix, and recent Wayland/DRM and connection robustness improvements.
- Preserved the host runtime during initial Windows window cleanup and retained the pinned native dependencies during MSVC setup.

## [1.3.0] - 2026-08-27

### Added

- Added signed in-app updates for stable Windows and macOS builds, including startup checks, optional automatic downloads, progress reporting, release notes, and guarded installation.
- Added a persistent LAN password setup prompt so a new installation clearly shows what must be configured before remote access is ready.

### Changed

- Remote-session tabs now prefer a local alias, then the remote device name, and finally the peer address.
- Explicit Web remote-access listeners now support VPN addresses outside the default private ranges when their CIDR is allowed, include those addresses in generated certificates, and provide clearer rejection diagnostics.
- Android release builds now require a fixed signing identity and verify every APK against a pinned certificate fingerprint before publication.

### Fixed

- Fixed Windows remote desktop and camera stalls caused by texture lifecycle races during session changes.
- Fixed Windows portable builds unpacking into RustDesk's data directory instead of an isolated SubnetDesk directory.
- Fixed known LAN peers being rejected before the user could confirm an unexpected device-identity fingerprint.
- Hardened macOS update extraction with unique mount paths, symlink-safe cleanup, code-signature verification, and Gatekeeper validation.

[2.0.0-rc.6]: https://github.com/zibo-chen/SubnetDesk/compare/v2.0.0-rc.5...v2.0.0-rc.6
[2.0.0-rc.5]: https://github.com/zibo-chen/SubnetDesk/compare/v2.0.0-rc.4...v2.0.0-rc.5
[2.0.0]: https://github.com/zibo-chen/SubnetDesk/compare/v1.3.0...v2.0.0
[1.3.0]: https://github.com/zibo-chen/SubnetDesk/compare/v1.2.3...v1.3.0
