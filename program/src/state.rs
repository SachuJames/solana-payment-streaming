use solana_program::{program_error::ProgramError, pubkey::Pubkey};

use crate::error::StreamError;

/// Serialized size of a Stream account: 4 x Pubkey + 8 x u64/i64 + 1 byte bump.
pub const STREAM_LEN: usize = 32 * 4 + 8 * 8 + 1;

/// Size of an SPL Token account.
pub const TOKEN_ACCOUNT_LEN: usize = 165;

/// A token vesting stream from `sender` to `recipient` for `mint`.
///
/// Linear vesting: `rate_per_sec` tokens unlock per second between `start_ts`
/// and `end_ts`. Nothing unlocks before `cliff_ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stream {
    pub sender: Pubkey,
    pub recipient: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub deposited: u64,
    pub withdrawn: u64,
    pub rate_per_sec: u64,
    pub start_ts: i64,
    pub end_ts: i64,
    pub cliff_ts: i64,
    pub nonce: u64,
    pub bump: u8,
}

fn read_pubkey(data: &[u8], off: &mut usize) -> Pubkey {
    let b: [u8; 32] = data[*off..*off + 32].try_into().unwrap();
    *off += 32;
    Pubkey::new_from_array(b)
}

fn read_u64(data: &[u8], off: &mut usize) -> u64 {
    let b: [u8; 8] = data[*off..*off + 8].try_into().unwrap();
    *off += 8;
    u64::from_le_bytes(b)
}

fn read_i64(data: &[u8], off: &mut usize) -> i64 {
    let b: [u8; 8] = data[*off..*off + 8].try_into().unwrap();
    *off += 8;
    i64::from_le_bytes(b)
}

fn write_pubkey(data: &mut [u8], off: &mut usize, p: &Pubkey) {
    data[*off..*off + 32].copy_from_slice(p.as_ref());
    *off += 32;
}

fn write_u64(data: &mut [u8], off: &mut usize, v: u64) {
    data[*off..*off + 8].copy_from_slice(&v.to_le_bytes());
    *off += 8;
}

fn write_i64(data: &mut [u8], off: &mut usize, v: i64) {
    data[*off..*off + 8].copy_from_slice(&v.to_le_bytes());
    *off += 8;
}

impl Stream {
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() != STREAM_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let mut off = 0;
        let sender = read_pubkey(data, &mut off);
        let recipient = read_pubkey(data, &mut off);
        let mint = read_pubkey(data, &mut off);
        let vault = read_pubkey(data, &mut off);
        let deposited = read_u64(data, &mut off);
        let withdrawn = read_u64(data, &mut off);
        let rate_per_sec = read_u64(data, &mut off);
        let start_ts = read_i64(data, &mut off);
        let end_ts = read_i64(data, &mut off);
        let cliff_ts = read_i64(data, &mut off);
        let nonce = read_u64(data, &mut off);
        let bump = data[off];
        Ok(Self {
            sender,
            recipient,
            mint,
            vault,
            deposited,
            withdrawn,
            rate_per_sec,
            start_ts,
            end_ts,
            cliff_ts,
            nonce,
            bump,
        })
    }

    pub fn pack(&self, data: &mut [u8]) -> Result<(), ProgramError> {
        if data.len() != STREAM_LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        let mut off = 0;
        write_pubkey(data, &mut off, &self.sender);
        write_pubkey(data, &mut off, &self.recipient);
        write_pubkey(data, &mut off, &self.mint);
        write_pubkey(data, &mut off, &self.vault);
        write_u64(data, &mut off, self.deposited);
        write_u64(data, &mut off, self.withdrawn);
        write_u64(data, &mut off, self.rate_per_sec);
        write_i64(data, &mut off, self.start_ts);
        write_i64(data, &mut off, self.end_ts);
        write_i64(data, &mut off, self.cliff_ts);
        write_u64(data, &mut off, self.nonce);
        data[off] = self.bump;
        Ok(())
    }

    /// Tokens unlocked as of `now_ts`, capped at `deposited`.
    pub fn vested_amount(&self, now_ts: i64) -> Result<u64, ProgramError> {
        if now_ts < self.cliff_ts {
            return Ok(0);
        }
        let elapsed: u64 = now_ts.saturating_sub(self.start_ts).max(0) as u64;
        let raw = self
            .rate_per_sec
            .checked_mul(elapsed)
            .ok_or(StreamError::ArithmeticOverflow)?;
        Ok(raw.min(self.deposited))
    }

    /// Tokens the recipient can pull right now.
    pub fn withdrawable(&self, now_ts: i64) -> Result<u64, ProgramError> {
        let vested = self.vested_amount(now_ts)?;
        vested
            .checked_sub(self.withdrawn)
            .ok_or(StreamError::ArithmeticOverflow.into())
    }
}

/// Minimal parse of an SPL Token account: (mint, owner, amount).
pub fn parse_token_account(data: &[u8]) -> Result<(Pubkey, Pubkey, u64), ProgramError> {
    if data.len() < 72 {
        return Err(ProgramError::InvalidAccountData);
    }
    let mint = Pubkey::new_from_array(data[0..32].try_into().unwrap());
    let owner = Pubkey::new_from_array(data[32..64].try_into().unwrap());
    let amount = u64::from_le_bytes(data[64..72].try_into().unwrap());
    Ok((mint, owner, amount))
}
