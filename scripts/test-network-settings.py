#!/usr/bin/env python3
"""Real sockets: optional LAN discovery, live port changes, and saved-port startup."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time


def wait(predicate, description, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError(f'timed out: {description}')


def reserve(kind=socket.SOCK_STREAM):
    sock = socket.socket(socket.AF_INET, kind)
    sock.bind(('0.0.0.0', 0))
    return sock


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path(__file__).resolve().parents[1] / 'target/debug/baneworkshop')
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    with tempfile.TemporaryDirectory(prefix='pixel-network-settings-') as directory:
        base = Path(directory)
        reservations = [reserve(), reserve(), reserve(), reserve(socket.SOCK_DGRAM)]
        ports = [sock.getsockname()[1] for sock in reservations]
        for sock in reservations:
            sock.close()
        processes = {}
        logs = []

        def status(index):
            try:
                return json.loads((base / str(index) / 'status.json').read_text())
            except (FileNotFoundError, json.JSONDecodeError):
                return {}

        def start(index, *options):
            folder = base / str(index)
            folder.mkdir(exist_ok=True)
            log = (folder / 'process.log').open('a')
            logs.append(log)
            processes[index] = subprocess.Popen([binary, 'node', '--data-dir', str(folder), '--discover', '--discover-port', str(ports[3]), *options], stdin=subprocess.PIPE, stdout=log, stderr=log, text=True)

        def send(index, command):
            processes[index].stdin.write(command + '\n')
            processes[index].stdin.flush()

        def stop(index):
            if processes[index].poll() is None:
                send(index, 'quit')
                processes[index].wait(timeout=10)
            assert processes[index].returncode == 0, (base / str(index) / 'process.log').read_text()
            processes[index].stdin.close()

        try:
            start(0, '--port', str(ports[0]))
            start(1, '--port', str(ports[1]))
            wait(lambda: all(status(i).get('connections', 0) >= 2 for i in [0, 1]), 'discovery establishes both TCP directions with lag enabled')
            assert all(status(i)['lag'] for i in [0, 1])
            identity = status(0)['identity']
            send(0, f'port {ports[1]}')
            wait(lambda: 'Address already in use' in (base / '0' / 'node.log').read_text(), 'busy port rejected')
            assert status(0)['port'] == ports[0] and status(0)['online']
            send(0, f'port {ports[2]}')
            send(0, 'threads 3')
            wait(lambda: status(0).get('port') == ports[2] and status(0).get('mining_threads') == 3 and status(0).get('connections', 0) >= 1, 'live port change reconnects')
            with socket.create_connection(('127.0.0.1', ports[2]), timeout=1):
                pass
            saved = json.loads((base / '0' / 'settings.json').read_text())
            assert saved == {'mining_threads': '3', 'port': ports[2], 'peers': []}
            stop(0)
            start(0)
            wait(lambda: status(0).get('port') == ports[2] and status(0).get('connections', 0) >= 1 and status(0).get('identity') == identity, 'restart loads saved port and identity')
            assert status(0)['mining_threads'] == 3
            stop(0)
            start(0, '--port', str(ports[0]))
            wait(lambda: status(0).get('port') == ports[0], 'CLI overrides saved port')
            assert json.loads((base / '0' / 'settings.json').read_text()) == saved
            stop(0)
            print('PASS: UDP discovery with lag, bidirectional TCP, busy-port rollback, live port changes, saved-port restart, and CLI overrides')
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
