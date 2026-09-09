use anchor_lang::prelude::*;

// Seed prefix for the escrow state PDA: ["escrow", maker, seed_le_bytes].
#[constant]
pub const ESCROW_SEED: &[u8] = b"escrow";
