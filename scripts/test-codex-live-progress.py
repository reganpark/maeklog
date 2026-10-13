#!/usr/bin/env python3
"""Native Codex TUI -> PM receiver with a localhost response fixture and fake Herdr.

Never reads user Codex config, calls an external model, or controls real Herdr panes.
"""
import argparse
import codecs
import fcntl
import http.server
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import tempfile
import termios
import threading
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', required=True, type=Path)
parser.add_argument('--server-binary', required=True, type=Path)
parser.add_argument('--pm-binary', required=True, type=Path)
parser.add_argument('--root', required=True, type=Path)
parser.add_argument('--partial-answer', action='store_true',
                    help='Also check partial_answer messages with a compatible app server')
args = parser.parse_args()
root = args.root.resolve()
root.mkdir(mode=0o700)
home, state, repo = (root / name for name in ('codex-home', 'state', 'repo'))
for directory in (home, state, repo):
    directory.mkdir(mode=0o700)
subprocess.run(['git', 'init', '-q', str(repo)], check=True)
(state / 'client-routing.json').write_text('{"clients":[],"turns":{}}')
plugin = Path(__file__).resolve().parents[1]
topology = root / 'topology.json'
socket_directory = tempfile.TemporaryDirectory(prefix='pm-codex-sock-', dir='/tmp')
socket = Path(socket_directory.name) / 'server.sock'
release_final = threading.Event()
release_partial = threading.Event()
requests = []
screen = []
decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')
main_calls = 0


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *unused):
        pass

    def do_POST(self):
        global main_calls
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        title = body.get('metadata', {}).get('thread_source') in {'system', 'thread_title'} or any(
            part.get('text', '').startswith('Generate a concise, single-line task title')
            for item in body.get('input', []) if item.get('role') == 'user'
            for part in item.get('content', []))
        requests.append({'path': self.path, 'title': title, 'model': body.get('model')})
        if not title:
            main_calls += 1
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()

        def send(event):
            payload = 'event: ' + event['type'] + '\ndata: ' + json.dumps(event) + '\n\n'
            self.wfile.write(payload.encode())
            self.wfile.flush()

        def message(item_id, text, phase):
            send({'type': 'response.output_item.done', 'item': {
                'type': 'message', 'role': 'assistant', 'id': item_id, 'phase': phase,
                'content': [{'type': 'output_text', 'text': text}]}})

        send({'type': 'response.created', 'response': {'id': 'native-progress-response'}})
        if title:
            message('title', '{"title":"Native progress fixture"}', 'final_answer')
        elif main_calls == 1:
            message('progress-report', 'PM_NATIVE_PROGRESS: geometry checked; rendering next.', 'commentary')
            send({'type': 'response.output_item.done', 'item': {
                'type': 'function_call', 'id': 'plan-item', 'call_id': 'plan-call',
                'name': 'update_plan', 'arguments': json.dumps({'plan': [
                    {'step': 'PM_NATIVE_GEOMETRY', 'status': 'completed'},
                    {'step': 'PM_NATIVE_RENDER', 'status': 'in_progress'}]})}})
        else:
            if args.partial_answer:
                if not release_partial.wait(45):
                    raise RuntimeError('Commentary and plan were not observed before partial answer')
                message('partial-report', 'PM_NATIVE_PARTIAL: first result ready; finishing next.',
                        'partial_answer')
            if not release_final.wait(45):
                raise RuntimeError('Progress bridge was not observed before final response')
            message('final-report', 'PM_NATIVE_FINAL', 'final_answer')
        send({'type': 'response.completed', 'response': {'id': 'native-progress-response',
             'usage': {'input_tokens': 0, 'input_tokens_details': None, 'output_tokens': 0,
                       'output_tokens_details': None, 'total_tokens': 0}}})


model = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Model)
threading.Thread(target=model.serve_forever, daemon=True).start()
(home / 'config.toml').write_text(f'''model = "gpt-6-luna"
model_provider = "pm_fixture"
approval_policy = "never"
sandbox_mode = "read-only"
suppress_unstable_features_warning = true
check_for_update_on_startup = false
[tools.update_plan]
enabled = true
[features]
hooks = false
[model_providers.pm_fixture]
name = "PM local fixture"
base_url = "http://127.0.0.1:{model.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
[projects.{json.dumps(str(repo))}]
trust_level = "trusted"
[analytics]
enabled = false
''')
env = {**os.environ, 'CODEX_HOME': str(home), 'MAEKLOG_STATE_DIR': str(state),
       'MAEKLOG_PLUGIN_ROOT': str(plugin), 'MAEKLOG_CLIENT_BIN': str(args.pm_binary.resolve()),
       'MAEKLOG_DISABLE_LUNA': '1', 'HERDR_ENV': '1', 'HERDR_SESSION': 'native-fixture',
       'HERDR_SOCKET_PATH': '/native-fixture/fake.sock', 'HERDR_WORKSPACE_ID': 'fixture',
       'HERDR_TAB_ID': 'fixture:t1', 'HERDR_PANE_ID': 'fixture:p1',
       'HERDR_BIN_PATH': str(plugin / 'scripts/mock-live-herdr.mjs'),
       'PM_CLIENT_TOPOLOGY': str(topology), 'TERM': 'xterm-256color'}
