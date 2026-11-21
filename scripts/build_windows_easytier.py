#!/usr/bin/env python3
"""为现代 Windows 发行构建固定版本的无 Npcap EasyTier core。"""

import argparse
from hashlib import sha256
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
from tempfile import TemporaryDirectory
from zipfile import ZIP_DEFLATED, ZipFile

from stage_windows_easytier import COMMIT, WINDOWS_ASSETS, check_runtime_pe

PNET_VERSION = "0.35.0"
PNET_SHA256 = "e79e70ec0be163102a332e1d2d5586d362ad76b01cec86f830241f2b6452a7b7"
RUST = "1.95"
# 保留原有组网能力，移除需要 Windows 数据链路捕获的 FakeTCP。
FEATURES = "wireguard,websocket,smoltcp,tun,socks5,kcp,quic,magic-dns,zstd,mimalloc"


def replace_once(path, before, after):
    text = path.read_text(encoding="utf-8")
    if text.count(before) != 1:
        raise ValueError(f"固定源码与预期不符: {path}")
    path.write_text(text.replace(before, after, 1), encoding="utf-8")


def patch_sources(source, archive):
    if sha256(archive.read_bytes()).hexdigest() != PNET_SHA256:
        raise ValueError("pnet_datalink 归档与上游 Cargo.lock 中的摘要不符")
    vendor = source / "subnetdesk-vendor"
    vendor.mkdir()
    with tarfile.open(archive, "r:gz") as package:
        package.extractall(vendor, filter="data")
    crate = vendor / f"pnet_datalink-{PNET_VERSION}"
    replace_once(
        crate / "src/lib.rs",
        """mod bindings;

#[cfg(windows)]
#[path = "winpcap.rs"]
mod backend;

#[cfg(windows)]
pub mod winpcap;""",
        """#[cfg(not(windows))]
mod bindings;

#[cfg(windows)]
mod backend {
    use super::{Channel, Config, NetworkInterface};
    use std::io;

    pub(super) fn channel(_: &NetworkInterface, _: &Config) -> io::Result<Channel> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Windows datalink capture is unavailable"))
    }

    pub(super) fn interfaces() -> Vec<NetworkInterface> {
        Vec::new()
    }
}""",
    )
    manifest = source / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8")
    if "[patch.crates-io]" in text:
        raise ValueError("固定 EasyTier 工作区已经定义其他 crates.io 补丁")
    manifest.write_text(
        text + f'\n[patch.crates-io]\npnet_datalink = {{ path = "{crate.relative_to(source).as_posix()}" }}\n',
        encoding="utf-8",
    )
    locked = f'name = "pnet_datalink"\nversion = "{PNET_VERSION}"\n'
    replace_once(
        source / "Cargo.lock",
        locked + f'source = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "{PNET_SHA256}"\n',
        locked,
    )
    replace_once(
        source / "easytier/src/instance/virtual_nic.rs",
        """        let Some(peer_manager) = self.peer_mgr.upgrade() else {
            tracing::warn!("peer manager is dropped, skip Windows UDP broadcast relay");
            return;
        };
        match super::windows_udp_broadcast::start(peer_manager, virtual_ipv4) {
            Ok(handle) => {
                self.windows_udp_broadcast_relay = Some(handle);
                tracing::info!("Windows UDP broadcast relay started");
            }
            Err(err) => {
                tracing::warn!(
                    ?err,
                    "failed to start Windows UDP broadcast relay; administrator privileges are required"
                );
            }
        }
""",
        """        let _ = virtual_ipv4;
        tracing::warn!("UDP broadcast capture is unavailable in this Windows build");
""",
    )
    return crate


def build(upstream, archive, protoc, arch, output):
    if os.name != "nt" or not (upstream / ".git").exists() or not protoc.is_file():
        raise ValueError("需要 Windows、上游 EasyTier Git 仓库及本地 protoc.exe")
    if output.exists():
        raise ValueError("请选择不存在的输出目录")
    target = f"{arch}-pc-windows-msvc"
    target_machine = WINDOWS_ASSETS[arch][2]
    cargo_args = [
        "cargo", f"+{RUST}", "build", "--locked", "--release", "--target", target,
        "-p", "easytier", "--bin", "easytier-core", "--no-default-features", "--features", FEATURES,
    ]

    with TemporaryDirectory() as temporary:
        source = Path(temporary) / "easytier"
        target_dir = Path(temporary) / "target"
        subprocess.run(["git", "clone", "--shared", "--no-checkout", "--", str(upstream), str(source)], check=True)
        subprocess.run(["git", "checkout", "--detach", COMMIT], cwd=source, check=True)
        crate = patch_sources(source, archive)
        env = {**os.environ, "PROTOC": str(protoc), "CARGO_TARGET_DIR": str(target_dir)}
        subprocess.run(cargo_args, cwd=source, env=env, check=True)
        core = target_dir / target / "release" / "easytier-core.exe"
        check_runtime_pe(core, target_machine)

        # 提供实际构建的修改后源码。上游 third_party 目录只含非构建必需的二进制。
        tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=source).decode("utf-8").split("\0")
        output.mkdir(parents=True)
        with ZipFile(output / "easytier-source.zip", "x", ZIP_DEFLATED) as sources:
            for name in tracked:
                if name and not name.startswith("easytier/third_party/") and (source / name).is_file():
                    sources.write(source / name, name)
            for path in sorted(crate.rglob("*")):
                if path.is_file():
                    sources.write(path, path.relative_to(source).as_posix())
            sources.writestr(
                "SUBNETDESK-BUILD.txt",
                f"EasyTier upstream commit: {COMMIT}\n"
                f"pnet_datalink original SHA-256: {PNET_SHA256}\n"
                f"Build command: {' '.join(cargo_args)}\n",
            )
        shutil.copyfile(core, output / core.name)
        print(f"{core.name}: {sha256(core.read_bytes()).hexdigest()}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", required=True, type=Path)
    parser.add_argument("--pnet-datalink", required=True, type=Path)
    parser.add_argument("--protoc", required=True, type=Path)
    parser.add_argument("--arch", required=True, choices=WINDOWS_ASSETS)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    build(args.upstream.resolve(), args.pnet_datalink.resolve(), args.protoc.resolve(), args.arch, args.output.resolve())


if __name__ == "__main__":
    main()
