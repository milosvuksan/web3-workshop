#!/usr/bin/env python3
"""Real TCP regressions for offline/online isolation and independent mining control."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time


def wait(predicate, description, timeout=90):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        result = predicate()
        if result:
            return result
        time.sleep(0.05)
    raise AssertionError(f"timed out: {description}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path(__file__).resolve().parents[1] / 'target/debug/baneworkshop')
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    processes = []
    logs = []
    reservations = []
    with tempfile.TemporaryDirectory(prefix='pixel-controls-') as directory:
        base = Path(directory)
        try:
            for _ in range(2):
                sock = socket.socket()
                sock.bind(('127.0.0.1', 0))
                reservations.append(sock)
            ports = [s.getsockname()[1] for s in reservations]
            addresses = [f'127.0.0.1:{port}' for port in ports]

            def start(index, offline):
                log = (base / f'{index}.log').open('w+')
                logs.append(log)
                command = [binary, 'node', '--listen', addresses[index], '--peers', addresses[1-index],
                           '--data-dir', str(base / str(index)), '--no-lag']
                if offline:
                    command.append('--offline')
                process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=log, stderr=log, text=True)
                processes.append(process)
                wait(lambda: (base / str(index) / 'status.json').exists(), 'startup', timeout=10)
                return process

            def status(index):
                return json.loads((base / str(index) / 'status.json').read_text())

            def send(index, command):
                processes[index].stdin.write(command + '\n')
                processes[index].stdin.flush()

            # Startup offline must work even when another socket owns our port.
            start(0, True)
            assert not status(0)['online'] and status(0)['connections'] == 0
            send(0, 'network on')
            time.sleep(0.2)
            send(0, 'status')
            assert not status(0)['online'], 'failed bind must keep node offline'
            reservations[0].close()
            reservations[1].close()
            start(1, False)
            send(0, 'network on')
            wait(lambda: status(0)['connections'] > 0 and status(1)['connections'] > 0, 'initial handshake')
            send(0, 'network off')
            wait(lambda: not status(0)['online'] and status(1)['connections'] == 0, 'disconnect all transports')
            with socket.socket() as probe:
                probe.settimeout(0.3)
                assert probe.connect_ex(('127.0.0.1', ports[0])) != 0, 'offline listener is still open'
            assert status(0)['connections'] == 0
            send(0, 'mining on')
            wait(lambda: status(0)['mining'], 'offline mining starts', timeout=5)
            wait(lambda: status(0)['height'] >= 1, 'offline block mined')
            send(0, 'mining off')
            wait(lambda: not status(0)['mining'], 'miner stops', timeout=5)
            stopped = status(0)['height']
            time.sleep(0.4)
            assert status(0)['height'] == stopped, 'stopped miner extended the chain'
            assert status(1)['height'] == 0, 'offline node leaked a block to a peer'
            assert not status(0)['online'], 'mining toggle reenabled networking'
            send(0, 'network on')
            wait(lambda: status(1)['tip'] == status(0)['tip'], 'reconnect and resynchronize')
            assert not status(0)['mining'], 'network toggle reenabled mining'
            # Exercise rapid toggles with connection events still in flight.
            send(0, 'network off\nnetwork on\nnetwork off')
            wait(lambda: not status(0)['online'] and status(0)['connections'] == 0, 'rapid toggles settle offline')
            time.sleep(0.3)
            assert status(0)['connections'] == 0
            for index in range(2):
                send(index, 'quit')
            for process in processes:
                assert process.wait(timeout=5) == 0
            print('PASS: offline binds no socket, closes sessions, isolates local mining, and reconnects without restarting mining')
        except BaseException:
            for log in logs:
                log.flush()
                log.seek(0)
                print(log.read())
            raise
        finally:
            for reservation in reservations:
                reservation.close()
            for process in processes:
                if process.poll() is None:
                    process.kill()
                process.communicate(timeout=5)
            for log in logs:
                log.close()


if __name__ == '__main__':
    main()
