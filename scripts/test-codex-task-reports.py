#!/usr/bin/env python3
"""Actual Codex hook context -> sandboxed primary tool -> host report drain.

Uses isolated config/state, a localhost Responses fixture and mocked Herdr. It
does not call an external model, open a real pane or alter user configuration.
"""
import argparse
import codecs
import fcntl
import hashlib
import http.server
import json
import os
from pathlib import Path
import pty
import select
import shlex
import shutil
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
args = parser.parse_args()
root = args.root.resolve()
root.mkdir(mode=0o700, parents=True)
script_sha = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
(root / 'run.json').write_text(json.dumps({'script_sha256': script_sha, 'passed': False}, indent=2))
home, repo = root / 'codex-home', root / 'repo'
for directory in (home, repo):
    directory.mkdir(mode=0o700)
private_parent = Path.home() / '.cache/maeklog/validation'
private_parent.mkdir(parents=True, exist_ok=True)
state = Path(tempfile.mkdtemp(prefix='task-native-state-', dir=private_parent))
plugin = Path(__file__).resolve().parents[1]
pm = args.pm_binary.resolve(strict=True)
subprocess.run(['git', 'init', '-q', '-b', 'main', str(repo)], check=True)
subprocess.run(['git', '-C', str(repo), 'remote', 'add', 'origin',
                'https://github.com/fixture/task-reports.git'], check=True)
(state / 'client-routing.json').write_text('{"clients":[],"turns":{}}')
topology = root / 'topology.json'
socket_directory = tempfile.TemporaryDirectory(prefix='pm-codex-sock-', dir='/tmp')
socket = Path(socket_directory.name) / 'server.sock'
request_text = 'fixture/task-reports#142 작업을 진행해 주세요.'
requests, tools, checks, screen, model_errors = [], [], [], [], []
decoder = codecs.getincrementaldecoder('utf-8')(errors='replace')
release_final = threading.Event()
main_calls = 0
relay = None


def tool_definitions(items, namespace=None):
    for item in items:
        if item.get('type') == 'namespace':
            yield from tool_definitions(item.get('tools', []), item['name'])
        else:
            yield item.get('function', item), namespace


def context_in(body):
    for item in body.get('input', []):
        if item.get('role') != 'developer':
            continue
        for part in item.get('content', []):
            text = part.get('text', '')
            for line in text.splitlines():
                if line.startswith('MAEKLOG_TASK_REPORT_CONTEXT_V1 '):
                    return json.loads(line.split(' ', 1)[1]), text
    return None, None


