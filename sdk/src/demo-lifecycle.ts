import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  sendAndConfirmTransaction,
  SystemProgram,
  Transaction,
} from "@solana/web3.js";
import {
  createAssociatedTokenAccount,
  createInitializeMintInstruction,
  createMintToInstruction,
  getMinimumBalanceForRentExemptMint,
  MINT_SIZE,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import {
  cancelStreamInstruction,
  createStreamInstruction,
  findStreamPDA,
  findVaultPDA,
  getStream,
  vestedAmount,
  withdrawInstruction,
} from "./index";

const PROGRAM_ID = new PublicKey(process.env.SOLSTREAM_PROGRAM_ID ?? "");

let failures = 0;
function check(name: string, cond: boolean, detail = "") {
  if (cond) {
    console.log(`  PASS: ${name}`);
  } else {
    failures++;
    console.log(`  FAIL: ${name} ${detail}`);
  }
}

async function main() {
  const connection = new Connection("http://127.0.0.1:8899", "confirmed");
  const payer = Keypair.generate();
  const sender = Keypair.generate();
  const recipient = Keypair.generate();

  console.log("Funding wallets...");
  for (const kp of [payer, sender, recipient]) {
    const sig = await connection.requestAirdrop(kp.publicKey, 2 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig, "confirmed");
  }

  console.log("Creating test mint...");
  const mint = Keypair.generate();
  const mintRent = await getMinimumBalanceForRentExemptMint(connection);
  const createMintTx = new Transaction().add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: mint.publicKey,
      lamports: mintRent,
      space: MINT_SIZE,
      programId: TOKEN_PROGRAM_ID,
    }),
    createInitializeMintInstruction(mint.publicKey, 6, payer.publicKey, null)
  );
  await sendAndConfirmTransaction(connection, createMintTx, [payer, mint]);

  const senderAta = await createAssociatedTokenAccount(
    connection,
    payer,
    mint.publicKey,
    sender.publicKey
  );
  const recipientAta = await createAssociatedTokenAccount(
    connection,
    payer,
    mint.publicKey,
    recipient.publicKey
  );
  const mintToTx = new Transaction().add(
    createMintToInstruction(mint.publicKey, senderAta, payer.publicKey, 10_000_000_000n)
  );
  await sendAndConfirmTransaction(connection, mintToTx, [payer]);

  // Stream 1,000,000 units over 100 seconds, started 40s ago: ~40% vested.
  const now = BigInt(Math.floor(Date.now() / 1000));
  const params = {
    sender: sender.publicKey,
    recipient: recipient.publicKey,
    mint: mint.publicKey,
    senderTokenAccount: senderAta,
    amount: 1_000_000n,
    ratePerSec: 10_000n,
    startTs: now - 40n,
    endTs: now + 60n,
    cliffTs: now - 40n,
    nonce: 1n,
  };

  console.log("Creating stream...");
  const createIx = await createStreamInstruction(PROGRAM_ID, params);
  await sendAndConfirmTransaction(
    connection,
    new Transaction().add(createIx),
    [sender]
  );
  const [stream] = await findStreamPDA(
    PROGRAM_ID,
    sender.publicKey,
    recipient.publicKey,
    mint.publicKey,
    1n
  );
  const [vault] = await findVaultPDA(PROGRAM_ID, stream);
  const s0 = await getStream(connection, stream);
  check("stream account exists", s0 !== null);
  check("vault holds full amount", s0 !== null && s0.deposited === 1_000_000n);

  console.log("Withdrawing vested tokens...");
  const withdrawIx = withdrawInstruction(
    PROGRAM_ID,
    recipient.publicKey,
    stream,
    vault,
    recipientAta
  );
  await sendAndConfirmTransaction(
    connection,
    new Transaction().add(withdrawIx),
    [recipient]
  );
  const s1 = (await getStream(connection, stream))!;
  const t1 = BigInt(Math.floor(Date.now() / 1000));
  const expected = vestedAmount({ ...s1, withdrawn: 0n }, t1);
  const recBal = await connection.getTokenAccountBalance(recipientAta);
  check(
    "recipient received vested amount",
    BigInt(recBal.value.amount) === s1.withdrawn && s1.withdrawn >= expected - 20_000n,
    `got ${recBal.value.amount}, withdrawn=${s1.withdrawn}`
  );

  console.log("Cancelling stream...");
  const cancelIx = cancelStreamInstruction(
    PROGRAM_ID,
    sender.publicKey,
    stream,
    vault,
    recipientAta,
    senderAta
  );
  await sendAndConfirmTransaction(
    connection,
    new Transaction().add(cancelIx),
    [sender]
  );
  const s2 = await getStream(connection, stream);
  const vaultInfo = await connection.getAccountInfo(vault);
  check("stream account closed", s2 === null);
  check("vault account closed", vaultInfo === null);

  const recFinal = BigInt((await connection.getTokenAccountBalance(recipientAta)).value.amount);
  const sendFinal = BigInt((await connection.getTokenAccountBalance(senderAta)).value.amount);
  check(
    "all tokens accounted for",
    recFinal + sendFinal === 10_000_000_000n,
    `recipient=${recFinal} sender=${sendFinal}`
  );

  // Negative case: withdrawing from a closed stream must fail.
  let threw = false;
  try {
    await sendAndConfirmTransaction(
      connection,
      new Transaction().add(
        withdrawInstruction(PROGRAM_ID, recipient.publicKey, stream, vault, recipientAta)
      ),
      [recipient]
    );
  } catch {
    threw = true;
  }
  check("withdraw on closed stream fails", threw);

  console.log(failures === 0 ? "\nDEMO RESULT: ALL CHECKS PASSED" : `\nDEMO RESULT: ${failures} CHECKS FAILED`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error("demo crashed:", e);
  process.exit(1);
});
