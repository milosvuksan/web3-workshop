#!/usr/bin/env python3
"""Exercise a real node against independent Python HTTP peers (loopback only)."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
VECTORS = json.loads((ROOT / 'tests/fixtures/grupa1-luka.json').read_text())
ZERO = '0' * 64


def wait(predicate, description, seconds=15):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        try:
            if predicate():
                return
        except (FileNotFoundError, json.JSONDecodeError, ConnectionError):
            pass
        time.sleep(0.05)
    raise AssertionError(f'timed out: {description}')


def free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


class ReferencePeer:
    def __init__(self):
        self.blocks = {VECTORS['one']['hash']: VECTORS['one']['block']}
        self.tip = VECTORS['one']['hash']
        self.received = []
        self.requests = 0
        peer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def reply(self, code, body):
                payload = json.dumps(body).encode()
                self.send_response(code)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def do_GET(self):
                peer.requests += 1
                if self.path == '/tip':
                    self.reply(200, dict(height=peer.blocks[peer.tip]['height'], hash=peer.tip))
                elif self.path.startswith('/blocks/') and self.path[8:] in peer.blocks:
                    self.reply(200, peer.blocks[self.path[8:]])
                else:
                    self.reply(404, {})

            def do_POST(self):
                peer.requests += 1
                assert self.headers['Content-Type'] == 'application/json'
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                if self.path == '/blocks':
                    assert set(body) == {'height', 'prev_hash', 'miner', 'nonce', 'transactions'}
                    header = '|'.join(str(body[k]) for k in ['height', 'prev_hash', 'miner', 'nonce'])
                    header += ''.join('\n' + '|'.join(str(tx[k]) for k in ['type', 'sender', 'nonce', 'parcel', 'value', 'signature']) for tx in body['transactions'])
                    digest = hashlib.sha256(header.encode()).hexdigest()
                    assert digest.startswith('00000')
                    peer.blocks[digest] = body
                    self.reply(200, {})
                elif self.path == '/transactions':
                    assert set(body) == {'type', 'sender', 'nonce', 'parcel', 'value', 'signature'}
                    peer.received.append(body)
                    self.reply(202, {})
                else:
                    self.reply(404, {})

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def address(self):
        return f'127.0.0.1:{self.server.server_port}'

    def advance(self, name):
        for key in ['one', 'two', 'three']:
            self.blocks[VECTORS[key]['hash']] = VECTORS[key]['block']
            if key == name:
                break
        self.tip = VECTORS[name]['hash']

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()


def verify_transaction(tx, directory):
    public = directory / 'public.der'
    public.write_bytes(bytes.fromhex('302a300506032b6570032100' + tx['sender']))
    payload = directory / 'payload'
    payload.write_bytes('|'.join(str(tx[k]) for k in ['type', 'sender', 'nonce', 'parcel', 'value']).encode())
    signature = directory / 'signature'
    signature.write_bytes(bytes.fromhex(tx['signature']))
    subprocess.run(['openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', str(public), '-keyform', 'DER', '-rawin', '-in', str(payload), '-sigfile', str(signature)], check=True, capture_output=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', default=str(ROOT / 'target/debug/baneworkshop'))
    binary = parser.parse_args().binary
    peers = [ReferencePeer(), ReferencePeer()]
    process = None
    with tempfile.TemporaryDirectory(prefix='pixel-compatibility-') as tmp:
        directory = Path(tmp)
        key = directory / 'identity.seed'
        key.write_text('07' * 32)
        key.chmod(0o600)
        port = free_port()
        output = (directory / 'process.log').open('w')

        def launch():
            return subprocess.Popen([binary, 'node', '--listen', f'127.0.0.1:{port}', '--data-dir', str(directory), '--no-lag'], stdin=subprocess.PIPE, stdout=output, stderr=output, text=True)

        def send(command):
            process.stdin.write(command + '\n')
            process.stdin.flush()

        def status():
            return json.loads((directory / 'status.json').read_text())

        try:
            process = launch()
            wait(lambda: (directory / 'status.json').exists(), 'node startup')
            for index, peer in enumerate(peers):
                send(f'peer add {peer.address} --compat grupa1-luka Luka {index}')
            wait(lambda: len(status()['network_peers']) == 2 and all(p['status'] == 'Connected' for p in status()['network_peers']), 'both Python HTTP peers connected')
            assert status()['height'] == 0
            assert status()['identity'] == VECTORS['a_public']
            assert status()['compatibility']['hash'] == VECTORS['one']['hash']
            parcel = VECTORS['parcel']
            send(f'grupa1 paint {parcel} red')
            send(f'grupa1 transfer {parcel} {VECTORS["b_public"]}')
            wait(lambda: all(len({tx['nonce'] for tx in p.received}) == 2 for p in peers), 'both peers receive locally signed paint and transfer')
            for peer in peers:
                unique = {tx['nonce']: tx for tx in peer.received}
                assert unique[0]['value'] == 'ff0000'
                assert unique[1]['type'] == 'transfer'
                for tx in unique.values():
                    verify_transaction(tx, directory)
            peers[0].advance('three')
            wait(lambda: status()['compatibility']['hash'] == VECTORS['three']['hash'], 'backward fetch and forward validation')
            wait(lambda: VECTORS['three']['hash'] in peers[1].blocks, 'received chain relayed to second peer')
            state = json.loads((directory / 'grupa1-state.json').read_text())['state']
            assert state['parcels'][parcel]['owner'] == VECTORS['b_public']
            assert state['parcels'][parcel]['color'] == '123456'
            send('network off')
            wait(lambda: not status()['online'], 'network off')
            counts = [p.requests for p in peers]
            time.sleep(2.3)
            assert counts == [p.requests for p in peers]
            with socket.socket() as probe:
                assert probe.connect_ex(('127.0.0.1', port)) != 0
            send('network on')
            wait(lambda: all(p['status'] == 'Connected' for p in status()['network_peers']), 'network on')
            send('quit')
            assert process.wait(timeout=5) == 0
            process = launch()
            wait(lambda: status()['online'] and status()['compatibility']['hash'] == VECTORS['three']['hash'] and all(p['status'] == 'Connected' for p in status()['network_peers']), 'saved profiles reconnect after restart')
            saved = json.loads((directory / 'settings.json').read_text())['peers']
            assert [p['compatibility'] for p in saved] == ['grupa1-luka'] * 2
            send(f'peer remove {peers[1].address}')
            wait(lambda: len(status()['network_peers']) == 1, 'live peer removal')
            send('quit')
            assert process.wait(timeout=5) == 0
            print('PASS independent Python HTTP peers: signed send/receive, chain sync, multiple profiles, offline, restart, removal')
        finally:
            if process and process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
            output.close()
            for peer in peers:
                peer.close()


if __name__ == '__main__':
    main()
