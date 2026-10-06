use solana_clock::Clock;
use solana_instruction::{AccountMeta, Instruction, InstructionError};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_program::system_instruction;
use solana_program_test::{processor, BanksClientError, ProgramTest, ProgramTestContext};
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_transaction::{Transaction, TransactionError};

const STREAM_TAG_CREATE: u8 = 0;
const STREAM_TAG_WITHDRAW: u8 = 1;
const STREAM_TAG_CANCEL: u8 = 2;

fn program_id() -> Pubkey {
    Pubkey::new_unique()
}

fn stream_pda(program_id: &Pubkey, sender: &Pubkey, recipient: &Pubkey, mint: &Pubkey, nonce: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            b"stream",
            sender.as_ref(),
            recipient.as_ref(),
            mint.as_ref(),
            &nonce.to_le_bytes(),
        ],
        program_id,
    )
}

fn vault_pda(program_id: &Pubkey, stream: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"vault", stream.as_ref()], program_id)
}

async fn start_context() -> (ProgramTestContext, Pubkey) {
    let pid = program_id();
    let mut pt = ProgramTest::new(
        "solstream",
        pid,
        processor!(solstream::process_instruction),
    );
    pt.add_program(
        "spl_token",
        spl_token::ID,
        processor!(spl_token::processor::Processor::process),
    );
    (pt.start_with_context().await, pid)
}

async fn send_tx(
    ctx: &mut ProgramTestContext,
    ixs: &[Instruction],
    signers: &[&Keypair],
) -> Result<(), BanksClientError> {
    let recent = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let msg = Message::new_with_blockhash(ixs, Some(&ctx.payer.pubkey()), &recent);
    let mut tx = Transaction::new_unsigned(msg);
    let mut all: Vec<&dyn Signer> = vec![&ctx.payer];
    for s in signers {
        all.push(*s);
    }
    tx.sign(&all, recent);
    ctx.banks_client.process_transaction(tx).await
}

async fn airdrop_sol(ctx: &mut ProgramTestContext, to: &Pubkey, lamports: u64) {
    let ix = system_instruction::transfer(&ctx.payer.pubkey(), to, lamports);
    send_tx(ctx, &[ix], &[]).await.unwrap();
}

async fn create_mint(ctx: &mut ProgramTestContext, authority: &Pubkey, decimals: u8) -> Keypair {
    let mint = Keypair::new();
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = 82usize;
    let lamports = rent.minimum_balance(space);
    let ixs = [
        system_instruction::create_account(
            &ctx.payer.pubkey(),
            &mint.pubkey(),
            lamports,
            space as u64,
            &spl_token::ID,
        ),
        spl_token::instruction::initialize_mint(
            &spl_token::ID,
            &mint.pubkey(),
            authority,
            None,
            decimals,
        )
        .unwrap(),
    ];
    send_tx(ctx, &ixs, &[&mint]).await.unwrap();
    mint
}

async fn create_token_account(
    ctx: &mut ProgramTestContext,
    owner: &Pubkey,
    mint: &Pubkey,
) -> Keypair {
    let acct = Keypair::new();
    let rent = ctx.banks_client.get_rent().await.unwrap();
    let space = 165usize;
    let lamports = rent.minimum_balance(space);
    let ixs = [
        system_instruction::create_account(
            &ctx.payer.pubkey(),
            &acct.pubkey(),
            lamports,
            space as u64,
            &spl_token::ID,
        ),
        spl_token::instruction::initialize_account(
            &spl_token::ID,
            &acct.pubkey(),
            mint,
            owner,
        )
        .unwrap(),
    ];
    send_tx(ctx, &ixs, &[&acct]).await.unwrap();
    acct
}

async fn mint_to(
    ctx: &mut ProgramTestContext,
    mint: &Pubkey,
    dest: &Pubkey,
    authority: &Keypair,
    amount: u64,
) {
    let ix = spl_token::instruction::mint_to(
        &spl_token::ID,
        mint,
        dest,
        &authority.pubkey(),
        &[],
        amount,
    )
    .unwrap();
    send_tx(ctx, &[ix], &[authority]).await.unwrap();
}

async fn token_balance(ctx: &mut ProgramTestContext, acct: &Pubkey) -> u64 {
    let data = ctx.banks_client.get_account(*acct).await.unwrap().unwrap().data;
    u64::from_le_bytes(data[64..72].try_into().unwrap())
}

async fn clock_now(ctx: &mut ProgramTestContext) -> i64 {
    ctx.banks_client
        .get_sysvar::<Clock>()
        .await
        .unwrap()
        .unix_timestamp
}

