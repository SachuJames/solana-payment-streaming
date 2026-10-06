import { useCallback, useEffect, useMemo, useState } from "react";
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
  getAssociatedTokenAddress,
  getMinimumBalanceForRentExemptMint,
  MINT_SIZE,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import bs58 from "./bs58";
import {
  cancelStreamInstruction,
  createStreamInstruction,
  findStreamPDA,
  getStream,
  getStreamsByWallet,
  StreamData,
  vestedAmount,
  withdrawableAmount,
  withdrawInstruction,
} from "@solstream/sdk";

const RPC_URL = "http://127.0.0.1:8899";

function fmt(n: bigint): string {
  return n.toLocaleString("en-US");
}

export default function App() {
  const [programId, setProgramId] = useState("");
  const [secretInput, setSecretInput] = useState("");
  const [keypair, setKeypair] = useState<Keypair | null>(null);
  const [streams, setStreams] = useState<StreamData[]>([]);
  const [now, setNow] = useState(() => BigInt(Math.floor(Date.now() / 1000)));
  const [status, setStatus] = useState("");
  const [mint, setMint] = useState("");

  // Create form
  const [recipient, setRecipient] = useState("");
  const [amount, setAmount] = useState("1000000");
  const [rate, setRate] = useState("1000");
  const [duration, setDuration] = useState("1000");
  const [cliff, setCliff] = useState("0");

  const connection = useMemo(() => new Connection(RPC_URL, "confirmed"), []);
  const pid = useMemo(() => {
    try {
      return programId ? new PublicKey(programId) : null;
    } catch {
      return null;
    }
  }, [programId]);

  useEffect(() => {
    const t = setInterval(() => setNow(BigInt(Math.floor(Date.now() / 1000))), 2000);
    return () => clearInterval(t);
  }, []);

  const refresh = useCallback(async () => {
    if (!pid || !keypair) return;
    try {
      const list = await getStreamsByWallet(connection, pid, keypair.publicKey);
      setStreams(list);
    } catch (e) {
      setStatus(`refresh failed: ${String(e)}`);
    }
  }, [connection, pid, keypair]);

  useEffect(() => {
    refresh();
  }, [refresh, now]);

  function loadKeypair() {
    try {
      const secret = bs58.decode(secretInput.trim());
      setKeypair(Keypair.fromSecretKey(secret));
      setStatus("keypair loaded (dev only, never use a funded key)");
    } catch {
      setStatus("invalid base58 secret key");
    }
  }

  function generateKeypair() {
    const kp = Keypair.generate();
    setKeypair(kp);
    setSecretInput(bs58.encode(kp.secretKey));
    setStatus("new keypair generated (dev only)");
  }

  async function airdrop() {
    if (!keypair) return;
    const sig = await connection.requestAirdrop(keypair.publicKey, 2 * LAMPORTS_PER_SOL);
    await connection.confirmTransaction(sig, "confirmed");
    setStatus("airdrop confirmed");
  }

  async function createTestMint() {
    if (!keypair) return;
    setStatus("creating mint...");
    const mintKp = Keypair.generate();
    const rent = await getMinimumBalanceForRentExemptMint(connection);
    const tx = new Transaction().add(
      SystemProgram.createAccount({
        fromPubkey: keypair.publicKey,
        newAccountPubkey: mintKp.publicKey,
        lamports: rent,
        space: MINT_SIZE,
        programId: TOKEN_PROGRAM_ID,
      }),
      createInitializeMintInstruction(mintKp.publicKey, 6, keypair.publicKey, null)
    );
    await sendAndConfirmTransaction(connection, tx, [keypair, mintKp]);
    const ata = await createAssociatedTokenAccount(
      connection,
      keypair,
      mintKp.publicKey,
      keypair.publicKey
    );
    const mintTx = new Transaction().add(
      createMintToInstruction(mintKp.publicKey, ata, keypair.publicKey, 100_000_000_000n)
    );
    await sendAndConfirmTransaction(connection, mintTx, [keypair]);
    setMint(mintKp.publicKey.toBase58());
    setStatus(`mint created: ${mintKp.publicKey.toBase58()}`);
  }

  async function onCreate() {
    if (!pid || !keypair || !mint) {
      setStatus("set program id, keypair, and mint first");
      return;
    }
    try {
      setStatus("creating stream...");
      const mintPk = new PublicKey(mint);
      const recipientPk = new PublicKey(recipient);
      const senderAta = await getAssociatedTokenAddress(mintPk, keypair.publicKey);
      const startTs = now;
      const dur = BigInt(duration);
      const params = {
        sender: keypair.publicKey,
        recipient: recipientPk,
        mint: mintPk,
        senderTokenAccount: senderAta,
        amount: BigInt(amount),
        ratePerSec: BigInt(rate),
        startTs,
        endTs: startTs + dur,
        cliffTs: startTs + BigInt(cliff),
        nonce: BigInt(Date.now()),
      };
      const ix = await createStreamInstruction(pid, params);
      await sendAndConfirmTransaction(connection, new Transaction().add(ix), [keypair]);
      const [stream] = await findStreamPDA(pid, keypair.publicKey, recipientPk, mintPk, params.nonce);
      const s = await getStream(connection, stream);
      setStatus(s ? `stream created: ${stream.toBase58()}` : "created but could not fetch");
      refresh();
    } catch (e) {
      setStatus(`create failed: ${String(e)}`);
    }
  }

  async function onWithdraw(s: StreamData) {
    if (!pid || !keypair) return;
    try {
      const recipientAta = await getAssociatedTokenAddress(s.mint, s.recipient);
      const ix = withdrawInstruction(pid, s.recipient, s.address, s.vault, recipientAta);
      await sendAndConfirmTransaction(connection, new Transaction().add(ix), [keypair]);
      setStatus("withdraw confirmed");
      refresh();
    } catch (e) {
      setStatus(`withdraw failed: ${String(e)}`);
    }
  }

  async function onCancel(s: StreamData) {
    if (!pid || !keypair) return;
    try {
      const recipientAta = await getAssociatedTokenAddress(s.mint, s.recipient);
      const senderAta = await getAssociatedTokenAddress(s.mint, s.sender);
      const ix = cancelStreamInstruction(pid, s.sender, s.address, s.vault, recipientAta, senderAta);
      await sendAndConfirmTransaction(connection, new Transaction().add(ix), [keypair]);
      setStatus("cancel confirmed");
      refresh();
    } catch (e) {
      setStatus(`cancel failed: ${String(e)}`);
    }
  }

  return (
    <div className="page">
      <header>
        <h1>SolStream</h1>
        <p className="sub">SPL token payment streaming on Solana</p>
      </header>

      <section className="card">
        <h2>Setup</h2>
        <label>
          Program ID
          <input value={programId} onChange={(e) => setProgramId(e.target.value)} placeholder="Deployed program id" />
        </label>
        <label>
          Secret key (base58, dev only, never a funded key)
          <input value={secretInput} onChange={(e) => setSecretInput(e.target.value)} placeholder="paste or generate" />
        </label>
        <div className="row">
          <button onClick={loadKeypair}>Load key</button>
          <button onClick={generateKeypair}>Generate</button>
          <button onClick={airdrop} disabled={!keypair}>Airdrop SOL</button>
          <button onClick={createTestMint} disabled={!keypair}>Create test mint</button>
        </div>
        {keypair && <p className="addr">wallet: {keypair.publicKey.toBase58()}</p>}
        {mint && <p className="addr">mint: {mint}</p>}
      </section>

      <section className="card">
        <h2>Create stream</h2>
        <div className="grid">
          <label>Recipient<input value={recipient} onChange={(e) => setRecipient(e.target.value)} /></label>
          <label>Amount<input value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
          <label>Rate / sec<input value={rate} onChange={(e) => setRate(e.target.value)} /></label>
          <label>Duration (sec)<input value={duration} onChange={(e) => setDuration(e.target.value)} /></label>
          <label>Cliff (sec)<input value={cliff} onChange={(e) => setCliff(e.target.value)} /></label>
        </div>
        <button onClick={onCreate}>Create stream</button>
      </section>

      <section className="card">
        <h2>Streams ({streams.length})</h2>
        {streams.map((s) => {
          const vested = vestedAmount(s, now);
          const canPull = withdrawableAmount(s, now);
          const isRecipient = keypair?.publicKey.equals(s.recipient);
          const isSender = keypair?.publicKey.equals(s.sender);
          return (
            <div key={s.address.toBase58()} className="stream">
              <div className="srow"><span>stream</span><code>{s.address.toBase58().slice(0, 12)}...</code></div>
              <div className="srow"><span>from</span><code>{s.sender.toBase58().slice(0, 12)}...</code></div>
              <div className="srow"><span>to</span><code>{s.recipient.toBase58().slice(0, 12)}...</code></div>
              <div className="srow"><span>deposited</span><b>{fmt(s.deposited)}</b></div>
              <div className="srow"><span>withdrawn</span><b>{fmt(s.withdrawn)}</b></div>
              <div className="srow"><span>vested</span><b>{fmt(vested)}</b></div>
              <div className="srow"><span>withdrawable</span><b className="hot">{fmt(canPull)}</b></div>
              <div className="row">
                {isRecipient && canPull > 0n && <button onClick={() => onWithdraw(s)}>Withdraw</button>}
                {isSender && <button className="danger" onClick={() => onCancel(s)}>Cancel</button>}
              </div>
            </div>
          );
        })}
      </section>

      {status && <p className="status">{status}</p>}
    </div>
  );
}
