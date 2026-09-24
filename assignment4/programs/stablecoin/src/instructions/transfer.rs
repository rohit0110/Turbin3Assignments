use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use spl_token_2022_interface::extension::transfer_fee::instruction::transfer_checked_with_fee;
use spl_token_2022_interface::extension::transfer_fee::TransferFeeConfig;
use spl_token_2022_interface::extension::{BaseStateWithExtensions, StateWithExtensions};
use spl_token_2022_interface::state::Mint;

use crate::error::StablecoinError;

// Task 2 + 3: fee is recomputed every call from TransferFeeConfig::calculate_epoch_fee
// against the live Clock (never cached), and mint state is read only via StateWithExtensions.
#[derive(Accounts)]
pub struct TransferWithFee<'info> {
    pub authority: Signer<'info>,

    #[account(mut)]
    /// CHECK: shape/ownership validated by the token-2022 CPI itself.
    pub source: UncheckedAccount<'info>,

    #[account(mut)]
    /// CHECK: shape/ownership validated by the token-2022 CPI itself.
    pub destination: UncheckedAccount<'info>,

    /// CHECK: read directly via `StateWithExtensions` in the handler.
    pub mint: UncheckedAccount<'info>,

    /// CHECK: address-constrained to the real Token-2022 program.
    #[account(address = spl_token_2022_interface::id())]
    pub token_program: UncheckedAccount<'info>,
}

pub fn handle_transfer_with_fee(ctx: Context<TransferWithFee>, amount: u64) -> Result<()> {
    let mint_ai = ctx.accounts.mint.to_account_info();
    let (decimals, fee) = {
        let mint_data = mint_ai.try_borrow_data()?;
        let mint_state = StateWithExtensions::<Mint>::unpack(&mint_data)?;
        let decimals = mint_state.base.decimals;

        let transfer_fee_config = mint_state
            .get_extension::<TransferFeeConfig>()
            .map_err(|_| StablecoinError::MissingTransferFeeConfig)?;

        let epoch = Clock::get()?.epoch;
        let fee = transfer_fee_config
            .calculate_epoch_fee(epoch, amount)
            .ok_or(StablecoinError::FeeCalculationOverflow)?;
        (decimals, fee)
    };

    let token_program_id = spl_token_2022_interface::id();
    let ix = transfer_checked_with_fee(
        &token_program_id,
        ctx.accounts.source.key,
        ctx.accounts.mint.key,
        ctx.accounts.destination.key,
        ctx.accounts.authority.key,
        &[],
        amount,
        decimals,
        fee,
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.source.to_account_info(),
            mint_ai,
            ctx.accounts.destination.to_account_info(),
            ctx.accounts.authority.to_account_info(),
        ],
    )?;

    Ok(())
}
