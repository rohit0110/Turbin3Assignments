//! End-to-end tests for the vault program, driven with LiteSVM.
//!
//! Run `anchor build` first so that `target/deploy/vault.so` exists, then
//! `cargo test`.

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    litesvm::LiteSVM,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

// The compiled program, baked into the test binary at build time.
const PROGRAM_SO: &[u8] =
    include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/vault.so"));

const ONE_SOL: u64 = 1_000_000_000;

struct TestCtx {
    svm: LiteSVM,
    user: Keypair,
    vault_state: Pubkey,
    vault: Pubkey,
    state_bump: u8,
    vault_bump: u8,
}

// Boot a fresh VM with the program loaded and a funded user, plus the two
// PDAs everyone is going to need.
fn setup() -> TestCtx {
    let program_id = vault::id();
    let mut svm = LiteSVM::new();
    svm.add_program(program_id, PROGRAM_SO).unwrap();

    let user = Keypair::new();
    svm.airdrop(&user.pubkey(), 100 * ONE_SOL).unwrap();

    let (vault_state, state_bump) = Pubkey::find_program_address(
        &[vault::constants::VAULT_STATE_SEED, user.pubkey().as_ref()],
        &program_id,
    );
    let (vault, vault_bump) = Pubkey::find_program_address(
        &[vault::constants::VAULT_SEED, vault_state.as_ref()],
        &program_id,
    );

    TestCtx { svm, user, vault_state, vault, state_bump, vault_bump }
}

// Sign and fire a single instruction from the user, returning whether it stuck.
fn send(ctx: &mut TestCtx, ix: Instruction) -> bool {
    let blockhash = ctx.svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&ctx.user.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&ctx.user]).unwrap();
    ctx.svm.send_transaction(tx).is_ok()
}

fn initialize_ix(ctx: &TestCtx) -> Instruction {
    Instruction::new_with_bytes(
        vault::id(),
        &vault::instruction::Initialize {}.data(),
        vault::accounts::Initialize {
            user: ctx.user.pubkey(),
            vault_state: ctx.vault_state,
            vault: ctx.vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn deposit_ix(ctx: &TestCtx, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        vault::id(),
        &vault::instruction::Deposit { amount }.data(),
        vault::accounts::Deposit {
            user: ctx.user.pubkey(),
            vault_state: ctx.vault_state,
            vault: ctx.vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn withdraw_ix(ctx: &TestCtx, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        vault::id(),
        &vault::instruction::Withdraw { amount }.data(),
        vault::accounts::Withdraw {
            user: ctx.user.pubkey(),
            vault_state: ctx.vault_state,
            vault: ctx.vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn close_ix(ctx: &TestCtx) -> Instruction {
    Instruction::new_with_bytes(
        vault::id(),
        &vault::instruction::Close {}.data(),
        vault::accounts::Close {
            user: ctx.user.pubkey(),
            vault_state: ctx.vault_state,
            vault: ctx.vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn read_state(ctx: &TestCtx) -> vault::state::VaultState {
    let acct = ctx.svm.get_account(&ctx.vault_state).unwrap();
    vault::state::VaultState::try_deserialize(&mut &acct.data[..]).unwrap()
}

#[test]
fn initialize_records_the_bumps() {
    let mut ctx = setup();
    let ix = initialize_ix(&ctx);
    assert!(send(&mut ctx, ix), "initialize should succeed");

    let state = read_state(&ctx);
    assert_eq!(state.state_bump, ctx.state_bump);
    assert_eq!(state.vault_bump, ctx.vault_bump);
}

#[test]
fn deposit_moves_lamports_into_the_vault() {
    let mut ctx = setup();
    let ix = initialize_ix(&ctx);
    send(&mut ctx, ix);

    let amount = 2 * ONE_SOL;
    let ix = deposit_ix(&ctx, amount);
    assert!(send(&mut ctx, ix), "deposit should succeed");

    assert_eq!(ctx.svm.get_balance(&ctx.vault).unwrap(), amount);
}

#[test]
fn withdraw_returns_only_what_was_asked_for() {
    let mut ctx = setup();
    let ix = initialize_ix(&ctx);
    send(&mut ctx, ix);
    let ix = deposit_ix(&ctx, 3 * ONE_SOL);
    send(&mut ctx, ix);

    let ix = withdraw_ix(&ctx, ONE_SOL);
    assert!(send(&mut ctx, ix), "withdraw should succeed");

    // 3 in, 1 out, 2 left.
    assert_eq!(ctx.svm.get_balance(&ctx.vault).unwrap(), 2 * ONE_SOL);
}

#[test]
fn withdraw_more_than_the_balance_is_rejected() {
    let mut ctx = setup();
    let ix = initialize_ix(&ctx);
    send(&mut ctx, ix);
    let ix = deposit_ix(&ctx, ONE_SOL);
    send(&mut ctx, ix);

    // Ask for more than is in the vault -> InsufficientBalance.
    let ix = withdraw_ix(&ctx, 5 * ONE_SOL);
    assert!(!send(&mut ctx, ix), "withdraw should fail");

    // Nothing moved.
    assert_eq!(ctx.svm.get_balance(&ctx.vault).unwrap(), ONE_SOL);
}

#[test]
fn close_drains_the_vault_and_removes_the_state() {
    let mut ctx = setup();
    let ix = initialize_ix(&ctx);
    send(&mut ctx, ix);
    let ix = deposit_ix(&ctx, 5 * ONE_SOL);
    send(&mut ctx, ix);

    let user_before = ctx.svm.get_balance(&ctx.user.pubkey()).unwrap();

    let ix = close_ix(&ctx);
    assert!(send(&mut ctx, ix), "close should succeed");

    // Vault is emptied...
    assert_eq!(ctx.svm.get_balance(&ctx.vault).unwrap_or(0), 0);
    // ...the state account is gone...
    assert!(ctx.svm.get_account(&ctx.vault_state).is_none());
    // ...and the user is richer than before (vault balance + rent, minus fees).
    let user_after = ctx.svm.get_balance(&ctx.user.pubkey()).unwrap();
    assert!(user_after > user_before);
}
