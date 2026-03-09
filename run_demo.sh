#!/bin/bash
set -e

# Configuration
NETWORK="${1:-regtest}"
FEATURES="${2:-}"  # Pass features: "assume-gsr"
# BITCOIN_CLI="/Users/julian/Code/bitcoin/gsr_with_cat_in_baseleaf/build/bin/bitcoin-cli"
# BITCOIND="/Users/julian/Code/bitcoin/gsr_with_cat_in_baseleaf/build/bin/bitcoind"
BITCOIN_CLI="/Users/julian/Code/bitcoin/gsr/build/bin/bitcoin-cli"
BITCOIND="/Users/julian/Code/bitcoin/gsr/build/bin/bitcoind"

echo "=== Bitcoin Circle STARK Demo ==="
echo "Network: $NETWORK"
[ -n "$FEATURES" ] && echo "Features: $FEATURES"
echo

# Example usage:
#   ./run_demo.sh regtest                                    # Default mode (72 transactions)
#   ./run_demo.sh regtest assume-gsr                      # OP_MUL + OP_MOD (1 transaction)

#$BITCOIN_CLI -"$NETWORK" stop
$BITCOIND --daemon -"$NETWORK" --maxmempool=1000 > /dev/null


# Build the demo
echo "[1/8] Building demo..."
if [ -n "$FEATURES" ]; then
    cargo build --features "$FEATURES" --bin demo --quiet
else
    cargo build --bin demo --quiet
fi

# Get initial instructions to extract addresses and amount
echo "[2/8] Getting demo parameters..."
OUTPUT=$(./target/debug/demo -n "$NETWORK")


# Create wallet and fund it if not already done
# Try to create wallet, ignore error if it already exists
if ! $BITCOIN_CLI -"$NETWORK" createwallet "starkdemo" 2>/dev/null; then
    echo "    Wallet 'starkdemo' already exists or failed to create, loading it..."
    $BITCOIN_CLI -"$NETWORK" loadwallet "starkdemo" 2>/dev/null || true
fi
$BITCOIN_CLI -"$NETWORK" generatetoaddress 301 "$($BITCOIN_CLI -"$NETWORK" getnewaddress)"

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

# Run the demo to generate transactions
echo "[6/8] Generating transactions..."
echo
./target/debug/demo -n "$NETWORK" -f "$FUNDING_TXID" -i "$INITIAL_TXID" --funding-tx-vout "$VOUT"

TX_COUNT=$(ls -1 demo/tx-*.txt 2>/dev/null | wc -l | tr -d ' ')
echo
echo "    Generated $TX_COUNT transactions"

# Broadcast all transactions to mempool (in order: tx-1, tx-2, ..., tx-N)
# Mine a block every BATCH_SIZE transactions to avoid "too-large-cluster" mempool rejection
BATCH_SIZE=5
echo "[7/8] Broadcasting transactions (mining every $BATCH_SIZE txs)..."
BROADCAST_COUNT=0
FAILED=0
MINER_ADDR=$($BITCOIN_CLI -$NETWORK getnewaddress)

# Disable exit-on-error for broadcast loop (we handle errors ourselves)
TOTAL_WEIGHT=0
TOTAL_VSIZE=0
MIN_WEIGHT=999999999
MAX_WEIGHT=0

