use anchor_lang::prelude::*;

#[error_code]
pub enum VaultError {
    #[msg("Not enough lamports in the vault to cover this withdrawal")]
    InsufficientBalance,
}