/// Warp forward until the test clock reaches `target` (safety-capped).
async fn warp_to_timestamp(ctx: &mut ProgramTestContext, target: i64) {
    for _ in 0..50 {
        let c: Clock = ctx.banks_client.get_sysvar().await.unwrap();
        if c.unix_timestamp >= target {
            return;
        }
        ctx.warp_to_slot(c.slot + 5000).unwrap();
    }
    panic!("warp_to_timestamp: clock did not advance to {}", target);
}

fn ix_create_stream(
    pid: &Pubkey,
    sender: &Pubkey,
    stream: &Pubkey,
    vault: &Pubkey,
    recipient: &Pubkey,
    mint: &Pubkey,
    sender_tokens: &Pubkey,
    amount: u64,
    rate: u64,
    start: i64,
    end: i64,
    cliff: i64,
    nonce: u64,
) -> Instruction {
    let mut data = vec![STREAM_TAG_CREATE];
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&rate.to_le_bytes());
    data.extend_from_slice(&start.to_le_bytes());
    data.extend_from_slice(&end.to_le_bytes());
    data.extend_from_slice(&cliff.to_le_bytes());
    data.extend_from_slice(&nonce.to_le_bytes());
    Instruction {
        program_id: *pid,
        accounts: vec![
            AccountMeta::new(*sender, true),
            AccountMeta::new(*stream, false),
            AccountMeta::new(*vault, false),
            AccountMeta::new_readonly(*recipient, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(*sender_tokens, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(solana_program::system_program::ID, false),
            AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
        ],
        data,
    }
}

fn ix_withdraw(
    pid: &Pubkey,
    recipient: &Pubkey,
    stream: &Pubkey,
    vault: &Pubkey,
    recipient_tokens: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *pid,
        accounts: vec![
            AccountMeta::new_readonly(*recipient, true),
            AccountMeta::new(*stream, false),
            AccountMeta::new(*vault, false),
            AccountMeta::new(*recipient_tokens, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(
                pubkey!("SysvarC1ock11111111111111111111111111111111"),
                false,
            ),
        ],
        data: vec![STREAM_TAG_WITHDRAW],
    }
}

fn ix_cancel(
    pid: &Pubkey,
    sender: &Pubkey,
    stream: &Pubkey,
    vault: &Pubkey,
    recipient_tokens: &Pubkey,
    sender_tokens: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *pid,
        accounts: vec![
            AccountMeta::new(*sender, true),
            AccountMeta::new(*stream, false),
            AccountMeta::new(*vault, false),
            AccountMeta::new(*recipient_tokens, false),
            AccountMeta::new(*sender_tokens, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(
                pubkey!("SysvarC1ock11111111111111111111111111111111"),
                false,
            ),
        ],
        data: vec![STREAM_TAG_CANCEL],
    }
}

struct Fixture {
    ctx: ProgramTestContext,
    pid: Pubkey,
    sender: Keypair,
    recipient: Keypair,
    mint: Keypair,
    sender_tokens: Keypair,
    recipient_tokens: Keypair,
    now: i64,
}

impl Fixture {
    async fn new() -> Self {
        let (mut ctx, pid) = start_context().await;
        let sender = Keypair::new();
        let recipient = Keypair::new();
        airdrop_sol(&mut ctx, &sender.pubkey(), 10_000_000_000).await;
        let mint = create_mint(&mut ctx, &sender.pubkey(), 6).await;
        let sender_tokens = create_token_account(&mut ctx, &sender.pubkey(), &mint.pubkey()).await;
        let recipient_tokens =
            create_token_account(&mut ctx, &recipient.pubkey(), &mint.pubkey()).await;
        mint_to(&mut ctx, &mint.pubkey(), &sender_tokens.pubkey(), &sender, 10_000_000_000).await;
        let now = clock_now(&mut ctx).await;
        Self {
            ctx,
            pid,
            sender,
            recipient,
            mint,
            sender_tokens,
            recipient_tokens,
            now,
        }
    }

    async fn create_stream(
        &mut self,
        amount: u64,
        rate: u64,
        start: i64,
        end: i64,
        cliff: i64,
        nonce: u64,
    ) -> Result<(Pubkey, Pubkey), BanksClientError> {
        let (stream, _) = stream_pda(
            &self.pid,
            &self.sender.pubkey(),
            &self.recipient.pubkey(),
            &self.mint.pubkey(),
            nonce,
        );
        let (vault, _) = vault_pda(&self.pid, &stream);
        let ix = ix_create_stream(
            &self.pid,
            &self.sender.pubkey(),
            &stream,
            &vault,
            &self.recipient.pubkey(),
            &self.mint.pubkey(),
            &self.sender_tokens.pubkey(),
            amount,
            rate,
            start,
            end,
            cliff,
            nonce,
        );
        send_tx(&mut self.ctx, &[ix], &[&self.sender]).await.map(|_| (stream, vault))
    }
}

fn custom_error(err: BanksClientError) -> Option<u32> {
    match err {
        BanksClientError::TransactionError(TransactionError::InstructionError(
            _,
            InstructionError::Custom(code),
        )) => Some(code),
        _ => None,
    }
}

#[tokio::test]
async fn test_full_lifecycle() {
    let mut f = Fixture::new().await;
    // Entirely in the past: fully vested at creation.
    let start = f.now - 2000;
    let end = f.now - 1000;
    let amount = 1_000_000u64;
    let rate = 1000u64; // 1000 * 1000s = 1_000_000
    let (stream, vault) = f
        .create_stream(amount, rate, start, end, start, 7)
        .await
        .unwrap();

    // Vault holds the full amount and is owned by the stream PDA.
    assert_eq!(token_balance(&mut f.ctx, &vault).await, amount);
    let vault_acct = f.ctx.banks_client.get_account(vault).await.unwrap().unwrap();
    assert_eq!(vault_acct.owner, spl_token::ID);

    // Withdraw everything.
    let ix = ix_withdraw(
        &f.pid,
        &f.recipient.pubkey(),
        &stream,
        &vault,
        &f.recipient_tokens.pubkey(),
    );
    send_tx(&mut f.ctx, &[ix], &[&f.recipient]).await.unwrap();
    assert_eq!(token_balance(&mut f.ctx, &f.recipient_tokens.pubkey()).await, amount);
    assert_eq!(token_balance(&mut f.ctx, &vault).await, 0);

    // Second withdraw has nothing left.
    let ix = ix_withdraw(
        &f.pid,
        &f.recipient.pubkey(),
        &stream,
        &vault,
        &f.recipient_tokens.pubkey(),
    );
    let err = send_tx(&mut f.ctx, &[ix], &[&f.recipient]).await.unwrap_err();
    assert_eq!(custom_error(err), Some(6)); // NothingToWithdraw

    // Cancel settles zero and closes both accounts.
    let ix = ix_cancel(
        &f.pid,
        &f.sender.pubkey(),
        &stream,
        &vault,
        &f.recipient_tokens.pubkey(),
        &f.sender_tokens.pubkey(),
    );
    send_tx(&mut f.ctx, &[ix], &[&f.sender]).await.unwrap();
    assert!(f.ctx.banks_client.get_account(stream).await.unwrap().is_none());
    assert!(f.ctx.banks_client.get_account(vault).await.unwrap().is_none());
}

#[tokio::test]
async fn test_withdraw_before_cliff_fails() {
    let mut f = Fixture::new().await;
    let start = f.now;
    let end = f.now + 1000;
    let cliff = f.now + 500;
    let (stream, vault) = f.create_stream(1_000_000, 1000, start, end, cliff, 8).await.unwrap();

    let ix = ix_withdraw(
        &f.pid,
        &f.recipient.pubkey(),
        &stream,
        &vault,
        &f.recipient_tokens.pubkey(),
    );
    let err = send_tx(&mut f.ctx, &[ix], &[&f.recipient]).await.unwrap_err();
    assert_eq!(custom_error(err), Some(5)); // CliffNotReached
}

#[tokio::test]
async fn test_double_withdraw_pays_newly_vested() {
    let mut f = Fixture::new().await;
    let start = f.now - 100;
    let end = f.now + 900;
    let amount = 1_000_000u64;
    let rate = 1000u64;
    let (stream, vault) = f.create_stream(amount, rate, start, end, start, 9).await.unwrap();

    // First withdraw: vested = 1000 * elapsed(~100s).
    let t0 = clock_now(&mut f.ctx).await;
    let ix = ix_withdraw(
        &f.pid,
        &f.recipient.pubkey(),
        &stream,
        &vault,
        &f.recipient_tokens.pubkey(),
    );
    send_tx(&mut f.ctx, &[ix], &[&f.recipient]).await.unwrap();
    let first = token_balance(&mut f.ctx, &f.recipient_tokens.pubkey()).await;
    assert_eq!(first, 1000 * (t0 - start) as u64);

    // Warp forward ~500s and withdraw again: only the newly vested part pays.
    warp_to_timestamp(&mut f.ctx, t0 + 500).await;
    let t1 = clock_now(&mut f.ctx).await;
    let ix = ix_withdraw(
        &f.pid,
        &f.recipient.pubkey(),
        &stream,
        &vault,
        &f.recipient_tokens.pubkey(),
    );
    send_tx(&mut f.ctx, &[ix], &[&f.recipient]).await.unwrap();
    let total = token_balance(&mut f.ctx, &f.recipient_tokens.pubkey()).await;
    let expected_total = (1000 * (t1 - start) as u64).min(amount);
    assert_eq!(total, expected_total);
    assert!(total > first);
}

#[tokio::test]
async fn test_cancel_by_non_sender_fails() {
    let mut f = Fixture::new().await;
    let attacker = Keypair::new();
    airdrop_sol(&mut f.ctx, &attacker.pubkey(), 10_000_000_000).await;
    let (stream, vault) = f
        .create_stream(1_000_000, 1000, f.now - 100, f.now + 900, f.now - 100, 10)
        .await
        .unwrap();

    let ix = ix_cancel(
        &f.pid,
        &attacker.pubkey(),
        &stream,
        &vault,
        &f.recipient_tokens.pubkey(),
        &f.sender_tokens.pubkey(),
    );
    let err = send_tx(&mut f.ctx, &[ix], &[&attacker]).await.unwrap_err();
    assert_eq!(custom_error(err), Some(4)); // Unauthorized
}

#[tokio::test]
async fn test_cancel_settles_both_parties() {
    let mut f = Fixture::new().await;
    // 25% vested at cancel time.
    let start = f.now - 250;
    let end = f.now + 750;
    let amount = 1_000_000u64;
    let (stream, vault) = f.create_stream(amount, 1000, start, end, start, 11).await.unwrap();

    let t = clock_now(&mut f.ctx).await;
    let vested = (1000 * (t - start) as u64).min(amount);

    let sender_before = token_balance(&mut f.ctx, &f.sender_tokens.pubkey()).await;
    let ix = ix_cancel(
        &f.pid,
        &f.sender.pubkey(),
        &stream,
        &vault,
        &f.recipient_tokens.pubkey(),
        &f.sender_tokens.pubkey(),
    );
    send_tx(&mut f.ctx, &[ix], &[&f.sender]).await.unwrap();

    assert_eq!(token_balance(&mut f.ctx, &f.recipient_tokens.pubkey()).await, vested);
    assert_eq!(
        token_balance(&mut f.ctx, &f.sender_tokens.pubkey()).await,
        sender_before + (amount - vested)
    );
    assert!(f.ctx.banks_client.get_account(stream).await.unwrap().is_none());
    assert!(f.ctx.banks_client.get_account(vault).await.unwrap().is_none());
}

#[tokio::test]
async fn test_invalid_schedules_rejected() {
    let mut f = Fixture::new().await;
    // end <= start
    let r = f.create_stream(1_000_000, 1000, f.now + 100, f.now + 100, f.now + 100, 20).await;
    assert_eq!(custom_error(r.unwrap_err()), Some(0)); // InvalidTimestamps
    // cliff after end
    let r = f.create_stream(1_000_000, 1000, f.now, f.now + 1000, f.now + 1001, 21).await;
    assert_eq!(custom_error(r.unwrap_err()), Some(1)); // InvalidCliff
    // zero amount
    let r = f.create_stream(0, 1000, f.now, f.now + 1000, f.now, 22).await;
    assert_eq!(custom_error(r.unwrap_err()), Some(2)); // InvalidAmount
    // rate * duration != amount
    let r = f.create_stream(1_000_000, 999, f.now, f.now + 1000, f.now, 23).await;
    assert_eq!(custom_error(r.unwrap_err()), Some(3)); // InvalidRate
    // overflow in rate * duration
    let r = f.create_stream(100, u64::MAX, f.now, f.now + 2, f.now, 24).await;
    assert_eq!(custom_error(r.unwrap_err()), Some(7)); // ArithmeticOverflow
}

#[tokio::test]
async fn test_withdraw_by_non_recipient_fails() {
    let mut f = Fixture::new().await;
    let (stream, vault) = f
        .create_stream(1_000_000, 1000, f.now - 100, f.now + 900, f.now - 100, 25)
        .await
        .unwrap();
    // Sender tries to withdraw.
    let ix = ix_withdraw(
        &f.pid,
        &f.sender.pubkey(),
        &stream,
        &vault,
        &f.sender_tokens.pubkey(),
    );
    let err = send_tx(&mut f.ctx, &[ix], &[&f.sender]).await.unwrap_err();
    assert_eq!(custom_error(err), Some(4)); // Unauthorized
}
