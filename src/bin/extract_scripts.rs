use bitcoin::consensus::Decodable;
use bitcoin::Transaction;
use std::fs;
use std::io::Write;

const INPUT_DIR: &str = "./demo";
const BASE_OUTPUT_DIR: &str = "./scripts";

fn main() {
    // Optional first argument: variant name (e.g. "assume-gsr")
    // Output goes to ./scripts/<variant>/ or ./scripts/default/
    let variant = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "default".to_string());
    let output_dir = format!("{}/{}", BASE_OUTPUT_DIR, variant);

    // Create output directory
    fs::create_dir_all(&output_dir).unwrap();

    // Get all tx files sorted by number
    let mut tx_files: Vec<_> = fs::read_dir(INPUT_DIR)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.starts_with("tx-") && s.ends_with(".txt"))
                .unwrap_or(false)
        })
        .collect();

    tx_files.sort_by(|a, b| {
        let num_a: u32 = a
            .file_name()
            .to_str()
            .unwrap()
            .strip_prefix("tx-")
            .unwrap()
            .strip_suffix(".txt")
            .unwrap()
            .parse()
            .unwrap();
        let num_b: u32 = b
            .file_name()
            .to_str()
            .unwrap()
            .strip_prefix("tx-")
            .unwrap()
            .strip_suffix(".txt")
            .unwrap()
            .parse()
            .unwrap();
        num_a.cmp(&num_b)
    });

    println!("Found {} transaction files", tx_files.len());

    for (i, entry) in tx_files.iter().enumerate() {
        let tx_hex = fs::read_to_string(entry.path()).unwrap();
        let tx_bytes = hex::decode(tx_hex.trim()).unwrap();
        let tx: Transaction = Transaction::consensus_decode(&mut tx_bytes.as_slice()).unwrap();

        // For taproot script-path spend, witness structure is:
        // [stack elements...] [script] [control block]
        // The script is the second-to-last element
        // The control block is the last element

        if let Some(input) = tx.input.first() {
            let witness = &input.witness;
            let witness_len = witness.len();

            if witness_len >= 2 {
                // Script is second-to-last element
                let script = &witness[witness_len - 2];
                let control_block = &witness[witness_len - 1];

                // Extract witness stack (all elements except script and control block)
                let stack_elements: Vec<&[u8]> = witness.iter().take(witness_len - 2).collect();

                println!(
                    "TX {}: witness_len={}, stack_elements={}, script_size={}, control_block_size={}",
                    i + 1,
                    witness_len,
                    stack_elements.len(),
                    script.len(),
                    control_block.len()
                );

                // Write script to file
                let script_path = format!("{}/script-{}.hex", output_dir, i + 1);
                let mut script_file = fs::File::create(&script_path).unwrap();
                script_file
                    .write_all(hex::encode(script).as_bytes())
                    .unwrap();

                // Write witness stack to file (each element on a separate line)
                let stack_path = format!("{}/stack-{}.hex", output_dir, i + 1);
                let mut stack_file = fs::File::create(&stack_path).unwrap();
                for elem in &stack_elements {
                    writeln!(stack_file, "{}", hex::encode(elem)).unwrap();
                }

                // Write control block to file
                let cb_path = format!("{}/control-{}.hex", output_dir, i + 1);
                let mut cb_file = fs::File::create(&cb_path).unwrap();
                cb_file
                    .write_all(hex::encode(control_block).as_bytes())
                    .unwrap();
            } else {
                println!(
                    "TX {}: unexpected witness structure (len={})",
                    i + 1,
                    witness_len
                );
            }
        }
    }

    println!("\nScripts extracted to {}/", output_dir);
}
