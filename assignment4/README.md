# Assignment 4

A Token-2022 remittance stablecoin: transfer fees, KYC-gated freezing, a
seizure authority, and confidential transfers. Based on the brief modeled on
[Princeadxisrael/token-22-turbin3-q3-](https://github.com/Princeadxisrael/token-22-turbin3-q3-/tree/master/programs/t22/src).

Program ID (devnet): `GSzcStei2DBZRXY7Vyw6pTDbcW2n6VDskMpHbBaXwB3p`

## Tasks

1. `initialize_mint` — a mint stacking `TransferFeeConfig`, `MetadataPointer`
   (pointed at itself), `DefaultAccountState::Frozen`, and
   `MintCloseAuthority`, sized via `try_calculate_account_len`, every
   extension-init instruction ordered before `InitializeMint2`.
2. `transfer_with_fee` — uses `transfer_checked_with_fee`; the fee is
   recomputed every call from `TransferFeeConfig::calculate_epoch_fee`
   against the live `Clock`, never cached.
3. Mint/account state is read only via `StateWithExtensions`, never a raw
   unpack — see `instructions/transfer.rs`.
4. `thaw_account` — freeze-authority-gated KYC unfreeze of one account,
   separate from the mint's default account state.
5. `initialize_mint_confidential` — re-issues the mint with
   `PermanentDelegate` (seizure) and `ConfidentialTransferMint` (manual
   approve policy). Confidential transfers can't be added to an
   already-initialized mint, so this is a fresh mint carrying the same
   task-1 extensions forward.
6. Full confidential lifecycle: `configure_confidential_account`,
   `deposit_confidential`, `apply_pending_balance`, `confidential_transfer`,
   `withdraw_confidential`.

## Two gaps this surfaced

- **Task 5**: token-2022 rejects `InitializeMint2` if a mint has
  `TransferFeeConfig` and `ConfidentialTransferMint` without a third
  extension, `ConfidentialTransferFeeConfig` — an unencrypted fee would leak
  the hidden transfer amount, so the withheld fee needs its own ElGamal key
  too. See `instructions/create_mint_confidential.rs`.
- **Task 6**: since the mint carries `TransferFeeConfig` forward, a plain
  confidential `Transfer` is rejected outright — every transfer has to go
  through `TransferWithFee`, five proofs instead of three (adds a fee-sigma
  proof and a fee-ciphertext-validity proof, widens the range proof to
  batched-U256). See `instructions/confidential/transfer.rs`.

## Layout

- `programs/stablecoin` — the Anchor program.
- `client/` — a standalone Rust crate (`cargo run --bin demo`) that drives
  every instruction on devnet with real transactions, including real ZK
  proof generation/verification for task 6.

## Tests

`programs/stablecoin/tests/stablecoin.rs` uses LiteSVM, which bundles a real
Token-2022 program build, covering tasks 1–5:

- `initialize_mint_stacks_all_four_extensions`
- `new_accounts_are_born_frozen_until_the_freeze_authority_thaws_them`
- `transfer_with_fee_withholds_the_current_epochs_fee`
- `transfer_with_fee_caps_at_the_maximum_fee`
- `reissued_mint_carries_the_base_extensions_forward_and_adds_seizure_plus_confidential`

Task 6's `ConfigureAccount`/`Transfer`/`Withdraw` need a real ZK ElGamal
proof program, which LiteSVM doesn't have — that lifecycle is exercised
against devnet instead, via `client/`.

```bash
anchor build   # produces target/deploy/stablecoin.so
cargo test -p stablecoin --test stablecoin
```

## Running the devnet client

```bash
anchor deploy           # from assignment4/, deploys per Anchor.toml
cd client
cargo run --bin demo
```

The demo funds two throwaway wallets from your configured keypair, then runs
every task in order against devnet.

### Devnet run

Tasks 1–5 and task 6 steps 1–3 (`ConfigureAccount`, `DepositConfidentialTokens`,
`ApplyPendingBalance`) came back clean end to end:

- mint: `7icCmY1ZJmnqHH5Lg2wWexZtmj2sz8wyP9NMrsWoYdps`, `initialize_mint` tx
  [`2hncAibq...`](https://explorer.solana.com/tx/2hncAibqFo9NQFpybcr4LXEx8cx7CruFVknJQ4eMWtV4YTcFKfaLmACNViMbkHqiD4LJxYPvs6Ci3SqQWcd61sub?cluster=devnet)
- `transfer_with_fee` tx
  [`3Nbtq9RU...`](https://explorer.solana.com/tx/3Nbtq9RU6LCi114BsjmxgdXfjAyF1QBhma3fnnxRiiiHkfSTDmUAhwXuRs7QutZs2vvV4G18N6XyU3UF3KwgAWkT?cluster=devnet) —
  alice 900,000 / bob 99,000 (100,000 minus the 1% fee), matching the LiteSVM
  test's math
- confidential mint: `ymu3MZRuGzoxbDAuDpjZqDY4wquUfwipjV1vjZhqfVK`,
  `initialize_mint_confidential` tx
  [`2vc4Mcmg...`](https://explorer.solana.com/tx/2vc4Mcmg4Wy8kB533pJqXAGrAbRCpViewqMQFMtiZCuXzdo2sAiHvs2q6tjWBqZgVAoFWyuf1tctJy5mJ4Fu35Do?cluster=devnet)
- `ConfigureAccount` for alice and bob, each gated by a real
  `PubkeyValidityProof` verified on-chain, plus `ApproveAccount` for both
  (manual approve policy) — 10 transactions, first one
  [`2QSwEvbi...`](https://explorer.solana.com/tx/2QSwEvbi9JJZgKVSrdU6Vq2yJRNs4MBKLy2GuHhvdhWjEgtGoyjr1TnCj823TJ6X8QrERLTYRmHHjvxtB6qcUgqB?cluster=devnet)
- `deposit_confidential` tx
  [`5gqJb4ng...`](https://explorer.solana.com/tx/5gqJb4ngsE8atADramFo4Qhrh4pcBVED5dKjxkqCEvdwVukEkpjeWvHMkZFmvWiriXcrNVMpK1PkN7UuBrnUjJgs?cluster=devnet)
- `apply_pending_balance` tx
  [`9yBWKQm1...`](https://explorer.solana.com/tx/9yBWKQm1p7m2zMYc7rRaNxM1DnsjsCcmaeETvrRNv3epBoE1F1oAuVFB1ZfAsXs9MsH4pmgoaUQQxt4pgZAYppR?cluster=devnet)

Step 4 (confidential `TransferWithFee`) got 19 transactions into uploading
and verifying its five proofs — including most of the way through the
batched U256 range proof, staged via an `spl-record` account since it's too
large to embed inline — before a run was stopped on a transient RPC
timeout, not a program or proof error. Step 5 (`WithdrawConfidentialTokens`)
wasn't reached in that run; the instruction is implemented the same way,
just with two proofs instead of five.

Along the way, running this against a live cluster (rather than just
LiteSVM) also turned up: an ecosystem crate-version split that changed the
ZK proof transcript format, `ConfigureAccount` needing a prior `Reallocate`
that it doesn't do for you, and the default 200k compute-unit budget being
too small for the heavier proof verifications.