def command_for(step):
    base = [relay['executable'], 'task']
    operation = 'set' if step == 1 else 'related'
    refs = ['fixture/task-reports#142' if step == 1 else
            'https://github.com/fixture/task-reports/pull/285']
    program = f'''import json,os,subprocess
base={base!r};spool={relay['spool']!r}
def run(action,data=None,env=None):
 return subprocess.run(base+[action,spool],input=json.dumps(data) if data is not None else None,capture_output=True,text=True,timeout=5,env=env)
context=run('context');assert context.returncode==0,context.stderr
ctx=json.loads(context.stdout);assert not ctx['pending'],ctx
assert all(isinstance(x,str) for x in ctx['current']),ctx
assert 'request' not in ctx and 'body' not in ctx,ctx
'''
    if step == 1:
        program += '''child=run('context',env={**os.environ,'CODEX_THREAD_ID':'fixture-child-thread'})
assert child.returncode!=0 and any(message in child.stderr for message in (
 '주 에이전트', 'Work reports must come from the current primary agent')),child
print('PM_CHILD_THREAD_REJECTED',flush=True)
'''
    if step < 3:
        program += f'''report={{'source_uid':ctx['source_uid'],'expected_revision':ctx['expected_revision'],'operation':{operation!r},'references':{refs!r},'quote':{request_text if step == 1 else ''!r},'created':{step == 2!r}}}
r=run('report',report);assert r.returncode==0,r.stderr
reply=json.loads(r.stdout);assert reply['status']=='queued',reply
print('PM_TASK_REPORT_'+str({step})+' '+json.dumps({{'context':ctx,'report':report,'reply':reply,'thread':os.environ.get('CODEX_THREAD_ID')}}),flush=True)
'''
    else:
        program += '''assert len(ctx['receipts'])>=2,ctx
assert all(x['status']=='processed' for x in ctx['receipts']),ctx
print('PM_TASK_RECEIPTS '+json.dumps(ctx),flush=True)
'''
    return 'python3 -c ' + shlex.quote(program)


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *unused):
        pass

    def do_POST(self):
        global main_calls, relay
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        title = body.get('metadata', {}).get('thread_source') == 'system' or any(
            part.get('text', '').startswith('Generate a concise, single-line task title')
            for item in body.get('input', []) if item.get('role') == 'user'
            for part in item.get('content', []))
        requests.append(body)
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()

        def send(event):
            self.wfile.write(('event: ' + event['type'] + '\ndata: ' +
                             json.dumps(event) + '\n\n').encode())
            self.wfile.flush()

        def message(text):
            send({'type': 'response.output_item.done', 'item': {
                'type': 'message', 'role': 'assistant', 'id': 'done',
                'phase': 'final_answer', 'content': [{'type': 'output_text', 'text': text}]}})

        send({'type': 'response.created', 'response': {'id': 'task-relay-response'}})
        try:
            if title:
                message('{"title":"Task report fixture"}')
            else:
                main_calls += 1
                current, developer_text = context_in(body)
                assert current, 'hook additionalContext absent from native model request'
                if relay is None:
                    relay = current
                    checks.append('official UserPromptSubmit additionalContext reaches main model')
                    assert request_text not in developer_text, 'raw user prompt duplicated in developer context'
                    assert len(developer_text) < 5000, len(developer_text)
                assert current == relay, (current, relay)
                if main_calls <= 3:
                    definitions = list(body.get('tools', []))
                    for item in body.get('input', []):
                        if item.get('type') == 'additional_tools':
                            definitions.extend(item.get('tools', []))
                    descriptor, namespace = next((d, ns) for d, ns in
                        tool_definitions(definitions) if d.get('name') == 'exec')
                    command = command_for(main_calls)
                    tools.append(command)
                    payload = {'type': 'custom_tool_call', 'id': f'report-item-{main_calls}',
                               'call_id': f'report-call-{main_calls}', 'name': descriptor['name'],
                               'input': 'let r = await tools.exec_command(' +
                               json.dumps({'cmd': command, 'yield_time_ms': 10000}) + '); '
                               'while (r.session_id) { r = await tools.write_stdin('
                               '{session_id: r.session_id, chars: "", yield_time_ms: 1000}); } text(r);'}
                    if namespace:
                        payload['namespace'] = namespace
                    send({'type': 'response.output_item.done', 'item': payload})
                else:
                    assert release_final.wait(40), 'host did not observe both reports'
                    message('PM_NATIVE_TASK_COMPLETE')
        except Exception as error:
            model_errors.append(repr(error))
            message('PM_FIXTURE_ERROR ' + repr(error))
        send({'type': 'response.completed', 'response': {'id': 'task-relay-response',
             'usage': {'input_tokens': 0, 'input_tokens_details': None, 'output_tokens': 0,
                       'output_tokens_details': None, 'total_tokens': 0}}})


model = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Model)
threading.Thread(target=model.serve_forever, daemon=True).start()
hooks, trust = {}, ''
for event, label in [('SessionStart', 'session_start'), ('UserPromptSubmit', 'user_prompt_submit'),
                     ('Stop', 'stop'), ('Interrupt', 'interrupt')]:
    handler = {'type': 'command', 'command': shlex.quote(str(pm)) + ' hook',
               'timeout': 3 if event == 'Interrupt' else 8, 'async': False}
    hooks[event] = [{'hooks': [handler]}]
    # Codex normalized identity -> sorted JSON SHA256, codex_config::version_for_toml.
    identity = {'event_name': label, 'hooks': [handler]}
    digest = 'sha256:' + hashlib.sha256(json.dumps(identity, sort_keys=True,
                                                  separators=(',', ':')).encode()).hexdigest()
    key = f'{home}/hooks.json:{label}:0:0'
    trust += f'\n[hooks.state.{json.dumps(key)}]\nenabled = true\ntrusted_hash = {json.dumps(digest)}\n'
