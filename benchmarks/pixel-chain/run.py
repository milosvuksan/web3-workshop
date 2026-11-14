"""Build, verify, warm up, and benchmark installed candidates sequentially."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import shutil
import statistics
import subprocess

root = Path(__file__).resolve().parent
os.chdir(root)
parser = argparse.ArgumentParser()
parser.add_argument('--only', nargs='+', help='Candidate IDs; default: all available')
parser.add_argument('--output', default='results.json')
args = parser.parse_args()
cache = root / '.cache'
(cache / 'runtime').mkdir(parents=True, exist_ok=True)
os.environ.update({
    'GOCACHE': str(cache / 'go'),
    'XDG_RUNTIME_DIR': str(cache / 'runtime'),
    'DOTNET_CLI_HOME': str(root / '.dotnet-home'),
    'DOTNET_CLI_TELEMETRY_OPTOUT': '1',
    'DOTNET_SKIP_FIRST_TIME_EXPERIENCE': '1',
    'DOTNET_GENERATE_ASPNET_CERTIFICATE': 'false',
    'DOTNET_TieredCompilation': '0',
})
# Allow an isolated benchmark toolchain without changing the user's global setup.
isolated_cargo = cache / 'cargo' / 'bin'
if (isolated_cargo / 'cargo').exists():
    os.environ['CARGO_HOME'] = str(cache / 'cargo')
    os.environ['RUSTUP_HOME'] = str(cache / 'rustup')
    os.environ['PATH'] = str(isolated_cargo) + os.pathsep + os.environ['PATH']

def selected(candidate):
    return args.only is None or candidate in args.only

def command(args):
    return subprocess.check_output(args, text=True).strip()

for candidate, compiler, output, extra in [
    ('c', 'gcc', 'c-evp', []), ('cpp', 'g++', 'cpp-evp', []),
    ('c-legacy', 'gcc', 'c-lowlevel', ['-DLOW_LEVEL', '-Wno-deprecated-declarations']),
]:
    if selected(candidate):
        subprocess.run([compiler, '-O3', '-march=native', 'hash.c', '-lcrypto', '-o', output, *extra], check=True)
if selected('java'):
    subprocess.run(['javac', 'HashBench.java'], check=True)
commands = {
    'c': ('C / OpenSSL EVP', ['./c-evp']),
    'cpp': ('C++ / OpenSSL EVP', ['./cpp-evp']),
    'c-legacy': ('C / OpenSSL low-level (deprecated API)', ['./c-lowlevel']),
    'java': ('Java / MessageDigest', ['java', '-cp', '.', 'HashBench']),
    'python': ('Python / hashlib', ['python3', 'hash.py']),
    'node-stream': ('Node.js / crypto.createHash', ['node', 'hash.mjs']),
    'node': ('Node.js / crypto.hash (one-shot)', ['node', 'hash-oneshot.mjs']),
}
if selected('go') and shutil.which('go'):
    subprocess.run(['go', 'build', '-o', 'go-hash', 'hash.go'], check=True)
    commands['go'] = ('Go / crypto/sha256', ['./go-hash'])
if (selected('csharp') or selected('csharp-incremental')) and shutil.which('dotnet'):
    subprocess.run(['dotnet', 'build', 'csharp/HashBench.csproj', '-c', 'Release', '--nologo'], check=True)
    prefix = ['dotnet', 'csharp/bin/Release/net10.0/HashBench.dll']
    commands['csharp'] = ('C# / SHA256.HashData', prefix)
    commands['csharp-incremental'] = ('C# / IncrementalHash (reused)', prefix)
if selected('rust') and shutil.which('cargo'):
    env = os.environ | {'RUSTFLAGS': '-C target-cpu=native'}
    subprocess.run(['cargo', 'build', '--locked', '--release', '--manifest-path', 'rust/Cargo.toml'], env=env, check=True)
    commands['rust'] = ('Rust / RustCrypto sha2', ['./rust/target/release/pixel-chain-hash-bench'])
if args.only:
    missing = set(args.only) - commands.keys()
    if missing:
        parser.error(f'Unknown or unavailable candidates: {sorted(missing)}')
commands = {name: cmd for key, (name, cmd) in commands.items() if selected(key)}
def invocation(name, n, size):
    return commands[name] + [str(n), str(size)] + (['incremental'] if 'IncrementalHash' in name else [])

cpu = min(os.sched_getaffinity(0))
os.sched_setaffinity(0, {cpu})
metadata = dict(platform=platform.platform(), cpu_affinity=cpu,
    versions={k: command(v) for k,v in {
        'gcc':['gcc','-dumpfullversion'], 'g++':['g++','-dumpfullversion'],
        'python':['python3','--version'], 'node':['node','--version'],
        'openssl':['openssl','version'], 'java':['java','--version'],
        **({'go': ['go', 'version']} if shutil.which('go') else {}),
        **({'dotnet': ['dotnet', '--version']} if shutil.which('dotnet') else {}),
        **({'rustc': ['rustc', '--version'], 'cargo': ['cargo', '--version']} if shutil.which('cargo') else {}),
    }.items()}, dotnet_tiered_compilation=False, rust_flags='-C target-cpu=native')
results = []
rng = random.Random(42)
for size in (128, 1024):
    n = 1000000
    reference = bytearray(j & 255 for j in range(size))
    reference[-8:] = (n-1).to_bytes(8, 'little')
    expected = hashlib.sha256(reference).hexdigest()
    for name in commands:
        command(invocation(name, 200000, size))
    samples = {name: [] for name in commands}
    for repeat in range(3):
        order = list(commands)
        rng.shuffle(order)
        for name in order:
            sample = json.loads(command(invocation(name, n, size)))
            assert sample['digest'] == expected, (name, sample)
            samples[name].append(sample)
            print(f'{size} bytes | repeat {repeat+1} | {name}: {sample["hashes_per_second"]:,.0f} H/s', flush=True)
    for name, values in samples.items():
        results.append(dict(language=name, size=size, iterations=n, samples=values,
            median_hps=statistics.median(v['hashes_per_second'] for v in values)))
Path(args.output).write_text(json.dumps(dict(metadata=metadata, results=results), indent=2) + '\n')
print('\nMEDIANS:')
for row in sorted(results, key=lambda r: (r['size'], -r['median_hps'])):
    print(f'{row["size"]} bytes | {row["language"]}: {row["median_hps"]:,.0f} H/s')
