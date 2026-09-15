# heir-sweep-poc
Proof-of-concept for one specific question: **can a Solana wallet owner
presign a single durable-nonce transaction, today, that a beneficiary
submits months later to sweep the owner's *live* balance — with no amount
ever frozen in the signature, and no on-chain vault/delegate authority set
up in advance?**

See `../deliverable-1-loi.md` for what this means for the Heirloom design.
Short answer from running this: **half true, half a real problem.** The
"amount isn't frozen" half works exactly as claimed. But the mechanism has a
fatal, previously-undocumented failure mode: a single premature submission
attempt — by anyone who has the bytes, not just the beneficiary, and even
though it fails — permanently destroys the only valid copy of the owner's
authorization. The owner, by the entire premise of an inheritance product,
isn't around to sign a replacement.

## What's here

- `src/lib.rs` — a minimal native (non-Anchor) Solana program: `Initialize`,
  `CheckIn`, and the disputed `HeirSweep` instruction (no amount parameter;
  reads `owner.lamports()` live, CPIs to the System Program to transfer it,
  gated by `Clock >= last_checkin + timeout_secs`).
- `tests/integration.rs` — fast in-process tests (via `solana-program-test`)
  proving the vault-gate + live-balance-CPI logic in isolation. **Durable
  nonce transactions are deliberately not tested here** — see below.
- `examples/durable_nonce_demo.rs` — the actual proof, run against a real
  `solana-test-validator`. Two scenarios; both matter.

## Why the durable-nonce part needs a real validator

`solana-program-test`'s in-process `BanksServer` can't submit a durable-nonce
transaction at all — it hard-panics:

```
thread panicked at solana-banks-server-2.3.13/src/banks_server.rs:329:
called `Option::unwrap()` on a `None` value
```

That line does `bank.get_blockhash_last_valid_block_height(recent_blockhash).unwrap()`,
unconditionally assuming `recent_blockhash` is a real, currently-queued
blockhash. For a durable-nonce transaction, `recent_blockhash` is the nonce
account's stored value instead, which was never in that queue — so the
lookup returns `None` and the test harness crashes. This is a real gap in
the test tooling, not a Solana protocol limitation, but it means this
specific mechanism can only be honestly tested against something that
behaves like the real runtime.

## Reproducing

```bash
cargo build-sbf
solana-test-validator --reset --quiet &
solana program deploy target/deploy/heir_sweep_poc.so \
    --program-id target/deploy/heir_sweep_poc-keypair.json \
    --url http://127.0.0.1:8899
cargo run --example durable_nonce_demo -- <PROGRAM_ID_FROM_ABOVE>
```

(`cargo test` runs the fast in-process tests, no validator needed.)

## Results

**Scenario A — sign once, hold, claim after timeout, balance changes in the
meantime.** Owner airdrops itself more SOL *after* signing the presigned
tx. The tx is submitted for the first time only after the timeout, with no
prior attempts. It succeeds, and the beneficiary receives the live balance
(2,774,077,680 lamports: the original 2 SOL + a later 0.777 SOL airdrop,
minus fees) — not the 2 SOL that existed at signing time. **Confirms the
core claim: no amount is frozen in the signature.**

**Scenario B — one early, failed submission attempt.** A copy of the exact
same presigned bytes is submitted early (before the timeout), with client-side
preflight simulation skipped — i.e. exactly what a naive keeper/relayer script,
or anyone malicious who has a copy of the bytes, would do to try to claim as
soon as possible, or to grief. The transaction lands on-chain and fails
(`Custom(4)`, the timeout gate), but the nonce account's stored value still
changes:

```
nonce after the FAILED early attempt = C75M7HJrG8xV7yiQW8KHgMoSSRx8fYTkHj9YRys97jSM  (changed: true)
```

This is documented, intentional Solana behavior — a nonce transaction's
`AdvanceNonceAccount` instruction commits even when a later instruction in
the same transaction fails, specifically so a nonce tx can't be replayed for
free. But it means the *only* valid signed copy of the owner's authorization
is now permanently dead. Resubmitting the identical original bytes after the
real timeout elapses fails with `Blockhash not found` — forever. The owner
cannot sign a replacement; that's the entire premise of the product.

## Why this matters more than the "frozen amount" objection

A bounded/unbounded SPL `approve()`-style delegation (Heirloom's current
default path for SPL/Token-2022 assets) has no equivalent failure mode: a
premature `claim_delegated_assets` call just returns an error and changes
nothing. You can retry it as many times as you want, forever, for free. A
presigned bearer transaction cannot make that guarantee — the very
durable-nonce trick that lets it stay valid indefinitely is also what makes
one bad attempt unrecoverable. That asymmetry is the real reason to prefer
an on-chain delegated-authority model over a presigned-transaction model for
native SOL, not the "you can't presign a variable amount" argument the
original draft of this LOI made (which this POC shows is simply wrong).

## What would need to be true to use this safely

Native SOL has no SPL-style `approve()` equivalent, so *some* version of
"capture authorization now, exercise it later" is unavoidable for it. A
presigned durable-nonce tx can still be part of the answer, but only with a
guard against exactly this failure mode — for example, routing every
submission through a single trusted relay that reads the vault's gate
on-chain before ever broadcasting (defeats "anyone can submit it," reduces
but doesn't remove the risk), or accepting that native SOL inheritance in
Heirloom requires an actual PDA-owned vault deposit (the existing opt-in
custodial path) rather than a leave-it-in-your-own-wallet default. That
tradeoff is the open decision this POC was built to inform.
