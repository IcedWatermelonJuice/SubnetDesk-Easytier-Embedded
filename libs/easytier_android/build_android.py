#!/usr/bin/env python3
"""有 Android NDK 后显式运行；Gradle 配置阶段不自动下载或构建。"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description="构建 SubnetDesk Android EasyTier JNI 库")
    parser.add_argument("--ndk-home", type=Path, required=True)
    parser.add_argument("--min-sdk", type=int, required=True,
                        help="填写原项目当前 Flutter SDK 的 minSdkVersion，不擅自更改")
    parser.add_argument("--abis", nargs="+", choices=["arm64-v8a", "armeabi-v7a", "x86_64", "x86"],
                        default=["arm64-v8a", "armeabi-v7a", "x86_64", "x86"])
    parser.add_argument("--toolchain", default="1.95")
    args = parser.parse_args()
    if args.min_sdk < 21:
        parser.error("Android NDK 的这些 ABI 至少要求 API 21")
    ndk = args.ndk_home.resolve(strict=True)
    if not (ndk / "source.properties").is_file():
        parser.error("--ndk-home 不是 Android NDK 根目录")
    cargo = shutil.which("cargo")
    if not cargo:
        parser.error("找不到 cargo，请先配置 Rust 与 cargo-ndk")
    crate = Path(__file__).resolve().parent
    output = crate.parent.parent / "flutter/android/app/src/main/jniLibs"
    environment = os.environ.copy()
    environment["ANDROID_NDK_HOME"] = str(ndk)
    command = [cargo, f"+{args.toolchain}", "ndk", "--platform", str(args.min_sdk), "-o", str(output)]
    for abi in args.abis:
        command.extend(["-t", abi])
    command.extend(["build", "--release"])
    if (crate / "Cargo.lock").exists():
        command.append("--locked")
    subprocess.run(command, cwd=crate, env=environment, check=True)
    for abi in args.abis:
        if not (output / abi / "libsubnetdesk_easytier_android.so").is_file():
            raise RuntimeError(f"构建未生成 {abi} 的 JNI 库")


if __name__ == "__main__":
    main()