env.pop('MAEKLOG_INTERNAL', None)
server_env = {**env, 'HERDR_WORKSPACE_ID': 'wrong-daemon', 'HERDR_TAB_ID': 'wrong-daemon-tab',
              'HERDR_PANE_ID': 'wrong-daemon-pane'}
server_log = (root / 'server.log').open('w')
server = subprocess.Popen([str(args.server_binary.resolve()), 'app-server', '--listen', f'unix://{socket}'],
                          env=server_env, cwd=repo, stdout=server_log, stderr=server_log)
pid, master = None, None
checks = []
partial = None


def until(predicate, label, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            checks.append(label)
            print('PASS', label, flush=True)
            return value
        if master is not None and select.select([master], [], [], .1)[0]:
            chunk = os.read(master, 65536)
            screen.append(decoder.decode(chunk))
            if b'\x1b[6n' in chunk:
                os.write(master, b'\x1b[1;1R')
        else:
            time.sleep(.1)
    raise RuntimeError('Timed out: ' + label + '\n' + ''.join(screen)[-2000:])


def progress():
    for path in state.glob('spaces/*/live-turn.json'):
        return json.loads(path.read_text())
    return None


try:
    until(socket.exists, 'isolated official app server started')
    ready_read, ready_write = os.pipe()
    pid, master = pty.fork()
    if pid == 0:
        os.close(ready_write)
        os.read(ready_read, 1)
        os.close(ready_read)
        fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack('HHHH', 42, 100, 0, 0))
        os.chdir(repo)
        os.execve(str(args.binary.resolve()), [str(args.binary.resolve()), '--remote', f'unix://{socket}',
                  '--no-alt-screen', '-C', str(repo)], env)
    os.close(ready_read)
    topology.write_text(json.dumps({'socket': env['HERDR_SOCKET_PATH'], 'panes': [{
        'pane_id': 'fixture:p1', 'workspace_id': 'fixture', 'tab_id': 'fixture:t1',
        'terminal_id': 'native-fixture-terminal', 'pid': pid, 'agent': 'codex', 'cwd': str(repo)}]}))
    os.write(ready_write, b'1')
    os.close(ready_write)
    until(lambda: json.loads((state / 'client-routing.json').read_text())['clients'],
          'native terminal client registered')
    until(lambda: 'Ask Codex to do anything' in ''.join(screen), 'native TUI ready')
    time.sleep(.6)
    os.write(master, b'PM_NATIVE_REQUEST')
    time.sleep(.25)
    os.write(master, b'\r')
    snapshot = until(lambda: (s := progress()) and not s['ended']
                     and any('PM_NATIVE_PROGRESS' in r['text'] for r in s['commentary'])
                     and len(s['plan']) == 2 and s,
                     'public commentary and structured plan collected before final response')
    assert snapshot['plan'][0]['status'] == 'completed'
    assert snapshot['plan'][1]['status'] == 'inProgress'
    assert snapshot['client_pid'] == pid
    assert all(json.loads(p.read_text())['tab_id'] == 'fixture:t1' for p in state.glob('spaces/*/meta.json'))
    assert not list(state.glob('spaces/*/live-summary.json'))
    assert not list(state.glob('spaces/*/inbox.jsonl'))
    assert not list(state.glob('spaces/*/selected-current.json'))
    checks.append('native stream has zero PM model calls and no durable queue writes')
    if args.partial_answer:
        release_partial.set()
        partial = until(lambda: (s := progress()) and not s['ended']
                        and any('PM_NATIVE_PARTIAL' in r['text'] for r in s['commentary']) and s,
                        'explicit partial answer collected as a transient report before completion')
        assert partial['client_pid'] == pid
        assert len(partial['plan']) == 2
        assert not list(state.glob('spaces/*/live-summary.json'))
        assert not list(state.glob('spaces/*/inbox.jsonl'))
        assert not list(state.glob('spaces/*/selected-current.json'))
        checks.append('partial answer has zero PM model calls and no durable queue writes')
    release_final.set()
    terminal = until(lambda: (s := progress()) and s['ended'] and s,
                     'native turn completion closes progress')
    assert all('PM_NATIVE_FINAL' not in r['text'] for r in terminal['commentary'])
    if args.partial_answer:
        assert any('PM_NATIVE_PARTIAL' in r['text'] for r in terminal['commentary'])
    checks.append('explicit final answer excluded from transient reports')
    os.write(master, b'/quit')
    time.sleep(.25)
    os.write(master, b'\r')
    until(lambda: not json.loads((state / 'client-routing.json').read_text())['clients'],
          'normal exit detaches candidate')
    _, status = os.waitpid(pid, 0)
    pid = None
    assert os.waitstatus_to_exitcode(status) == 0
    (root / 'result.json').write_text(json.dumps({'checks': checks, 'requests': requests,
        'collected': snapshot, 'partial': partial, 'terminal': terminal}, indent=2))
    print('PASS native live bridge', root, flush=True)
finally:
    release_partial.set()
    release_final.set()
    (root / 'screen.ansi').write_text(''.join(screen))
    if pid is not None:
        try:
            os.kill(pid, signal.SIGTERM)
            os.waitpid(pid, 0)
        except ProcessLookupError:
            pass
    if master is not None:
        os.close(master)
    server.terminate()
    server.wait(timeout=15)
    server_log.close()
    model.shutdown()
    socket_directory.cleanup()
