use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction},
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::{pubkey, Pubkey},
};

use crate::{
    error::StreamError,
    instruction::StreamInstruction,
    state::{parse_token_account, Stream, STREAM_LEN, TOKEN_ACCOUNT_LEN},
};

/// SPL Token program id.
pub const TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

/// System program id.
const SYSTEM_PROGRAM_ID: Pubkey = pubkey!("11111111111111111111111111111111");

/// Rent sysvar id.
const RENT_SYSVAR_ID: Pubkey = pubkey!("SysvarRent111111111111111111111111111111111");

/// Clock sysvar id.
const CLOCK_SYSVAR_ID: Pubkey = pubkey!("SysvarC1ock11111111111111111111111111111111");

/// Unix timestamp from a clock sysvar account (bincode layout, field at [32..40]).
fn clock_timestamp(clock_info: &AccountInfo) -> Result<i64, ProgramError> {
    if clock_info.key != &CLOCK_SYSVAR_ID {
        return Err(ProgramError::InvalidArgument);
    }
    let data = clock_info.data.borrow();
    if data.len() < 40 {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(i64::from_le_bytes(data[32..40].try_into().unwrap()))
}

/// Extra bytes counted for rent on every account.
const ACCOUNT_STORAGE_OVERHEAD: u64 = 128;

/// Minimum lamports for rent exemption, parsed from the rent sysvar account.
///
/// The sysvar holds bincode-serialized Rent:
/// lamports_per_byte_year (u64) | exemption_threshold (f64) | burn_percent (u8).
fn rent_minimum_balance(rent_data: &[u8], data_len: usize) -> Result<u64, ProgramError> {
    if rent_data.len() < 17 {
        return Err(ProgramError::InvalidAccountData);
    }
    let lamports_per_byte_year = u64::from_le_bytes(rent_data[0..8].try_into().unwrap());
    let exemption_threshold = f64::from_le_bytes(rent_data[8..16].try_into().unwrap());
    let bytes = ACCOUNT_STORAGE_OVERHEAD
        .checked_add(data_len as u64)
        .ok_or(StreamError::ArithmeticOverflow)?;
    let numerator = bytes
        .checked_mul(lamports_per_byte_year)
        .ok_or(StreamError::ArithmeticOverflow)?;
    Ok((numerator as f64 * exemption_threshold) as u64)
}

/// Build a system CreateAccount instruction (index 0).
fn system_create_account(
    from: &Pubkey,
    to: &Pubkey,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let mut data = Vec::with_capacity(4 + 8 + 8 + 32);
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());
    data.extend_from_slice(&space.to_le_bytes());
    data.extend_from_slice(owner.as_ref());
    Instruction {
        program_id: SYSTEM_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*from, true),
            AccountMeta::new(*to, true),
        ],
        data,
    }
}

