//! Tests for tasks 1-5, run with LiteSVM, which bundles a real Token-2022
//! program build. Task 6's confidential lifecycle needs a ZK proof program
//! LiteSVM doesn't have, so that's exercised against devnet instead -- see
//! `../../client`.
//!
//! Run `anchor build` first so `target/deploy/stablecoin.so` exists, then
//! `cargo test`.

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    spl_associated_token_account_interface::{
        instruction::create_associated_token_account,
        address::get_associated_token_address_with_program_id,
    },
    spl_token_2022_interface::{
        extension::{
            confidential_transfer::ConfidentialTransferMint,
            default_account_state::DefaultAccountState,
            metadata_pointer::MetadataPointer,
            mint_close_authority::MintCloseAuthority,
            permanent_delegate::PermanentDelegate,
            transfer_fee::{TransferFeeAmount, TransferFeeConfig},
            BaseStateWithExtensions, StateWithExtensions,
        },
        state::{Account as TokenAccount, AccountState, Mint},
    },
};

const STABLECOIN_SO: &[u8] =
    include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/stablecoin.so"));

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

fn token_program_id() -> Pubkey {
    spl_token_2022_interface::id()
}

fn send(svm: &mut LiteSVM, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    svm.send_transaction(tx).unwrap();
}

#[allow(clippy::result_large_err)]
fn try_send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> Result<(), litesvm::types::FailedTransactionMetadata> {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    svm.send_transaction(tx).map(|_| ())
}

fn create_ata(svm: &mut LiteSVM, payer: &Keypair, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    let ix = create_associated_token_account(&payer.pubkey(), owner, mint, &token_program_id());
    send(svm, &[ix], payer, &[payer]);
    get_associated_token_address_with_program_id(owner, mint, &token_program_id())
}

fn mint_to(svm: &mut LiteSVM, authority: &Keypair, mint: &Pubkey, dest: &Pubkey, amount: u64) {
    let ix = spl_token_2022_interface::instruction::mint_to(
        &token_program_id(),
        mint,
        dest,
        &authority.pubkey(),
        &[],
        amount,
    )
    .unwrap();
    send(svm, &[ix], authority, &[authority]);
}

fn read_mint(svm: &LiteSVM, mint: &Pubkey) -> StateWithExtensions<'static, Mint> {
    let data = svm.get_account(mint).expect("mint should exist").data;
    // `unpack` borrows; leak the Vec for the lifetime of this short-lived
    // test process so the returned view can outlive this function.
    let data: &'static [u8] = Box::leak(data.into_boxed_slice());
    StateWithExtensions::<Mint>::unpack(data).unwrap()
}

fn read_token_account(svm: &LiteSVM, account: &Pubkey) -> StateWithExtensions<'static, TokenAccount> {
    let data = svm.get_account(account).expect("token account should exist").data;
    let data: &'static [u8] = Box::leak(data.into_boxed_slice());
    StateWithExtensions::<TokenAccount>::unpack(data).unwrap()
}

// --- instruction builders ---------------------------------------------

