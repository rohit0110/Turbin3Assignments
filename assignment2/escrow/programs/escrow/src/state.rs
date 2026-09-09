use anchor_lang::prelude::*;

// One of these per open trade. It remembers who opened it, which two mints
// are involved, and how much of mint_b the maker wants back. The tokens
// themselves live in a separate vault ATA owned by this account.
#[account]
#[derive(InitSpace)]
pub struct Escrow {
    // Maker-chosen number so one maker can run several escrows at once.
    pub seed: u64,
    pub maker: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    // How much of mint_b the taker has to hand over.
    pub receive: u64,
    pub bump: u8,
}
