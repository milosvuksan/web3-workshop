#!/usr/bin/env python3
"""Regress Ctrl-C shutdown while a node's stdin pipe remains open (no mining)."""
import argparse
import json
from pathlib import Path
import selectors
import signal
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", type=Path,
        default=Path(__file__).resolve().parents[1] / "target/debug/baneworkshop",
    )
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="pixel-shutdown-test-") as folder:
        with (Path(folder) / "node.log").open("w+") as log:
            process = subprocess.Popen(
                [str(args.binary.resolve()), "node", "--listen", "127.0.0.1:0",
                 "--data-dir", folder, "--no-lag"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log,
            )
            try:
                # A response proves the event loop is running and stdin works.
                process.stdin.write(b"status\n")
                process.stdin.flush()
                with selectors.DefaultSelector() as selector:
                    selector.register(process.stdout, selectors.EVENT_READ)
                    if not selector.select(timeout=5):
                        raise AssertionError("node did not respond to status")
                    status = json.loads(process.stdout.readline())
                assert status["height"] == 0 and not status["mining"]
                # Leave the writer open while the next blocking read starts.
                time.sleep(0.2)
                process.send_signal(signal.SIGINT)
                try:
                    code = process.wait(timeout=3)
                except subprocess.TimeoutExpired as error:
                    raise AssertionError("Ctrl-C hung with stdin still open") from error
                assert code == 0, f"Ctrl-C exited with {code}, expected graceful success"
                final_status = json.loads((Path(folder) / "status.json").read_text())
                assert final_status["height"] == 0
                assert not process.stdin.closed, "test must retain the stdin writer"
            except BaseException:
                log.seek(0)
                print(log.read())
                raise
            finally:
                if process.poll() is None:
                    process.kill()
                process.communicate(timeout=3)
    print("PASS: Ctrl-C exits successfully with stdin pipe still open")


if __name__ == "__main__":
    main()
