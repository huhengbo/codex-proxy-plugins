#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PLUGINS = ROOT / "plugins"
SDK_GIT = "https://github.com/zyycn/codex-proxy-rs.git"


def plugin_names() -> list[str]:
    if not PLUGINS.is_dir():
        return []
    return sorted(
        path.parent.name
        for path in PLUGINS.glob("*/plugin.json")
        if path.parent.is_dir()
    )


def load(name: str) -> tuple[dict, dict, Path]:
    plugin_dir = PLUGINS / name
    manifest_path = plugin_dir / "plugin.json"
    cargo_path = plugin_dir / "backend" / "Cargo.toml"
    if not manifest_path.is_file():
        raise ValueError(f"{name}: 缺少 plugin.json")
    if not cargo_path.is_file():
        raise ValueError(f"{name}: 缺少 backend/Cargo.toml")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    with cargo_path.open("rb") as handle:
        cargo = tomllib.load(handle)
    return manifest, cargo, plugin_dir


def sdk_dependency(cargo: dict) -> dict:
    dependency = cargo.get("dependencies", {}).get("gateway-plugin-sdk")
    if not isinstance(dependency, dict):
        raise ValueError("backend/Cargo.toml 必须使用表形式声明 gateway-plugin-sdk")
    return dependency


def metadata(name: str) -> dict[str, str]:
    manifest, cargo, plugin_dir = load(name)
    package = cargo.get("package", {})
    sdk = sdk_dependency(cargo)
    frontend = plugin_dir / "frontend"
    if (frontend / "package.json").is_file():
        frontend_mode = "pnpm"
    elif frontend.is_dir():
        frontend_mode = "static"
    else:
        frontend_mode = "none"
    resources = manifest.get("resources") or {}
    return {
        "name": name,
        "id": f"{manifest.get('publisher', '')}.{manifest.get('name', '')}",
        "version": str(manifest.get("version", "")),
        "package": str(package.get("name", "")),
        "sdk_rev": str(sdk.get("rev", "")),
        "frontend_mode": frontend_mode,
        "resource_mode": "web" if resources else "none",
        "engine": str((manifest.get("engines") or {}).get("codex-proxy-rs", "")),
        "description": str(manifest.get("description", "")),
    }


