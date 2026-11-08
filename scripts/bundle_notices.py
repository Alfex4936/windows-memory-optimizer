"""Collect the locked Windows dependency licenses for the portable distribution."""
import json
from pathlib import Path
import subprocess
import sys


def main():
    root = Path(__file__).resolve().parent.parent
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1",
        "--filter-platform", "x86_64-pc-windows-msvc",
    ], cwd=root))
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [metadata["resolve"]["root"]]
    reachable = set()
    while pending:
        package = pending.pop()
        if package in reachable:
            continue
        reachable.add(package)
        pending.extend(dependency["pkg"] for dependency in nodes[package]["deps"])
    packages = sorted(
        (p for p in metadata["packages"] if p["id"] in reachable and p["source"]),
        key=lambda p: (p["name"], p["version"]),
    )
    assert packages, "No dependency licenses found"
    sections = [
        "MEMORI - THIRD-PARTY NOTICES\n\n"
        "Generated from Cargo.lock for x86_64-pc-windows-msvc.\n"
        "Some listed packages are build-time dependencies. Font licenses are included.\n"
    ]
    for package in packages:
        source = Path(package["manifest_path"]).parent
        licenses = sorted(p for p in source.rglob("*") if p.is_file() and (
            "license" in p.name.lower() or "copying" in p.name.lower()
            or "copyright" in p.name.lower() or "notice" in p.name.lower()
            or p.name.lower() in {"ofl.txt", "ufl.txt", "unlicense"}
            or (p.parent.name == "fonts" and p.suffix.lower() == ".txt")
        ) and p.suffix.lower() not in {".rs", ".py", ".png", ".ttf", ".woff", ".woff2"})
        sections.append("\n" + "=" * 72 + "\n"
            + f'{package["name"]} {package["version"]}\n'
            + f'License: {package.get("license") or "See source license"}\n'
            + f'https://crates.io/crates/{package["name"]}/{package["version"]}\n')
        for license_file in licenses:
            sections.append(f"\n--- {license_file.relative_to(source).as_posix()} ---\n"
                + license_file.read_text(encoding="utf-8", errors="replace") + "\n")
        if not licenses:
            sections.append("License identifier and source link supplied above.\n")
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    for name in ["COPYRIGHT.html", "COPYRIGHT.txt"]:
        copyright_file = sysroot / "share/doc/rust/html" / name
        if copyright_file.is_file():
            sections.append("\n" + "=" * 72 + "\nRust toolchain copyright notices\n"
                + copyright_file.read_text(encoding="utf-8", errors="replace"))
            break
    output = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "dist/Memori/THIRD_PARTY_NOTICES.txt"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("".join(sections), encoding="utf-8")
    print(f"Bundled notices for {len(packages)} locked dependency packages.")


if __name__ == "__main__":
    main()
