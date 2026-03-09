#!/bin/bash
set -e

# Configuration
NETWORK="${1:-regtest}"
# BITCOIN_CLI="/Users/julian/Code/bitcoin/gsr_with_cat_in_baseleaf/build/bin/bitcoin-cli"
# BITCOIND="/Users/julian/Code/bitcoin/gsr_with_cat_in_baseleaf/build/bin/bitcoind"
BITCOIN_CLI="/Users/julian/Code/bitcoin/gsr/build/bin/bitcoin-cli"
BITCOIND="/Users/julian/Code/bitcoin/gsr/build/bin/bitcoind"

echo "=== Bitcoin Circle STARK Demo ==="
echo "Network: $NETWORK"
echo

#$BITCOIN_CLI -"$NETWORK" stop
$BITCOIND --daemon -"$NETWORK" --maxmempool=1000 > /dev/null


# Build the demo
echo "[1/8] Building demo..."
cargo build --bin demo --quiet

# Get initial instructions to extract addresses and amount
echo "[2/8] Getting demo parameters..."
OUTPUT=$(./target/debug/demo -n "$NETWORK")


# Create wallet and fund it if not already done
# Try to create wallet, ignore error if it already exists
if ! $BITCOIN_CLI -"$NETWORK" createwallet "starkdemo" > /dev/null 2>&1; then
    $BITCOIN_CLI -"$NETWORK" loadwallet "starkdemo" > /dev/null 2>&1 || true
fi
$BITCOIN_CLI -"$NETWORK" generatetoaddress 301 "$($BITCOIN_CLI -"$NETWORK" getnewaddress)" > /dev/null

# Parse the required BTC amount
AMOUNT=$(echo "$OUTPUT" | grep -o 'prepare [0-9.]*' | grep -o '[0-9.]*')
echo "    Required amount: $AMOUNT BTC"

# Parse program and caboose addresses
PROGRAM_ADDR=$(echo "$OUTPUT" | grep -oE 'bcrt1p[a-z0-9]+' | head -1)
CABOOSE_ADDR=$(echo "$OUTPUT" | grep -oE 'bcrt1q[a-z0-9]+' | head -1)
PROGRAM_AMOUNT=$(echo "$OUTPUT" | grep -oE '"bcrt1p[^"]+":"?[0-9.]+' | grep -oE '[0-9]+\.[0-9]+')

echo "    Program address: ${PROGRAM_ADDR:0:20}..."
echo "    Caboose address: ${CABOOSE_ADDR:0:20}..."

# Create funding UTXO
echo "[3/8] Creating funding UTXO..."
WALLET_ADDR=$($BITCOIN_CLI -$NETWORK getnewaddress)
FUNDING_TXID=$($BITCOIN_CLI -$NETWORK sendtoaddress "$WALLET_ADDR" "$AMOUNT")
echo "    Funding txid: $FUNDING_TXID"

# Find the vout by looking at the transaction outputs
# The funding UTXO is the one we control (sent to our wallet address)
TX_INFO=$($BITCOIN_CLI -$NETWORK gettransaction "$FUNDING_TXID")

# Try to find vout for our wallet address in the details
VOUT=$(echo "$TX_INFO" | grep -B5 "\"address\": \"$WALLET_ADDR\"" | grep '"vout"' | grep -oE '[0-9]+' | head -1)

# Fallback: try to find any "receive" category with matching amount
if [ -z "$VOUT" ]; then
    VOUT=$(echo "$TX_INFO" | grep -B5 '"category": "receive"' | grep '"vout"' | grep -oE '[0-9]+' | head -1)
fi

# Final fallback to 0
[ -z "$VOUT" ] && VOUT=0
echo "    Vout: $VOUT"

# Create raw transaction (sequence 4294967293 = 0xfffffffd for RBF, required by covenant)
echo "[4/8] Creating and signing initial transaction..."
RAW_TX=$($BITCOIN_CLI -$NETWORK createrawtransaction \
    "[{\"txid\":\"$FUNDING_TXID\", \"vout\": $VOUT, \"sequence\": 4294967293}]" \
    "[{\"$PROGRAM_ADDR\":$PROGRAM_AMOUNT}, {\"$CABOOSE_ADDR\":0.0000033}]")

SIGNED_TX=$($BITCOIN_CLI -$NETWORK signrawtransactionwithwallet "$RAW_TX" | grep '"hex"' | cut -d'"' -f4)
INITIAL_TXID=$($BITCOIN_CLI -$NETWORK sendrawtransaction "$SIGNED_TX")
echo "    Initial program txid: $INITIAL_TXID"

# Mine a block to confirm initial transaction
echo "[5/8] Mining initial confirmation block..."
$BITCOIN_CLI -$NETWORK generatetoaddress 1 "$($BITCOIN_CLI -$NETWORK getnewaddress)" > /dev/null

# Verify the transaction is confirmed
CONFIRMATIONS=$($BITCOIN_CLI -$NETWORK gettransaction "$INITIAL_TXID" | grep '"confirmations"' | grep -oE '[0-9]+' | head -1)
if [ "$CONFIRMATIONS" -lt 1 ]; then
    echo "    Error: Initial transaction not confirmed!"
    exit 1
fi
echo "    Initial transaction confirmed ($CONFIRMATIONS confirmations)"

# Clear old demo files
rm -rf demo/

# Run the demo to generate transaction
echo "[6/8] Generating transaction..."
echo
./target/debug/demo -n "$NETWORK" -f "$FUNDING_TXID" -i "$INITIAL_TXID" --funding-tx-vout "$VOUT"
echo

# Broadcast the transaction (use -stdin to avoid ARG_MAX limits on large txs)
echo "[7/8] Broadcasting transaction..."
TX_HEX=$(cat demo/tx-1.txt)
set +e
TXID=$(echo "$TX_HEX" | $BITCOIN_CLI -"$NETWORK" -stdin sendrawtransaction 2>&1)
RESULT=$?
set -e
if [ $RESULT -ne 0 ]; then
    echo "    Broadcast failed: $TXID"
    exit 1
fi
echo "    Broadcast: $TXID"

DECODED=$(echo "$TX_HEX" | $BITCOIN_CLI -"$NETWORK" -stdin decoderawtransaction 2>/dev/null)
WEIGHT=$(echo "$DECODED" | grep '"weight"' | head -1 | grep -oE '[0-9]+')
VSIZE=$(echo "$DECODED" | grep '"vsize"' | head -1 | grep -oE '[0-9]+')

# Mine a block to confirm
echo "[8/8] Mining confirmation block..."
MINER_ADDR=$($BITCOIN_CLI -$NETWORK getnewaddress)
$BITCOIN_CLI -$NETWORK generatetoaddress 1 "$MINER_ADDR" > /dev/null
echo "    Done"

echo
echo "=== Transaction Statistics ==="
echo "Weight:  $WEIGHT WU"
echo "Vsize:   $VSIZE vB"
for RATE in 2 7 15; do
    FEE=$((VSIZE * RATE))
    echo "Est. fee @${RATE} sat/vB: $FEE sat ($(echo "scale=8; $FEE / 100000000" | bc) BTC)"
done

echo
echo "=== Demo complete! ==="

$BITCOIN_CLI -"$NETWORK" stop