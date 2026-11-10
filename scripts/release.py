#!/usr/bin/env python3
"""Validate version tags and package release archives (Python 3.11+, stdlib only)."""
import argparse
import hashlib
from pathlib import Path
import re
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TARGETS = (
    "x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin", "aarch64-apple-darwin",
)


def version():
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def validate(tag):
    expected = "v" + version()
    if not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", tag):
        raise ValueError("release tags must be version tags such as v0.1.0")
    if tag != expected:
        raise ValueError(f"tag {tag} does not match Cargo.toml ({expected})")
    return tag


def package(target, binary, output):
    if target not in TARGETS:
        raise ValueError(f"unsupported release target: {target}")
    binary = Path(binary)
    if not binary.is_file():
        raise ValueError(f"missing release executable: {binary}")
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    name = f"saumux-v{version()}-{target}"
    archive = output / (name + ".tar.gz")
    files = [(binary, "saumux"), (ROOT / "README.md", "README.md"),
             (ROOT / "LICENSE", "LICENSE"),
             (ROOT / "docs/implementation.md", "docs/implementation.md"),
             (ROOT / "terminfo/saumux.terminfo", "terminfo/saumux.terminfo")]
    with tarfile.open(archive, "w:gz") as tar:
        for source, destination in files:
            info = tar.gettarinfo(str(source), f"{name}/{destination}")
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mode = 0o755 if destination == "saumux" else 0o644
            with source.open("rb") as stream:
                tar.addfile(info, stream)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("validate")
    check.add_argument("tag")
    pack = commands.add_parser("package")
    pack.add_argument("--target", choices=TARGETS, required=True)
    pack.add_argument("--binary", required=True)
    pack.add_argument("--output", default="dist")
    args = parser.parse_args()
    try:
        if args.command == "validate":
            print(validate(args.tag))
        else:
            print(package(args.target, args.binary, args.output))
    except (ValueError, OSError) as error:
        parser.exit(1, f"release: {error}\n")


if __name__ == "__main__":
    main()
