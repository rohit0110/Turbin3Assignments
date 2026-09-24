use anchor_lang::prelude::*;

pub mod error;
pub mod instructions;
pub mod mint_setup;

pub use instructions::*;

declare_id!("GSzcStei2DBZRXY7Vyw6pTDbcW2n6VDskMpHbBaXwB3p");

#[program]
pub mod stablecoin {
    use super::*;

    // Task 1
    #[allow(clippy::too_many_arguments)]
    pub fn initialize_mint(
        ctx: Context<InitializeMint>,
        decimals: u8,
        mint_authority: Pubkey,
        freeze_authority: Pubkey,
        transfer_fee_config_authority: Pubkey,
        withdraw_withheld_authority: Pubkey,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
        close_authority: Pubkey,
    ) -> Result<()> {
        handle_initialize_mint(
            ctx,
            decimals,
            mint_authority,
            freeze_authority,
            transfer_fee_config_authority,
            withdraw_withheld_authority,
            transfer_fee_basis_points,
            maximum_fee,
            close_authority,
        )
    }

    // Task 2 + 3
    pub fn transfer_with_fee(ctx: Context<TransferWithFee>, amount: u64) -> Result<()> {
        handle_transfer_with_fee(ctx, amount)
    }

    // Task 4
    pub fn thaw_account(ctx: Context<ThawAccount>) -> Result<()> {
        handle_thaw_account(ctx)
    }

    // Task 5
    #[allow(clippy::too_many_arguments)]
    pub fn initialize_mint_confidential(
        ctx: Context<InitializeMintConfidential>,
        decimals: u8,
        mint_authority: Pubkey,
        freeze_authority: Pubkey,
        transfer_fee_config_authority: Pubkey,
        withdraw_withheld_authority: Pubkey,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
        close_authority: Pubkey,
        permanent_delegate: Pubkey,
        confidential_transfer_authority: Pubkey,
        withdraw_withheld_authority_elgamal_pubkey: [u8; 32],
    ) -> Result<()> {
        handle_initialize_mint_confidential(
            ctx,
            decimals,
            mint_authority,
            freeze_authority,
            transfer_fee_config_authority,
            withdraw_withheld_authority,
            transfer_fee_basis_points,
            maximum_fee,
            close_authority,
            permanent_delegate,
            confidential_transfer_authority,
            withdraw_withheld_authority_elgamal_pubkey,
        )
    }

    // Task 6
    pub fn configure_confidential_account(
        ctx: Context<ConfigureConfidentialAccount>,
        decryptable_zero_balance: [u8; 36],
        maximum_pending_balance_credit_counter: u64,
    ) -> Result<()> {
        handle_configure_confidential_account(
            ctx,
            decryptable_zero_balance,
            maximum_pending_balance_credit_counter,
        )
    }

    pub fn deposit_confidential(
        ctx: Context<DepositConfidential>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        handle_deposit_confidential(ctx, amount, decimals)
    }

    pub fn apply_pending_balance(
        ctx: Context<ApplyPendingBalance>,
        expected_pending_balance_credit_counter: u64,
        new_decryptable_available_balance: [u8; 36],
    ) -> Result<()> {
        handle_apply_pending_balance(
            ctx,
            expected_pending_balance_credit_counter,
            new_decryptable_available_balance,
        )
    }

    pub fn confidential_transfer(
        ctx: Context<ConfidentialTransfer>,
        new_source_decryptable_available_balance: [u8; 36],
        transfer_amount_auditor_ciphertext_lo: [u8; 64],
        transfer_amount_auditor_ciphertext_hi: [u8; 64],
    ) -> Result<()> {
        handle_confidential_transfer(
            ctx,
            new_source_decryptable_available_balance,
            transfer_amount_auditor_ciphertext_lo,
            transfer_amount_auditor_ciphertext_hi,
        )
    }

    pub fn withdraw_confidential(
        ctx: Context<WithdrawConfidential>,
        amount: u64,
        decimals: u8,
        new_decryptable_available_balance: [u8; 36],
    ) -> Result<()> {
        handle_withdraw_confidential(ctx, amount, decimals, new_decryptable_available_balance)
    }
}
