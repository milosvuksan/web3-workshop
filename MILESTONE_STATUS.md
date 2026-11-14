# Milestone and stretch-goal status

Status as of **2026-11-14**, following the implementation and Sol Medium audit on branch `milos`. Source checklist: [original_requirements.md](original_requirements.md). Evidence: [AUDIT_REPORT.md](AUDIT_REPORT.md), [VALIDATION.md](VALIDATION.md), the Rust tests, and the executable demonstration/fault scripts.

This report covers the Rust prototype in this repository. No evidence has been supplied that the other team members' independently implemented nodes have passed these checks. A local run of three copies of the Rust executable establishes local behavior; it does not complete a milestone requiring different people's implementations, the whole group, or the mentor. The original requirements also say a stretch goal counts only when **every node in the group** has it.

## Canvas interface

**Current interface: CLI plus generated image; no web interface.**

- Each node writes a 64×32 binary PPM image to `<data-dir>/canvas.ppm`, refreshed about every two seconds and on local export commands. Open it in a PPM-capable image viewer.
- The CLI supports identity/status, mining control, painting, transferring, exporting, and quitting. See [README.md](README.md) for commands.
- `<data-dir>/state.json` contains ownership, RGB colors, and replay counters; `<data-dir>/status.json` contains node diagnostics.
- `--listen` (default `127.0.0.1:7001`) is the newline-framed TCP peer protocol. It is not HTTP and cannot serve the canvas to a browser.

A web UI is **not implemented**. It is also not required by the original canvas-display requirement, which explicitly permits a generated image.

## Milestones 0–6

| # | Original milestone | Completed? | Evidence and remaining work |
| --- | --- | --- | --- |
| 0 | Alati / Tools | **Yes — for the Rust node** | Persistent Ed25519 keys, signing/verification, SHA-256, RFC 8032 and SHA-256 standard vectors, and independent Python/OpenSSL checks pass. Each other participant must establish the same for their language. |
| 1 | Grupna specifikacija / Group specification | **No — provisional completion only** | `SPEC.md` is preserved and still contains unresolved fields. [PROTOTYPE_DEFAULTS.md](PROTOTYPE_DEFAULTS.md) supplies explicit defaults and exact encodings; fixtures include keys, transactions, genesis, PoW-valid blocks, and parcel outcomes. These defaults were chosen for the working prototype under the implementation request. They are not a group-authored/approved specification. The group must resolve/approve the shared specification and verify the vectors in its own implementations. |
| 2 | Samostalni čvor / Standalone node | **Yes — for the Rust prototype** | The node passes its provisional protocol fixtures, mines at the real 23-bit target, assigns parcels, validates state transitions/replay, and exports the canvas. Painting and transfers work. Shared-vector approval under milestone 1 remains outstanding for workshop interoperability. |
| 3 | Prvi kontakt / First contact | **No — Rust-to-Rust equivalent passes** | Local nodes exchange transactions and blocks with delay/loss and converge on ownership, colors, counters, and tip. The original milestone requires two different people's implementations; that test has not been demonstrated. Connect to a teammate's implementation after agreeing on the specification/vectors. |
| 4 | Cela mreža / Whole network | **No — local mesh equivalent passes** | A three-process Rust full mesh passes concurrent mining, fork convergence, and late join with lag enabled. A full-group mesh of the independently implemented nodes, including their late join/fork behavior, has not been demonstrated. |
| 5 | Padovi / Crashes | **No — local crash/restart equivalent passes** | The demo kills one Rust process using SIGKILL, verifies continued work by survivors, restarts with the same seed, and downloads/validates the chain again. Fault tests also cover a disappearing source during catch-up. Repeat the scenario across the full group and for each implementation before marking the group milestone complete. |
| 6 | Završni test / Final test | **No — mentor acceptance pending** | The automated local Rust scenario passes all five behavioral steps below. There is no evidence of the complete team scenario being observed and accepted by the mentor. |

**Summary:** milestones **0 and 2 are complete for this Rust prototype**. Milestone **1 needs group agreement**. Milestones **3–5 have passing local Rust equivalents but still need the required team/interoperability demonstrations**. Milestone **6 needs the mentor-observed final test**.

## Original final-test scenario

All five steps are implemented and verified locally by [scripts/workshop-demo](scripts/workshop-demo), using real 23-bit PoW and default-on lag. Group/mentor completion remains pending.

| Step | Original acceptance action | Local Rust result | Group/mentor result |
| --- | --- | --- | --- |
| 1 | Start nodes with lag; mine, paint, and transfer | **Passed** | Not demonstrated |
| 2 | Forcibly kill one process while peers continue | **Passed — SIGKILL** | Not demonstrated |
| 3 | Remaining nodes continue without getting stuck | **Passed** | Not demonstrated |
| 4 | Restart the killed node and let it catch up | **Passed — same identity, chain fetched from survivors** | Not demonstrated |
| 5 | Stop activity/mining; reach identical tip and canvas | **Passed — tip, full ownership/color/counter state, and PPM compared** | Not demonstrated |

