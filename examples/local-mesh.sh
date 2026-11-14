#!/usr/bin/env bash
# Run one line per terminal; peers may start in any order.
# For an automated crash/restart demonstration use scripts/workshop-demo.
# Node A:
# target/release/baneworkshop node --listen 127.0.0.1:7001 --peers 127.0.0.1:7002,127.0.0.1:7003 --data-dir data/a --lag-seed 41
# Node B:
# target/release/baneworkshop node --listen 127.0.0.1:7002 --peers 127.0.0.1:7001,127.0.0.1:7003 --data-dir data/b --lag-seed 42
# Node C:
# target/release/baneworkshop node --listen 127.0.0.1:7003 --peers 127.0.0.1:7001,127.0.0.1:7002 --data-dir data/c --lag-seed 43
