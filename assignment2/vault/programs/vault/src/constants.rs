use anchor_lang::prelude::*;

// Seed for the per-user state account that remembers our bumps.
#[constant]
pub const VAULT_STATE_SEED: &[u8] = b"state";

// Seed for the actual vault: a plain SystemAccount PDA that just holds lamports.
#[constant]
pub const VAULT_SEED: &[u8] = b"vault";
