use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::solana_program::program::invoke;
use anchor_lang::solana_program::system_instruction;
use spl_token_2022_interface::extension::ExtensionType;
use spl_token_2022_interface::instruction::initialize_mint2;
use spl_token_2022_interface::state::Mint;

// Sizes the mint via try_calculate_account_len, runs every extension-init
// CPI (each just touches [mint]), then calls InitializeMint2 last.
#[allow(clippy::too_many_arguments)]
pub fn create_mint_with_extensions<'info>(
    payer: &AccountInfo<'info>,
    mint: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    token_program_id: &Pubkey,
    extension_types: &[ExtensionType],
    extension_init_ixs: &[Instruction],
    mint_authority: &Pubkey,
    freeze_authority: Option<&Pubkey>,
    decimals: u8,
) -> Result<()> {
    let space = ExtensionType::try_calculate_account_len::<Mint>(extension_types)?;
    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(space);

    invoke(
        &system_instruction::create_account(
            payer.key,
            mint.key,
            lamports,
            space as u64,
            token_program_id,
        ),
        &[payer.clone(), mint.clone(), system_program.clone()],
    )?;

    for ix in extension_init_ixs {
        invoke(ix, std::slice::from_ref(mint))?;
    }

    invoke(
        &initialize_mint2(token_program_id, mint.key, mint_authority, freeze_authority, decimals)?,
        std::slice::from_ref(mint),
    )?;

    Ok(())
}
