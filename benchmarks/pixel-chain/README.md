# Pixel Chain language benchmark

Updated recommendation: Rust and C++ are effectively tied for mining speed with
the tested modern APIs. Go is close, especially for larger inputs. C#'s tested
SHA-256 interfaces are slower on this Linux machine. This measures a mining hot
loop, not a complete peer-to-peer node, general language execution speed, or
development time. It cannot establish a universal winner among languages.

The supplied workshop README recommends SHA-256, but leaves the final algorithm
and block encoding to the group. These benchmark inputs are synthetic and do not
define a protocol or replace the group's specification.

## Method

- Host: AMD Ryzen 7 PRO 8840HS, Linux x86-64, CPU SHA extensions available.
- One worker pinned to one logical CPU; candidates run sequentially.
- Hash a reusable 128-byte or 1024-byte buffer with SHA-256 after changing an
  8-byte little-endian nonce at its end. Other bytes initially equal index mod 256.
- 1,000,000 attempts per sample; three samples; report median attempts/second.
- Candidate ordering is shuffled with a fixed seed on each round.
- Untimed 200,000-attempt warmup per candidate and size; Java and C# also warm up
  inside each measured process. C# uses a Release build with tiered compilation
  disabled, so timed code uses the optimizing JIT from the start. Process launch
  and compilation are outside measured time.
- Every sample's final digest is checked against a Python reference for the same
  bytes and nonce. This checks agreement on the workload, not full node correctness.
- No JSON encoding, networking, signature validation, persistence, prefix-state
  reuse, parallel mining, target check, or early stopping is included.
- C and C++ compile the same C-compatible source with `-O3 -march=native` and use
  OpenSSL EVP with a fetched algorithm and reused context. Comparing these two
  measures toolchain variation around the same library, rather than distinct
  native SHA implementations. Java reuses its MessageDigest and output buffer;
  Python and Node use their normal built-in digest interfaces.
- Go uses `crypto/sha256.Sum256`. Rust uses RustCrypto `sha2` 0.11.1 with a
  release build, LTO, and `-C target-cpu=native`; black-box barriers prevent
  elimination of hashing attempts. C# uses an output buffer with either
  `SHA256.HashData` or a reused `IncrementalHash` object. No per-attempt byte
  arrays are allocated by the C# benchmark code.
- Toolchains: Rust 1.100.0, Go 1.27.3, .NET SDK 10.0.113/runtime 10.0.13,
  GCC/G++ 16.2.1, Java 25.0.4.1, Python 3.15.2, Node.js 24.18.0, OpenSSL 4.0.3.

## Measured results

Million hashes per second; larger is better.

| Implementation | 128 bytes | 1024 bytes |
|---|---:|---:|
| Rust / RustCrypto sha2 | 9.85 | 2.07 |
| C++ / OpenSSL EVP | 9.72 | 2.06 |
| C / OpenSSL EVP | 9.50 | 2.06 |
| Go / crypto/sha256 | 8.43 | 2.00 |
| Java / MessageDigest | 7.59 | 1.98 |
| C# / IncrementalHash (reused) | 2.91 | 1.37 |
| Python / hashlib | 2.42 | 1.23 |
| C# / SHA256.HashData | 2.32 | 1.23 |
| Node.js / crypto.hash | 1.31 | 0.86 |
| Node.js / crypto.createHash | 0.95 | 0.67 |
| C / legacy OpenSSL SHA256 API | 10.22 | 2.09 |

The legacy API is included to illustrate API overhead; it is deprecated. Use EVP
for new OpenSSL applications. Node's one-shot API beats its streaming interface
here, so the one-shot result is the appropriate primary Node comparison.

Rust is only about 1.4% ahead of C++ for 128-byte inputs and 0.6% ahead at
1024 bytes; these short tests do not establish a meaningful language advantage.
Go is about 13% below C++ for 128-byte inputs and 3% below at 1024 bytes.
C#'s reused-object interface is faster than its one-shot interface here, but
still behind Go, Rust, and C++ for this workload. These comparisons reflect the
selected crypto interfaces and implementations, not only language overhead.
Java is also close for 1024-byte inputs. All tested languages can plausibly meet
this workshop's adjustable 5–30-second group block target; hash rate affects
relative mining rewards.

The full updated run contains 66 samples, all with final hashes checked against
the reference. `results.json` is the current full comparison. The original
comparison is preserved in `results-initial.json`; `results-go-csharp.json` is
the preliminary Go/C#/C++ run. Values in the table all come from the current run.

The largest next optimizations are moving mining off the networking thread,
partitioning nonce ranges across workers, avoiding serialization in the attempt
loop, and reusing a hashed fixed prefix if the group's chosen bytes permit it.
Those optimizations were not measured. Actual performance also depends on CPU,
crypto provider, input layout, worker count, and other load on the machine.

## Reproduce

From the workspace root:

```sh
python3 benchmarks/pixel-chain/run.py

# Only the newly requested languages and the C++ baseline:
python3 benchmarks/pixel-chain/run.py --only cpp go rust csharp csharp-incremental --output results-subset.json
```

The full comparison requires GCC, G++, OpenSSL development headers/library,
Python, Node.js with `crypto.hash`, and a JDK. It includes Go, Rust, and .NET when
available. `--only` requires each explicitly selected candidate to be available.
The runner overwrites generated binaries, Java class, and its output JSON in this
directory. `results.json` contains all raw samples and tool versions.

Go and .NET were available in the system installation. The user's reported
rustup installation was not visible in the execution environment. An official
minimal Rust toolchain was therefore installed locally under `.cache/rustup`
and `.cache/cargo`, with no changes to the global shell configuration. The
runner detects this isolated toolchain. RustCrypto and its dependencies are
pinned by `rust/Cargo.lock`; the first build needs crates.io access unless they
are cached. Go and C# require no third-party packages. Generated toolchains,
build artifacts, and caches are ignored by `.gitignore`.

## Primary references

- [OpenSSL SHA APIs and EVP recommendation](https://docs.openssl.org/3.6/man3/SHA256_Init/)
- [Node crypto.hash and crypto.createHash](https://nodejs.org/docs/latest-v24.x/api/crypto.html)
- [Python hashlib and GIL behavior](https://docs.python.org/3/library/hashlib.html)
- [RustCrypto SHA-2 hardware backends](https://docs.rs/sha2/latest/sha2/)
- [Go crypto/sha256](https://pkg.go.dev/crypto/sha256)
- [.NET SHA256.HashData overloads](https://learn.microsoft.com/en-us/dotnet/api/system.security.cryptography.sha256.hashdata?view=net-10.0)
