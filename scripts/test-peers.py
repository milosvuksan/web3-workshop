#!/usr/bin/env python3
"""Check saved peer aliases, live connection updates, offline edits, and restarts."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time


def wait(predicate, description, timeout=20):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError(f'timed out: {description}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path(__file__).resolve().parents[1] / 'target/debug/baneworkshop')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='pixel-peers-') as directory:
        base = Path(directory)
        reservations = [socket.socket() for _ in range(4)]
        for sock in reservations:
            sock.bind(('127.0.0.1', 0))
        addresses = [f'127.0.0.1:{sock.getsockname()[1]}' for sock in reservations]
        for sock in reservations:
            sock.close()
        processes = {}
        logs = []

        def status(index):
            try:
                return json.loads((base / str(index) / 'status.json').read_text())
            except (FileNotFoundError, json.JSONDecodeError):
                return {}

        def peers():
            return status(0).get('network_peers', [])

        def saved():
            return json.loads((base / '0' / 'settings.json').read_text())

        def start(index, *options):
            folder = base / str(index)
            folder.mkdir(exist_ok=True)
            log = (folder / 'process.log').open('a')
            logs.append(log)
            processes[index] = subprocess.Popen([str(args.binary.resolve()), 'node', '--listen', addresses[index], '--data-dir', str(folder), '--no-lag', *options], stdin=subprocess.PIPE, stdout=log, stderr=log, text=True)

        def send(command):
            processes[0].stdin.write(command + '\n')
            processes[0].stdin.flush()

        def stop(index):
            process = processes[index]
            if process.poll() is None:
                process.stdin.write('quit\n')
                process.stdin.flush()
                process.wait(timeout=10)
            assert process.returncode == 0, (base / str(index) / 'process.log').read_text()
            process.stdin.close()

        try:
            for index in range(3):
                start(index)
            wait(lambda: all(status(i).get('online') for i in range(3)), 'listeners start')
            send(f'peer add {addresses[1]} Workshop one')
            wait(lambda: len(peers()) == 1 and peers()[0]['status'] == 'Connected', 'new saved peer connects live')
            wait(lambda: status(1).get('connections') == 1, 'remote handshake completes')
            session = status(1)['network_sessions'][0]['address']
            send(f'peer add {addresses[2]} Workshop two')
            wait(lambda: len(peers()) == 2 and all(p['status'] == 'Connected' for p in peers()), 'second peer connects')
            assert status(1)['network_sessions'][0]['address'] == session, 'editing targets interrupted an existing connection'
            send(f'peer add {addresses[1]} New alias with spaces')
            wait(lambda: peers()[0]['alias'] == 'New alias with spaces', 'alias updates')
            assert len(saved()['peers']) == 2
            assert status(1)['network_sessions'][0]['address'] == session
            send(f'peer edit {addresses[1]} {addresses[3]} Moved peer')
            wait(lambda: peers()[0]['address'] == addresses[3] and peers()[0]['status'] == 'Retrying', 'unavailable endpoint retries')
            wait(lambda: status(1).get('connections') == 0, 'old target closes')
            assert peers()[1]['status'] == 'Connected', 'unrelated connection interrupted'
            send(f'peer edit {addresses[3]} {addresses[1]} Restored alias')
            wait(lambda: peers()[0]['status'] == 'Connected', 'edited endpoint reconnects')
            send(f'peer remove {addresses[2]}')
            wait(lambda: len(peers()) == 1 and status(2).get('connections') == 0, 'removal cancels reconnect loop and session')
            identity = status(0)['identity']
            stop(0)
            start(0, '--peers', addresses[2])
            wait(lambda: status(0).get('online') and status(0).get('connections') == 2, 'restart combines saved peers with startup peers')
            assert status(0)['identity'] == identity
            assert peers()[0]['alias'] == 'Restored alias' and len(saved()['peers']) == 1
            send('network off')
            wait(lambda: status(0).get('online') is False, 'offline')
            send(f'peer add {addresses[2]} Offline addition')
            wait(lambda: len(peers()) == 2 and all(p['status'] == 'Offline' for p in peers()), 'offline edits stay offline')
            assert not status(0)['mining']
            send('network on')
            wait(lambda: status(0).get('connections') == 2 and all(p['status'] == 'Connected' for p in peers()), 'online restores saved and startup targets without duplicates')
            send('threads 3\nport ' + addresses[0].split(':')[1])
            wait(lambda: saved().get('mining_threads') == '3', 'other settings preserve peers')
            assert len(saved()['peers']) == 2
            stop(0)
            start(0)
            wait(lambda: status(0).get('online') and status(0).get('connections') == 2, 'all saved targets reconnect without --peers')
            assert len(saved()['peers']) == 2 and status(0)['mining_threads'] == 3
            print('PASS: live saved peers, aliases, unchanged sessions, endpoint edits, removal, retries, offline changes, CLI merging, and automatic reconnection after restart')
        finally:
            for index in processes:
                try:
                    stop(index)
                except Exception:
                    processes[index].kill()
                    processes[index].wait(timeout=5)
            for log in logs:
                log.close()


if __name__ == '__main__':
    main()
