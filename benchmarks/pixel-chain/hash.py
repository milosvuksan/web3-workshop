import hashlib
import json
import struct
import sys
import time

n = int(sys.argv[1])
size = int(sys.argv[2])
buf = bytearray(j & 255 for j in range(size))
start = time.perf_counter()
for i in range(n):
    struct.pack_into('<Q', buf, size - 8, i)
    digest = hashlib.sha256(buf).digest()
elapsed = time.perf_counter() - start
print(json.dumps(dict(seconds=elapsed, hashes_per_second=n / elapsed, digest=digest.hex())))