(home / 'hooks.json').write_text(json.dumps({'hooks': hooks}))
(home / 'config.toml').write_text(f'''model = "gpt-6-luna"
model_provider = "pm_fixture"
approval_policy = "never"
sandbox_mode = "workspace-write"
suppress_unstable_features_warning = true
check_for_update_on_startup = false
[features]
hooks = true
[model_providers.pm_fixture]
name = "PM task report local fixture"
base_url = "http://127.0.0.1:{model.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
[projects.{json.dumps(str(repo))}]
trust_level = "trusted"
[analytics]
enabled = false
{trust}
''')
env = {**os.environ, 'CODEX_HOME': str(home), 'MAEKLOG_STATE_DIR': str(state),
       'MAEKLOG_PLUGIN_ROOT': str(plugin), 'MAEKLOG_CLIENT_BIN': str(pm),
       'MAEKLOG_DISABLE_LUNA': '1', 'HERDR_ENV': '1', 'HERDR_SESSION': 'task-native-fixture',
       'HERDR_SOCKET_PATH': '/task-native-fixture/fake.sock', 'HERDR_WORKSPACE_ID': 'fixture',
       'HERDR_TAB_ID': 'fixture:t1', 'HERDR_PANE_ID': 'fixture:p1',
       'HERDR_BIN_PATH': str(plugin / 'scripts/mock-live-herdr.py'),
       'PM_CLIENT_TOPOLOGY': str(topology), 'TERM': 'xterm-256color'}
for key in ['MAEKLOG_INTERNAL', 'OPENAI_API_KEY', 'OPENAI_BASE_URL', 'CODEX_THREAD_ID', 'CODEX_SESSION_ID']:
    env.pop(key, None)
server_env = {**env, 'HERDR_WORKSPACE_ID': 'wrong-daemon', 'HERDR_TAB_ID': 'wrong-daemon-tab',
              'HERDR_PANE_ID': 'wrong-daemon-pane'}
server_log = (root / 'server.log').open('w')
server = subprocess.Popen([str(args.server_binary.resolve()), 'app-server', '--listen', f'unix://{socket}'],
                          env=server_env, cwd=repo, stdout=server_log, stderr=server_log)
pid, master = None, None


def until(predicate, label, timeout=35):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            checks.append(label)
            print('PASS', label, flush=True)
            return value
        if model_errors:
            raise RuntimeError(str(model_errors))
        if master is not None and select.select([master], [], [], .1)[0]:
            chunk = os.read(master, 65536)
            screen.append(decoder.decode(chunk))
            if b'\x1b[6n' in chunk:
                os.write(master, b'\x1b[1;1R')
        else:
            time.sleep(.1)
    raise RuntimeError('Timed out: ' + label + '\n' + ''.join(screen)[-3500:])


def task_state():
    for path in state.glob('spaces/*/task-links.json'):
        return json.loads(path.read_text())
    return None


def collected():
    if relay is None:
        return None
    spool = Path(relay['spool'])
    receipts = [json.loads(path.read_text()) for path in spool.glob('receipt-*.json')]
    if len(receipts) != 2 or not all(item['status'] == 'processed' for item in receipts):
        return None
    task = task_state()
    active = task['threads'][task['active_thread']] if task and task['active_thread'] else None
    if active and active['current'] and any(r['id'] == 'fixture/task-reports#285'
                                            for r in active['candidates']):
        return {'task': task, 'receipts': receipts}
    return None


