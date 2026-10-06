use solana_program::program_error::ProgramError;

/// Custom errors for the SolStream program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    /// Timestamps are inconsistent (end <= start).
    InvalidTimestamps = 0,
    /// Cliff is outside [start, end].
    InvalidCliff = 1,
    /// Amount must be greater than zero.
    InvalidAmount = 2,
    /// rate_per_sec * duration does not equal amount.
    InvalidRate = 3,
    /// Caller is not the expected signer (sender or recipient).
    Unauthorized = 4,
    /// Withdraw attempted before the cliff timestamp.
    CliffNotReached = 5,
    /// Nothing new has vested since the last withdraw.
    NothingToWithdraw = 6,
    /// Checked arithmetic overflowed.
    ArithmeticOverflow = 7,
    /// Expected the SPL Token program.
    InvalidTokenProgram = 8,
    /// Mint account is not owned by the Token program.
    InvalidMint = 9,
    /// Vault account failed validation.
    InvalidVault = 10,
    /// Stream account is already initialized.
    StreamAlreadyExists = 11,
    /// A PDA did not derive to the expected address.
    InvalidSeeds = 12,
}

impl From<StreamError> for ProgramError {
    fn from(e: StreamError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
