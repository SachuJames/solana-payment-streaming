#!/usr/bin/env bash
# SolStream end-to-end demo.
#
# Starts a local solana-test-validator, builds the program, deploys it,
# creates a test SPL mint, then runs a full stream lifecycle
# (create -> partial withdraw -> cancel) through the TypeScript SDK.
# Everything happens locally; no mainnet, no external faucets.
#
# The 4.x validator needs the io_uring syscall, which some sandboxes block,
# so the demo prefers the 2.1.x toolchain when it is available and falls back
# to whatever solana-test-validator is on PATH otherwise.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"

PINNED_211="$HOME/workspace/.solana/solana-2.1.21/solana-release/bin"
if [ -x "$PINNED_211/solana-test-validator" ]; then
  SOLANA_BIN="$PINNED_211"
else
  SOLANA_BIN="$(dirname "$(command -v solana-test-validator)")"
fi
PINNED_43="$HOME/workspace/.solana/solana-release/bin"
if [ -x "$PINNED_43/cargo-build-sbf" ]; then
  SBF_BIN="$PINNED_43"
else
  SBF_BIN="$(dirname "$(command -v cargo-build-sbf)")"
fi
# cargo-build-sbf comes from the 4.x toolchain (its Cargo understands the
# dependency tree); the validator and CLI come from 2.1.x where available.
export PATH="$HOME/.cargo/bin:$SBF_BIN:$SOLANA_BIN:$PATH"
VALIDATOR="$SOLANA_BIN/solana-test-validator"
SOLANA="$SOLANA_BIN/solana"
KEYGEN="$SOLANA_BIN/solana-keygen"
RPC="http://127.0.0.1:8899"

# Sandbox workaround: some sandboxes block UDP sendto(), which breaks the
# validator's QUIC/UDP traffic (transactions never land). scripts/
# udp_sendto_shim.c is a small LD_PRELOAD shim that rewrites sendto() as
# connect()+send(). It is only built and used when sendto() is blocked here.
udp_sendto_blocked() {
  ! python3 -c "import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.sendto(b'x', ('127.0.0.1', 9))" 2>/dev/null
}
if udp_sendto_blocked; then
  if [ ! -f "$ROOT/scripts/udp_shim.so" ] && [ -f "$ROOT/scripts/udp_sendto_shim.c" ] && command -v gcc >/dev/null 2>&1; then
    echo "    UDP sendto() is blocked; building preload shim..."
    gcc -shared -fPIC -o "$ROOT/scripts/udp_shim.so" "$ROOT/scripts/udp_sendto_shim.c" -ldl -lpthread
  fi
  if [ -f "$ROOT/scripts/udp_shim.so" ]; then
    export LD_PRELOAD="$ROOT/scripts/udp_shim.so"
    echo "    using UDP shim: $ROOT/scripts/udp_shim.so"
  else
    echo "    WARNING: UDP sendto() is blocked and no shim is available; transactions will not land."
  fi
fi

TMPDIR_DEMO="$(mktemp -d)"
VALIDATOR_PID=""
RESULT="FAILED"

cleanup() {
  if [ -n "$VALIDATOR_PID" ] && kill -0 "$VALIDATOR_PID" 2>/dev/null; then
    kill "$VALIDATOR_PID" 2>/dev/null || true
    wait "$VALIDATOR_PID" 2>/dev/null || true
  fi
  rm -rf "$TMPDIR_DEMO"
  echo "----------------------------------------"
  echo "DEMO RESULT: $RESULT"
}
trap cleanup EXIT

echo "== SolStream demo =="
echo "[1/5] Starting solana-test-validator..."
cd "$TMPDIR_DEMO"
$VALIDATOR --reset -q --ledger "$TMPDIR_DEMO/ledger" >validator.log 2>&1 &
VALIDATOR_PID=$!
cd "$ROOT"
for i in $(seq 1 60); do
  if $SOLANA cluster-version --url "$RPC" >/dev/null 2>&1; then
    echo "    validator is up"
    break
  fi
  if [ "$i" -eq 60 ]; then
    echo "    validator failed to start; see $TMPDIR_DEMO/validator.log"
    exit 1
  fi
  sleep 2
done

echo "[2/5] Building program..."
(cd "$ROOT/program" && cargo build-sbf)

echo "[3/5] Deploying program..."
FAUCET_KP="$TMPDIR_DEMO/ledger/faucet-keypair.json"
PAYER="$TMPDIR_DEMO/payer.json"
$KEYGEN new --no-bip39-passphrase -s -o "$PAYER" >/dev/null 2>&1
$SOLANA transfer --url "$RPC" --keypair "$FAUCET_KP" \
  "$($KEYGEN pubkey "$PAYER")" 5 --allow-unfunded-recipient >/dev/null 2>&1
DEPLOY_OUT=$($SOLANA program deploy "$ROOT/program/target/deploy/solstream.so" \
  --url "$RPC" --keypair "$PAYER" --output json 2>/dev/null)
PROGRAM_ID=$(echo "$DEPLOY_OUT" | python3 -c "import json,sys; print(json.load(sys.stdin)['programId'])")
echo "    program id: $PROGRAM_ID"

echo "[4/5] Running lifecycle demo via the TypeScript SDK..."
(cd "$ROOT/sdk" && npm run build >/dev/null 2>&1)
(cd "$ROOT/sdk" && SOLSTREAM_PROGRAM_ID="$PROGRAM_ID" FAUCET_KEYPAIR="$FAUCET_KP" npm run demo)

RESULT="ALL CHECKS PASSED"