/// Build an SPL Token Transfer instruction (index 3).
fn token_transfer_ix(
    source: &Pubkey,
    dest: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> Instruction {
    let mut data = vec![3u8];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*source, false),
            AccountMeta::new(*dest, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    match StreamInstruction::unpack(input)? {
        StreamInstruction::CreateStream {
            amount,
            rate_per_sec,
            start_ts,
            end_ts,
            cliff_ts,
            nonce,
        } => process_create_stream(
            program_id,
            accounts,
            amount,
            rate_per_sec,
            start_ts,
            end_ts,
            cliff_ts,
            nonce,
        ),
        StreamInstruction::Withdraw => process_withdraw(program_id, accounts),
        StreamInstruction::CancelStream => process_cancel(program_id, accounts),
    }
}

#[allow(clippy::too_many_arguments)]
fn process_create_stream(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
    rate_per_sec: u64,
    start_ts: i64,
    end_ts: i64,
    cliff_ts: i64,
    nonce: u64,
) -> ProgramResult {
    let iter = &mut accounts.iter();
    let sender = next_account_info(iter)?;
    let stream = next_account_info(iter)?;
    let vault = next_account_info(iter)?;
    let recipient = next_account_info(iter)?;
    let mint = next_account_info(iter)?;
    let sender_tokens = next_account_info(iter)?;
    let token_program = next_account_info(iter)?;
    let system_program = next_account_info(iter)?;
    let rent_info = next_account_info(iter)?;

    if !sender.is_signer {
        return Err(StreamError::Unauthorized.into());
    }
    if token_program.key != &TOKEN_PROGRAM_ID {
        return Err(StreamError::InvalidTokenProgram.into());
    }
    if system_program.key != &SYSTEM_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if mint.owner != &TOKEN_PROGRAM_ID {
        return Err(StreamError::InvalidMint.into());
    }
    if !stream.data_is_empty() {
        return Err(StreamError::StreamAlreadyExists.into());
    }

    // Schedule validation.
    if end_ts <= start_ts {
        return Err(StreamError::InvalidTimestamps.into());
    }
    if cliff_ts < start_ts || cliff_ts > end_ts {
        return Err(StreamError::InvalidCliff.into());
    }
    if amount == 0 {
        return Err(StreamError::InvalidAmount.into());
    }
    if rate_per_sec == 0 {
        return Err(StreamError::InvalidRate.into());
    }
    let duration = (end_ts - start_ts) as u64;
    let expected = rate_per_sec
        .checked_mul(duration)
        .ok_or(StreamError::ArithmeticOverflow)?;
    if expected != amount {
        msg!("rate_per_sec * duration must equal amount");
        return Err(StreamError::InvalidRate.into());
    }

    // PDA checks.
    let nonce_bytes = nonce.to_le_bytes();
    let (stream_key, bump) = Pubkey::find_program_address(
        &[
            b"stream",
            sender.key.as_ref(),
            recipient.key.as_ref(),
            mint.key.as_ref(),
            &nonce_bytes,
        ],
        program_id,
    );
    if stream_key != *stream.key {
        return Err(StreamError::InvalidSeeds.into());
    }
    let (vault_key, vault_bump) =
        Pubkey::find_program_address(&[b"vault", stream.key.as_ref()], program_id);
    if vault_key != *vault.key {
        return Err(StreamError::InvalidSeeds.into());
    }

    // Sender must actually hold the tokens being streamed.
    let (src_mint, _, src_amount) = parse_token_account(&sender_tokens.data.borrow())?;
    if src_mint != *mint.key {
        return Err(StreamError::InvalidMint.into());
    }
    if src_amount < amount {
        msg!("sender token balance below stream amount");
        return Err(StreamError::InvalidAmount.into());
    }

    if rent_info.key != &RENT_SYSVAR_ID {
        return Err(ProgramError::InvalidArgument);
    }
    let rent_data = rent_info.data.borrow();

    // Create the stream state account.
    let stream_seeds: &[&[u8]] = &[
        b"stream",
        sender.key.as_ref(),
        recipient.key.as_ref(),
        mint.key.as_ref(),
        &nonce_bytes,
        &[bump],
    ];
    invoke_signed(
        &system_create_account(
            sender.key,
            stream.key,
            rent_minimum_balance(&rent_data, STREAM_LEN)?,
            STREAM_LEN as u64,
            program_id,
        ),
        &[sender.clone(), stream.clone(), system_program.clone()],
        &[stream_seeds],
    )?;

    // Create the vault token account, owned by the stream PDA.
    let vault_seeds: &[&[u8]] = &[b"vault", stream.key.as_ref(), &[vault_bump]];
    invoke_signed(
        &system_create_account(
            sender.key,
            vault.key,
            rent_minimum_balance(&rent_data, TOKEN_ACCOUNT_LEN)?,
            TOKEN_ACCOUNT_LEN as u64,
            &TOKEN_PROGRAM_ID,
        ),
        &[sender.clone(), vault.clone(), system_program.clone()],
        &[vault_seeds],
    )?;
    // InitializeAccount (index 1).
    invoke(
        &Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*vault.key, false),
                AccountMeta::new_readonly(*mint.key, false),
                AccountMeta::new_readonly(*stream.key, false),
                AccountMeta::new_readonly(RENT_SYSVAR_ID, false),
            ],
            data: vec![1],
        },
        &[vault.clone(), mint.clone(), stream.clone(), rent_info.clone()],
    )?;

    // Fund the vault from the sender.
    invoke(
        &token_transfer_ix(sender_tokens.key, vault.key, sender.key, amount),
        &[sender_tokens.clone(), vault.clone(), sender.clone()],
    )?;

    let state = Stream {
        sender: *sender.key,
        recipient: *recipient.key,
        mint: *mint.key,
        vault: *vault.key,
        deposited: amount,
        withdrawn: 0,
        rate_per_sec,
        start_ts,
        end_ts,
        cliff_ts,
        nonce,
        bump,
    };
    state.pack(&mut stream.data.borrow_mut())?;

    msg!(
        "stream created: {} -> {}, amount {}, rate {}/s",
        sender.key,
        recipient.key,
        amount,
        rate_per_sec
    );
    Ok(())
}

