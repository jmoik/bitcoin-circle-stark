use crate::dsl::plonk::hints::Hints;
use crate::treepp::*;
use crate::utils::hash;
use crate::OP_HINT;
use anyhow::Result;
use bitcoin::taproot::LeafVersion;
use bitcoin_script_dsl::compiler::Compiler;
use bitcoin_script_dsl::constraint_system::Element;
use bitcoin_script_dsl::ldm::LDM;
use covenants_gadgets::utils::stack_hash::StackHash;
use covenants_gadgets::CovenantProgram;
use sha2::digest::Update;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub type Witness = Vec<Vec<u8>>;

pub struct PlonkVerifierProgram {}

#[derive(Clone)]
pub struct PlonkVerifierInput {
    pub stack: Witness,
    pub hints: Witness,
}

impl From<PlonkVerifierInput> for Script {
    fn from(input: PlonkVerifierInput) -> Script {
        script! {
            for elem in input.stack {
                { elem }
            }
            for elem in input.hints {
                { elem }
            }
        }
    }
}

/// The state of the Plonk split program.
#[derive(Clone, Debug)]
pub struct PlonkVerifierState {
    /// The program counter.
    pub pc: usize,
    /// The hash of the stack.
    pub stack_hash: Vec<u8>,
    /// The stack from the execution.
    pub stack: Vec<Vec<u8>>,
}

impl From<PlonkVerifierState> for Script {
    fn from(v: PlonkVerifierState) -> Self {
        script! {
            { v.pc }
            { v.stack_hash }
        }
    }
}

pub struct PlonkAllInformation {
    pub script: Script,
    pub witness: Witness,
    pub output: Witness,
}

pub static PLONK_ALL_INFORMATION: OnceLock<PlonkAllInformation> = OnceLock::new();

impl PlonkAllInformation {
    pub fn get_input(&self) -> PlonkVerifierInput {
        PlonkVerifierInput {
            stack: vec![],
            hints: self.witness.clone(),
        }
    }
}

pub fn compute_all_information() -> PlonkAllInformation {
    let mut individual_scripts = vec![];
    let mut individual_witnesses = vec![];

    let hints = Hints::instance();
    let mut ldm = LDM::new();

    let num_to_str = |v: i32| -> Vec<u8> {
        if v == 0 {
            return vec![];
        }
        let bytes = (v as u64).to_le_bytes();
        let len = 8 - bytes.iter().rev().take_while(|&&b| b == 0).count();
        bytes[..len].to_vec()
    };

    let extract_witness = |program: &bitcoin_script_dsl::compiler::CompiledProgram,
                           num_to_str: &dyn Fn(i32) -> Vec<u8>|
     -> Witness {
        let mut witness = vec![];
        for entry in program.hint.iter() {
            match &entry {
                Element::Num(v) => {
                    witness.push(num_to_str(*v));
                }
                Element::Str(v) => {
                    witness.push(v.clone());
                }
            }
        }
        witness
    };

    for f in [
        super::part1_fiat_shamir1::generate_cs,
        super::part2_fiat_shamir2_and_constraint_num::generate_cs,
        super::part3_constraint_denom::generate_cs,
        super::part4_pair_vanishing_and_alphas::generate_cs,
        super::part5_column_line_coeffs1::generate_cs,
        super::part6_column_line_coeffs2::generate_cs,
        super::part7_column_line_coeffs3::generate_cs,
    ] {
        let cs = f(&hints, &mut ldm).unwrap();
        let program = Compiler::compile(cs).unwrap();
        let witness = extract_witness(&program, &num_to_str);
        individual_scripts.push(program.script);
        individual_witnesses.push(witness);
    }

    for query_idx in 0..8 {
        for f in [
            super::per_query_part1_folding::generate_cs,
            super::per_query_part2_num_trace::generate_cs,
            super::per_query_part3_num_constant::generate_cs,
            super::per_query_part4_num_composition::generate_cs,
            super::per_query_part5_num_interaction_shifted::generate_cs,
            super::per_query_part6_num_interaction1::generate_cs,
            super::per_query_part7_num_interaction2::generate_cs,
            super::per_query_part8_last_step::generate_cs,
        ] {
            let dsl = f(&hints, &mut ldm, query_idx).unwrap();
            let program = Compiler::compile(dsl).unwrap();
            let witness = extract_witness(&program, &num_to_str);
            individual_scripts.push(program.script);
            individual_witnesses.push(witness);
        }
    }

    {
        let cs = super::part8_cleanup::generate_cs(&hints, &mut ldm).unwrap();
        let program = Compiler::compile(cs).unwrap();
        let witness = extract_witness(&program, &num_to_str);
        individual_scripts.push(program.script);
        individual_witnesses.push(witness);
    }

    assert_eq!(individual_scripts.len(), 72);
    assert_eq!(individual_witnesses.len(), 72);

    // All 72 scripts are concatenated into a single transaction.
    // Each compiled script pulls its hints from the stack bottom via OP_HINT
    // (OP_DEPTH OP_1SUB OP_ROLL). When concatenated, unprocessed hints from
    // later scripts remain at the stack bottom — each script naturally finds
    // its hints after the previous script finishes.
    let merged_script = script! {
        for s in individual_scripts.iter() {
            { s.clone() }
        }
    };

    let merged_witness: Witness = individual_witnesses.iter().flatten().cloned().collect();

    let final_output = convert_to_witness(script! {
        { ldm.hash_var.as_ref().unwrap().value.clone() }
    })
    .unwrap();

    println!(
        "Merged 72 steps into 1 transaction: script={} bytes ({} KB), hints={}",
        merged_script.len(),
        merged_script.len() / 1024,
        merged_witness.len(),
    );

    PlonkAllInformation {
        script: merged_script,
        witness: merged_witness,
        output: final_output,
    }
}

