#!/usr/bin/env python3
"""Exercise install preflight, launcher upgrades and backups in a fake home."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch


INSTALLER = Path(__file__).with_name("install-client-routing.py")
spec = importlib.util.spec_from_file_location("routing_installer", INSTALLER)
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class InstallationFixture:
    def __init__(self, home):
        self.home = home
        self.codex_home = home / ".codex"
        self.plugin = home / ".config/herdr/plugins/config/maeklog"
        self.state = home / "state"
        self.resources = home / "resources"
        self.launchers = home / ".local/bin"
        for path in [self.codex_home, self.plugin / "bin", self.state,
                     self.resources / "bin", self.resources / "codex-resources",
                     self.resources / "codex-path", self.launchers]:
            path.mkdir(parents=True, exist_ok=True)
        (self.plugin / "hook-runtime.json").write_text(json.dumps({"state_dir": str(self.state)}))
        (self.codex_home / "hooks.json").write_text(json.dumps({"hooks": {
            "SessionStart": [{"hooks": [{"type": "command", "command": "/fixture/herdr-agent-state.sh"}]}],
            "UserPromptSubmit": [{"hooks": [{"command": "/fixture/collect.sh"}]}],
        }}))
        self.hook_id = f"{self.codex_home}/hooks.json:session_start:0:0"
        self.config = (f'model = "preserve-me"\n[hooks.state.{json.dumps(self.hook_id)}]\n'
                       'enabled = false\ntrusted_hash = "unchanged"\n')
        self.config_path = self.codex_home / "config.toml"
        self.config_path.write_text(self.config)
        self.pm_hook = self.plugin / "bin/maeklog-hook"
        self.pm_hook.write_text("original PM binary")
        self.registry = self.state / "client-routing.json"
        self.registry_content = '{"clients":[],"turns":{"preserve":"existing"}}\n'
        self.registry.write_text(self.registry_content)
        self.official = home / "official"
        self.official.write_text("official package stays intact")
        (self.resources / "bin/codex").write_text("official CLI must not overwrite patched CLI")
        (self.resources / "bin/codex-code-mode-host").write_text("matching official helper")
        (self.resources / "codex-resources/preserve").write_text("matching model resources")
        (self.resources / "codex-path/preserve").write_text("matching path resources")
        (self.launchers / "codex").symlink_to(self.official)
        (self.launchers / "codex-maeklog").symlink_to(home / "old-local-package")
        self.codex = home / "candidate codex"
        self.pm = home / "candidate pm"
        self.set_candidate("0.159.2")
        self.write_version(self.pm, "maeklog 0.14.0")
        self.env = {**os.environ, "HOME": str(home), "CODEX_HOME": str(self.codex_home),
                    "XDG_CONFIG_HOME": str(home / ".config")}

    @staticmethod
    def write_version(path, version):
        path.write_text(f'#!/bin/sh\necho "{version}"\n')
        path.chmod(0o700)

    def set_candidate(self, version):
        self.version = version
        self.write_version(self.codex, f"codex-cli {version}")
        (self.resources / "codex-package.json").write_text(json.dumps({"version": version}))

    def package(self, revision=1):
        return self.home / f".local/share/maeklog/codex-{self.version}-maeklog.{revision}"

    def command(self, *extra):
        return [str(INSTALLER), "--codex-binary", str(self.codex),
                "--pm-binary", str(self.pm), "--resource-package", str(self.resources),
                "--codex-version", self.version, "--base-commit", "a" * 40, *extra]

    def run(self, *extra):
        return subprocess.run([sys.executable, *self.command(*extra)], env=self.env,
                              capture_output=True, text=True)

    def run_migration(self, *extra):
        return subprocess.run([sys.executable, str(INSTALLER), "--launcher-only", *extra],
                              env=self.env, capture_output=True, text=True)

    def make_legacy_installation(self):
        lock = self.home / ".cache/maeklog/codex-update/update.lock"
        lock.parent.mkdir(parents=True)
        lock.touch()
        package = self.package()
        shutil.copytree(self.resources, package)
        shutil.copy2(self.codex, package / "bin/codex")
        shutil.copy2(self.pm, package / "maeklog-hook")
        hashes = {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in [package / "bin/codex", package / "maeklog-hook", self.pm_hook,
                               package / "bin/codex-code-mode-host"]}
        manifest = {"codex_version": self.version, "base_commit": "a" * 40,
                    "local_revision": 1, "sha256": hashes,
                    "bridge_patch_sha256": "b" * 64, "routing_registry": str(self.registry),
                    "backup": "original backup reference"}
        (package / "maeklog-install.json").write_text(json.dumps(manifest))
        for name in ["codex", "codex-maeklog"]:
            (self.launchers / name).unlink()
            (self.launchers / name).symlink_to(package / "bin/codex")
        return manifest

    def snapshot(self):
        entries = {}
        for path in sorted(self.home.rglob("*")):
            relative = str(path.relative_to(self.home))
            if path.is_symlink():
                entries[relative] = ("link", os.readlink(path))
            elif path.is_file():
                entries[relative] = ("file", path.read_bytes(), path.stat().st_mode)
            else:
                entries[relative] = ("directory",)
        return entries


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="pm-install-test-")
        self.addCleanup(self.directory.cleanup)
        self.fixture = InstallationFixture(Path(self.directory.name) / "home with spaces")

    def assertSuccess(self, result):
        self.assertEqual(result.returncode, 0, result.stderr or result.stdout)

    def assertGatePreservesHome(self, expected):
        fixture = self.fixture
        before = fixture.snapshot()
        result = fixture.run("--apply")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(expected, result.stderr)
        self.assertEqual(fixture.snapshot(), before)

    def assertExistingEntriesPreserved(self, before):
        after = self.fixture.snapshot()
        for path, value in before.items():
            with self.subTest(path=path):
                self.assertEqual(after.get(path), value)

    def test_dry_run_preserves_every_existing_file(self):
        fixture = self.fixture
        before = fixture.snapshot()
        result = fixture.run()
        self.assertSuccess(result)
        self.assertFalse(json.loads(result.stdout)["restart_processes"])
        self.assertEqual(fixture.snapshot(), before)
        self.assertFalse(fixture.package().exists())

    def test_version_gates_preserve_the_active_installation(self):
        fixture = self.fixture
        fixture.write_version(fixture.codex, "codex-cli 0.159.3")
        self.assertGatePreservesHome("Unexpected binary version")
        fixture.write_version(fixture.codex, "codex-cli 0.159.2")
        fixture.write_version(fixture.pm, "unrelated-cli 0.14.0")
        self.assertGatePreservesHome("Unexpected binary version")
        fixture.write_version(fixture.pm, "maeklog 0.14.0")
        (fixture.resources / "codex-package.json").write_text('{"version":"0.157.1"}')
        self.assertGatePreservesHome("Resource package must match")

    def test_missing_resources_preserve_the_active_installation(self):
        (self.fixture.resources / "codex-package.json").unlink()
        self.assertGatePreservesHome("Missing package resource")

    def test_install_packages_checked_binary_helpers_and_backups(self):
        fixture = self.fixture
        self.assertSuccess(fixture.run("--apply"))
        package = fixture.package()
        self.assertEqual((fixture.launchers / "codex-maeklog").resolve(), package / "bin/codex")
        launcher = fixture.home / ".local/share/maeklog/codex-launcher.py"
        self.assertEqual((fixture.launchers / "codex").resolve(), launcher)
        self.assertEqual((package / "bin/codex").read_bytes(), fixture.codex.read_bytes())
        self.assertEqual((package / "bin/codex-code-mode-host").read_text(), "matching official helper")
        self.assertEqual((package / "codex-resources/preserve").read_text(), "matching model resources")
        self.assertEqual((package / "codex-path/preserve").read_text(), "matching path resources")
        self.assertEqual(fixture.config_path.read_text(), fixture.config)
        self.assertEqual(fixture.registry.read_text(), fixture.registry_content)
        self.assertEqual(fixture.official.read_text(), "official package stays intact")
        installed = json.loads((package / "maeklog-install.json").read_text())
        backups = {entry["path"]: entry for entry in json.loads(
            (Path(installed["backup"]) / "restore-manifest.json").read_text())}
        self.assertEqual(backups[str(fixture.launchers / "codex")]["symlink"], str(fixture.official))
        self.assertEqual(backups[str(fixture.launchers / "codex-maeklog")]["symlink"],
                         str(fixture.home / "old-local-package"))
        self.assertEqual(Path(backups[str(fixture.pm_hook)]["backup"]).read_text(), "original PM binary")
        self.assertEqual(installed["base_commit"], "a" * 40)
        self.assertEqual(fixture.pm_hook.read_bytes(), fixture.pm.read_bytes())
        self.assertEqual(installed["launcher"]["path"], str(launcher))
        self.assertIn(str(package / "bin/codex"), installed["sha256"])
        self.assertNotIn(str(launcher), installed["sha256"])

    def test_next_install_preserves_launcher_and_switches_the_checked_binary(self):
        fixture = self.fixture
        self.assertSuccess(fixture.run("--apply"))
        launcher = (fixture.launchers / "codex").resolve()
        original_launcher = launcher.read_bytes()
        original_package = fixture.package()
        original_binary = (original_package / "bin/codex").read_bytes()
        fixture.set_candidate("0.159.3")
        self.assertSuccess(fixture.run("--apply"))
        self.assertEqual((fixture.launchers / "codex").resolve(), launcher)
        self.assertEqual(launcher.read_bytes(), original_launcher)
        self.assertEqual((fixture.launchers / "codex-maeklog").resolve(), fixture.package() / "bin/codex")
        result = subprocess.run([str(fixture.launchers / "codex"), "--version"], env=fixture.env,
                                capture_output=True, text=True)
        self.assertSuccess(result)
        self.assertEqual(result.stdout.strip(), "codex-cli 0.159.3")
        self.assertEqual((original_package / "bin/codex").read_bytes(), original_binary)
        self.assertEqual(fixture.config_path.read_text(), fixture.config)
        self.assertEqual(fixture.registry.read_text(), fixture.registry_content)

    def test_duplicate_installation_does_not_replace_live_files(self):
        fixture = self.fixture
        self.assertSuccess(fixture.run("--apply"))
        self.assertGatePreservesHome("already exists")

    def test_similar_names_wrong_types_and_shell_chains_do_not_disable_hooks(self):
        fixture = self.fixture
        hook_file = fixture.codex_home / "hooks.json"
        original = hook_file.read_text()
        for handler in [
            {"type": "command", "command": "/fixture/herdr-agent-state.sh.backup"},
            {"type": "command", "command": "echo /fixture/herdr-agent-state.sh"},
            {"type": "prompt", "command": "/fixture/herdr-agent-state.sh"},
            {"type": "command", "command": "/fixture/herdr-agent-state.sh; /fixture/other.sh"},
            {"type": "command", "command": "/fixture/herdr-agent-state.sh $(other)"},
            {"type": "command", "command": "/fixture/herdr-agent-state.sh\nprintf unrelated"},
            {"type": "command", "command": "/fixture/herdr-agent-state.sh\rprintf unrelated"},
            {"type": "command", "command": "bash -c '/fixture/herdr-agent-state.sh'"},
            {"type": "command", "command": "bash '/fixture/herdr-agent-state.sh.backup' session"},
            {"type": "command", "command": "bash '/fixture/herdr-agent-state.sh' session; other"},
            {"type": "command", "command": "bash '/fixture/herdr-agent-state.sh' $(other)"},
            {"type": "command", "command": "bash '/fixture/herdr-agent-state.sh'\nprintf other"},
        ]:
            with self.subTest(handler=handler):
                data = json.loads(original)
                data["hooks"]["SessionStart"][0]["hooks"][0] = handler
                hook_file.write_text(json.dumps(data))
                if installer.ambiguous_herdr_session_handler(handler):
                    self.assertGatePreservesHome("Ambiguous Herdr SessionStart")
                else:
                    before = fixture.snapshot()
                    self.assertSuccess(fixture.run())
                    self.assertEqual(fixture.snapshot(), before)
                    self.assertEqual(fixture.config_path.read_text(), fixture.config)
        hook_file.write_text(original)

    def test_fresh_home_without_herdr_hook_or_local_codex_installs_without_changing_preferences(self):
        fixture = self.fixture
        (fixture.codex_home / "hooks.json").write_text('{"hooks":{}}')
        fixture.config_path.write_text('model = "preserve-me"\n[features]\nhooks = true\n')
        fixture.registry.unlink()
        for name in ["codex", "codex-maeklog"]:
            (fixture.launchers / name).unlink()
        preference = fixture.plugin / "model-settings.json"
        preference.write_bytes(b'{"model":"preserve-me"}\n')
        record = fixture.state / "records.json"
        record.write_bytes(b'{"synthetic":"preserve-me"}\n')
        # An official npm-installed CLI elsewhere on PATH is never touched.
        fixture.official.chmod(0o700)
        fixture.env["PATH"] = str(fixture.official.parent) + os.pathsep + fixture.env.get("PATH", "")
        config = fixture.config_path.read_bytes()
        definitions = (fixture.codex_home / "hooks.json").read_bytes()
        before = fixture.snapshot()
        self.assertSuccess(fixture.run())
        self.assertEqual(fixture.snapshot(), before)
        self.assertSuccess(fixture.run("--apply"))
        self.assertEqual(fixture.config_path.read_bytes(), config)
        self.assertEqual((fixture.codex_home / "hooks.json").read_bytes(), definitions)
        self.assertEqual(preference.read_bytes(), b'{"model":"preserve-me"}\n')
        self.assertEqual(record.read_bytes(), b'{"synthetic":"preserve-me"}\n')
        self.assertEqual(fixture.official.read_text(), "official package stays intact")
        manifest = json.loads((fixture.package() / "maeklog-install.json").read_text())
        snapshots = json.loads((Path(manifest["backup"]) / "restore-manifest.json").read_text())
        entry = next(saved for saved in snapshots if saved["path"] == str(fixture.launchers / "codex"))
        self.assertTrue(entry["absent"])
        self.assertEqual((fixture.launchers / "codex-maeklog").resolve(), fixture.package() / "bin/codex")

    def test_first_install_can_leave_a_missing_codex_config_absent(self):
        fixture = self.fixture
        fixture.config_path.unlink()
        (fixture.codex_home / "hooks.json").write_text('{"hooks":{}}')
        self.assertSuccess(fixture.run("--apply"))
        self.assertFalse(fixture.config_path.exists())

    def test_unreviewed_generated_herdr_hook_is_disabled_without_creating_trust(self):
        fixture = self.fixture
        fixture.config_path.write_text('model = "preserve-me"\n')
        self.assertSuccess(fixture.run("--apply"))
        parsed = tomllib.loads(fixture.config_path.read_text())
        self.assertEqual(parsed, {"model": "preserve-me", "hooks": {"state": {fixture.hook_id: {"enabled": False}}}})

    def test_multiple_generated_herdr_hooks_stop_without_mutation(self):
        fixture = self.fixture
        hook_file = fixture.codex_home / "hooks.json"
        data = json.loads(hook_file.read_text())
        data["hooks"]["SessionStart"].append(data["hooks"]["SessionStart"][0])
        hook_file.write_text(json.dumps(data))
        self.assertGatePreservesHome("Multiple Herdr SessionStart")

    def test_quoted_herdr_handler_preserves_unrelated_similar_names(self):
        fixture = self.fixture
        hook_file = fixture.codex_home / "hooks.json"
        data = json.loads(hook_file.read_text())
        data["hooks"]["SessionStart"][0]["hooks"] = [
            {"type": "command", "command": "'/fixture with spaces/herdr-agent-state.sh'"},
            {"type": "command", "command": "/fixture/herdr-agent-state.sh.backup"},
        ]
        hook_file.write_text(json.dumps(data))
        fixture.config_path.write_text(fixture.config.replace("enabled = false", "enabled = true"))
        self.assertSuccess(fixture.run("--apply"))
        self.assertEqual(fixture.config_path.read_text(), fixture.config)
        self.assertEqual(json.loads(hook_file.read_text()), data)

    def test_actual_generated_bash_herdr_handler_is_recognized(self):
        fixture = self.fixture
        hook_file = fixture.codex_home / "hooks.json"
        data = json.loads(hook_file.read_text())
        data["hooks"]["SessionStart"][0]["hooks"][0]["command"] = "bash '/fixture/herdr-agent-state.sh' session"
        hook_file.write_text(json.dumps(data))
        fixture.config_path.write_text(fixture.config.replace("enabled = false", "enabled = true"))
        self.assertSuccess(fixture.run("--apply"))
        self.assertEqual(fixture.config_path.read_text(), fixture.config)
        self.assertEqual(json.loads(hook_file.read_text()), data)

    def test_quoted_bash_and_script_preserve_unrelated_similar_names(self):
        fixture = self.fixture
        hook_file = fixture.codex_home / "hooks.json"
        data = json.loads(hook_file.read_text())
        data["hooks"]["SessionStart"][0]["hooks"] = [
            {"type": "command", "command": "'/bin/bash' '/fixture with spaces/herdr-agent-state.sh' session"},
            {"type": "command", "command": "bash /fixture/herdr-agent-state.sh.backup session"},
        ]
        hook_file.write_text(json.dumps(data))
        fixture.config_path.write_text(fixture.config.replace("enabled = false", "enabled = true"))
        self.assertSuccess(fixture.run("--apply"))
        self.assertEqual(fixture.config_path.read_text(), fixture.config)
        self.assertEqual(json.loads(hook_file.read_text()), data)

    def test_only_existing_session_start_enablement_changes(self):
        fixture = self.fixture
        fixture.config_path.write_text(fixture.config.replace("enabled = false", "enabled = true") +
                                       '\n[hooks.state.other]\nenabled = true\ntrusted_hash = "other"\n')
        hooks = (fixture.codex_home / "hooks.json").read_bytes()
        expected = fixture.config + '\n[hooks.state.other]\nenabled = true\ntrusted_hash = "other"\n'
        self.assertSuccess(fixture.run("--apply"))
        self.assertEqual(fixture.config_path.read_text(), expected)
        self.assertEqual((fixture.codex_home / "hooks.json").read_bytes(), hooks)

    def test_missing_registry_is_initialized_before_activation(self):
        fixture = self.fixture
        fixture.registry.unlink()
        self.assertSuccess(fixture.run("--apply"))
        self.assertEqual(json.loads(fixture.registry.read_text()), {"clients": [], "turns": {}})
        self.assertEqual(fixture.registry.stat().st_mode & 0o777, 0o600)

    def test_migration_preserves_binary_hashes_config_hook_and_registry(self):
        fixture = self.fixture
        legacy = fixture.make_legacy_installation()
        unchanged_paths = [fixture.package() / "bin/codex", fixture.pm_hook,
                           fixture.config_path, fixture.registry,
                           fixture.codex_home / "hooks.json"]
        before = {path: path.read_bytes() for path in unchanged_paths}
        snapshot = fixture.snapshot()
        self.assertSuccess(fixture.run_migration())
        self.assertEqual(fixture.snapshot(), snapshot)
        self.assertSuccess(fixture.run_migration("--apply"))
        launcher = fixture.home / ".local/share/maeklog/codex-launcher.py"
        self.assertEqual((fixture.launchers / "codex").resolve(), launcher)
        self.assertEqual((fixture.launchers / "codex-maeklog").resolve(), fixture.package() / "bin/codex")
        self.assertEqual((fixture.launchers / "maeklog-update").resolve(),
                         INSTALLER.with_name("update-codex-maeklog.py").resolve())
        self.assertEqual({path: path.read_bytes() for path in unchanged_paths}, before)
        migrated = json.loads((fixture.package() / "maeklog-install.json").read_text())
        for key, value in legacy.items():
            self.assertEqual(migrated[key], value)
        self.assertEqual(migrated["launcher"]["path"], str(launcher))
        self.assertEqual(migrated["launcher"]["sha256"], hashlib.sha256(launcher.read_bytes()).hexdigest())
        self.assertNotIn(str(launcher), migrated["sha256"])
        applied = fixture.snapshot()
        self.assertSuccess(fixture.run_migration("--apply"))
        self.assertEqual(fixture.snapshot(), applied)

    def test_migration_does_not_depend_on_hook_configuration(self):
        fixture = self.fixture
        fixture.make_legacy_installation()
        fixture.config_path.write_text("intentionally invalid TOML = [")
        (fixture.codex_home / "hooks.json").unlink()
        before = fixture.config_path.read_bytes()
        self.assertSuccess(fixture.run_migration("--apply"))
        self.assertEqual(fixture.config_path.read_bytes(), before)
        self.assertFalse((fixture.codex_home / "hooks.json").exists())

    def test_migration_rejects_changed_checked_bytes_or_mismatched_links(self):
        fixture = self.fixture
        fixture.make_legacy_installation()
        binary = fixture.package() / "bin/codex"
        checked = binary.read_bytes()
        binary.write_bytes(checked + b"\n# tampered\n")
        before = fixture.snapshot()
        result = fixture.run_migration("--apply")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(fixture.snapshot(), before)
        binary.write_bytes(checked)
        (fixture.launchers / "codex").unlink()
        (fixture.launchers / "codex").symlink_to(fixture.official)
        before = fixture.snapshot()
        result = fixture.run_migration("--apply")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(fixture.snapshot(), before)

    def test_migration_rejects_an_unrelated_updater_without_overwriting_it(self):
        fixture = self.fixture
        fixture.make_legacy_installation()
        (fixture.launchers / "maeklog-update").write_text("unrelated updater")
        before = fixture.snapshot()
        result = fixture.run_migration("--apply")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("maeklog-update", result.stderr)
        self.assertEqual(fixture.snapshot(), before)

    def test_migration_cannot_be_combined_with_rebuild_inputs(self):
        fixture = self.fixture
        fixture.make_legacy_installation()
        before = fixture.snapshot()
        result = fixture.run_migration("--apply", "--codex-binary", str(fixture.codex))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(fixture.snapshot(), before)

    def test_migration_rolls_back_after_launcher_files_were_published(self):
        fixture = self.fixture
        fixture.make_legacy_installation()
        before = fixture.snapshot()
        install_files = installer.launcher.install_files

        def fail_after_publish(home, binary):
            install_files(home, binary)
            raise OSError("fixture publication failure")

        with patch.object(installer.launcher, "install_files", side_effect=fail_after_publish), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, "original files restored"):
                installer.launcher.migrate(fixture.home, apply=True)
        self.assertExistingEntriesPreserved(before)
        self.assertFalse((fixture.home / ".local/share/maeklog/codex-launcher.py").exists())
        self.assertFalse((fixture.launchers / "maeklog-update").exists())
        self.assertEqual((fixture.launchers / "codex").resolve(), fixture.package() / "bin/codex")

    def test_installation_rolls_back_all_live_targets_on_verification_failure(self):
        fixture = self.fixture
        before = fixture.snapshot()
        with patch.dict(os.environ, fixture.env, clear=True), \
             patch.object(sys, "argv", fixture.command("--apply")), \
             patch.object(installer.launcher, "verify", return_value=False), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, "original files restored"):
                installer.main()
        self.assertExistingEntriesPreserved(before)
        self.assertFalse((fixture.home / ".local/share/maeklog/codex-launcher.py").exists())
        self.assertFalse((fixture.launchers / "maeklog-update").exists())
        self.assertEqual((fixture.launchers / "codex").resolve(), fixture.official)

    def test_rollback_preserves_concurrent_client_registration(self):
        fixture = self.fixture
        fixture.registry.unlink()
        before = fixture.snapshot()
        registered = b'{"clients":[{"pid":1234}],"turns":{}}\n'

        def reject_after_client_registration(*args):
            fixture.registry.write_bytes(registered)
            return False

        with patch.dict(os.environ, fixture.env, clear=True), \
             patch.object(sys, "argv", fixture.command("--apply")), \
             patch.object(installer.launcher, "verify", side_effect=reject_after_client_registration), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, "original files restored"):
                installer.main()
        self.assertExistingEntriesPreserved(before)
        self.assertEqual(fixture.registry.read_bytes(), registered)
        self.assertEqual((fixture.launchers / "codex").resolve(), fixture.official)

    def test_first_install_rollback_removes_new_launchers_and_preserves_records(self):
        fixture = self.fixture
        (fixture.codex_home / "hooks.json").write_text('{"hooks":{}}')
        for name in ["codex", "codex-maeklog"]:
            (fixture.launchers / name).unlink()
        before = fixture.snapshot()
        with patch.dict(os.environ, fixture.env, clear=True), \
             patch.object(sys, "argv", fixture.command("--apply")), \
             patch.object(installer.launcher, "verify", return_value=False), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, "original files restored"):
                installer.main()
        self.assertExistingEntriesPreserved(before)
        for name in ["codex", "codex-maeklog", "maeklog-update"]:
            self.assertFalse((fixture.launchers / name).exists())
            self.assertFalse((fixture.launchers / name).is_symlink())


if __name__ == "__main__":
    unittest.main()