def validate_one(name: str) -> list[str]:
    errors: list[str] = []
    try:
        manifest, cargo, plugin_dir = load(name)
    except (ValueError, json.JSONDecodeError, tomllib.TOMLDecodeError) as exc:
        return [str(exc)]

    for field in ("manifestVersion", "publisher", "name", "version", "main", "runtime"):
        if field not in manifest:
            errors.append(f"{name}: plugin.json 缺少 {field}")
    if manifest.get("manifestVersion") != 1:
        errors.append(f"{name}: manifestVersion 必须为 1")
    if manifest.get("name") != name:
        errors.append(f"{name}: 目录名必须与 plugin.json.name 一致")
    if manifest.get("main") != "bin/plugin":
        errors.append(f"{name}: main 必须为 bin/plugin")
    if manifest.get("runtime") != "trustedProcess":
        errors.append(f"{name}: runtime 必须为 trustedProcess")

    identifier = re.compile(r"^[a-z0-9][a-z0-9._-]*$")
    for field in ("publisher", "name"):
        value = manifest.get(field)
        if not isinstance(value, str) or not identifier.fullmatch(value):
            errors.append(f"{name}: {field} 只能使用小写字母、数字、点、下划线和连字符")

    package = cargo.get("package", {})
    if str(package.get("version", "")) != str(manifest.get("version", "")):
        errors.append(f"{name}: Cargo package.version 必须与 plugin.json.version 一致")
    package_name = package.get("name")
    if not isinstance(package_name, str) or not package_name:
        errors.append(f"{name}: Cargo package.name 不能为空")

    try:
        sdk = sdk_dependency(cargo)
    except ValueError as exc:
        errors.append(f"{name}: {exc}")
    else:
        if sdk.get("git") != SDK_GIT:
            errors.append(f"{name}: gateway-plugin-sdk.git 必须固定为 {SDK_GIT}")
        rev = sdk.get("rev")
        if not isinstance(rev, str) or not re.fullmatch(r"[0-9a-f]{40}", rev):
            errors.append(f"{name}: gateway-plugin-sdk.rev 必须固定为 40 位 commit SHA")

    resources = manifest.get("resources") or {}
    if not isinstance(resources, dict):
        errors.append(f"{name}: resources 必须是对象")
    elif resources:
        if not (plugin_dir / "frontend").is_dir():
            errors.append(f"{name}: 声明 resources 时必须存在 frontend/")
        non_web = [path for path in resources if not path.startswith("web/")]
        if non_web:
            errors.append(f"{name}: 通用打包约定只支持 web/ 资源前缀")

    frontend_package = plugin_dir / "frontend" / "package.json"
    if frontend_package.is_file():
        try:
            package_json = json.loads(frontend_package.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            errors.append(f"{name}: frontend/package.json 无效：{exc}")
        else:
            manager = package_json.get("packageManager", "")
            if not isinstance(manager, str) or not manager.startswith("pnpm@"):
                errors.append(f"{name}: 前端 packageManager 必须固定 pnpm 版本")
            if not (plugin_dir / "frontend" / "pnpm-lock.yaml").is_file():
                errors.append(f"{name}: pnpm 前端必须提交 pnpm-lock.yaml")
    return errors


def validate_all() -> int:
    names = plugin_names()
    if not names:
        print("没有发现 plugins/*/plugin.json", file=sys.stderr)
        return 1
    errors: list[str] = []
    ids: dict[str, str] = {}
    sdk_revs: dict[str, list[str]] = {}
    for name in names:
        errors.extend(validate_one(name))
        try:
            meta = metadata(name)
        except Exception:
            continue
        plugin_id = meta["id"]
        sdk_revs.setdefault(meta["sdk_rev"], []).append(name)
        if plugin_id in ids:
            errors.append(f"{name}: 插件 ID {plugin_id} 与 {ids[plugin_id]} 重复")
        else:
            ids[plugin_id] = name
    if len(sdk_revs) > 1:
        details = ", ".join(f"{rev[:12]}: {'/'.join(names)}" for rev, names in sorted(sdk_revs.items()))
        errors.append(f"所有自定义插件必须使用同一 SDK commit；当前为 {details}")
    if errors:
        print("\n".join(f"- {error}" for error in errors), file=sys.stderr)
        return 1
    print(f"validated {len(names)} plugin(s): {', '.join(names)}")
    return 0



def sync_sdk(rev: str) -> int:
    if not re.fullmatch(r"[0-9a-f]{40}", rev):
        print("SDK commit 必须是 40 位小写十六进制 SHA", file=sys.stderr)
        return 1

    pattern = re.compile(
        r'(gateway-plugin-sdk\s*=\s*\{[^\n}]*\brev\s*=\s*")[0-9a-f]{40}(")'
    )
    changed: list[str] = []
    for name in plugin_names():
        cargo_path = PLUGINS / name / "backend" / "Cargo.toml"
        source = cargo_path.read_text(encoding="utf-8")
        updated, count = pattern.subn(rf'\g<1>{rev}\g<2>', source)
        if count != 1:
            print(
                f"{name}: gateway-plugin-sdk 必须在一行内且恰好包含一个 rev 字段",
                file=sys.stderr,
            )
            return 1
        if updated != source:
            cargo_path.write_text(updated, encoding="utf-8")
            changed.append(name)

    if changed:
        print(f"updated SDK to {rev}: {', '.join(changed)}")
    else:
        print(f"all plugins already use SDK {rev}")
    return 0


def release_notes() -> None:
    names = plugin_names()
    print("# Codex Proxy Plugins")
    print()
    print("本 Release 包含以下独立插件安装包。请选择与宿主平台匹配的 .tar.gz，并使用同名 .sha256 校验摘要。")
    print()
    print("| 插件 | 版本 | 宿主兼容范围 | 说明 |")
    print("| --- | --- | --- | --- |")
    for name in names:
        meta = metadata(name)
        desc = meta["description"].replace("|", "\\|")
        print(f"| {meta['id']} | {meta['version']} | {meta['engine']} | {desc} |")
    print()
    print("每个插件独立维护版本；Release tag 是仓库级 bundle 版本，不等同于任一插件版本。")


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("list")
    sub.add_parser("validate")
    meta = sub.add_parser("meta")
    meta.add_argument("plugin")
    meta.add_argument(
        "field",
        choices=["id", "version", "package", "sdk_rev", "frontend_mode", "resource_mode", "engine", "description"],
    )
    sub.add_parser("release-notes")
    sync = sub.add_parser("sync-sdk")
    sync.add_argument("rev")
    args = parser.parse_args()

    if args.command == "list":
        for name in plugin_names():
            print(name)
        return 0
    if args.command == "validate":
        return validate_all()
    if args.command == "meta":
        try:
            print(metadata(args.plugin)[args.field])
        except (ValueError, json.JSONDecodeError, tomllib.TOMLDecodeError) as exc:
            print(exc, file=sys.stderr)
            return 1
        return 0
    if args.command == "release-notes":
        release_notes()
        return 0
    if args.command == "sync-sdk":
        return sync_sdk(args.rev)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
