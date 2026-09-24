use anchor_lang::prelude::*;

#[error_code]
pub enum StablecoinError {
    #[msg("Mint is missing the TransferFeeConfig extension")]
    MissingTransferFeeConfig,
    #[msg("Fee overflowed while computing the current epoch's transfer fee")]
    FeeCalculationOverflow,
    #[msg("Token-2022 CPI returned an unexpected account count")]
    InvalidAccountList,
}
