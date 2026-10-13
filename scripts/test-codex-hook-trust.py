#!/usr/bin/env python3
"""Check manual hook approval gates with synthetic private configuration only."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from codex_hook_trust import EVENTS, approved_hooks, command_hash, hook_command


class HookFixture:
    def __init__(self, home):
        self.home = home
        self.plugin = home / ".config/herdr/plugins/config/maeklog"
        self.codex_home = home / "custom codex home"
        self.binary = self.plugin / "bin/maeklog-hook"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_text('#!/bin/sh\necho "maeklog 0.14.0"\n')
        self.binary.chmod(0o700)
        self.codex_home.mkdir()
        (self.plugin / "hook-runtime.json").write_text(json.dumps({
            "state_dir": str(home / "state"), "plugin_root": str(home / "plugin-source")}))
        self.definitions = {"hooks": {event: [{"hooks": [{"type": "command",
            "command": hook_command(self.binary), "timeout": 3 if event == "Interrupt" else 5}]}]
            for event in EVENTS}}
        self.config = "[features]\nhooks = true\n"
        for event, label in EVENTS.items():
            group = self.definitions["hooks"][event][0]
            identity = f"{self.codex_home}/hooks.json:{label}:0:0"
            fingerprint = command_hash(event, group, group["hooks"][0])
            self.config += (f'\n[hooks.state.{json.dumps(identity)}]\nenabled = true\n'
                            f'trusted_hash = {json.dumps(fingerprint)}\n')
        self.save()
        self.env = {"HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"),
                    "CODEX_HOME": str(self.codex_home)}

    def save(self):
        (self.codex_home / "hooks.json").write_text(json.dumps(self.definitions))
        (self.codex_home / "config.toml").write_text(self.config)

    def snapshot(self):
        return {str(path.relative_to(self.home)): path.read_bytes()
                for path in self.home.rglob("*") if path.is_file()}


class HookTrustTests(unittest.TestCase):
    def test_fingerprint_matches_native_codex_0_162_1_hooks_list_vectors(self):
        # Obtained by initialize + hooks/list against the checked native client,
        # using an isolated home and no thread/turn/model request. Freeze the
        # native answers so the Python implementation cannot approve itself.
        command = "'/tmp/Maeklog user'\\''s fixture/bin/maeklog-hook' hook"
        expected = {
            "SessionStart": "bde051f8fb86ef865904d656132e244a9c706a347aba799735537f90fbcf0ed5",
            "UserPromptSubmit": "b30c73a9924d6a20b8123892907d388cdd20e8ea87d40f439a0c6cdba943eb8c",
            "Stop": "25fa0feff1eca1c068e53e54bb542fddf001277b61b07c7349c37c6d096a99f7",
            "Interrupt": "8960f544c1eb8e08defb82a822c006a2f867ee8c418304359b3c9a16cbd75c5d",
        }
        for event, fingerprint in expected.items():
            handler = {"type": "command", "command": command,
                       "timeout": 3 if event == "Interrupt" else 5}
            self.assertEqual(command_hash(event, {}, handler), "sha256:" + fingerprint)
        handler = {"type": "command", "command": command, "timeout": 999,
                   "additionalContextLimit": 7000}
        self.assertEqual(command_hash("Interrupt", {}, handler), "sha256:" + expected["Interrupt"])

    def test_all_four_current_approvals_use_configured_codex_home_and_do_not_write(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = HookFixture(Path(directory))
            before = fixture.snapshot()
            with patch.dict(os.environ, fixture.env, clear=True):
                binary, approved = approved_hooks()
            self.assertEqual(binary, fixture.binary)
            self.assertEqual(set(approved), set(EVENTS))
            self.assertEqual(fixture.snapshot(), before)

    def test_separate_plugin_config_override_is_rejected_before_any_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            fixture = HookFixture(Path(directory))
            before = fixture.snapshot()
            with patch.dict(os.environ, {**fixture.env, "HERDR_PLUGIN_CONFIG_DIR": str(fixture.home / "other-plugin")}, clear=True):
                with self.assertRaisesRegex(RuntimeError, "separate HERDR_PLUGIN_CONFIG_DIR is not supported"):
                    approved_hooks()
            self.assertEqual(fixture.snapshot(), before)

    def test_disabled_missing_stale_duplicate_and_malformed_hooks_fail_without_writing_trust(self):
        for failure in ["feature-off", "disabled", "missing-trust", "stale-trust", "changed-timeout",
                        "missing-hook", "duplicate", "invalid-events", "invalid-groups", "invalid-handler",
                        "invalid-timeout", "invalid-async", "invalid-runtime", "invalid-hooks-table",
                        "invalid-description", "filtered", "async"]:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                fixture = HookFixture(Path(directory))
                group = fixture.definitions["hooks"]["UserPromptSubmit"][0]
                handler = group["hooks"][0]
                if failure == "feature-off":
                    fixture.config = fixture.config.replace("hooks = true", "hooks = false")
                elif failure == "disabled":
                    fixture.config = fixture.config.replace("enabled = true", "enabled = false", 1)
                elif failure == "missing-trust":
                    fixture.config = "\n".join(line for line in fixture.config.splitlines()
                                                 if not line.startswith("trusted_hash")) + "\n"
                elif failure == "stale-trust":
                    fixture.config = fixture.config.replace("sha256:", "sha256:stale-")
                elif failure == "changed-timeout":
                    handler["timeout"] = 10
                elif failure == "missing-hook":
                    del fixture.definitions["hooks"]["Interrupt"]
                elif failure == "duplicate":
                    fixture.definitions["hooks"]["Stop"].append(fixture.definitions["hooks"]["Stop"][0])
                elif failure == "invalid-events":
                    fixture.definitions["hooks"] = []
                elif failure == "invalid-groups":
                    fixture.definitions["hooks"]["Stop"] = {}
                elif failure == "invalid-handler":
                    group["hooks"] = ["invalid"]
                elif failure == "invalid-timeout":
                    handler["timeout"] = "5"
                elif failure == "invalid-async":
                    handler["async"] = "false"
                elif failure == "invalid-runtime":
                    (fixture.plugin / "hook-runtime.json").write_text('{"state_dir":"relative"}')
                elif failure == "invalid-hooks-table":
                    fixture.config = "hooks = false\n"
                elif failure == "invalid-description":
                    fixture.definitions["description"] = []
                elif failure == "filtered":
                    group["matcher"] = "startup"
                elif failure == "async":
                    handler["async"] = True
                fixture.save()
                before = fixture.snapshot()
                with patch.dict(os.environ, fixture.env, clear=True):
                    with self.assertRaises(RuntimeError):
                        approved_hooks()
                self.assertEqual(fixture.snapshot(), before)


if __name__ == "__main__":
    unittest.main()
