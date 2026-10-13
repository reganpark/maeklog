#!/usr/bin/env python3
"""Check the installed launcher with fake executables and no update downloads."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


LAUNCHER = Path(__file__).with_name("codex-launcher.py")


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="pm-launcher-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.home = self.root / "home with spaces"
        self.launchers = self.home / ".local/bin"
        self.package = self.home / ".local/share/maeklog/codex-0.159.3-maeklog.1"
        self.installed_launcher = self.package.parent / "codex-launcher.py"
        self.binary = self.package / "bin/codex"
        self.updater = self.launchers / "maeklog-update"
        self.launchers.mkdir(parents=True)
        self.binary.parent.mkdir(parents=True)
        shutil.copy2(LAUNCHER, self.installed_launcher)
        self.installed_launcher.chmod(0o700)
        (self.launchers / "codex").symlink_to(self.installed_launcher)
        (self.launchers / "codex-maeklog").symlink_to(self.binary)
        self.write_probe(self.binary, "codex")
        self.write_probe(self.updater, "updater")
        self.cwd = self.root / "working directory with spaces"
        self.cwd.mkdir()
        self.env = {**os.environ, "HOME": str(self.home), "PM_FIXTURE_EXIT": "0"}

    @staticmethod
    def write_probe(path, kind):
        path.write_text(f"#!{sys.executable}\n"
                        "import json, os, sys\n"
                        f"print(json.dumps({{'kind': {kind!r}, 'argv': sys.argv[1:], "
                        "'pid': os.getpid(), 'cwd': os.getcwd()}))\n"
                        "sys.exit(int(os.environ.get('PM_FIXTURE_EXIT', '0')))\n")
        path.chmod(0o700)

    def run_launcher(self, *args, env=None):
        process = subprocess.Popen([str(self.launchers / "codex"), *args],
                                   env=env or self.env, cwd=self.cwd,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        stdout, stderr = process.communicate(timeout=10)
        return process, stdout.decode(), stderr.decode()

    def assertProbe(self, args, kind, forwarded, env=None):
        process, stdout, stderr = self.run_launcher(*args, env=env)
        self.assertEqual(process.returncode, int((env or self.env)["PM_FIXTURE_EXIT"]), stderr)
        probe = json.loads(stdout)
        self.assertEqual(probe["kind"], kind)
        self.assertEqual(probe["argv"], forwarded)
        self.assertEqual(probe["pid"], process.pid)
        self.assertEqual(probe["cwd"], str(self.cwd))

    def test_only_first_argument_update_is_reserved(self):
        cases = [[], ["--help"], ["-h"], ["--version"],
                 ["exec", "update", "--json"],
                 ["resume", "update", "--last"],
                 ["--", "update"], ["--no-alt-screen", "update"],
                 ["exec", "please update this file", "quotes ' \" and `literal` $(literal)"]]
        for args in cases:
            with self.subTest(args=args):
                self.assertProbe(args, "codex", args)

    def test_native_binary_retains_pid_process_name_and_codex_argv0(self):
        # A native executable is necessary: a shebang fixture would change the
        # process name and let its interpreter replace argv[0].
        shutil.copy2("/bin/sh", self.binary)
        self.env["PM_FIXTURE_EXIT"] = "37"
        script = ('IFS= read -r process_name < "/proc/$$/comm"; '
                  'printf "%s\\0" "$$" "$0" "$process_name" "$PWD" "$@"; '
                  'exit "$PM_FIXTURE_EXIT"')
        process, stdout, stderr = self.run_launcher("-c", script)
        self.assertEqual(process.returncode, 37, stderr)
        fields = stdout.split("\0")[:-1]
        self.assertEqual(fields, [str(process.pid), "codex", "codex", str(self.cwd)])
        arguments = ["argv marker", "spaces and 한글", "line one\nline two",
                     "literal '$()' and `backticks`", "--"]
        process, stdout, stderr = self.run_launcher("-c", script, *arguments)
        self.assertEqual(process.returncode, 37, stderr)
        self.assertEqual(stdout.split("\0")[:-1],
                         [str(process.pid), *arguments[:1], "codex", str(self.cwd), *arguments[1:]])

    def test_default_update_and_explicit_version_apply(self):
        for args, expected in [(["update"], ["--apply"]),
                               (["update", "0.159.3"], ["--apply", "0.159.3"]),
                               (["update", "--jobs", "2", "--pane", "pane with spaces"],
                                ["--apply", "--jobs", "2", "--pane", "pane with spaces"]),
                               (["update", "--", "0.159.3"], ["--apply", "--", "0.159.3"]),
                               (["update", "0.159.3", "--apply"], ["0.159.3", "--apply"])]:
            with self.subTest(args=args):
                self.assertProbe(args, "updater", expected)

    def test_check_and_preflight_never_add_apply(self):
        for args in [["update", "--check"], ["update", "0.159.3", "--check"],
                     ["update", "--check", "--jobs", "2"], ["update", "--preflight"],
                     ["update", "0.162.1", "--preflight"]]:
            with self.subTest(args=args):
                self.assertProbe(args, "updater", args[1:])

    def test_update_help_is_local_and_does_not_start_the_updater(self):
        self.updater.unlink()
        for flag in ["--help", "-h"]:
            with self.subTest(flag=flag):
                process, stdout, stderr = self.run_launcher("update", flag)
                self.assertEqual(process.returncode, 0, stderr)
                self.assertIn("codex update", stdout)
                self.assertIn("--check", stdout)
                self.assertIn("--preflight", stdout)
                self.assertIn("--apply", stdout)
                self.assertNotIn('"kind"', stdout)

    def test_update_version_recommends_the_normal_codex_version_command(self):
        process, stdout, stderr = self.run_launcher("update", "--version")
        self.assertNotEqual(process.returncode, 0)
        self.assertIn("codex --version", stderr)
        self.assertNotIn('"kind"', stdout)

    def test_update_return_status_is_not_hidden(self):
        env = {**self.env, "PM_FIXTURE_EXIT": "19"}
        self.assertProbe(["update", "--check"], "updater", ["--check"], env)

    def test_paths_do_not_follow_inherited_home_or_path(self):
        decoy = self.root / "unrelated home"
        decoy_bin = decoy / ".local/bin"
        decoy_bin.mkdir(parents=True)
        self.write_probe(decoy_bin / "codex-maeklog", "wrong codex")
        self.write_probe(decoy_bin / "maeklog-update", "wrong updater")
        env = {**self.env, "HOME": str(decoy),
               "PATH": str(decoy_bin) + os.pathsep + os.environ.get("PATH", os.defpath)}
        self.assertProbe(["resume", "--last"], "codex", ["resume", "--last"], env)
        self.assertProbe(["update", "--check"], "updater", ["--check"], env)

    def test_missing_updater_is_an_error_without_upstream_fallback(self):
        self.updater.unlink()
        process, stdout, stderr = self.run_launcher("update")
        self.assertNotEqual(process.returncode, 0)
        self.assertIn("maeklog-update", stderr)
        self.assertNotIn('"kind"', stdout)

    def test_missing_checked_binary_fails_clearly(self):
        self.binary.unlink()
        process, stdout, stderr = self.run_launcher("resume", "--last")
        self.assertNotEqual(process.returncode, 0)
        self.assertIn(str(self.binary), stderr)
        self.assertNotIn('"kind"', stdout)

    def test_binary_pointer_cannot_recurse_or_escape_the_private_package(self):
        for target in [self.installed_launcher, self.updater]:
            with self.subTest(target=target):
                pointer = self.launchers / "codex-maeklog"
                pointer.unlink()
                pointer.symlink_to(target)
                process, stdout, stderr = self.run_launcher("resume", "--last")
                self.assertNotEqual(process.returncode, 0)
                self.assertTrue(stderr, "Expected an installation diagnostic")
                self.assertNotIn('"kind"', stdout)


if __name__ == "__main__":
    unittest.main()
