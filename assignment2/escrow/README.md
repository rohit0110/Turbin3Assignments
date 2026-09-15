# Escrow

A classic two-party SPL-token escrow. The maker locks some amount of token A and
says how much of token B they want for it. A taker can come along and fill that
order, or the maker can call it off and get their token A back.

Program ID: `C8Y9h7TEePGLvZK9YJBgRxAVdwEpVtv1r5duvXMtK7xt`

## State

```rust
pub struct Escrow {
    pub seed: u64,      // maker-chosen, lets one maker run several escrows
    pub maker: Pubkey,
    pub mint_a: Pubkey, // what the maker deposited
    pub mint_b: Pubkey, // what the maker wants back
    pub receive: u64,   // how much of mint_b the taker must pay
    pub bump: u8,
}
```

- `escrow` PDA: `[b"escrow", maker, seed.to_le_bytes()]`
- `vault`: the associated token account for `mint_a` owned by the `escrow` PDA. The
  deposited tokens live here until the trade settles.

## Instructions

### `make(seed, deposit, receive)` — signed by maker
Creates the `escrow` record and its `vault`, writes down the terms, and moves
`deposit` of `mint_a` from the maker into the vault.

### `update(receive)` — signed by maker
Re-prices an open trade. Only changes `escrow.receive`; the locked tokens stay
put. `has_one = maker` makes sure nobody else can touch it.

### `take()` — signed by taker
Settles the trade in one transaction:
1. taker pays `escrow.receive` of `mint_b` to the maker,
2. the vault releases all of `mint_a` to the taker (the `escrow` PDA signs),
3. the empty vault is closed and the `escrow` account is closed, both refunding
   rent to the maker.

The taker's `mint_a` account and the maker's `mint_b` account are created with
`init_if_needed` if they don't exist yet.

### `refund()` — signed by maker
The maker calls off the trade: the vault sends all of `mint_a` back to the
maker's token account, then the vault and the `escrow` account are closed.

> `Take`'s accounts are `Box`ed — it validates a lot of token accounts at once
> and that is enough to blow the 4KB instruction stack frame otherwise.

## Tests

`programs/escrow/tests/escrow.rs` uses LiteSVM, which bundles the SPL Token and
Associated Token programs, so the tests mint real tokens and move them around.
Setup gives the maker 1,000,000 of `mint_a` and the taker 1,000,000 of `mint_b`.

- `make_locks_the_deposit_and_records_the_terms` — vault holds the deposit, maker's account is drained, `Escrow` fields are correct.
- `update_rewrites_the_receive_amount` — `receive` goes from 500,000 to 250,000.
- `take_completes_the_swap_and_closes_everything` — taker ends with all of `mint_a`, maker is paid the agreed `mint_b`, `escrow` and `vault` are gone.
- `refund_gives_the_maker_their_tokens_back` — maker's `mint_a` balance is whole again, `escrow` and `vault` are gone.

```bash
anchor build   # produces target/deploy/escrow.so
cargo test
```