impl CovenantProgram for PlonkVerifierProgram {
    type State = PlonkVerifierState;
    type Input = PlonkVerifierInput;
    const CACHE_NAME: &'static str = "PLONK";

    fn new() -> Self::State {
        PlonkVerifierState {
            pc: 0,
            stack_hash: vec![0u8; 32],
            stack: vec![],
        }
    }

    fn get_hash(state: &Self::State) -> Vec<u8> {
        assert_eq!(state.stack_hash.len(), 32);
        let pc = state.pc as u64;
        let pc_bytes = if pc == 0 {
            vec![]
        } else {
            let bytes = pc.to_le_bytes();
            let len = 8 - bytes.iter().rev().take_while(|&&b| b == 0).count();
            bytes[..len].to_vec()
        };
        let mut sha256 = Sha256::new();
        Update::update(&mut sha256, &pc_bytes);
        Update::update(&mut sha256, &state.stack_hash);
        sha256.finalize().to_vec()
    }

    fn get_all_scripts() -> BTreeMap<usize, Script> {
        let all_information = PLONK_ALL_INFORMATION.get_or_init(compute_all_information);

        let mut map = BTreeMap::new();
        map.insert(
            0,
            script! {
                OP_SWAP { 1 } OP_EQUALVERIFY
                OP_ROT { 0 } OP_EQUALVERIFY
                OP_SWAP { vec![0u8; 32] } OP_EQUALVERIFY
                OP_TOALTSTACK

                { all_information.script.clone() }

                OP_DEPTH
                { 1 }
                OP_EQUALVERIFY

                { StackHash::hash_drop(1) }
                OP_FROMALTSTACK OP_EQUALVERIFY
                OP_TRUE
            },
        );

        map
    }

    fn get_common_prefix() -> Script {
        script! {
            // hint:
            // - old_state
            // - new_state
            //
            // input:
            // - old_state_hash
            // - new_state_hash
            //
            // output:
            // - old pc
            // - old stack hash
            // - new pc
            // - new stack hash
            //

            OP_TOALTSTACK OP_TOALTSTACK

            for _ in 0..2 {
                OP_HINT OP_1ADD OP_1SUB OP_DUP 0 OP_GREATERTHANOREQUAL OP_VERIFY
                OP_HINT OP_SIZE 32 OP_EQUALVERIFY

                OP_2DUP
                OP_CAT
                hash
                OP_FROMALTSTACK OP_EQUALVERIFY
            }
        }
    }

    fn leaf_version() -> LeafVersion {
        LeafVersion::from_consensus(0xc2).unwrap()
    }

    fn run(_id: usize, _: &Self::State, _: &Self::Input) -> Result<Self::State> {
        let all_information = PLONK_ALL_INFORMATION.get_or_init(compute_all_information);

        let final_stack = all_information.output.to_vec();
        let stack_hash = StackHash::compute(&final_stack);
        Ok(Self::State {
            pc: 1,
            stack_hash,
            stack: final_stack,
        })
    }
}

#[cfg(test)]
mod test {
    use crate::dsl::plonk::covenant::{compute_all_information, PLONK_ALL_INFORMATION};
    use crate::treepp::*;
    use bitcoin::hashes::Hash;
    use bitcoin::TapLeafHash;
    use bitcoin_scriptexec::{Exec, ExecCtx, FmtStack, Options, TxTemplate};

    /// Test that the merged verifier script executes correctly.
    ///
    /// We directly create an Exec with OP_MUL/OP_MOD enabled and stack limit
    /// disabled, matching GSR leaf version 0xc2 behavior.
    #[test]
    fn test_integration() {
        let all_information = PLONK_ALL_INFORMATION.get_or_init(compute_all_information);

        let input = all_information.get_input();
        let expected_output = &all_information.output;

        let mut script_bytes = script! {
            for elem in input.hints.iter() {
                { elem.clone() }
            }
        }
        .to_bytes();

        script_bytes.extend_from_slice(all_information.script.as_bytes());

        script_bytes.extend_from_slice(
            script! {
                for elem in expected_output.iter().rev() {
                    { elem.clone() }
                    OP_EQUALVERIFY
                }
                OP_TRUE
            }
            .as_bytes(),
        );

        let script = Script::from_bytes(script_bytes);

        let mut options = Options::default();
        options.experimental.op_mul = true;
        options.experimental.op_mod = true;
        options.enforce_stack_limit = false;

        let mut exec = Exec::new(
            ExecCtx::Tapscript,
            options,
            TxTemplate {
                tx: bitcoin::Transaction {
                    version: bitcoin::transaction::Version::TWO,
                    lock_time: bitcoin::locktime::absolute::LockTime::ZERO,
                    input: vec![],
                    output: vec![],
                },
                prevouts: vec![],
                input_idx: 0,
                taproot_annex_scriptleaf: Some((TapLeafHash::all_zeros(), None)),
            },
            script,
            vec![],
        )
        .expect("error creating exec");

        loop {
            if exec.exec_next().is_err() {
                break;
            }
        }
        let res = exec.result().unwrap();
        if !res.success {
            println!("{:8}", FmtStack(exec.stack().clone()));
            println!("{:?}", res.error);
            panic!(
                "Verification failed: {:?}, max_stack={}",
                res.error,
                exec.stats().max_nb_stack_items,
            );
        }
        println!(
            "Verification passed! script={} KB, max_stack={}, opcodes={}",
            all_information.script.len() / 1024,
            exec.stats().max_nb_stack_items,
            exec.stats().opcode_count,
        );
    }
}