The final audited local run converged at height 22; its recorded tip/state hashes and test commands are in [AUDIT_REPORT.md](AUDIT_REPORT.md).

## All eight stretch goals

“Partial” below means some supporting functionality exists; it does **not** mean the original stretch goal is complete.

| # | Original stretch goal | Completed? | Current capability and remaining work |
| --- | --- | --- | --- |
| 1 | Povezivanje sa drugom grupom / Connect with another group | **No** | No other-group integration is demonstrated. Chain/version identification rejects a different genesis or protocol version. Coordinate compatible rules and the mentor's connection details, then demonstrate cross-group operation. |
| 2 | Atomska razmena / Atomic parcel swap | **No — not implemented** | Transactions support paint or a single-owner transfer. There is no atomic two-party swap transaction, joint authorization, or all-or-nothing exchange protocol. Implement and validate it on every group node. |
| 3 | Prilagođavanje težine / Difficulty adjustment | **No — not implemented** | Difficulty is fixed at 23 leading zero bits. Measurements do not change it. Define shared adjustment rules, implement validation/mining behavior, and verify across the group. |
| 4 | Delegirano bojenje / Delegated painting | **No — not implemented** | Only the current parcel owner can authorize a paint transaction. No delegate permission, revocation, or delegated authorization format exists. |
| 5 | Retka mreža / Sparse topology | **Partial support — not completed** | Received valid blocks/transactions are forwarded and peers are explicitly configurable, so basic relay machinery exists. The delivered topology and validated demo are full mesh; no dedicated sparse-topology scenario or full-group sparse interoperability has been verified. Test line/ring/partial-peer configurations and recovery, then validate on every group node. |
| 6 | Preživeti mentorov napad lažnim ili neispravnim porukama / Survive mentor's fake or malformed messages | **Partial support — not completed** | Strict framing/hex/JSON decoding, size limits, bounded queues/pending storage, bad handshake rejection, invalid transaction/block checks, and prompt socket closure have regression coverage. The mentor's attack scenario and the behavior of every group implementation have not been demonstrated. Existing history snapshots also grow without pruning. |
| 7 | Preživeti podelu mreže / Survive a network partition | **Partial support — not completed** | Fork choice, side branches, reorganization, and restoration of ownership/colors/replay state are implemented and tested. Concurrent connected miners settle to one branch. There is no recorded test that explicitly isolates two network halves, mines independently in both, reconnects them, and verifies full convergence; run that scenario across the group. |
| 8 | Čuvati lanac na disku i oporaviti se posle ponovnog pokretanja / Store chain on disk and recover after restart | **No — not implemented** | The identity seed persists, but block history and state snapshots are held in memory. Restart recovery downloads history from surviving peers. `state.json`, `status.json`, and `canvas.ppm` are diagnostic exports and are not loaded as durable chain storage. Restarting all nodes with no survivor loses the chain. |

**Completed stretch goals: 0 of 8.** Three have supporting functionality (sparse relay, malformed-message handling, and fork recovery), but none has the required demonstrated completion across every group node.

## Required functionality 1–9

The original document separately lists nine mandatory capabilities. All nine are implemented and locally verified in this Rust prototype; team-level completion remains subject to milestones 1 and 3–6 above.

| # | Required capability | Rust prototype status / evidence |
| --- | --- | --- |
| 1 | Generate keys, sign and verify | **Complete locally** — persistent private seed, Ed25519, standard/project vectors and independent checks |
| 2 | PoW mining and hash-derived parcel rewards | **Complete locally** — real 23-bit mining, deterministic allocation, occupied-candidate cycles and exhaustion tests; 100-block benchmarks |
| 3 | Block/transaction validation and replay protection | **Complete locally** — signatures, ownership, per-parcel counters, atomic block transitions, invalid/duplicate rejection |
| 4 | Exchange transactions/blocks; direct full mesh | **Complete locally** — lagged three-process Rust mesh; independent team implementations still need interoperability testing |
| 5 | Fork resolution and late catch-up | **Complete locally** — work/tip-hash selection, reorganization, missing ancestors, retries and late-join demonstration |
| 6 | Sudden peer failure/departure | **Complete locally** — SIGKILL/restart and disconnect-during-sync tests; chain recovery requires a surviving peer |
| 7 | Painting and ownership transfer | **Complete locally** — signed CLI transactions, pending counters, confirmation and reorganization tests |
| 8 | Display the canvas somehow | **Complete locally** — deterministic 64×32 PPM export; browser UI absent |
| 9 | Default-on random network delay/loss, with disable option | **Complete locally** — per-session/message delay 20–250 ms and 10% loss; `--no-lag` explicitly disables simulation; local mesh demonstration keeps it enabled |

## Work needed for workshop completion

1. Agree on the shared specification and exact vectors; replace provisional defaults where the group chooses differently.
2. Verify those vectors in every language and connect two independently implemented nodes with lag enabled.
3. Run the complete group mesh, fork/late-join scenario, and crash/restart checks for each participant's implementation.
4. Perform the final five-step scenario while the mentor observes.
5. Pursue stretch goals only as group-wide features, recording explicit tests before marking any complete.