/// Validated stream state plus owned PDA signer seeds for CPI signing.
struct LoadedStream {
    stream: Stream,
    seeds: Vec<Vec<u8>>,
}

fn load_stream(
    program_id: &Pubkey,
    stream_info: &AccountInfo,
    vault: &AccountInfo,
    token_program: &AccountInfo,
) -> Result<LoadedStream, ProgramError> {
    if stream_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if token_program.key != &TOKEN_PROGRAM_ID {
        return Err(StreamError::InvalidTokenProgram.into());
    }
    let stream = Stream::unpack(&stream_info.data.borrow())?;
    if *vault.key != stream.vault {
        return Err(StreamError::InvalidVault.into());
    }
    let (vault_mint, vault_owner, _) = parse_token_account(&vault.data.borrow())?;
    if vault_mint != stream.mint || vault_owner != *stream_info.key {
        return Err(StreamError::InvalidVault.into());
    }
    // Re-derive the PDA from stored fields to prove the seeds are genuine.
    let nonce_bytes = stream.nonce.to_le_bytes();
    let derived = Pubkey::create_program_address(
        &[
            b"stream".as_ref(),
            stream.sender.as_ref(),
            stream.recipient.as_ref(),
            stream.mint.as_ref(),
            nonce_bytes.as_ref(),
            core::slice::from_ref(&stream.bump),
        ],
        program_id,
    )
    .map_err(|_| StreamError::InvalidSeeds)?;
    if derived != *stream_info.key {
        return Err(StreamError::InvalidSeeds.into());
    }
    let seeds: Vec<Vec<u8>> = vec![
        b"stream".to_vec(),
        stream.sender.as_ref().to_vec(),
        stream.recipient.as_ref().to_vec(),
        stream.mint.as_ref().to_vec(),
        nonce_bytes.to_vec(),
        vec![stream.bump],
    ];
    Ok(LoadedStream { stream, seeds })
}

fn signer_seeds(seeds: &[Vec<u8>]) -> Vec<&[u8]> {
    seeds.iter().map(|v| v.as_slice()).collect()
}

