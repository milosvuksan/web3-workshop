#!/usr/bin/env python3
"""Measure real single-node block intervals at the production difficulty.

Requires a release build. Runs without peers, with normal exports/coordinator,
worker lifecycle, validation, and default-on lag. Warm-up intervals are excluded.
No chain difficulty changes and no access to existing node identities.
"""
import argparse
import datetime
import json
import math
from pathlib import Path
import queue
import statistics
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/baneworkshop')
parser.add_argument('--blocks', type=int, default=50)
parser.add_argument('--warmup', type=int, default=5)
parser.add_argument('--timeout', type=float, default=300)
parser.add_argument('--output', type=Path)
args = parser.parse_args()
if args.blocks < 1 or args.warmup < 1 or not math.isfinite(args.timeout) or args.timeout <= 0:
    parser.error('blocks, warmup, and timeout must be positive')

with tempfile.TemporaryDirectory(prefix='pixel-node-benchmark-') as temporary:
    log_lines = queue.Queue()
    process = subprocess.Popen([
        str(args.binary.resolve()), 'node', '--listen', '127.0.0.1:0',
        '--data-dir', temporary, '--mine',
    ], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, bufsize=1)

    def read_log():
        for line in process.stderr:
            log_lines.put((time.perf_counter(), line.rstrip()))
        log_lines.put((time.perf_counter(), None))

    reader = threading.Thread(target=read_log, daemon=True)
    reader.start()
    samples = []
    previous = None
    seen = 0
    final_tip = None
    last_lines = []
    started_at = datetime.datetime.now(datetime.timezone.utc).isoformat()
    deadline = time.monotonic() + args.timeout
    try:
        while len(samples) < args.blocks:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f'Node benchmark timed out after {len(samples)} samples: {last_lines}')
            try:
                observed, line = log_lines.get(timeout=min(remaining, 1))
            except queue.Empty:
                continue
            if line is None:
                raise RuntimeError(f'Node exited early: {last_lines}')
            last_lines = (last_lines + [line])[-10:]
            words = line.split()
            if len(words) == 4 and words[0] == 'tip' and words[2] == 'height':
                height = int(words[3])
                if height != seen + 1:
                    raise RuntimeError(f'Unexpected height sequence: {line}')
                seen = height
                if seen > args.warmup and previous is not None:
                    samples.append({'height': height, 'tip': words[1], 'seconds': observed - previous})
                previous = observed
                final_tip = words[1]
    finally:
        if process.poll() is None:
            try:
                process.stdin.write('mining off\nquit\n')
                process.stdin.flush()
                process.wait(timeout=10)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                process.kill()
                process.wait(timeout=5)
        reader.join(timeout=2)
    if process.returncode != 0:
        raise RuntimeError(f'Node exited {process.returncode}: {last_lines}')
    final_status = json.loads((Path(temporary) / 'status.json').read_text())
    if not final_status['lag'] or final_status['mining'] or final_status['height'] < seen:
        raise RuntimeError(f'Invalid final node state: {final_status}')
    seconds = [sample['seconds'] for sample in samples]
    result = {
        'benchmark': 'live_single_node_block_intervals',
        'started_at_utc': started_at,
        'difficulty_bits': 23,
        'mining_workers': 1,
        'peers': 0,
        'lag_enabled': True,
        'transactions_per_block': 0,
        'warmup_blocks': args.warmup,
        'sample_count': len(samples),
        'elapsed_seconds': sum(seconds),
        'mean_seconds': statistics.mean(seconds),
        'median_seconds': statistics.median(seconds),
        'p95_seconds': sorted(seconds)[math.ceil(len(seconds) * 0.95) - 1],
        'min_seconds': min(seconds),
        'max_seconds': max(seconds),
        'final_observed_tip': final_tip,
        'timing_scope': 'monotonic timestamps of stderr tip notifications; includes worker replacement, candidate construction, proof of work, validation, coordinator scheduling and normal exports; excludes startup/warmup; no peer propagation',
        'samples': samples,
    }
    rendered = json.dumps(result, indent=2) + '\n'
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)
    print(rendered, end='')