set +e
for i in $(seq 1 "$TX_COUNT"); do
    tx_file="demo/tx-${i}.txt"
    if [ -f "$tx_file" ]; then
        TX_HEX=$(cat "$tx_file")

        # Broadcast the transaction
        TXID=$($BITCOIN_CLI -"$NETWORK" sendrawtransaction "$TX_HEX" 2>&1)
        RESULT=$?

        if [ $RESULT -eq 0 ]; then
            BROADCAST_COUNT=$((BROADCAST_COUNT + 1))

            # Collect weight stats from the raw hex before it gets mined
            DECODED=$($BITCOIN_CLI -"$NETWORK" decoderawtransaction "$TX_HEX" 2>/dev/null)
            if [ -n "$DECODED" ]; then
                WEIGHT=$(echo "$DECODED" | grep '"weight"' | head -1 | grep -oE '[0-9]+')
                VSIZE=$(echo "$DECODED" | grep '"vsize"' | head -1 | grep -oE '[0-9]+')
                if [ -n "$WEIGHT" ]; then
                    TOTAL_WEIGHT=$((TOTAL_WEIGHT + WEIGHT))
                    TOTAL_VSIZE=$((TOTAL_VSIZE + VSIZE))
                    [ "$WEIGHT" -lt "$MIN_WEIGHT" ] && MIN_WEIGHT=$WEIGHT
                    [ "$WEIGHT" -gt "$MAX_WEIGHT" ] && MAX_WEIGHT=$WEIGHT
                fi
            fi

            echo "    [$i/$TX_COUNT] Broadcast: ${TXID:0:20}..."
        else
            echo "    [$i/$TX_COUNT] Failed: $TXID"
            FAILED=1
            # Stop on first failure since subsequent transactions depend on previous ones
            break
        fi

        # Mine a block every BATCH_SIZE transactions to keep cluster size small
        if [ $((BROADCAST_COUNT % BATCH_SIZE)) -eq 0 ]; then
            $BITCOIN_CLI -$NETWORK generatetoaddress 1 "$MINER_ADDR" > /dev/null
            echo "    --- Mined block (confirmed $BROADCAST_COUNT txs so far) ---"
        fi
    else
        echo "    [$i/$TX_COUNT] File not found: $tx_file"
        FAILED=1
        break
    fi
done
set -e  # Re-enable exit-on-error

echo "    Successfully broadcast: $BROADCAST_COUNT/$TX_COUNT"

# Mine final block to confirm remaining transactions
echo "[8/8] Mining final confirmation block..."
$BITCOIN_CLI -$NETWORK generatetoaddress 1 "$MINER_ADDR" > /dev/null
echo "    Done"

# Print transaction weight statistics
echo
echo "=== Transaction Statistics ==="
if [ "$BROADCAST_COUNT" -gt 0 ] && [ "$TOTAL_WEIGHT" -gt 0 ]; then
    AVG_WEIGHT=$((TOTAL_WEIGHT / BROADCAST_COUNT))
    AVG_VSIZE=$((TOTAL_VSIZE / BROADCAST_COUNT))

    echo "Transactions:     $BROADCAST_COUNT"
    echo "Total weight:     $TOTAL_WEIGHT WU"
    echo "Total vsize:      $TOTAL_VSIZE vB"
    echo "Avg tx weight:    $AVG_WEIGHT WU"
    echo "Avg tx vsize:     $AVG_VSIZE vB"
    echo "Min tx weight:    $MIN_WEIGHT WU"
    echo "Max tx weight:    $MAX_WEIGHT WU"

    # Estimate total fee at common fee rates
    for RATE in 2 7 15; do
        TOTAL_FEE=$((TOTAL_VSIZE * RATE))
        echo "Est. total fee @${RATE} sat/vB: $TOTAL_FEE sat ($(echo "scale=8; $TOTAL_FEE / 100000000" | bc) BTC)"
    done
else
    echo "No weight data collected."
fi

echo
echo "=== Demo complete! ==="
echo "Transaction files saved in ./demo/"
echo "Total transactions: $TX_COUNT"
echo "Broadcast to mempool: $BROADCAST_COUNT"

# Show final mempool status
MEMPOOL_SIZE=$($BITCOIN_CLI -$NETWORK getmempoolinfo | grep '"size"' | grep -oE '[0-9]+')
echo "Mempool size: $MEMPOOL_SIZE (should be 0 after mining)"


$BITCOIN_CLI -"$NETWORK" stop
# 