#[allow(clippy::too_many_arguments)]
fn initialize_mint_ix(
    payer: &Pubkey,
    mint: &Pubkey,
    decimals: u8,
    mint_authority: Pubkey,
    freeze_authority: Pubkey,
    transfer_fee_config_authority: Pubkey,
    withdraw_withheld_authority: Pubkey,
    transfer_fee_basis_points: u16,
    maximum_fee: u64,
    close_authority: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::InitializeMint {
            decimals,
            mint_authority,
            freeze_authority,
            transfer_fee_config_authority,
            withdraw_withheld_authority,
            transfer_fee_basis_points,
            maximum_fee,
            close_authority,
        }
        .data(),
        stablecoin::accounts::InitializeMint {
            payer: *payer,
            mint: *mint,
            token_program: token_program_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn transfer_with_fee_ix(
    authority: &Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
    mint: &Pubkey,
    amount: u64,
) -> Instruction {
    Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::TransferWithFee { amount }.data(),
        stablecoin::accounts::TransferWithFee {
            authority: *authority,
            source: *source,
            destination: *destination,
            mint: *mint,
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    )
}

fn thaw_account_ix(freeze_authority: &Pubkey, token_account: &Pubkey, mint: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::ThawAccount {}.data(),
        stablecoin::accounts::ThawAccount {
            freeze_authority: *freeze_authority,
            token_account: *token_account,
            mint: *mint,
            token_program: token_program_id(),
        }
        .to_account_metas(None),
    )
}

#[allow(clippy::too_many_arguments)]
fn initialize_mint_confidential_ix(
    payer: &Pubkey,
    mint: &Pubkey,
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
) -> Instruction {
    Instruction::new_with_bytes(
        stablecoin::id(),
        &stablecoin::instruction::InitializeMintConfidential {
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
        }
        .data(),
        stablecoin::accounts::InitializeMintConfidential {
            payer: *payer,
            mint: *mint,
            token_program: token_program_id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

// --- fixture -------------------------------------------------------------

struct Env {
    svm: LiteSVM,
    payer: Keypair,
    issuer: Keypair,
    alice: Keypair,
    bob: Keypair,
}

fn setup() -> Env {
    let mut svm = LiteSVM::new();
    svm.add_program(stablecoin::id(), STABLECOIN_SO).unwrap();

    let payer = Keypair::new();
    let issuer = Keypair::new();
    let alice = Keypair::new();
    let bob = Keypair::new();
    for kp in [&payer, &issuer, &alice, &bob] {
        svm.airdrop(&kp.pubkey(), 100 * LAMPORTS_PER_SOL).unwrap();
    }

    Env { svm, payer, issuer, alice, bob }
}

/// Creates a task-1 mint (1% transfer fee, max fee 1_000 units, 6 decimals)
/// with `issuer` holding every authority, then thaws + funds `alice`'s ATA
/// with `amount` units so tests can transfer right away.
fn mint_and_fund_alice(env: &mut Env, amount: u64) -> (Pubkey, Pubkey, Pubkey) {
    let mint = Keypair::new();
    let ix = initialize_mint_ix(
        &env.payer.pubkey(),
        &mint.pubkey(),
        6,
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        100,   // 1%
        1_000, // max fee
        env.issuer.pubkey(),
    );
    send(&mut env.svm, &[ix], &env.payer, &[&env.payer, &mint]);

    let alice_ata = create_ata(&mut env.svm, &env.payer, &env.alice.pubkey(), &mint.pubkey());
    let bob_ata = create_ata(&mut env.svm, &env.payer, &env.bob.pubkey(), &mint.pubkey());

    for ata in [&alice_ata, &bob_ata] {
        let thaw = thaw_account_ix(&env.issuer.pubkey(), ata, &mint.pubkey());
        send(&mut env.svm, &[thaw], &env.issuer, &[&env.issuer]);
    }

    mint_to(&mut env.svm, &env.issuer, &mint.pubkey(), &alice_ata, amount);

    (mint.pubkey(), alice_ata, bob_ata)
}

// --- tests ----------------------------------------------------------------

#[test]
fn initialize_mint_stacks_all_four_extensions() {
    let mut env = setup();
    let mint = Keypair::new();

    let ix = initialize_mint_ix(
        &env.payer.pubkey(),
        &mint.pubkey(),
        6,
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        250,     // 2.5%
        10_000,  // max fee
        env.issuer.pubkey(),
    );
    send(&mut env.svm, &[ix], &env.payer, &[&env.payer, &mint]);

    let mint_state = read_mint(&env.svm, &mint.pubkey());
    assert_eq!(mint_state.base.decimals, 6);
    assert_eq!(Option::<Pubkey>::from(mint_state.base.mint_authority), Some(env.issuer.pubkey()));
    assert_eq!(Option::<Pubkey>::from(mint_state.base.freeze_authority), Some(env.issuer.pubkey()));

    let fee_config = mint_state.get_extension::<TransferFeeConfig>().unwrap();
    assert_eq!(u16::from(fee_config.older_transfer_fee.transfer_fee_basis_points), 250);
    assert_eq!(u64::from(fee_config.older_transfer_fee.maximum_fee), 10_000);

    let metadata_pointer = mint_state.get_extension::<MetadataPointer>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(metadata_pointer.metadata_address),
        Some(mint.pubkey())
    );

    let default_state = mint_state.get_extension::<DefaultAccountState>().unwrap();
    assert_eq!(AccountState::try_from(default_state.state).unwrap(), AccountState::Frozen);

    let close_authority = mint_state.get_extension::<MintCloseAuthority>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(close_authority.close_authority),
        Some(env.issuer.pubkey())
    );
}

#[test]
fn new_accounts_are_born_frozen_until_the_freeze_authority_thaws_them() {
    let mut env = setup();
    let mint = Keypair::new();
    let ix = initialize_mint_ix(
        &env.payer.pubkey(),
        &mint.pubkey(),
        0,
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        0,
        0,
        env.issuer.pubkey(),
    );
    send(&mut env.svm, &[ix], &env.payer, &[&env.payer, &mint]);

    let alice_ata = create_ata(&mut env.svm, &env.payer, &env.alice.pubkey(), &mint.pubkey());
    let account_state = read_token_account(&env.svm, &alice_ata);
    assert_eq!(account_state.base.state, AccountState::Frozen);

    // Minting into a still-frozen account must fail.
    let mint_1 = spl_token_2022_interface::instruction::mint_to(
        &token_program_id(),
        &mint.pubkey(),
        &alice_ata,
        &env.issuer.pubkey(),
        &[],
        1,
    )
    .unwrap();
    assert!(try_send(&mut env.svm, &[mint_1], &env.issuer, &[&env.issuer]).is_err());

    // The KYC thaw path, separate from any mint-level default-state change.
    let thaw = thaw_account_ix(&env.issuer.pubkey(), &alice_ata, &mint.pubkey());
    send(&mut env.svm, &[thaw], &env.issuer, &[&env.issuer]);

    let account_state = read_token_account(&env.svm, &alice_ata);
    assert_eq!(account_state.base.state, AccountState::Initialized);

    // Now minting succeeds. A different amount than the failed attempt
    // above, so this is a distinct transaction (same amount + same
    // blockhash would be a byte-identical, and thus rejected, replay).
    let mint_2 = spl_token_2022_interface::instruction::mint_to(
        &token_program_id(),
        &mint.pubkey(),
        &alice_ata,
        &env.issuer.pubkey(),
        &[],
        5,
    )
    .unwrap();
    send(&mut env.svm, &[mint_2], &env.issuer, &[&env.issuer]);
    let account_state = read_token_account(&env.svm, &alice_ata);
    assert_eq!(account_state.base.amount, 5);
}

#[test]
fn transfer_with_fee_withholds_the_current_epochs_fee() {
    let mut env = setup();
    let (mint, alice_ata, bob_ata) = mint_and_fund_alice(&mut env, 1_000_000);

    // 1% of 100_000 = 1_000, under the 1_000-unit max fee cap.
    let ix = transfer_with_fee_ix(&env.alice.pubkey(), &alice_ata, &bob_ata, &mint, 100_000);
    send(&mut env.svm, &[ix], &env.alice, &[&env.alice]);

    let alice_state = read_token_account(&env.svm, &alice_ata);
    assert_eq!(alice_state.base.amount, 900_000);

    // The destination is credited amount-minus-fee; the fee itself sits,
    // still counted in bob's balance, flagged as withheld inside his own
    // TransferFeeAmount extension until the withdraw-withheld authority
    // harvests it.
    let bob_state = read_token_account(&env.svm, &bob_ata);
    assert_eq!(bob_state.base.amount, 99_000);

    let withheld = bob_state.get_extension::<TransferFeeAmount>().unwrap();
    assert_eq!(u64::from(withheld.withheld_amount), 1_000);
}

#[test]
fn transfer_with_fee_caps_at_the_maximum_fee() {
    let mut env = setup();
    let (mint, alice_ata, bob_ata) = mint_and_fund_alice(&mut env, 10_000_000);

    // 1% of 1_000_000 would be 10_000, but max_fee caps it at 1_000.
    let ix = transfer_with_fee_ix(&env.alice.pubkey(), &alice_ata, &bob_ata, &mint, 1_000_000);
    send(&mut env.svm, &[ix], &env.alice, &[&env.alice]);

    let bob_state = read_token_account(&env.svm, &bob_ata);
    assert_eq!(bob_state.base.amount, 999_000);
    let withheld = bob_state.get_extension::<TransferFeeAmount>().unwrap();
    assert_eq!(u64::from(withheld.withheld_amount), 1_000);
}

#[test]
fn reissued_mint_carries_the_base_extensions_forward_and_adds_seizure_plus_confidential() {
    let mut env = setup();
    let mint = Keypair::new();

    let ix = initialize_mint_confidential_ix(
        &env.payer.pubkey(),
        &mint.pubkey(),
        6,
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        env.issuer.pubkey(),
        100,
        1_000,
        env.issuer.pubkey(),
        env.issuer.pubkey(), // permanent delegate = issuer, for the test
        env.issuer.pubkey(), // confidential-transfer authority
        // Not a validated curve point at init time -- token-2022 only
        // checks that at proof-verification time, when fees are actually
        // withdrawn. A placeholder is fine for exercising mint creation.
        [7u8; 32],
    );
    send(&mut env.svm, &[ix], &env.payer, &[&env.payer, &mint]);

    let mint_state = read_mint(&env.svm, &mint.pubkey());

    // The task-1 set is still all there.
    assert!(mint_state.get_extension::<TransferFeeConfig>().is_ok());
    assert!(mint_state.get_extension::<MetadataPointer>().is_ok());
    assert!(mint_state.get_extension::<DefaultAccountState>().is_ok());
    assert!(mint_state.get_extension::<MintCloseAuthority>().is_ok());

    // Plus the two the task asked for...
    let delegate = mint_state.get_extension::<PermanentDelegate>().unwrap();
    assert_eq!(Option::<Pubkey>::from(delegate.delegate), Some(env.issuer.pubkey()));

    let confidential = mint_state.get_extension::<ConfidentialTransferMint>().unwrap();
    assert!(!bool::from(confidential.auto_approve_new_accounts));

    // ...and the one token-2022 forces once TransferFeeConfig and
    // ConfidentialTransferMint are both present: without this, InitializeMint2
    // itself fails with InvalidExtensionCombination (see the doc comment on
    // `handle_initialize_mint_confidential`).
    assert!(mint_state
        .get_extension::<spl_token_2022_interface::extension::confidential_transfer_fee::ConfidentialTransferFeeConfig>()
        .is_ok());
}
