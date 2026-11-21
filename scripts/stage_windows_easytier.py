#!/usr/bin/env python3
"""为现代 Windows Flutter 发行目录准备 EasyTier v2.6.4 资源。"""

import argparse
from hashlib import sha256
from io import BytesIO
import json
import os
from pathlib import Path
import re
from tempfile import TemporaryDirectory
from urllib.parse import urlsplit
from urllib.request import urlopen
from zipfile import ZipFile

from audit_win7_pe import inspect

VERSION = "v2.6.4"
COMMIT = "8428a89d2dabc94c97d370ec607c6ca142473626"
UPSTREAM = "https://github.com/EasyTier/EasyTier"
RAW = f"https://raw.githubusercontent.com/EasyTier/EasyTier/{COMMIT}"
WINDOWS_ASSETS = {
    "x86_64": ("x86_64", "amd64", 0x8664, "27af91e270e554709b048bd32327fefd2dfce5062ae1e8701af7550c6f525f84"),
    "aarch64": ("arm64", "arm64", 0xAA64, "37023f8a3451c9234b17ee2089a03dc344ce90d803b5b359cb6c46682b0549b4"),
}
WINTUN_URL = "https://www.wintun.net/builds/wintun-0.14.1.zip"
WINTUN_SHA256 = "07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51"


def checked_url(url):
    parsed = urlsplit(url)
    if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password or parsed.query:
        raise ValueError("EasyTier 发行来源必须为不含凭据或查询参数的 HTTPS URL")
    return url


def download(url, digest=None):
    with urlopen(checked_url(url), timeout=60) as response:
        data = response.read()
    if digest is not None and sha256(data).hexdigest() != digest:
        raise ValueError("EasyTier 或 Wintun 发行资源 SHA-256 不符")
    return data


def check_runtime_pe(path, machine):
    report = inspect(path)
    if report["machine"] != machine:
        raise ValueError(f"{path.name} 的 PE 架构与 Windows 目标不符")
    unavailable = {"packet.dll", "wpcap.dll", "windivert.dll"} & report["imports"].keys()
    if unavailable:
        raise ValueError(f"{path.name} 依赖未随包分发的组件: {', '.join(sorted(unavailable))}")


def stage(runtime, arch, cores):
    if not (runtime / "SubnetDesk.exe").is_file():
        raise ValueError("Windows Flutter 发行目录缺少 SubnetDesk.exe")
    destination = runtime / "easytier"
    if destination.exists():
        raise ValueError("EasyTier 发行目录已存在，不能覆盖")
    for target in WINDOWS_ASSETS:
        entry = cores[target]
        checked_url(entry["url"])
        checked_url(entry["source"])
        if not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]):
            raise ValueError(f"{target} 的 core 需要小写 SHA-256")

    release_arch, wintun_arch, machine, official_sha = WINDOWS_ASSETS[arch]
    asset_directory = f"easytier-windows-{release_arch}"
    official_url = f"{UPSTREAM}/releases/download/{VERSION}/{asset_directory}-{VERSION}.zip"
    core = {key: cores[arch][key] for key in ("url", "sha256", "source")}

    with TemporaryDirectory(dir=runtime) as temporary:
        prepared = Path(temporary) / "easytier"
        prepared.mkdir()
        core_path = prepared / "easytier-core.exe"
        core_path.write_bytes(download(core["url"], core["sha256"]))
        check_runtime_pe(core_path, machine)
        with ZipFile(BytesIO(download(official_url, official_sha))) as upstream:
            (prepared / "easytier-cli.exe").write_bytes(upstream.read(f"{asset_directory}/easytier-cli.exe"))
        with ZipFile(BytesIO(download(WINTUN_URL, WINTUN_SHA256))) as wintun:
            (prepared / "wintun.dll").write_bytes(wintun.read(f"wintun/bin/{wintun_arch}/wintun.dll"))
            (prepared / "LICENSE-Wintun.txt").write_bytes(wintun.read("wintun/LICENSE.txt"))
        for name in ("easytier-cli.exe", "wintun.dll"):
            check_runtime_pe(prepared / name, machine)
        (prepared / "LICENSE-EasyTier.txt").write_bytes(download(f"{RAW}/LICENSE"))
        (prepared / "LICENSE-GPL-3.0.txt").write_bytes(download("https://www.gnu.org/licenses/gpl-3.0.txt"))
        source = {
            "version": VERSION, "upstream_commit": COMMIT, "architecture": arch,
            "upstream_source": f"{UPSTREAM}/tree/{COMMIT}",
            "core": core,
            "cli_archive": {"url": official_url, "sha256": official_sha},
            "wintun_archive": {"url": WINTUN_URL, "sha256": WINTUN_SHA256},
        }
        (prepared / "sources.json").write_text(json.dumps(source, indent=2) + "\n", encoding="utf-8")
        prepared.rename(destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--arch", required=True, choices=WINDOWS_ASSETS)
    args = parser.parse_args()
    stage(args.runtime, args.arch, json.loads(os.environ["SUBNETDESK_EASYTIER_WINDOWS_CORES"]))


if __name__ == "__main__":
    main()
