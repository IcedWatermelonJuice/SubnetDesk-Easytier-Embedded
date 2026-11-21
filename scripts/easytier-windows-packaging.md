# Windows EasyTier 发行资源

现代 Windows Flutter 构建在 `SubnetDesk.exe` 同级的 `easytier` 目录暂存 EasyTier v2.6.4 core、CLI、Wintun 和许可证。现有绿色 ZIP、单文件 EXE、MSI 及未签名制品会递归收集该目录。项目签名服务跳过其中的第三方文件，保留上游文件字节。此路径适用于 x64 和 ARM64。

官方 Windows x64 core 的 PE 导入表包含 `Packet.dll`。v2.6.4 在 Windows 的接口枚举失败时仍依赖 libpnet 的 Npcap 回退，关闭 FakeTCP 本身无法移除此依赖。随包 core 应使用固定上游提交的可再分发构建，其实际 PE 不得静态或延迟导入 Packet、Wpcap 或外置 WinDivert DLL。精简资源对应 TCP、UDP 和 QUIC。ARM64 上游包的 WinDivert 文件是占位文本。此配置不提供依赖物理网卡捕获的功能。[EasyTier v2.6.4](https://github.com/EasyTier/EasyTier/releases/tag/v2.6.4) · [固定版本回退实现](https://github.com/EasyTier/EasyTier/blob/v2.6.4/easytier/src/common/network.rs#L267-L290) · [Npcap OEM 条款](https://npcap.com/oem/)

准备好两个架构的 core 后，在仓库变量 `SUBNETDESK_EASYTIER_WINDOWS_CORES` 设置以下 JSON。每个 `source` 对应实际 core 的完整可获取源码及构建修改，`sha256` 填 core EXE 的实际小写摘要。

```json
{
  "x86_64": {"url": "https://example.com/x64/easytier-core.exe", "sha256": "<64位摘要>", "source": "https://example.com/x64/easytier-source.zip"},
  "aarch64": {"url": "https://example.com/arm64/easytier-core.exe", "sha256": "<64位摘要>", "source": "https://example.com/arm64/easytier-source.zip"}
}
```

变量为空时，现代 Windows 制品保留原有发布流程，EasyTier 资源暂不随包。启用后，任一架构的配置缺失、归档摘要不符、PE 架构错误或存在上述导入都会中止该打包任务。脚本在 CI 下载和校验资源，运行时由 SubnetDesk 使用同级文件。

## 构建可随包的 core

使用 Windows、独立 Rust 1.95 工具链、目标架构的 MSVC 环境与本地 `protoc.exe`。准备包括固定提交 `8428a89d2dabc94c97d370ec607c6ca142473626` 的 EasyTier Git 仓库以及 [pnet_datalink 0.35.0 crate](https://static.crates.io/crates/pnet_datalink/pnet_datalink-0.35.0.crate)。从 SubnetDesk 根目录运行下列命令，对每个目标指定不同的输出目录。

```powershell
python scripts/build_windows_easytier.py --upstream <EasyTier仓库> --pnet-datalink <pnet_datalink-0.35.0.crate> --protoc <protoc.exe> --arch x86_64 --output <新输出目录>
```

ARM64 使用 `--arch aarch64`。脚本在临时目录检出固定提交并核验 crate 摘要，保留 Windows 现有接口枚举，关闭物理数据链路捕获和 FakeTCP。它输出通过 PE 依赖检查的 core、摘要及改动后的源码归档。构建仍可能按上游锁文件获取依赖。该命令由发行者单独执行，Flutter 的 Rust 工具链和 CI 默认设置保持原样。填上面变量前需确认 core 可启动并保留基础组网。本次交付尚未运行构建。
