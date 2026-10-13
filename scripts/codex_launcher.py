"""Shared installation and verification for the stable Codex launcher."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile


SOURCE = Path(__file__).resolve().with_name("codex-launcher.py")
UPDATER = Path(__file__).resolve().with_name("update-codex-maeklog.py")


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def paths(home):
    home = home.resolve()
    return {"path": home / ".local/share/maeklog/codex-launcher.py",
            "codex": home / ".local/bin/codex",
            "binary_link": home / ".local/bin/codex-maeklog",
            "updater": home / ".local/bin/maeklog-update"}


def record(home):
    return {**{key: str(value) for key, value in paths(home).items()},
            "sha256": sha256(SOURCE), "updater_target": str(UPDATER)}


def package_binary(binary, home):
    binary = binary.resolve(strict=True)
    if (binary.name != "codex" or binary.parent.name != "bin"
            or binary.parent.parent.parent != paths(home)["path"].parent
            or not re.fullmatch(r"codex-\d+\.\d+\.\d+-maeklog\.[1-9]\d*", binary.parent.parent.name)
            or not binary.is_file() or not os.access(binary, os.X_OK)):
        raise RuntimeError(f"Invalid checked Codex package binary: {binary}")
    return binary


def verify(manifest, binary, home):
    """Verify the launcher separately from the checked package's binary hashes."""
    try:
        expected = record(home)
        installed = paths(home)
        return (manifest.get("launcher") == expected
                and installed["codex"].is_symlink()
                and installed["codex"].resolve(strict=True) == installed["path"]
                and not installed["path"].is_symlink()
                and installed["path"].is_file()
                and os.access(installed["path"], os.X_OK)
                and sha256(installed["path"]) == expected["sha256"]
                and installed["binary_link"].is_symlink()
                and installed["binary_link"].resolve(strict=True) == package_binary(binary, home)
                and installed["updater"].is_symlink()
                and installed["updater"].resolve(strict=True) == UPDATER
                and os.access(UPDATER, os.X_OK))
    except (OSError, RuntimeError, ValueError):
        return False


def atomic_copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix=destination.name + ".pm-", dir=destination.parent)
    os.close(descriptor)
    temporary = Path(name)
    try:
        shutil.copy2(source, temporary)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def atomic_bytes(path, content, mode=0o600):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix=path.name + ".pm-", dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            os.fchmod(stream.fileno(), mode)
            stream.write(content)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def atomic_link(target, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix=destination.name + ".pm-", dir=destination.parent)
    os.close(descriptor)
    temporary = Path(name)
    temporary.unlink()
    try:
        temporary.symlink_to(target)
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)


def updater_preflight(home):
    updater = paths(home)["updater"]
    if not UPDATER.is_file() or not os.access(UPDATER, os.X_OK):
        raise RuntimeError(f"Repository updater is not executable: {UPDATER}")
    if updater.exists() or updater.is_symlink():
        if not updater.is_symlink() or updater.resolve() != UPDATER:
            raise RuntimeError(f"Existing updater does not point to {UPDATER}: {updater}")


def install_files(home, binary):
    """Publish ready package and stable launcher pointers, one atomic replacement each."""
    installed = paths(home)
    if (not installed["path"].is_file() or installed["path"].is_symlink()
            or sha256(installed["path"]) != sha256(SOURCE)
            or not os.access(installed["path"], os.X_OK)):
        atomic_bytes(installed["path"], SOURCE.read_bytes(), mode=0o755)
    if not installed["updater"].is_symlink():
        atomic_link(UPDATER, installed["updater"])
    if not installed["binary_link"].is_symlink() or installed["binary_link"].resolve() != binary:
        atomic_link(binary, installed["binary_link"])
    if not installed["codex"].is_symlink() or installed["codex"].resolve() != installed["path"]:
        atomic_link(installed["path"], installed["codex"])


def snapshot(targets, backup, remove_if_content=None):
    snapshots = []
    for index, target in enumerate(targets):
        if target.is_symlink():
            snapshots.append({"path": str(target), "symlink": os.readlink(target)})
        elif target.is_file():
            saved = backup / str(index)
            shutil.copy2(target, saved)
            snapshots.append({"path": str(target), "backup": str(saved)})
        elif not target.exists():
            saved = {"path": str(target), "absent": True}
            if remove_if_content and target in remove_if_content:
                saved["remove_if_sha256"] = hashlib.sha256(remove_if_content[target]).hexdigest()
            snapshots.append(saved)
        else:
            raise RuntimeError(f"Expected a file or symlink: {target}")
    atomic_bytes(backup / "restore-manifest.json", (json.dumps(snapshots, indent=2) + "\n").encode())
    return snapshots


def restore(snapshots):
    for saved in reversed(snapshots):
        target = Path(saved["path"])
        if "symlink" in saved:
            atomic_link(saved["symlink"], target)
        elif "backup" in saved:
            atomic_copy(Path(saved["backup"]), target)
        else:
            # A hook can populate a newly initialized registry during install.
            # Preserve those entries even if a later launcher change fails.
            if "remove_if_sha256" not in saved:
                target.unlink(missing_ok=True)
            elif (not target.is_symlink() and target.is_file()
                  and sha256(target) == saved["remove_if_sha256"]):
                target.unlink(missing_ok=True)


