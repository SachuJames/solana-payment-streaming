use solana_program::program_error::ProgramError;

/// Instructions for the SolStream program.
pub enum StreamInstruction {
    /// Create a new vesting stream.
    ///
    /// Accounts:
    /// 0. `[signer, writable]` sender (payer)
    /// 1. `[writable]` stream PDA
    /// 2. `[writable]` vault token PDA
    /// 3. `[]` recipient
    /// 4. `[]` mint
    /// 5. `[writable]` sender token account
    /// 6. `[]` SPL Token program
    /// 7. `[]` system program
    /// 8. `[]` rent sysvar
    CreateStream {
        amount: u64,
        rate_per_sec: u64,
        start_ts: i64,
        end_ts: i64,
        cliff_ts: i64,
        nonce: u64,
    },
    /// Withdraw newly vested tokens to the recipient.
    ///
    /// Accounts:
    /// 0. `[signer]` recipient
    /// 1. `[writable]` stream PDA
    /// 2. `[writable]` vault token PDA
    /// 3. `[writable]` recipient token account
    /// 4. `[]` SPL Token program
    /// 5. `[]` clock sysvar
    Withdraw,
    /// Cancel the stream, settle both sides, close the accounts.
    ///
    /// Accounts:
    /// 0. `[signer, writable]` sender
    /// 1. `[writable]` stream PDA
    /// 2. `[writable]` vault token PDA
    /// 3. `[writable]` recipient token account
    /// 4. `[writable]` sender token account
    /// 5. `[]` SPL Token program
    /// 6. `[]` clock sysvar
    CancelStream,
}

impl StreamInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (&tag, rest) = input
            .split_first()
            .ok_or(ProgramError::InvalidInstructionData)?;
        Ok(match tag {
            0 => {
                if rest.len() != 48 {
                    return Err(ProgramError::InvalidInstructionData);
                }
                let mut off = 0;
                let amount = u64::from_le_bytes(rest[off..off + 8].try_into().unwrap());
                off += 8;
                let rate_per_sec = u64::from_le_bytes(rest[off..off + 8].try_into().unwrap());
                off += 8;
                let start_ts = i64::from_le_bytes(rest[off..off + 8].try_into().unwrap());
                off += 8;
                let end_ts = i64::from_le_bytes(rest[off..off + 8].try_into().unwrap());
                off += 8;
                let cliff_ts = i64::from_le_bytes(rest[off..off + 8].try_into().unwrap());
                off += 8;
                let nonce = u64::from_le_bytes(rest[off..off + 8].try_into().unwrap());
                Self::CreateStream {
                    amount,
                    rate_per_sec,
                    start_ts,
                    end_ts,
                    cliff_ts,
                    nonce,
                }
            }
            1 => Self::Withdraw,
            2 => Self::CancelStream,
            _ => return Err(ProgramError::InvalidInstructionData),
        })
    }
}
