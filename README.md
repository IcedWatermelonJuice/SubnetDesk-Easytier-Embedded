<p align="center"><img src="res/subnetdesk-mark.png" alt="SubnetDesk" width="120"></p>

# SubnetDesk · EasyTier Embedded

Remote desktop with integrated EasyTier networking. This repository maintains our own SubnetDesk branch, built on [SubnetDesk](https://github.com/zibo-chen/SubnetDesk) and [RustDesk](https://github.com/rustdesk/rustdesk).

**Current release: 2.0.0-rc9 · build 78** · [简体中文](README.zh-CN.md) · [Download RC9](https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded/releases/tag/rc9) · [Report an issue](https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded/issues)

The default branch, `easytier-embedded`, is the shared source line for Windows and Android. Platform-specific code and toolchains remain separate; each release tag identifies the source used for both packages. Feature branches are integrated after platform checks.

## What this branch adds

- Integrated EasyTier profiles: create, edit, duplicate, import/export TOML, select and start a network from the app.
- EasyTier device cards with direct/relay paths, next-hop information and latency from the current network snapshot.
- Quick network startup and **EasyTier remote access only**, accepting current node virtual IPv4 addresses while retaining remote-access authentication and permissions.
- A **Prefer lower latency** profile switch (`latency_first`). Save and restart the running network to apply changes.
- Desktop configuration backup/restore and access-credential management, subject to platform support.
- Android networking through `VpnService`, plus a remote-view rotation menu: 0°, 90°, 180° and 270°.

SubnetDesk connects directly to a reachable host IP/hostname; its default remote-access TCP port is **21118**. It does not require a RustDesk rendezvous server. EasyTier provides the private network and may use its own peers/relays; direct connectivity is not guaranteed.

## RC9 downloads

| Platform | RC9 package | Notes |
| --- | --- | --- |
| Windows x64 | Portable ZIP | Unsigned test build. Extract the complete archive and start the top-level `SubnetDesk.exe`. |
| Android arm64-v8a | APK | Android 7.0/API 24 or later. Test-signed with the same certificate as the previous Android test APK. |

Download packages and `SHA256SUMS` from the [RC9 release](https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded/releases/tag/rc9). RC9 is a pre-release. This release does not provide macOS, Linux, Windows ARM64 or other Android ABI packages; inherited source support does not imply that these variants have been built or tested here.

For Windows upgrades, exit the app from the tray, replace the program files and preserve `userdata/`. For Android, install the APK over the previous test build; VPN permission and any screen-sharing/input permissions are separate.

## Quick start

1. Install SubnetDesk on both devices. Configure the controlled device's access username/password and enable incoming remote access. Complete the permissions required by its operating system.
2. Open **Network** and create or import compatible EasyTier profiles on both devices. Configure your own network identity and peer addresses; never share the network secret publicly.
3. Start the profiles. On Android, approve the system VPN request. Check that both devices have virtual IP addresses and can reach one another.
4. Connect to the controlled device's EasyTier IP, for example `10.88.0.2:21118`. Verify its device fingerprint, then authenticate with its access credentials.
5. Optionally enable **EasyTier remote access only** on the controlled device. The option restricts incoming sources; it does not replace the remote-access toggle, authentication or permissions.

Android can run only one system VPN at a time. Host remote-control capabilities also depend on Android's screen-sharing and input permissions. Network discovery and remote access are separate controls.

## Configuration behavior

Both Windows and Android force these values when preparing/saving profiles and starting a network, including old stored profiles:

```toml
[flags]
disable_quic_input = true
disable_kcp_input = true
enable_quic_proxy = false
enable_kcp_proxy = false
```

QUIC/KCP TCP acceleration proxies recreate the final TCP connection locally, hiding the original node IP from our current source allowlist. These overrides disable that proxy path. **Ordinary QUIC tunnels remain available.** The overrides apply even when EasyTier-only access is off; `latency_first` remains user-controlled.

Android additionally forces `proxy_forward_by_system = false`, `no_tun = false` and `enable_udp_broadcast_relay = false`; runtime `bind_device` is also false. `accept_dns = true` is rejected rather than silently changed. Normal profiles do not capture the default route automatically; an exit route must be explicitly configured and advertised.

The EasyTier-only source policy accepts current node virtual IPv4 addresses, not entire published proxy subnets. Failed/stale snapshots reject new incoming connections; stopping the network intentionally turns off the restriction. See [access controls](docs/design/easytier-embedded/access-controls.md) and [Android support](docs/design/easytier-embedded/android-support.md).

## Build from source

```powershell
git clone --recurse-submodules --branch easytier-embedded https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded.git
cd SubnetDesk-Easytier-Embedded
```

The RC9 builds use Flutter 3.44.1, Rust 1.82 for the main application and Rust 1.95 for the independent Android EasyTier library. Android uses JDK 17, NDK r28c, cargo-ndk 3.1.2, min SDK 24 and compile/target SDK 36. Keep the Flutter SDK expressions in the project; do not raise the minimum SDK just to build the networking library.

After installing platform native dependencies and generating the Flutter/Rust bridge files required by the project, the Windows build entry is:

```powershell
python build.py --portable --flutter --skip-portable-pack --hwcodec
```

Android requires the main remote-control library, the separate EasyTier JNI library and the NDK C++ runtime before packaging the APK. See [the Android library guide](libs/easytier_android/README.md), [Android implementation notes](docs/design/easytier-embedded/android-support.md), and the existing [build workflow](.github/workflows/flutter-build.yml) / [bridge workflow](.github/workflows/bridge.yml) for dependency and generation steps. Building only the networking library does not produce a complete app.

## Validation and limits

RC9 passed targeted configuration/UI tests and full Windows/Android Release builds. Windows ZIP integrity, file/architecture checks, Android signature verification, native dependency completeness and 16 KiB alignment were checked. A complete real-device remote-control/VPN acceptance pass has not been performed for this build.

Report reproducible issues in [this repository](https://github.com/IcedWatermelonJuice/SubnetDesk-Easytier-Embedded/issues). Include platform, app version and redacted logs; exclude passwords, private keys and EasyTier network secrets. See [RC9 release notes](docs/releases/2.0.0-rc9.md).

## Credits and license

Thanks to [SubnetDesk](https://github.com/zibo-chen/SubnetDesk), [RustDesk](https://github.com/rustdesk/rustdesk) and [EasyTier](https://github.com/EasyTier/EasyTier). This is an independently maintained branch, not an official release of those upstream projects.

SubnetDesk source retains the [GNU Affero General Public License v3.0](LICENCE). Bundled EasyTier, Wintun and other components retain their own licenses and notices. Windows packages include the corresponding EasyTier core source archive and license notices. Use remote control only on devices you are authorized to administer.
