#!/usr/bin/env python3
"""Independent Python byte encoding/SHA-256 + OpenSSL Ed25519 fixture check.
This is a second implementation of the encoding, not a network interoperability claim.
"""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
v = json.loads((ROOT / 'tests/fixtures/prototype-v1.json').read_text())

def hx(value):
    return bytes.fromhex(value)

def digest(value):
    return hashlib.sha256(value).digest()

def signing(tx):
    action = tx['action']
    tag, payload = (1, action['color']) if action['type'] == 'paint' else (2, action['recipient'])
    return b'PIXEL-TX-v1\0' + struct.pack('>BHQ', tag, tx['parcel'], tx['counter']) + hx(tx['sender']) + hx(payload)

def canonical(tx):
    return signing(tx) + hx(tx['signature'])

def header(block):
    transactions = b''.join(struct.pack('>H', len(canonical(tx))) + canonical(tx) for tx in block['transactions'])
    return (b'PIXEL-BLOCK-v1\0' + hx(block['previous']) + struct.pack('>Q', block['height'])
            + hx(block['miner']) + struct.pack('>QH', block['timestamp'], len(block['transactions']))
            + digest(transactions) + struct.pack('>Q', block['nonce']))

with tempfile.TemporaryDirectory(prefix='pixel-vectors-') as temporary:
    tmp = Path(temporary)
    for name in ['a', 'b']:
        # RFC 8410 PKCS#8 DER wrapping of the public test seed.
        key = tmp / 'key.der'
        key.write_bytes(hx('302e020100300506032b657004220420') + hx(v[f'{name}_seed']))
        public = subprocess.check_output(['openssl', 'pkey', '-inform', 'DER', '-in', str(key), '-pubout', '-outform', 'DER'])
        assert public[-32:] == hx(v[f'{name}_public']), name
    for name in ['paint', 'transfer', 'b_paint', 'wrong_signer']:
        vector = v[name]
        tx = vector['transaction']
        assert signing(tx).hex() == vector['signing_hex'], name
        assert canonical(tx).hex() == vector['canonical_hex'], name
        assert digest(b'PIXEL-ID-v1\0' + canonical(tx)).hex() == vector['id'], name
        public = tmp / 'public.der'
        public.write_bytes(hx('302a300506032b6570032100') + hx(tx['sender']))
        message, signature = tmp / 'message', tmp / 'signature'
        message.write_bytes(signing(tx))
        signature.write_bytes(hx(tx['signature']))
        subprocess.run(['openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', str(public), '-keyform', 'DER', '-rawin', '-in', str(message), '-sigfile', str(signature)], check=True, stdout=subprocess.DEVNULL)
        message.write_bytes(signing(tx) + b'changed')
        rejected = subprocess.run(['openssl', 'pkeyutl', '-verify', '-pubin', '-inkey', str(public), '-keyform', 'DER', '-rawin', '-in', str(message), '-sigfile', str(signature)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        assert rejected.returncode != 0

for name in ['genesis', 'one', 'two', 'three', 'fork2', 'fork3', 'fork4', 'invalid', 'duplicate']:
    vector = v[name]
    data = header(vector['block'])
    assert data.hex() == vector['canonical_hex'], name
    block_hash = digest(data)
    assert block_hash.hex() == vector['hash'], name
    if name != 'genesis':
        assert block_hash[:2] == b'\0\0' and block_hash[2] < 2, name
assert int(v['one']['hash'][-4:], 16) % 2048 == v['reward_one']
p = int(v['occupied_retry']['hash'][-4:], 16) % 2048
reward = None
for _ in range(2048):
    if p not in v['occupied_retry']['occupied']:
        reward = p
        break
    p = p * v['occupied_retry']['timestamp'] % 2048
assert reward == v['occupied_retry']['reward']
assert v['wrong_signer']['transaction']['sender'] != v['a_public']
print('Independent Python/OpenSSL check passed: keys, signatures, encodings, hashes, 23-bit PoW, and rewards.')
