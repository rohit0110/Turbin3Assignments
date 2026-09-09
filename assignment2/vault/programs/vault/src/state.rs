use anchor_lang::prelude::*;

// Tiny bookkeeping account. We only need to stash the two bumps so that
// every later instruction can re-derive the PDAs without re-searching.
#[account]
#[derive(InitSpace)]
pub struct VaultState {
    pub vault_bump: u8,
    pub state_bump: u8,
}
