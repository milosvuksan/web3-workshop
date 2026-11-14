#!/usr/bin/env python3
"""Independent Grupa 1 fixtures: Python SHA-256 + OpenSSL Ed25519, real 20-bit PoW."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def run(*args):
    return subprocess.check_output(args, stderr=subprocess.PIPE)


def generate():
    with tempfile.TemporaryDirectory(prefix='grupa1-vectors-') as directory:
        root = Path(directory)
        keys = []
        for byte in [7, 8]:
            path = root / f'{byte}.der'
            path.write_bytes(bytes.fromhex('302e020100300506032b657004220420') + bytes([byte]) * 32)
            public = run('openssl', 'pkey', '-inform', 'DER', '-in', str(path), '-pubout', '-outform', 'DER')[-32:].hex()
            keys.append((path, public))

        def transaction(key, kind, nonce, parcel, value):
            path, sender = key
            payload = f'{kind}|{sender}|{nonce}|{parcel}|{value}'.encode()
            message = root / 'message'
            message.write_bytes(payload)
            signature = run('openssl', 'pkeyutl', '-sign', '-inkey', str(path), '-keyform', 'DER', '-rawin', '-in', str(message)).hex()
            return dict(type=kind, sender=sender, nonce=nonce, parcel=parcel, value=value, signature=signature)

        def mine(height, previous, miner, transactions):
            suffix = ''.join('\n' + '|'.join(str(tx[field]) for field in ['type', 'sender', 'nonce', 'parcel', 'value', 'signature']) for tx in transactions).encode()
            prefix = f'{height}|{previous}|{miner}|'.encode()
            nonce = 0
            while True:
                digest = hashlib.sha256(prefix + str(nonce).encode() + suffix).hexdigest()
                if digest.startswith('00000'):
                    return dict(hash=digest, block=dict(height=height, prev_hash=previous, miner=miner, nonce=nonce, transactions=transactions))
                nonce += 1

        one = mine(1, '0' * 64, keys[0][1], [])
        parcel = int(one['hash'][-2:], 16)
        paint = transaction(keys[0], 'paint', 0, parcel, 'aabbcc')
        transfer = transaction(keys[0], 'transfer', 1, parcel, keys[1][1])
        two = mine(2, one['hash'], keys[0][1], [paint, transfer])
        paint_b = transaction(keys[1], 'paint', 0, parcel, '123456')
        three = mine(3, two['hash'], keys[1][1], [paint_b])
        fork_two = mine(2, one['hash'], keys[1][1], [])
        fork_three = mine(3, fork_two['hash'], keys[1][1], [])
        return dict(a_public=keys[0][1], b_public=keys[1][1], parcel=parcel, one=one, two=two, three=three, fork_two=fork_two, fork_three=fork_three)


if __name__ == '__main__':
    output = Path(__file__).resolve().parents[1] / 'tests/fixtures/grupa1-luka.json'
    output.write_text(json.dumps(generate(), indent=2) + '\n')
    print(output)