def unchanged(snapshots):
    for saved in snapshots:
        target = Path(saved["path"])
        if "symlink" in saved:
            if not target.is_symlink() or os.readlink(target) != saved["symlink"]:
                return False
        elif "backup" in saved:
            original = Path(saved["backup"])
            if (target.is_symlink() or not target.is_file()
                    or target.stat().st_mode != original.stat().st_mode
                    or sha256(target) != sha256(original)):
                return False
        elif target.exists() or target.is_symlink():
            return False
    return True


def migrate(home, apply=False):
    """Activate the launcher for an existing checked package without rebuilding it."""
    installed = paths(home)
    if not installed["binary_link"].is_symlink():
        raise RuntimeError(f"Expected an existing checked package pointer: {installed['binary_link']}")
    binary = package_binary(installed["binary_link"], home)
    manifest_path = binary.parent.parent / "maeklog-install.json"
    old_manifest = manifest_path.read_bytes()
    manifest = json.loads(old_manifest)
    if not isinstance(manifest, dict):
        raise RuntimeError("Installed package manifest must be an object")
    version = manifest.get("codex_version")
    if (not isinstance(version, str) or not re.fullmatch(r"\d+\.\d+\.\d+", version)
            or not binary.parent.parent.name.startswith(f"codex-{version}-maeklog.")):
        raise RuntimeError("Installed package identity does not match its manifest")
    hashes = manifest.get("sha256", {})
    if (not isinstance(hashes, dict) or str(binary) not in hashes
            or any(not isinstance(path, str) or not isinstance(expected, str)
                   or not re.fullmatch(r"[0-9a-f]{64}", expected)
                   or sha256(path) != expected for path, expected in hashes.items())):
        raise RuntimeError("Installed package differs from the checked binary or hook hashes")
    if (not installed["codex"].is_symlink()
            or installed["codex"].resolve() not in [binary, installed["path"]]):
        raise RuntimeError("Existing codex entry point is neither the checked binary nor its stable launcher")
    updater_preflight(home)
    if installed["path"].exists() or installed["path"].is_symlink():
        old_record = manifest.get("launcher", {})
        if (installed["path"].is_symlink() or not installed["path"].is_file()
                or (sha256(installed["path"]) != sha256(SOURCE)
                    and (not isinstance(old_record, dict)
                         or old_record.get("path") != str(installed["path"])
                         or old_record.get("sha256") != sha256(installed["path"])))):
            raise RuntimeError(f"Existing launcher is not a verified Maeklog launcher: {installed['path']}")
    current = verify(manifest, binary, home)
    targets = [installed["codex"], installed["path"], installed["updater"], manifest_path]
    changes = []
    if not current:
        if installed["codex"].resolve() != installed["path"]:
            changes.append(str(installed["codex"]))
        if (not installed["path"].is_file() or sha256(installed["path"]) != sha256(SOURCE)
                or not os.access(installed["path"], os.X_OK)):
            changes.append(str(installed["path"]))
        if not installed["updater"].is_symlink():
            changes.append(str(installed["updater"]))
        changes.append(str(manifest_path))
    plan = {"package": str(binary.parent.parent), "codex_version": version,
            "launcher": record(home), "replace": changes,
            "already_current": current, "restart_processes": False}
    print(json.dumps(plan, indent=2), flush=True)
    if not apply or current:
        return plan
    backup_root = installed["path"].parent / "install-backups"
    backup_root.mkdir(parents=True, exist_ok=True, mode=0o700)
    backup = Path(tempfile.mkdtemp(prefix="codex-launcher-", dir=backup_root))
    saved = snapshot(targets, backup)
    if manifest_path.read_bytes() != old_manifest or not unchanged(saved):
        raise RuntimeError(f"Existing installation changed during launcher preparation; nothing changed; backup: {backup}")
    try:
        manifest["launcher"] = record(home)
        manifest["launcher_backup"] = str(backup)
        # Publish metadata before switching the user entry point. The package and
        # binary pointer remain intact throughout this launcher-only transition.
        if manifest_path.read_bytes() != old_manifest:
            raise RuntimeError("Installed manifest changed during launcher preparation")
        atomic_bytes(manifest_path, (json.dumps(manifest, indent=2) + "\n").encode())
        install_files(home, binary)
        if not verify(manifest, binary, home):
            raise RuntimeError("Installed launcher verification failed")
    except BaseException as error:
        try:
            restore(saved)
        except OSError as restore_error:
            raise RuntimeError(f"Launcher installation failed ({error}); restore failed ({restore_error}); "
                               f"backup: {backup}") from error
        raise RuntimeError(f"Launcher installation failed; original files restored; backup: {backup}: {error}") from error
    plan["backup"] = str(backup)
    print(json.dumps({"installed_launcher": record(home), "backup": str(backup)}, indent=2))
    return plan
