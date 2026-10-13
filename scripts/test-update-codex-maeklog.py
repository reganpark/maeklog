#!/usr/bin/env python3
"""Check update gates and caller selection without downloads or model calls."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("updater", Path(__file__).with_name("update-codex-maeklog.py"))
updater = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updater)
install_spec = importlib.util.spec_from_file_location(
    "installer_fixtures", Path(__file__).with_name("test-install-client-routing.py"))
installer_fixtures = importlib.util.module_from_spec(install_spec)
install_spec.loader.exec_module(installer_fixtures)


class CheckedInstallation:
    def __init__(self, home):
        self.home = home
        self.package = home / ".local/share/maeklog/codex-0.159.3-maeklog.1"
        self.binary = self.package / "bin/codex"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"checked codex")
        self.binary.chmod(0o700)
        self.helper = self.package / "bin/codex-code-mode-host"
        self.helper.write_bytes(b"matching official helper")
        self.launchers = home / ".local/bin"
        self.launchers.mkdir(parents=True)
        self.launcher = self.package.parent / "codex-launcher.py"
        shutil.copy2(Path(__file__).with_name("codex-launcher.py"), self.launcher)
        self.launcher.chmod(0o700)
        (self.launchers / "codex").symlink_to(self.launcher)
        (self.launchers / "codex-maeklog").symlink_to(self.binary)
        self.updater_link = self.launchers / "maeklog-update"
        self.updater_link.symlink_to(Path(__file__).with_name("update-codex-maeklog.py").resolve())
        self.manifest = self.package / "maeklog-install.json"
        self.data = {
            "codex_version": "0.159.3",
            "bridge_patch_sha256": hashlib.sha256(updater.bridge_patch("0.159.3").read_bytes()).hexdigest(),
            "sha256": {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                       for path in [self.binary, self.helper]},
            "launcher": {
                "path": str(self.launcher),
                "sha256": hashlib.sha256(self.launcher.read_bytes()).hexdigest(),
                "codex": str(self.launchers / "codex"),
                "binary_link": str(self.launchers / "codex-maeklog"),
                "updater": str(self.updater_link),
                "updater_target": str(self.updater_link.resolve()),
            },
        }
        self.save()

    def save(self):
        self.manifest.write_text(json.dumps(self.data))


class UpdateTests(unittest.TestCase):
    def test_shared_daemon_environment_cannot_select_another_pane(self):
        agents = {"result": {"agents": [
            {"pane_id": "wrong", "agent_session": {"value": "other-thread"}},
            {"pane_id": "actual", "agent_session": {"value": "our-thread"}},
        ]}}
        with patch.dict(os.environ, {"HERDR_ENV": "1", "CODEX_THREAD_ID": "our-thread",
                                     "HERDR_PANE_ID": "wrong"}, clear=True):
            with patch.object(updater, "output", return_value=json.dumps(agents)):
                self.assertEqual(updater.caller_pane(), "actual")
            with patch.object(updater, "output", return_value='{"result":{"agents":[]}}'):
                with self.assertRaisesRegex(RuntimeError, "Cannot identify"):
                    updater.caller_pane()

    def test_check_does_not_require_herdr_or_build(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(updater.Path, "home", return_value=Path(directory)), \
                 patch.dict(os.environ, {}, clear=True), \
                 patch.object(updater.sys, "argv", ["updater", "--check"]), \
                 patch.object(updater, "latest_version", return_value="0.159.3"), \
                 patch.object(updater, "update") as build, \
                 contextlib.redirect_stdout(io.StringIO()) as output:
                updater.main()
                build.assert_not_called()
                self.assertIn("target: 0.159.3", output.getvalue())
                self.assertEqual(list(Path(directory).iterdir()), [])

    def test_stable_release_selection_rejects_drafts_prereleases_and_nonrelease_tags(self):
        for release in [{"tag_name": "rust-v0.159.3", "draft": True},
                        {"tag_name": "rust-v0.159.3", "prerelease": True},
                        {"tag_name": "rust-v0.159.3-alpha.1"}, {"tag_name": "latest"}]:
            with self.subTest(release=release), \
                 patch.object(updater, "output", return_value=json.dumps(release)):
                with self.assertRaisesRegex(RuntimeError, "official stable"):
                    updater.latest_version()

    def test_cli_rejects_invalid_or_conflicting_flags_before_release_lookup(self):
        for args in [["--check", "--apply"], ["--preflight", "--apply"], ["--preflight", "--check"],
                     ["--jobs", "0"], ["0.159.3-alpha"]]:
            with self.subTest(args=args), \
                 patch.object(updater.sys, "argv", ["updater", *args]), \
                 patch.object(updater, "latest_version") as latest, \
                 patch.object(updater, "update") as build, \
                 contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as stopped:
                    updater.main()
                self.assertEqual(stopped.exception.code, 2)
                latest.assert_not_called()
                build.assert_not_called()

    def test_untrusted_hooks_stop_before_release_lookup_build_or_home_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            with patch.object(updater.Path, "home", return_value=home), \
                 patch.dict(os.environ, {"HERDR_ENV": "1"}, clear=True), \
                 patch.object(updater.sys, "argv", ["updater", "--apply"]), \
                 patch.object(updater, "latest_version") as latest, \
                 patch.object(updater, "update") as build:
                with self.assertRaisesRegex(RuntimeError, "hook binary is not installed"):
                    updater.main()
                latest.assert_not_called()
                build.assert_not_called()
                self.assertEqual(list(home.iterdir()), [])

    def test_preflight_works_from_herdr_shell_without_release_lookup_build_or_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            with patch.object(updater.Path, "home", return_value=home), \
                 patch.dict(os.environ, {"HERDR_ENV": "1"}, clear=True), \
                 patch.object(updater.sys, "argv", ["updater", "--preflight"]), \
                 patch.object(updater, "approved_hooks", return_value=(home / "maeklog-hook", {})), \
                 patch.object(updater.shutil, "which", return_value="/fixture/tool"), \
                 patch.object(updater, "output", side_effect=["maeklog 0.14.0", "cargo-nextest 0.fixture", '{"result":{"pane":{"pane_id":"actual"}}}']), \
                 patch.object(updater, "latest_version") as latest, \
                 patch.object(updater, "update") as build, \
                 contextlib.redirect_stdout(io.StringIO()) as printed:
                updater.main()
                latest.assert_not_called()
                build.assert_not_called()
                self.assertIn("all four current", printed.getvalue())
                self.assertEqual(list(home.iterdir()), [])

    def test_patch_selection_can_reuse_the_latest_older_patch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            folder = root / "integrations/codex"
            folder.mkdir(parents=True)
            for version in ["0.157.1", "0.159.2", "0.159.3"]:
                (folder / f"codex-{version}-maeklog.patch").touch()
            with patch.object(updater, "ROOT", root):
                self.assertEqual(updater.bridge_patch("0.159.4").name, "codex-0.159.3-maeklog.patch")
                self.assertEqual(updater.bridge_patch("0.159.2").name, "codex-0.159.2-maeklog.patch")

    def test_installed_version_checks_launcher_and_binary_independently(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            installed = CheckedInstallation(home)
            with patch.object(updater.Path, "home", return_value=home):
                self.assertTrue(updater.installation_current("0.159.3"))
                self.assertFalse(updater.installation_current("0.159.4"))
                checked = installed.binary.read_bytes()
                installed.binary.write_bytes(b"unverified codex")
                self.assertFalse(updater.installation_current("0.159.3"))
                installed.binary.write_bytes(checked)
                self.assertTrue(updater.installation_current("0.159.3"))
                checked = installed.launcher.read_bytes()
                installed.launcher.write_bytes(checked + b"\n# tampered launcher\n")
                self.assertFalse(updater.installation_current("0.159.3"))
                installed.data["launcher"]["sha256"] = hashlib.sha256(installed.launcher.read_bytes()).hexdigest()
                installed.save()
                self.assertFalse(updater.installation_current("0.159.3"),
                                 "Self-reported hashes must not bless an altered launcher")

    def test_saved_package_patch_helpers_and_launcher_metadata_remain_required(self):
        cases = ["patch", "helper", "binary-hash", "launcher-metadata", "launcher-hash", "updater-target"]
        for change in cases:
            with self.subTest(change=change), tempfile.TemporaryDirectory() as directory:
                installed = CheckedInstallation(Path(directory))
                if change == "patch":
                    installed.data["bridge_patch_sha256"] = "0" * 64
                elif change == "helper":
                    installed.helper.write_bytes(b"different helper")
                elif change == "binary-hash":
                    del installed.data["sha256"][str(installed.binary)]
                elif change == "launcher-metadata":
                    del installed.data["launcher"]
                elif change == "launcher-hash":
                    installed.data["launcher"]["sha256"] = "0" * 64
                else:
                    installed.updater_link.unlink()
                    installed.updater_link.symlink_to(installed.binary)
                installed.save()
                with patch.object(updater.Path, "home", return_value=installed.home):
                    self.assertFalse(updater.installation_current("0.159.3"))

    def test_direct_codex_link_does_not_count_as_an_installed_update_launcher(self):
        with tempfile.TemporaryDirectory() as directory:
            installed = CheckedInstallation(Path(directory))
            (installed.launchers / "codex").unlink()
            (installed.launchers / "codex").symlink_to(installed.binary)
            with patch.object(updater.Path, "home", return_value=installed.home):
                self.assertFalse(updater.installation_current("0.159.3"))

    def test_install_verified_uses_real_installer_and_checks_native_binary_separately(self):
        for apply in [False, True]:
            with self.subTest(apply=apply), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                fixture = installer_fixtures.InstallationFixture(root / "home with spaces")
                candidate = root / "candidate"
                shutil.copytree(fixture.resources, candidate)
                shutil.copy2(fixture.codex, candidate / "bin/codex")
                steps = []

                def run_installer(label, command, evidence, *args, **kwargs):
                    steps.append(label.split(":", 1)[0])
                    completed = subprocess.run([str(arg) for arg in command], env=fixture.env,
                                               capture_output=True, text=True)
                    self.assertEqual(completed.returncode, 0, completed.stderr or completed.stdout)

                before = fixture.snapshot()
                with patch.dict(os.environ, fixture.env, clear=True), \
                     patch.object(updater.Path, "home", return_value=fixture.home), \
                     patch.object(updater, "step", side_effect=run_installer):
                    updater.install_verified(candidate, fixture.resources, fixture.pm,
                                             fixture.version, "a" * 40, root, apply)
                    self.assertEqual(steps, ["install-preflight", "install"] if apply else ["install-preflight"])
                    if not apply:
                        self.assertEqual(fixture.snapshot(), before)
                        continue
                    self.assertTrue(updater.installation_current(fixture.version))
                    launcher = (fixture.launchers / "codex").resolve()
                    checked = (fixture.launchers / "codex-maeklog").resolve()
                    self.assertNotEqual(launcher.read_bytes(), checked.read_bytes())
                    self.assertEqual(checked.read_bytes(), (candidate / "bin/codex").read_bytes())
                    source = launcher.read_bytes()
                    updater.install_verified(candidate, fixture.resources, fixture.pm,
                                             fixture.version, "a" * 40, root, True)
                    self.assertEqual((fixture.launchers / "codex").resolve(), launcher)
                    self.assertEqual(launcher.read_bytes(), source)
                    self.assertEqual((fixture.launchers / "codex-maeklog").resolve(),
                                     fixture.package(revision=2) / "bin/codex")
                    self.assertTrue(updater.installation_current(fixture.version))

    def test_lock_normalization_cannot_upgrade_external_dependencies(self):
        original = '[[package]]\nname="codex-tui"\nversion="0.0.0"\n\n' \
                   '[[package]]\nname="dep"\nversion="1.0.0"\nsource="registry"\n'
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            (source / "codex-rs").mkdir()
            lock = source / "codex-rs/Cargo.lock"
            with patch.object(updater, "output", return_value=original):
                lock.write_text(original.replace('version="0.0.0"', 'version="0.159.3"'))
                updater.check_lock_normalization(source)
                lock.write_text(lock.read_text().replace('version="1.0.0"', 'version="1.1.0"'))
                with self.assertRaisesRegex(RuntimeError, "dependency graph changed"):
                    updater.check_lock_normalization(source)

    def test_resources_can_be_hoisted_or_nested_and_must_match(self):
        layouts = ["node_modules/@openai/codex-linux-x64/vendor/triple",
                   "node_modules/@openai/codex/node_modules/@openai/codex-linux-x64/vendor/triple"]
        for layout in layouts:
            with self.subTest(layout=layout), tempfile.TemporaryDirectory() as directory:
                prefix = Path(directory)
                resources = prefix / layout
                resources.mkdir(parents=True)
                manifest = resources / "codex-package.json"
                manifest.write_text('{"version":"0.159.3"}')
                self.assertEqual(updater.matching_resources(prefix, "0.159.3"), resources)
                manifest.write_text('{"version":"0.159.2"}')
                with self.assertRaisesRegex(RuntimeError, "matching official"):
                    updater.matching_resources(prefix, "0.159.3")

    def test_every_preinstallation_failure_keeps_installation_untouched(self):
        for failure in ["clone", "patch-check", "patch", "fetch", "bazel-lock", "fmt",
                        "bridge-test", "lint", "build", "resources", "candidate-version",
                        "installer-test", "launcher-test", "routing-test", "live-test"]:
            with self.subTest(failure=failure):
                self.exercise_update(failure=failure)

    def test_installation_runs_after_live_validation(self):
        self.exercise_update()

    def exercise_update(self, failure=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            (target / "release").mkdir(parents=True)
            (target / "release/codex").write_bytes(b"candidate CLI")
            resources = root / "resources"
            (resources / "bin").mkdir(parents=True)
            (resources / "bin/codex").write_bytes(b"official CLI")
            active = root / "active-home"
            active.mkdir()
            (active / "codex").write_bytes(b"active checked binary")
            (active / "config.toml").write_bytes(b"active config")
            (active / "client-routing.json").write_bytes(b"active history")
            before = {path: path.read_bytes() for path in active.iterdir()}
            steps = []

            def step(label, *args, **kwargs):
                name = label.split(":", 1)[0]
                steps.append(name)
                if name == failure:
                    raise RuntimeError("fixture failure")

            def output(command, *args):
                if command[-1] == "--version":
                    return "codex-cli 0.159.2" if failure == "candidate-version" else "codex-cli 0.159.3"
                return "a" * 40

            with patch.object(updater, "step", side_effect=step), \
                 patch.object(updater, "output", side_effect=output), \
                 patch.object(updater, "prepare_bazel"), \
                 patch.object(updater, "check_lock_normalization"), \
                 patch.object(updater, "matching_resources", return_value=resources), \
                 patch.object(updater, "install_verified") as install, \
                 patch.object(updater.Path, "home", return_value=active), \
                 contextlib.redirect_stdout(io.StringIO()):
                if failure:
                    expected = "Built CLI version" if failure == "candidate-version" else "fixture failure"
                    with self.assertRaisesRegex(RuntimeError, expected):
                        updater.update("0.159.3", "actual", root, target, 1, True)
                    install.assert_not_called()
                    self.assertFalse((root / "verification.json").exists())
                else:
                    updater.update("0.159.3", "actual", root, target, 1, True)
                    install.assert_called_once()
                    self.assertEqual(steps[-1], "live-test")
                    self.assertTrue(json.loads((root / "verification.json").read_text())["applied"])
                self.assertEqual({path: path.read_bytes() for path in active.iterdir()}, before)


if __name__ == "__main__":
    unittest.main()
