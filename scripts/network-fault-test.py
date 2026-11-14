#!/usr/bin/env python3
"""Exercise real TCP recovery with deliberate response loss and mid-sync disconnect."""
import argparse
from collections import Counter
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/baneworkshop')
args = parser.parse_args()
v = json.loads((ROOT / 'tests/fixtures/prototype-v1.json').read_text())
fixtures = {v[name]['hash']: v[name]['block'] for name in ['one', 'two', 'three', 'fork2', 'fork3', 'fork4']}
with tempfile.TemporaryDirectory(prefix='pixel-faults-') as folder:
    folder = Path(folder)
    reservation = socket.socket()
    reservation.bind(('127.0.0.1', 0))
    port = reservation.getsockname()[1]
    reservation.close()
    with (folder / 'process.log').open('w') as log:
        process = subprocess.Popen([str(args.binary.resolve()), 'node', '--listen', f'127.0.0.1:{port}', '--data-dir', str(folder), '--lag-seed', '918'], stdin=subprocess.PIPE, stdout=log, stderr=log, text=True)
        connection = None
        buffer = b''
        requests = Counter()
        target = 'three'
        disconnected = False
        got_three = False
        last_hello = 0
        deadline = time.monotonic() + 90
        def send(message):
            connection.sendall(json.dumps(message).encode() + b'\n')
        try:
            while time.monotonic() < deadline:
                assert process.poll() is None, (folder / 'process.log').read_text()
                if connection is None:
                    try:
                        connection = socket.create_connection(('127.0.0.1', port), timeout=0.2)
                        connection.settimeout(0.1)
                        buffer = b''
                        last_hello = 0
                    except OSError:
                        time.sleep(0.1)
                        continue
                if time.monotonic() - last_hello > 1:
                    send({'type': 'hello', 'version': 1, 'genesis': v['genesis']['hash'], 'identity': v['b_public']})
                    # The initial block announcement is deliberately omitted: tips must recover it.
                    send({'type': 'tip', 'hash': v[target]['hash'], 'height': v[target]['block']['height']})
                    last_hello = time.monotonic()
                try:
                    data = connection.recv(8192)
                    assert data, 'node unexpectedly closed fixture peer'
                    buffer += data
                except socket.timeout:
                    pass
                reset = False
                while b'\n' in buffer:
                    line, buffer = buffer.split(b'\n', 1)
                    message = json.loads(line)
                    if message['type'] == 'get_block':
                        block_hash = message['hash']
                        requests[block_hash] += 1
                        # Drop the first request/response for every block, forcing retries.
                        if requests[block_hash] == 1:
                            continue
                        if block_hash == v['two']['hash'] and not disconnected:
                            connection.close()
                            connection = None
                            disconnected = True
                            reset = True
                            break
                        if block_hash in fixtures:
                            send({'type': 'block', 'block': fixtures[block_hash]})
                if reset:
                    continue
                try:
                    status = json.loads((folder / 'status.json').read_text())
                    state = json.loads((folder / 'state.json').read_text())
                except (FileNotFoundError, json.JSONDecodeError):
                    continue
                if state['tip'] != status['tip']:
                    continue
                if status['tip'] == v['three']['hash'] and not got_three:
                    assert state['state']['parcels'][v['reward_one']]['counter'] == 3
                    got_three = True
                    target = 'fork4'
                    last_hello = 0
                    # Invalid side branch and delayed old block must not corrupt active state.
                    send({'type': 'block', 'block': v['invalid']['block']})
                    send({'type': 'block', 'block': v['one']['block']})
                if status['tip'] == v['fork4']['hash']:
                    p = state['state']['parcels'][v['reward_one']]
                    assert p['counter'] == 0 and p['owner'] == v['a_public'] and p['color'] == '000000'
                    assert status['mempool'] == 3 and status['waiting'] == 0
                    assert got_three and disconnected
                    assert all(requests[h] >= 2 for h in fixtures)
                    # Malformed and oversized connections must leave the coordinator alive.
                    for payload in [b'{"type":"unknown"}\n', b' ' * 262145]:
                        with socket.create_connection(('127.0.0.1', port), timeout=1) as bad:
                            bad.sendall(payload)
                    print('PASS deliberate loss, missing ancestors, reconnect during catch-up, delayed messages, fork rollback, and malformed TCP peers', flush=True)
                    break
            else:
                raise TimeoutError((folder / 'process.log').read_text())
        finally:
            if connection is not None:
                connection.close()
            if process.poll() is None:
                process.stdin.write('quit\n')
                process.stdin.flush()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            assert process.returncode == 0, (folder / 'process.log').read_text()
