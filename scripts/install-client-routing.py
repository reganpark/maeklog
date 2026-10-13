#!/usr/bin/env python3
"""Install the verified local client package without restarting any process.

All replaced files are backed up before mutation. The official Codex package,
daemon, PM history, hook commands and trusted hashes remain intact.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib
from codex_hook_trust import installation_paths


launcher_spec = importlib.util.spec_from_file_location(
    'maeklog_codex_launcher', Path(__file__).resolve().with_name('codex_launcher.py'))
launcher = importlib.util.module_from_spec(launcher_spec)
launcher_spec.loader.exec_module(launcher)


def atomic_copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(prefix=destination.name+'.pm-', dir=destination.parent)
    os.close(descriptor)
    temporary = Path(name)
    shutil.copy2(source, temporary)
    os.replace(temporary, destination)


def atomic_bytes(path, content, mode=0o600):
    descriptor, name = tempfile.mkstemp(prefix=path.name+'.pm-', dir=path.parent)
    temporary = Path(name)
    with os.fdopen(descriptor, 'wb') as stream:
        os.fchmod(stream.fileno(), mode)
        stream.write(content)
    os.replace(temporary, path)


def is_herdr_session_handler(handler):
    """Identify a generated Herdr hook without disabling similar names or shell chains."""
    command = handler.get('command')
    if handler.get('type') != 'command' or not isinstance(command, str):
        return False
    # Generated Herdr handlers call the script directly. Complex shell commands
    # require separate review and must not be silently disabled by this installer.
    if any(char in command for char in '$`\r\n'):
        return False
    lexer = shlex.shlex(command, posix=True, punctuation_chars=';&|<>')
    lexer.whitespace_split = True
    lexer.commenters = ''
    try:
        words = list(lexer)
    except ValueError:
        return False
    if not words or any(any(char in word for char in ';&|<>') for word in words):
        return False
    # Herdr also generates a plain bash invocation. No shell options, -c
    # commands, substitutions or chained commands are owned by this installer.
    if words[0] in ['bash', '/bin/bash', '/usr/bin/bash']:
        words = words[1:]
    return bool(words) and Path(words[0]).name == 'herdr-agent-state.sh'


def ambiguous_herdr_session_handler(handler):
    command = handler.get('command')
    return (isinstance(command, str) and not is_herdr_session_handler(handler)
            and re.search(r'''(?:^|[/\s'"])herdr-agent-state\.sh(?=$|[\s'";&|$`<>])''', command))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codex-binary', type=Path)
    parser.add_argument('--pm-binary', type=Path)
    parser.add_argument('--resource-package', type=Path)
    parser.add_argument('--codex-version')
    parser.add_argument('--base-commit')
    parser.add_argument('--revision', type=int)
    parser.add_argument('--bridge-patch-sha256', help='Identity of the checked client bridge source')
    parser.add_argument('--launcher-only', action='store_true',
                        help='Activate the stable launcher on the existing checked package; no rebuild or config changes')
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    home = Path.home()
    if args.launcher_only:
        if any(getattr(args, name) is not None for name in [
                'codex_binary', 'pm_binary', 'resource_package', 'codex_version',
                'base_commit', 'revision', 'bridge_patch_sha256']):
            parser.error('--launcher-only cannot be combined with package installation arguments')
        if args.apply:
            lock_path = home / '.cache/maeklog/codex-update/update.lock'
            lock_path.parent.mkdir(parents=True, exist_ok=True)
            with lock_path.open('a') as lock:
                try:
                    fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError:
                    raise RuntimeError('Another Codex integration update is already running') from None
                launcher.migrate(home, apply=True)
        else:
            launcher.migrate(home)
        return
    for name in ['codex_binary', 'pm_binary', 'resource_package', 'codex_version', 'base_commit']:
        if getattr(args, name) is None:
            parser.error('--' + name.replace('_', '-') + ' is required for package installation')
    if args.revision is None:
        args.revision = 1
    if not re.fullmatch(r'\d+\.\d+\.\d+', args.codex_version) or args.revision < 1:
        parser.error('Expected a stable Codex version and a positive local revision')
    if not re.fullmatch(r'[0-9a-f]{40}', args.base_commit):
        parser.error('Expected the full upstream base commit')
    if args.bridge_patch_sha256 and not re.fullmatch(r'[0-9a-f]{64}', args.bridge_patch_sha256):
        parser.error('Expected a SHA-256 bridge patch identity')
    launcher.updater_preflight(home)
    plugin_config, codex_home = installation_paths(home)
    runtime = json.loads((plugin_config / 'hook-runtime.json').read_text())
    state = Path(runtime['state_dir'])
    config_path = codex_home / 'config.toml'
    old_config = config_path.read_bytes() if config_path.exists() else b''
    config_text = old_config.decode()
    parsed = tomllib.loads(config_text)
    hooks = json.loads((codex_home / 'hooks.json').read_text())
    matches = []
    for group_index, group in enumerate(hooks['hooks'].get('SessionStart', [])):
        for handler_index, handler in enumerate(group.get('hooks', [])):
            if is_herdr_session_handler(handler):
                matches.append(f'{codex_home}/hooks.json:session_start:{group_index}:{handler_index}')
            elif ambiguous_herdr_session_handler(handler):
                raise SystemExit('Ambiguous Herdr SessionStart command; review it before installation; nothing changed')
    if len(matches) > 1:
        raise SystemExit('Multiple Herdr SessionStart hooks; review them before installation; nothing changed')
    hook_id = matches[0] if matches else None
    new_config = config_text
    if hook_id:
        header = '[hooks.state.' + json.dumps(hook_id) + ']'
        pattern = re.compile(r'(?ms)^' + re.escape(header) + r'\n(.*?)(?=^\[|\Z)')
        def disable(match):
            body = match.group(1)
            if re.search(r'(?m)^enabled\s*=', body):
                body = re.sub(r'(?m)^enabled\s*=.*$', 'enabled = false', body)
            else:
                body = body.rstrip() + '\nenabled = false\n\n'
            return header + '\n' + body
        if hook_id in parsed.get('hooks', {}).get('state', {}):
            new_config, count = pattern.subn(disable, config_text)
            if count != 1:
                raise SystemExit('Cannot update exact existing hook table; nothing changed')
        else:
            # Herdr may have generated the hook before its first trust review.
            # Disable it without inventing or copying an approval hash.
            new_config = config_text.rstrip() + '\n\n' + header + '\nenabled = false\n'
    new_parsed = tomllib.loads(new_config)
    expected = tomllib.loads(config_text)
    if hook_id:
        expected.setdefault('hooks', {}).setdefault('state', {}).setdefault(hook_id, {})['enabled'] = False
    assert new_parsed == expected, 'Unrelated config changed'
    for binary, expected_prefix in [(args.codex_binary, f'codex-cli {args.codex_version}'), (args.pm_binary, 'maeklog ')]:
        version = subprocess.check_output([str(binary.resolve()), '--version'], text=True).strip()
        if (binary == args.codex_binary and version != expected_prefix) or not version.startswith(expected_prefix):
            raise SystemExit(f'Unexpected binary version: {version}')
    for item in ['bin', 'codex-resources', 'codex-path', 'codex-package.json']:
        if not (args.resource_package / item).exists():
            raise SystemExit(f'Missing package resource: {item}')
    if json.loads((args.resource_package / 'codex-package.json').read_text()).get('version') != args.codex_version:
        raise SystemExit(f'Resource package must match Codex {args.codex_version}')
    package = home / f'.local/share/maeklog/codex-{args.codex_version}-maeklog.{args.revision}'
    # The panel/worker protocol is unchanged. Keep its mapped executable intact;
    # existing panels may spawn workers through current_exe(). Only hooks and
    # the terminal-client adapter need this new code at runtime.
    installed_launcher = launcher.paths(home)
    targets = [plugin_config / 'bin/maeklog-hook', config_path,
               installed_launcher['codex'], installed_launcher['binary_link'],
               installed_launcher['path'], installed_launcher['updater']]
    registry = state / 'client-routing.json'
    print(json.dumps({'package':str(package),'replace':[str(p) for p in targets],
                      'launcher':launcher.record(home), 'disable_hook':hook_id,
                      'initialize_registry':not registry.exists(),'restart_processes':False}, indent=2))
    if not args.apply:
        return
    if package.exists() or package.is_symlink():
        raise SystemExit('Prepared package already exists; inspect it before reinstalling')
    if not targets[0].is_file():
        raise SystemExit(f'Expected installed Maeklog hook: {targets[0]}')
    for target in targets[1:]:
        if target.exists() and not target.is_symlink() and not target.is_file():
            raise SystemExit(f'Expected a file, symlink or absent target: {target}')
    backup_root = home / '.local/share/maeklog/install-backups'
    backup_root.mkdir(parents=True, exist_ok=True, mode=0o700)
    backup = Path(tempfile.mkdtemp(prefix='client-routing-', dir=backup_root))
    initial_registry = b'{"clients":[],"turns":{}}\n'
    snapshots = launcher.snapshot(targets + ([] if registry.exists() else [registry]), backup,
                                  remove_if_content={registry: initial_registry})
    (package / 'bin').mkdir(parents=True, mode=0o700)
    # New releases may ship helper executables beside codex (e.g. code-mode-host).
    # Preserve the complete matching official package; replace only the CLI.
    shutil.copytree(args.resource_package / 'bin', package / 'bin',
                    dirs_exist_ok=True, ignore=shutil.ignore_patterns('codex'))
    atomic_copy(args.codex_binary, package / 'bin/codex')
    atomic_copy(args.pm_binary, package / 'maeklog-hook')
    for item in ['codex-resources', 'codex-path']:
        shutil.copytree(args.resource_package / item, package / item)
    atomic_copy(args.resource_package / 'codex-package.json', package / 'codex-package.json')
    current_config = config_path.read_bytes() if config_path.exists() else b''
    if current_config != old_config or not launcher.unchanged(snapshots):
        raise SystemExit('Existing installation changed during preparation; active installation left unchanged')
    hash_paths = [package / 'maeklog-hook',
                  *sorted(p for p in (package / 'bin').rglob('*') if p.is_file())]
    hashes = {str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in hash_paths}
    hashes[str(targets[0])] = hashlib.sha256(args.pm_binary.read_bytes()).hexdigest()
    manifest = {'backup':str(backup),'base_commit':args.base_commit,'codex_version':args.codex_version,
                'local_revision':args.revision,'sha256':hashes,'routing_registry':str(registry),
                'launcher':launcher.record(home)}
    if args.bridge_patch_sha256:
        manifest['bridge_patch_sha256'] = args.bridge_patch_sha256
    atomic_bytes(package/'maeklog-install.json',json.dumps(manifest,indent=2).encode())
    try:
        # Prepare strict routing first: no new hook can misattribute an unregistered client.
        if not registry.exists():
            atomic_bytes(registry, initial_registry)
        atomic_copy(args.pm_binary, targets[0])
        if new_config.encode() != old_config:
            atomic_bytes(config_path, new_config.encode())
        launcher.install_files(home, package / 'bin/codex')
        if not launcher.verify(manifest, package / 'bin/codex', home):
            raise RuntimeError('Installed launcher verification failed')
    except BaseException as error:
        try:
            launcher.restore(snapshots)
        except OSError as restore_error:
            raise RuntimeError(f'Installation failed ({error}); restore failed ({restore_error}); backup: {backup}') from error
        raise RuntimeError(f'Installation failed; original files restored; backup: {backup}: {error}') from error
    print(json.dumps(manifest,indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, OSError, ValueError) as error:
        print(f'Installation stopped: {error}', file=sys.stderr)
        sys.exit(1)