try:
    until(socket.exists, 'isolated official app server started')
    read_pipe, write_pipe = os.pipe()
    pid, master = pty.fork()
    if pid == 0:
        os.close(write_pipe)
        os.read(read_pipe, 1)
        os.close(read_pipe)
        fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack('HHHH', 42, 110, 0, 0))
        os.chdir(repo)
        os.execve(str(args.binary.resolve()), [str(args.binary.resolve()), '--remote', f'unix://{socket}',
                  '--no-alt-screen', '-C', str(repo)], env)
    os.close(read_pipe)
    topology.write_text(json.dumps({'socket': env['HERDR_SOCKET_PATH'], 'panes': [{
        'pane_id': 'fixture:p1', 'workspace_id': 'fixture', 'tab_id': 'fixture:t1',
        'terminal_id': 'task-native-terminal', 'pid': pid, 'agent': 'codex', 'cwd': str(repo)}]}))
    os.write(write_pipe, b'1')
    os.close(write_pipe)
    until(lambda: json.loads((state / 'client-routing.json').read_text())['clients'],
          'primary terminal client registered without daemon attribution')
    until(lambda: 'Ask Codex to do anything' in ''.join(screen), 'native TUI ready')
    os.write(master, request_text.encode())
    time.sleep(.2)
    os.write(master, b'\r')
    observed = until(collected, 'panel absent: Set and subsequent Related report processed in same turn')
    until(lambda: 'PM_TASK_RECEIPTS' in ''.join(screen), 'sandbox context reads two receipts and latest revision')
    assert 'PM_CHILD_THREAD_REJECTED' in ''.join(screen)
    assert all(not json.loads(path.read_text()).get('visible')
               for path in state.glob('spaces/*/ui.json')), 'unexpected PM panel'
    checks.append('child execution thread rejected; no PM panel or permission prompt needed')
    active = observed['task']['threads'][observed['task']['active_thread']]
    assert active['current'][0]['id'] == 'fixture/task-reports#142'
    assert observed['task']['active_thread'] == relay['thread_id']
    assert all(r['receipt']['current'] == ['fixture/task-reports#142'] for r in observed['receipts'])
    assert len({r['receipt']['revision'] for r in observed['receipts']}) == 2
    assert all(json.loads(path.read_text())['tab_id'] == 'fixture:t1'
               for path in state.glob('spaces/*/meta.json'))
    assert not list(state.rglob('model-calls.jsonl'))
    release_final.set()
    until(lambda: 'PM_NATIVE_TASK_COMPLETE' in ''.join(screen), 'normal task completed after reports')
    os.write(master, b'/quit')
    time.sleep(.2)
    os.write(master, b'\r')
    until(lambda: not json.loads((state / 'client-routing.json').read_text())['clients'],
          'normal client detach preserved')
    _, status = os.waitpid(pid, 0)
    pid = None
    assert os.waitstatus_to_exitcode(status) == 0
    assert hashlib.sha256(Path(__file__).read_bytes()).hexdigest() == script_sha, 'fixture changed while running'
    (root / 'result.json').write_text(json.dumps({'checks': checks, 'main_calls': main_calls,
        'request_count': len(requests), 'relay': relay, 'collected': observed,
        'pm_binary_sha256': hashlib.sha256(pm.read_bytes()).hexdigest(), 'script_sha256': script_sha}, indent=2))
    (root / 'run.json').write_text(json.dumps({'script_sha256': script_sha,
        'pm_binary_sha256': hashlib.sha256(pm.read_bytes()).hexdigest(), 'passed': True}, indent=2))
    print('PASS actual native primary-agent task report producer', root, flush=True)
finally:
    release_final.set()
    (root / 'screen.ansi').write_text(''.join(screen))
    (root / 'requests.json').write_text(json.dumps(requests, indent=2))
    (root / 'model-errors.json').write_text(json.dumps(model_errors))
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
    if relay:
        shutil.rmtree(Path(relay['spool']).parent, ignore_errors=True)
    shutil.copytree(state, root / 'state-final', dirs_exist_ok=True)
    shutil.rmtree(state)
