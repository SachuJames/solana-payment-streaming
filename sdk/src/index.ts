import {
  Connection,
  PublicKey,
  SYSVAR_CLOCK_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  SystemProgram,
  TransactionInstruction,
} from "@solana/web3.js";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";

/** Serialized size of a Stream account on chain. */
export const STREAM_LEN = 193;

/** Offsets into the Stream account layout (for memcmp filters). */
export const STREAM_OFFSETS = {
  sender: 0,
  recipient: 32,
  mint: 64,
  vault: 96,
  deposited: 128,
  withdrawn: 136,
  ratePerSec: 144,
  startTs: 152,
  endTs: 160,
  cliffTs: 168,
  nonce: 176,
  bump: 184,
} as const;

export interface StreamData {
  address: PublicKey;
  sender: PublicKey;
  recipient: PublicKey;
  mint: PublicKey;
  vault: PublicKey;
  deposited: bigint;
  withdrawn: bigint;
  ratePerSec: bigint;
  startTs: bigint;
  endTs: bigint;
  cliffTs: bigint;
  nonce: bigint;
  bump: number;
}

export interface CreateStreamParams {
  sender: PublicKey;
  recipient: PublicKey;
  mint: PublicKey;
  senderTokenAccount: PublicKey;
  amount: bigint;
  ratePerSec: bigint;
  startTs: bigint;
  endTs: bigint;
  cliffTs: bigint;
  nonce: bigint;
}

/** Derive the stream PDA. Seeds: ["stream", sender, recipient, mint, nonce:u64]. */
export async function findStreamPDA(
  programId: PublicKey,
  sender: PublicKey,
  recipient: PublicKey,
  mint: PublicKey,
  nonce: bigint
): Promise<[PublicKey, number]> {
  const nonceBuf = Buffer.alloc(8);
  nonceBuf.writeBigUInt64LE(nonce);
  return PublicKey.findProgramAddress(
    [
      Buffer.from("stream"),
      sender.toBuffer(),
      recipient.toBuffer(),
      mint.toBuffer(),
      nonceBuf,
    ],
    programId
  );
}

/** Derive the vault PDA. Seeds: ["vault", stream]. */
export async function findVaultPDA(
  programId: PublicKey,
  stream: PublicKey
): Promise<[PublicKey, number]> {
  return PublicKey.findProgramAddress(
    [Buffer.from("vault"), stream.toBuffer()],
    programId
  );
}

function encodeCreateStream(p: CreateStreamParams): Buffer {
  const data = Buffer.alloc(1 + 8 * 6);
  data.writeUInt8(0, 0);
  data.writeBigUInt64LE(p.amount, 1);
  data.writeBigUInt64LE(p.ratePerSec, 9);
  data.writeBigInt64LE(p.startTs, 17);
  data.writeBigInt64LE(p.endTs, 25);
  data.writeBigInt64LE(p.cliffTs, 33);
  data.writeBigUInt64LE(p.nonce, 41);
  return data;
}

export async function createStreamInstruction(
  programId: PublicKey,
  p: CreateStreamParams
): Promise<TransactionInstruction> {
  const [stream] = await findStreamPDA(
    programId,
    p.sender,
    p.recipient,
    p.mint,
    p.nonce
  );
  const [vault] = await findVaultPDA(programId, stream);
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: p.sender, isSigner: true, isWritable: true },
      { pubkey: stream, isSigner: false, isWritable: true },
      { pubkey: vault, isSigner: false, isWritable: true },
      { pubkey: p.recipient, isSigner: false, isWritable: false },
      { pubkey: p.mint, isSigner: false, isWritable: false },
      { pubkey: p.senderTokenAccount, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: SYSVAR_RENT_PUBKEY, isSigner: false, isWritable: false },
    ],
    data: encodeCreateStream(p),
  });
}

export function withdrawInstruction(
  programId: PublicKey,
  recipient: PublicKey,
  stream: PublicKey,
  vault: PublicKey,
  recipientTokenAccount: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: recipient, isSigner: true, isWritable: false },
      { pubkey: stream, isSigner: false, isWritable: true },
      { pubkey: vault, isSigner: false, isWritable: true },
      { pubkey: recipientTokenAccount, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([1]),
  });
}

export function cancelStreamInstruction(
  programId: PublicKey,
  sender: PublicKey,
  stream: PublicKey,
  vault: PublicKey,
  recipientTokenAccount: PublicKey,
  senderTokenAccount: PublicKey
): TransactionInstruction {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: sender, isSigner: true, isWritable: true },
      { pubkey: stream, isSigner: false, isWritable: true },
      { pubkey: vault, isSigner: false, isWritable: true },
      { pubkey: recipientTokenAccount, isSigner: false, isWritable: true },
      { pubkey: senderTokenAccount, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SYSVAR_CLOCK_PUBKEY, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([2]),
  });
}

function readPubkey(data: Buffer, off: number): PublicKey {
  return new PublicKey(data.subarray(off, off + 32));
}

/** Parse a Stream account. Returns null when the account does not exist. */
export async function getStream(
  connection: Connection,
  address: PublicKey
): Promise<StreamData | null> {
  const info = await connection.getAccountInfo(address);
  if (!info || info.data.length !== STREAM_LEN) return null;
  const d = info.data;
  return {
    address,
    sender: readPubkey(d, STREAM_OFFSETS.sender),
    recipient: readPubkey(d, STREAM_OFFSETS.recipient),
    mint: readPubkey(d, STREAM_OFFSETS.mint),
    vault: readPubkey(d, STREAM_OFFSETS.vault),
    deposited: d.readBigUInt64LE(STREAM_OFFSETS.deposited),
    withdrawn: d.readBigUInt64LE(STREAM_OFFSETS.withdrawn),
    ratePerSec: d.readBigUInt64LE(STREAM_OFFSETS.ratePerSec),
    startTs: d.readBigInt64LE(STREAM_OFFSETS.startTs),
    endTs: d.readBigInt64LE(STREAM_OFFSETS.endTs),
    cliffTs: d.readBigInt64LE(STREAM_OFFSETS.cliffTs),
    nonce: d.readBigUInt64LE(STREAM_OFFSETS.nonce),
    bump: d.readUInt8(STREAM_OFFSETS.bump),
  };
}

/** All streams where `wallet` is the sender or the recipient. */
export async function getStreamsByWallet(
  connection: Connection,
  programId: PublicKey,
  wallet: PublicKey
): Promise<StreamData[]> {
  const out: StreamData[] = [];
  for (const offset of [STREAM_OFFSETS.sender, STREAM_OFFSETS.recipient]) {
    const accounts = await connection.getProgramAccounts(programId, {
      filters: [
        { dataSize: STREAM_LEN },
        { memcmp: { offset, bytes: wallet.toBase58() } },
      ],
    });
    for (const a of accounts) {
      const s = await getStream(connection, a.pubkey);
      if (s && !out.some((x) => x.address.equals(s.address))) out.push(s);
    }
  }
  return out;
}

/** Tokens unlocked as of `nowTs` (unix seconds), capped at deposited. */
export function vestedAmount(s: StreamData, nowTs: bigint): bigint {
  if (nowTs < s.cliffTs) return 0n;
  const elapsed = nowTs - s.startTs > 0n ? nowTs - s.startTs : 0n;
  const raw = s.ratePerSec * elapsed;
  return raw < s.deposited ? raw : s.deposited;
}

/** Tokens the recipient can pull right now. */
export function withdrawableAmount(s: StreamData, nowTs: bigint): bigint {
  const vested = vestedAmount(s, nowTs);
  return vested - s.withdrawn;
}
