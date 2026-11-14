import { createHash } from 'node:crypto';
const n = Number(process.argv[2]);
const size = Number(process.argv[3]);
const buf = Buffer.alloc(size);
for (let j = 0; j < size; j++) buf[j] = j & 255;
let digest;
// Safe for these test counts, while encoding the same uint64 little endian nonce.
const start = process.hrtime.bigint();
for (let i = 0; i < n; i++) {
    buf.writeUInt32LE(i >>> 0, size - 8);
    buf.writeUInt32LE(Math.floor(i / 4294967296), size - 4);
    digest = createHash('sha256').update(buf).digest();
}
const elapsed = Number(process.hrtime.bigint() - start) / 1e9;
console.log(JSON.stringify({seconds: elapsed, hashes_per_second: n / elapsed, digest: digest.toString('hex')}));
