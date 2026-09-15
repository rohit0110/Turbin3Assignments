//! Fast in-process tests for the vault/gate/live-amount-CPI logic in
//! `heir_sweep`, using `solana-program-test`'s in-process bank (no durable
//! nonce involved here -- see `examples/durable_nonce_demo.rs` and README.md
//! for why that part needs a real validator instead).

use heir_sweep_poc::{vault_pda, HeirInstruction};
use solana_program::system_program;
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

fn program_test() -> ProgramTest {
    ProgramTest::new(
        "heir_sweep_poc",
        heir_sweep_poc::id_for_tests(),
        processor!(heir_sweep_poc::process_instruction),
    )
}

fn heir_ix(
    program_id: &Pubkey,
    owner: &Pubkey,
    beneficiary: &Pubkey,
    vault: &Pubkey,
    data: HeirInstruction,
) -> Instruction {
    let accounts = match data {
        HeirInstruction::Initialize { .. } => vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new(*vault, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        HeirInstruction::CheckIn => vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new(*vault, false),
        ],
        HeirInstruction::HeirSweep => vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new(*beneficiary, false),
            AccountMeta::new(*vault, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    };
    Instruction {
        program_id: *program_id,
        accounts,
        data: borsh::to_vec(&data).unwrap(),
    }
}

#[tokio::test]
async fn heir_sweep_rejected_before_timeout() {
    let program_id = heir_sweep_poc::id_for_tests();
    let owner = Keypair::new();
    let beneficiary = Keypair::new();
    let (vault, _bump) = vault_pda(&owner.pubkey(), &program_id);

    let mut pt = program_test();
    pt.add_account(
        owner.pubkey(),
        Account {
            lamports: 5_000_000_000,
            ..Account::default()
        },
    );
    let ctx = pt.start_with_context().await;

    let timeout_secs: i64 = 60 * 24 * 60 * 60;
    let init_ix = heir_ix(
        &program_id,
        &owner.pubkey(),
        &beneficiary.pubkey(),
        &vault,
        HeirInstruction::Initialize {
            beneficiary: beneficiary.pubkey(),
            timeout_secs,
        },
    );
    let bh = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(&[init_ix], Some(&owner.pubkey()), &[&owner], bh);
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let sweep_ix = heir_ix(
        &program_id,
        &owner.pubkey(),
        &beneficiary.pubkey(),
        &vault,
        HeirInstruction::HeirSweep,
    );
    let bh = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(&[sweep_ix], Some(&owner.pubkey()), &[&owner], bh);
    let err = ctx.banks_client.process_transaction(tx).await.unwrap_err();
    println!("rejected before timeout, as expected: {err:?}");

    let beneficiary_balance = ctx
        .banks_client
        .get_balance(beneficiary.pubkey())
        .await
        .unwrap();
    assert_eq!(beneficiary_balance, 0);
}

#[tokio::test]
async fn heir_sweep_moves_the_live_balance_after_timeout() {
    let program_id = heir_sweep_poc::id_for_tests();
    let owner = Keypair::new();
    let beneficiary = Keypair::new();
    let (vault, _bump) = vault_pda(&owner.pubkey(), &program_id);

    let mut pt = program_test();
    pt.add_account(
        owner.pubkey(),
        Account {
            lamports: 5_000_000_000,
            ..Account::default()
        },
    );
    let mut ctx = pt.start_with_context().await;

    let timeout_secs: i64 = 60 * 24 * 60 * 60;
    let init_ix = heir_ix(
        &program_id,
        &owner.pubkey(),
        &beneficiary.pubkey(),
        &vault,
        HeirInstruction::Initialize {
            beneficiary: beneficiary.pubkey(),
            timeout_secs,
        },
    );
    let bh = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(&[init_ix], Some(&owner.pubkey()), &[&owner], bh);
    ctx.banks_client.process_transaction(tx).await.unwrap();

    // Balance changes after "initialize", before the sweep -- the swept
    // amount must reflect this, not whatever the balance was at setup time.
    let mut owner_account = ctx
        .banks_client
        .get_account(owner.pubkey())
        .await
        .unwrap()
        .unwrap();
    owner_account.lamports += 777_000_000;
    ctx.set_account(&owner.pubkey(), &owner_account.into());

    let clock: solana_program::clock::Clock = ctx.banks_client.get_sysvar().await.unwrap();
    let mut warped_clock = clock.clone();
    warped_clock.unix_timestamp += timeout_secs + 1;
    ctx.set_sysvar(&warped_clock);

    let live_owner_balance = ctx
        .banks_client
        .get_balance(owner.pubkey())
        .await
        .unwrap();

    let sweep_ix = heir_ix(
        &program_id,
        &owner.pubkey(),
        &beneficiary.pubkey(),
        &vault,
        HeirInstruction::HeirSweep,
    );
    let bh = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(&[sweep_ix], Some(&owner.pubkey()), &[&owner], bh);
    ctx.banks_client.process_transaction(tx).await.unwrap();

    let beneficiary_balance = ctx
        .banks_client
        .get_balance(beneficiary.pubkey())
        .await
        .unwrap();
    let owner_balance_after = ctx
        .banks_client
        .get_balance(owner.pubkey())
        .await
        .unwrap();

    // owner is also the fee payer for the sweep tx itself, so the swept
    // amount is the live balance minus that one signature's fee -- not
    // exactly the `live_owner_balance` snapshot taken just before
    // submission, but it must be very close to it, and nowhere near the
    // 5_000_000_000 the account started with before the +777_000_000 change.
    let fee_paid = live_owner_balance - beneficiary_balance;
    assert!(fee_paid > 0 && fee_paid < 100_000, "unexpected fee: {fee_paid}");
    assert_eq!(owner_balance_after, 0);
    assert!(
        beneficiary_balance > 5_700_000_000,
        "swept amount ({beneficiary_balance}) must reflect the live balance \
         (5_000_000_000 + 777_000_000 deposited after setup), not the balance at vault-init time"
    );
}
