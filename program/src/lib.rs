use solana_program::entrypoint;

mod error;
mod instruction;
mod processor;
mod state;

pub use processor::{process_instruction, TOKEN_PROGRAM_ID};

entrypoint!(process_instruction);
