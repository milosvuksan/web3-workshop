#!/usr/bin/env python3
"""Drive the real dashboard through a Unix PTY and check terminal cleanup."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import socket
import struct
import subprocess
import tempfile
import termios
import time


def screen_text(transcript):
    """Decode the cursor-addressed output used by Crossterm, ignoring styles."""
    screen = [[' '] * 240 for _ in range(80)]
    row = col = index = 0
    text = transcript.decode('utf-8', errors='replace')
    while index < len(text):
        char = text[index]
        if char == '\x1b':
            match = re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])', text[index:])
            if not match:
                index += 1
                continue
            params, _, code = match.groups()
            numbers = [int(p) if p.isdigit() else 1 for p in params.split(';')]
            if code in ('H', 'f'):
                row, col = numbers[0] - 1, (numbers[1] if len(numbers) > 1 else 1) - 1
            elif code == 'J' and params == '2':
                screen = [[' '] * 240 for _ in range(80)]
            elif code == 'G':
                col = numbers[0] - 1
            elif code == 'A':
                row -= numbers[0]
            elif code == 'B':
                row += numbers[0]
            elif code == 'C':
                col += numbers[0]
            elif code == 'D':
                col -= numbers[0]
            index += len(match.group())
            continue
        if char == '\r':
            col = 0
        elif char == '\n':
            row += 1
        elif char >= ' ':
            if 0 <= row < 80 and 0 <= col < 240:
                screen[row][col] = char
            col += 1
        index += 1
    return '\n'.join(''.join(line).rstrip() for line in screen)


def ownership_case(binary):
    """Use full-difficulty fixtures to test receipts and editing without mining."""
    fixtures = json.loads((Path(__file__).resolve().parents[1] / 'tests/fixtures/prototype-v1.json').read_text())
    with tempfile.TemporaryDirectory(prefix='pixel-owned-ui-') as directory:
        folder = Path(directory)
        seed = folder / 'identity.seed'
        seed.write_text(fixtures['b_seed'])
        seed.chmod(0o600)
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0))
            port = reservation.getsockname()[1]
        master, slave = pty.openpty()
        original = termios.tcgetattr(slave)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 32, 120, 0, 0))
        environment = {key: value for key, value in os.environ.items() if key != 'NO_COLOR'}
        environment.update(TERM='xterm-256color', COLORTERM='truecolor')
        process = subprocess.Popen([str(binary.resolve()), 'tui', '--listen', f'127.0.0.1:{port}', '--data-dir', directory, '--no-lag'],
                                   stdin=slave, stdout=slave, stderr=slave, env=environment)
        transcript = bytearray()
        peer = None

        def pump(duration=0.15):
            until = time.monotonic() + duration
            while time.monotonic() < until:
                if select.select([master], [], [], 0.02)[0]:
                    transcript.extend(os.read(master, 65536))

        def wait(predicate, description, timeout=10):
            until = time.monotonic() + timeout
            while time.monotonic() < until:
                pump(0.05)
                if predicate():
                    return
                assert process.poll() is None, f'dashboard exited: {transcript[-4000:]!r}'
            raise AssertionError(f'{description}\n{screen_text(transcript)}')

        def status():
            path = folder / 'status.json'
            return json.loads(path.read_text()) if path.exists() else {}

        def press(keys):
            os.write(master, keys)
            pump()

        def inject(names):
            for name in names:
                peer.sendall((json.dumps({'type': 'block', 'block': fixtures[name]['block']}) + '\n').encode())

        def owned_response():
            lines = (folder / 'node.log').read_text().splitlines()
            responses = [json.loads(line.split(' ', 1)[1]) for line in lines if 'received_at_utc' in line]
            return responses[-1] if responses else []

        try:
            wait(lambda: status().get('online'), 'ownership node startup')
            peer = socket.create_connection(('127.0.0.1', port), timeout=3)
            peer.sendall((json.dumps({'type': 'hello', 'version': 1, 'genesis': fixtures['genesis']['hash'], 'identity': fixtures['a_public']}) + '\n').encode())
            inject(['one', 'two', 'three'])
            wait(lambda: status().get('height') == 3, 'fixture chain synchronization')
            press(b'7')
            wait(lambda: 'Acquisition details' in screen_text(transcript) and 'transfer' in screen_text(transcript) and 'mining' in screen_text(transcript), 'owned list with both receipt methods')
            assert re.search(r'181\s+\(\s*53,\s*2\)', screen_text(transcript)), 'missing x,y coordinates'
            assert '2023-11-14 22:13:22 UTC' in screen_text(transcript), 'transfer date is not its confirmation time'
            press(b'\rre\x1b[B\t')
            wait(lambda: '> red' in screen_text(transcript) and '#ff0000' in screen_text(transcript), 'color name autocomplete and preview')
            press(b'\r')
            wait(lambda: status().get('mempool') == 1, 'named color submitted')
            press(b'p#123ABC\r')
            wait(lambda: status().get('mempool') == 2, 'hex color submitted with pending counter')
            wait(lambda: '#123abc' in screen_text(transcript) and 'pending' in screen_text(transcript), 'latest pending color is visible before mining')
            press(b'gv')
            wait(lambda: 'Ownership • x' in screen_text(transcript), 'ownership overlay enabled')
            press(b'pblue\r')
            wait(lambda: status().get('mempool') == 3 and 'Canvas • x' in screen_text(transcript), 'successful paint returns to RGB view')
            press(f':transfer 181 {fixtures["a_public"]}\r'.encode())
            wait(lambda: status().get('mempool') == 4, 'pending ownership transfer')
            press(b'porange\r')
            wait(lambda: 'Edit parcel color' in screen_text(transcript) and 'sender is not parcel owner' in screen_text(transcript), 'rejected color stays open with its error')
            assert status()['mempool'] == 4
            press(b'\x1b')
            press(b'7')
            press(b':owned\r')
            wait(lambda: bool(owned_response()), 'scriptable owned history')
            receipt = next(row for row in owned_response() if row['parcel'] == 181)
            assert receipt['x'] == 53 and receipt['y'] == 2
            assert receipt['acquisition']['method'] == 'transfer'
            assert receipt['acquisition']['height'] == 2
            assert receipt['color'] == '0000ff', 'pending edits changed confirmed color'
            press(b'gp')  # Locate parcel 181, then edit it from the map.
            wait(lambda: 'Edit parcel color' in screen_text(transcript), 'map opens selected owned parcel editor')
            inject(['fork2', 'fork3', 'fork4'])
            wait(lambda: status().get('height') == 4, 'ownership reorg while editing')
            pending_before = status()['mempool']
            press(b'red\r')
            wait(lambda: 'no longer owned' in screen_text(transcript), 'stale editor rejects lost ownership')
            assert status()['mempool'] == pending_before, 'stale editor submitted a transaction'
            press(b'\x1b')
            press(b':owned\r')
            wait(lambda: all(row['parcel'] != 181 for row in owned_response()), 'orphaned transfer disappears from owned history')
            target = status()['owned'][0]
            confirmed_path = folder / 'state.json'
            counter_before = json.loads(confirmed_path.read_text())['state']['parcels'][target]['counter']
            press(b'7\x1b[H\rred\r')
            wait(lambda: '#ff0000' in screen_text(transcript) and 'pending' in screen_text(transcript), 'named edit shows its pending preview')
            press(b'p#456DEF\r')
            wait(lambda: '#456def' in screen_text(transcript) and 'pending' in screen_text(transcript), 'second edit replaces preview with the latest hex color')
            press(b'm')
            wait(lambda: json.loads(confirmed_path.read_text())['state']['parcels'][target]['color'] == '456def', 'real mining confirms both queued color edits', timeout=90)
            press(b'm')
            wait(lambda: status().get('mining') is False, 'stop after confirmation')
            confirmed = json.loads(confirmed_path.read_text())['state']['parcels'][target]
            assert confirmed['counter'] == counter_before + 2, 'one of the sequential paint transactions was not confirmed'
            wait(lambda: f'Parcel {target}: color #456def CONFIRMED' in screen_text(transcript), 'confirmation feedback replaces pending state')
            press(b'q')
            process.wait(timeout=5)
            assert process.returncode == 0
            assert termios.tcgetattr(slave) == original
        finally:
            if peer is not None:
                peer.close()
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
            os.close(master)
            os.close(slave)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path(__file__).resolve().parents[1] / 'target/debug/baneworkshop')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='pixel-tui-') as directory:
        master, slave = pty.openpty()
        original = termios.tcgetattr(slave)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 32, 120, 0, 0))
        environment = {key: value for key, value in os.environ.items() if key != 'NO_COLOR'}
        environment.update(TERM='xterm-256color', COLORTERM='truecolor')
        # Exercise 'all' with at most two CPUs in this test child.
        limit_cpus = None
        if hasattr(os, 'sched_setaffinity'):
            affinity = set(sorted(os.sched_getaffinity(0))[:2])
            limit_cpus = lambda: os.sched_setaffinity(0, affinity)
        process = subprocess.Popen([str(args.binary.resolve()), 'tui', '--offline', '--listen', '127.0.0.1:0', '--data-dir', directory],
                                   stdin=slave, stdout=slave, stderr=slave, env=environment, preexec_fn=limit_cpus)
        transcript = bytearray()

        def pump(duration=0.2):
            until = time.monotonic() + duration
            while time.monotonic() < until:
                if select.select([master], [], [], 0.03)[0]:
                    transcript.extend(os.read(master, 65536))

        def wait(predicate, description):
            end = time.monotonic() + 10
            while time.monotonic() < end:
                pump(0.05)
                if predicate():
                    return
                if process.poll() is not None:
                    break
            raise AssertionError(f'{description}\n{transcript[-4000:]!r}')

        def status():
            path = Path(directory) / 'status.json'
            return json.loads(path.read_text()) if path.exists() else {}

        def press(keys):
            os.write(master, keys)
            pump()

        try:
            wait(lambda: 'PIXEL CHAIN' in screen_text(transcript) and status().get('online') is False, 'initial canvas')
            assert b'38;2;' in transcript or b'48;2;' in transcript, 'canvas did not emit RGB colors'
            press(b'8')
            wait(lambda: 'Use all available' in screen_text(transcript), 'settings page')
            press(b'n2\r')
            wait(lambda: status().get('mining_threads') == 2, 'exact thread count setting')
            assert status()['active_mining_threads'] == 0 and not status()['mining'] and not status()['online']
            press(b'2')
            wait(lambda: 'Toggle networking' in screen_text(transcript), 'controls menu')
            press(b'm')
            wait(lambda: status().get('mining') is True and status().get('active_mining_threads') == 2, 'two mining workers start')
            press(b'sn1\r')
            wait(lambda: status().get('active_mining_threads') == 1, 'live thread-count change')
            assert status()['mining'] and not status()['online']
            press(b'm')
            wait(lambda: status().get('mining') is False and status().get('active_mining_threads') == 0, 'stop every mining worker')
            press(b'\x1b[A\r')
            wait(lambda: status().get('mining_thread_setting') == 'all', 'all available threads setting')
            assert status()['mining_threads'] == status()['available_threads']
            assert not status()['mining'] and not status()['online']
            assert json.loads((Path(directory) / 'settings.json').read_text())['mining_threads'] == 'all'
            with socket.socket() as reservation:
                reservation.bind(('127.0.0.1', 0))
                saved_port = reservation.getsockname()[1]
            press(b'\x1b[B\x1b[Bn' + str(saved_port).encode() + b'\r')
            wait(lambda: status().get('port') == saved_port, 'TCP port from settings prompt')
            assert not status()['online'] and not status()['mining']
            assert json.loads((Path(directory) / 'settings.json').read_text()) == {'mining_threads': 'all', 'port': saved_port, 'peers': []}
            press(b'9a192.0.2.7\rWorkshop peer\r')
            wait(lambda: len(status().get('network_peers', [])) == 1, 'add saved peer from Network tab')
            assert status()['network_peers'][0]['alias'] == 'Workshop peer'
            assert status()['network_peers'][0]['address'] == '192.0.2.7:7001'
            assert status()['network_peers'][0]['status'] == 'Offline' and not status()['online']
            wait(lambda: 'Workshop peer' in screen_text(transcript), 'saved peer visible')
            press(b'e\x15Renamed peer\r')
            wait(lambda: status()['network_peers'][0]['alias'] == 'Renamed peer', 'edit peer alias')
            press(b'a127.0.0.1:7002\rCancelled peer\x1b')
            assert len(status()['network_peers']) == 1
            press(b'd')
            wait(lambda: status().get('network_peers') == [], 'remove saved peer')
            assert json.loads((Path(directory) / 'settings.json').read_text())['peers'] == []
            if limit_cpus is not None:
                press(b'm')
                wait(lambda: status().get('mining') and status().get('active_mining_threads') == status().get('available_threads'), 'all available mining workers start')
                press(b'm')
                wait(lambda: not status().get('mining') and status().get('active_mining_threads') == 0, 'all-mode workers stop')
            press(b'2o')
            wait(lambda: status().get('online') is True, 'online shortcut')
            press(b'o')
            wait(lambda: status().get('online') is False, 'offline shortcut')
            press(b'3')
            wait(lambda: 'All validation checks' in screen_text(transcript), 'test menu')
            press(b'\x1b[B' * 6 + b'\r')
            def diagnostic_passed():
                logs = list((Path(directory) / 'diagnostics').glob('*.log'))
                return logs and 'PASS' in logs[0].read_text()
            wait(diagnostic_passed, 'run format check from submenu')
            wait(lambda: 'PASS' in screen_text(transcript), 'visible diagnostic result')
            press(b'4')
            wait(lambda: 'Status' in screen_text(transcript) and 'Config / files' in screen_text(transcript), 'debug submenu')
            press(b'5')
            wait(lambda: 'Node log' in screen_text(transcript), 'logs menu')
            press(b'6')
            press(b'\x1b[H')  # Home: show the beginning of help.
            wait(lambda: 'Switch pages' in screen_text(transcript), 'help menu')
            press(b':status\r')
            press(b'1v\x1b[C')
            wait(lambda: 'Ownership' in screen_text(transcript), 'live ownership overlay')
            # Resize to a narrow viewport and exercise bottom-right selection.
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 16, 48, 0, 0))
            process.send_signal(signal.SIGWINCH)
            press(b'\x1b[B' * 35)
            press(b'q')
            wait(lambda: process.poll() is not None, 'graceful quit')
            assert process.returncode == 0
            pump()
            assert b'\x1b[?1049l' in transcript, 'alternate screen was not restored'
            final = termios.tcgetattr(slave)
            assert final == original, 'terminal attributes were not restored'
            assert not status()['online'] and not status()['mining']
            for extra, expected in [([], 'all'), (['--threads', '1'], '1')]:
                restarted = subprocess.run([str(args.binary.resolve()), 'node', '--offline', '--data-dir', directory, *extra],
                                           input='status\nquit\n', text=True, capture_output=True, timeout=5)
                assert restarted.returncode == 0, restarted.stderr
                restored = json.loads(restarted.stdout.splitlines()[0])
                assert restored['mining_thread_setting'] == expected and not restored['mining']
                assert restored['active_mining_threads'] == 0
                assert restored['port'] == saved_port
            assert json.loads((Path(directory) / 'settings.json').read_text())['mining_threads'] == 'all'
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
            os.close(master)
            os.close(slave)
    ownership_case(args.binary)
    print('PASS: PTY navigation, settings persistence, TCP port selection and live thread changes, controls, outlined RGB parcels, owned receipts, pending previews, rejected edits, real color confirmation, and terminal restoration')


if __name__ == '__main__':
    main()