fn process_withdraw(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let recipient = next_account_info(iter)?;
    let stream_info = next_account_info(iter)?;
    let vault = next_account_info(iter)?;
    let recipient_tokens = next_account_info(iter)?;
    let token_program = next_account_info(iter)?;
    let clock_info = next_account_info(iter)?;

    if !recipient.is_signer {
        return Err(StreamError::Unauthorized.into());
    }

    let loaded = load_stream(program_id, stream_info, vault, token_program)?;
    let mut stream = loaded.stream;
    if *recipient.key != stream.recipient {
        return Err(StreamError::Unauthorized.into());
    }
    let (dst_mint, _, _) = parse_token_account(&recipient_tokens.data.borrow())?;
    if dst_mint != stream.mint {
        return Err(StreamError::InvalidMint.into());
    }

    let now = clock_timestamp(clock_info)?;
    if now < stream.cliff_ts {
        return Err(StreamError::CliffNotReached.into());
    }
    let payout = stream.withdrawable(now)?;
    if payout == 0 {
        return Err(StreamError::NothingToWithdraw.into());
    }

    let refs = signer_seeds(&loaded.seeds);
    invoke_signed(
        &token_transfer_ix(vault.key, recipient_tokens.key, stream_info.key, payout),
        &[vault.clone(), recipient_tokens.clone(), stream_info.clone()],
        &[refs.as_slice()],
    )?;

    stream.withdrawn = stream
        .withdrawn
        .checked_add(payout)
        .ok_or(StreamError::ArithmeticOverflow)?;
    stream.pack(&mut stream_info.data.borrow_mut())?;

    msg!("withdrew {} to {}", payout, recipient.key);
    Ok(())
}

fn process_cancel(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let iter = &mut accounts.iter();
    let sender = next_account_info(iter)?;
    let stream_info = next_account_info(iter)?;
    let vault = next_account_info(iter)?;
    let recipient_tokens = next_account_info(iter)?;
    let sender_tokens = next_account_info(iter)?;
    let token_program = next_account_info(iter)?;
    let clock_info = next_account_info(iter)?;

    if !sender.is_signer {
        return Err(StreamError::Unauthorized.into());
    }

    let loaded = load_stream(program_id, stream_info, vault, token_program)?;
    let stream = loaded.stream;
    if *sender.key != stream.sender {
        return Err(StreamError::Unauthorized.into());
    }
    let (dst_mint, _, _) = parse_token_account(&recipient_tokens.data.borrow())?;
    if dst_mint != stream.mint {
        return Err(StreamError::InvalidMint.into());
    }
    let (src_mint, _, _) = parse_token_account(&sender_tokens.data.borrow())?;
    if src_mint != stream.mint {
        return Err(StreamError::InvalidMint.into());
    }

    let now = clock_timestamp(clock_info)?;
    let vested = stream.vested_amount(now)?;
    let to_recipient = vested
        .checked_sub(stream.withdrawn)
        .ok_or(StreamError::ArithmeticOverflow)?;
    let to_sender = stream
        .deposited
        .checked_sub(vested)
        .ok_or(StreamError::ArithmeticOverflow)?;

    let refs = signer_seeds(&loaded.seeds);
    let signer = [refs.as_slice()];

    if to_recipient > 0 {
        invoke_signed(
            &token_transfer_ix(vault.key, recipient_tokens.key, stream_info.key, to_recipient),
            &[vault.clone(), recipient_tokens.clone(), stream_info.clone()],
            &signer,
        )?;
    }
    if to_sender > 0 {
        invoke_signed(
            &token_transfer_ix(vault.key, sender_tokens.key, stream_info.key, to_sender),
            &[vault.clone(), sender_tokens.clone(), stream_info.clone()],
            &signer,
        )?;
    }

    // Close the vault, sending its rent to the sender. CloseAccount is index 9.
    invoke_signed(
        &Instruction {
            program_id: TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*vault.key, false),
                AccountMeta::new(*sender.key, false),
                AccountMeta::new_readonly(*stream_info.key, true),
            ],
            data: vec![9],
        },
        &[vault.clone(), sender.clone(), stream_info.clone()],
        &signer,
    )?;

    // Close the stream account, returning rent to the sender.
    let stream_lamports = stream_info.lamports();
    **sender.try_borrow_mut_lamports()? = sender
        .lamports()
        .checked_add(stream_lamports)
        .ok_or(StreamError::ArithmeticOverflow)?;
    **stream_info.try_borrow_mut_lamports()? = 0;
    stream_info.data.borrow_mut().fill(0);

    msg!(
        "stream cancelled: {} to recipient, {} returned to sender",
        to_recipient,
        to_sender
    );
    Ok(())
}
