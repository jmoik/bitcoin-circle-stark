#[cfg(not(feature = "assume-gsr"))]
use super::m31_limbs::{m31_to_limbs_gadget, M31LimbsVar};
use super::table::TableVar;
use crate::treepp::*;
use anyhow::Result;
use bitcoin_script_dsl::bvar::{AllocVar, AllocationMode, BVar};
use bitcoin_script_dsl::constraint_system::{ConstraintSystemRef, Element};
use bitcoin_script_dsl::options::Options;
use bitcoin_script_dsl::stack::Stack;
use std::ops::{Add, Mul, Neg, Sub};
use stwo_prover::core::fields::m31::M31;
use stwo_prover::core::fields::FieldExpOps;

#[cfg(feature = "assume-gsr")]
const M31_MOD: u32 = (1u32 << 31) - 1;

#[derive(Debug, Clone)]
pub struct M31Var {
    pub variable: usize,
    pub value: M31,
    pub cs: ConstraintSystemRef,
}

impl BVar for M31Var {
    type Value = M31;

    fn cs(&self) -> ConstraintSystemRef {
        self.cs.clone()
    }

    fn variables(&self) -> Vec<usize> {
        vec![self.variable]
    }

    fn length() -> usize {
        1
    }

    fn value(&self) -> Result<Self::Value> {
        Ok(self.value)
    }
}

impl AllocVar for M31Var {
    fn new_variable(
        cs: &ConstraintSystemRef,
        data: <Self as BVar>::Value,
        mode: AllocationMode,
    ) -> Result<Self> {
        Ok(Self {
            variable: cs.alloc(Element::Num(data.0 as i32), mode)?,
            value: data,
            cs: cs.clone(),
        })
    }
}

#[cfg(not(feature = "assume-gsr"))]
impl Add for &M31Var {
    type Output = M31Var;

    fn add(self, rhs: Self) -> Self::Output {
        let res = self.value + rhs.value;

        let cs = self.cs.and(&rhs.cs);

        cs.insert_script(rust_bitcoin_m31::m31_add, [self.variable, rhs.variable])
            .unwrap();

        M31Var::new_variable(&cs, res, AllocationMode::FunctionOutput).unwrap()
    }
}

#[cfg(feature = "assume-gsr")]
impl Add for &M31Var {
    type Output = M31Var;

    fn add(self, rhs: Self) -> Self::Output {
        let res = self.value + rhs.value;

        let cs = self.cs.and(&rhs.cs);

        cs.insert_script(m31_add_op_mod_gadget, [self.variable, rhs.variable])
            .unwrap();

        M31Var::new_variable(&cs, res, AllocationMode::FunctionOutput).unwrap()
    }
}

impl Sub for &M31Var {
    type Output = M31Var;

    fn sub(self, rhs: Self) -> Self::Output {
        let res = self.value - rhs.value;

        let cs = self.cs.and(&rhs.cs);

        cs.insert_script(rust_bitcoin_m31::m31_sub, [self.variable, rhs.variable])
            .unwrap();

        M31Var::new_variable(&cs, res, AllocationMode::FunctionOutput).unwrap()
    }
}

#[cfg(not(feature = "assume-gsr"))]
impl Mul for &M31Var {
    type Output = M31Var;

    fn mul(self, rhs: Self) -> Self::Output {
        let res = self.value * rhs.value;

        let cs = self.cs.and(&rhs.cs);

        cs.insert_script(rust_bitcoin_m31::m31_mul, [self.variable, rhs.variable])
            .unwrap();

        M31Var::new_function_output(&cs, res).unwrap()
    }
}

#[cfg(feature = "assume-gsr")]
impl Mul for &M31Var {
    type Output = M31Var;

    fn mul(self, rhs: Self) -> Self::Output {
        let res = self.value * rhs.value;

        let cs = self.cs.and(&rhs.cs);

        cs.insert_script(m31_mul_op_mul_gadget, [self.variable, rhs.variable])
            .unwrap();

        M31Var::new_function_output(&cs, res).unwrap()
    }
}

#[cfg(not(feature = "assume-gsr"))]
impl Mul<(&TableVar, &M31Var)> for &M31Var {
    type Output = M31Var;

    fn mul(self, rhs: (&TableVar, &M31Var)) -> Self::Output {
        let table = rhs.0;
        let rhs = rhs.1;

        let self_limbs = M31LimbsVar::from(self);
        let rhs_limbs = M31LimbsVar::from(rhs);
        &self_limbs * (table, &rhs_limbs)
    }
}

#[cfg(feature = "assume-gsr")]
impl Mul<(&TableVar, &M31Var)> for &M31Var {
    type Output = M31Var;

    fn mul(self, rhs: (&TableVar, &M31Var)) -> Self::Output {
        // With OP_MUL, ignore the table and use direct multiplication
        let rhs = rhs.1;
        self * rhs
    }
}

#[cfg(not(feature = "assume-gsr"))]
impl Neg for &M31Var {
    type Output = M31Var;

    fn neg(self) -> Self::Output {
        let res = -self.value;

        let cs = self.cs();

        cs.insert_script(rust_bitcoin_m31::m31_neg, [self.variable])
            .unwrap();

        M31Var::new_function_output(&cs, res).unwrap()
    }
}

#[cfg(feature = "assume-gsr")]
impl Neg for &M31Var {
    type Output = M31Var;

    fn neg(self) -> Self::Output {
        let res = -self.value;

        let cs = self.cs();

        cs.insert_script(m31_neg_op_mod_gadget, [self.variable])
            .unwrap();

        M31Var::new_function_output(&cs, res).unwrap()
    }
}

impl M31Var {
    pub fn is_zero(&self) {
        assert_eq!(self.value.0, 0);
        self.cs
            .insert_script(m31_is_zero_gadget, [self.variable])
            .unwrap();
    }

    pub fn is_one(&self) {
        assert_eq!(self.value.0, 1);
        self.cs
            .insert_script(m31_is_one_gadget, [self.variable])
            .unwrap();
    }

    #[cfg(not(feature = "assume-gsr"))]
    pub fn inverse(&self, table: &TableVar) -> Self {
        let self_limbs = M31LimbsVar::from(self);
        let inv_limbs = self_limbs.inverse(table);

        let cs = self.cs.and(&table.cs);
        let inv = M31Var::new_hint(&cs, self.value.inverse()).unwrap();

        cs.insert_script(
            m31_to_limbs_gadget,
            inv.variables()
                .iter()
                .chain(inv_limbs.variables().iter())
                .copied(),
        )
        .unwrap();

        inv
    }

    #[cfg(feature = "assume-gsr")]
    pub fn inverse(&self, _table: &TableVar) -> Self {
        // With OP_MUL, no need for limbs/table — just hint and verify
        self.inverse_without_table()
    }

    pub fn inverse_without_table(&self) -> Self {
        let inv = M31Var::new_hint(&self.cs, self.value.inverse()).unwrap();

        let res = self * &inv;
        res.is_one();

        inv
    }

    pub fn trim(&self, logn: usize) -> Self {
        let res = self.value.0 & ((1 << logn) - 1);
        self.cs
            .insert_script_complex(
                m31_trim_gadget,
                vec![self.variable],
                &Options::new().with_u32("logn", logn as u32),
            )
            .unwrap();
        M31Var::new_function_output(&self.cs, M31::from_u32_unchecked(res)).unwrap()
    }
}

/// OP_MUL + OP_MOD based M31 multiplication gadget.
///
/// Stack input:  a, b
/// Stack output: r = (a*b) % MOD
#[cfg(feature = "assume-gsr")]
fn m31_mul_op_mul_gadget() -> Script {
    script! {
        OP_MUL        // a*b
        { M31_MOD }   // a*b, MOD
        OP_MOD        // r = (a*b) % MOD
    }
}

/// OP_MOD based M31 addition gadget.
///
/// Stack input:  a, b  (both in [0, MOD))
/// Stack output: (a+b) % MOD
#[cfg(feature = "assume-gsr")]
fn m31_add_op_mod_gadget() -> Script {
    script! {
        OP_ADD
        { M31_MOD }
        OP_MOD
    }
}

/// OP_MOD based M31 negation gadget.
///
/// Stack input:  a  (in [0, MOD))
/// Stack output: (-a) % MOD = (MOD-a) % MOD
#[cfg(feature = "assume-gsr")]
fn m31_neg_op_mod_gadget() -> Script {
    script! {
        { M31_MOD }
        OP_SWAP
        OP_SUB          // MOD-a (always >= 0 since a < MOD)
        { M31_MOD }
        OP_MOD          // handles neg(0) = MOD -> 0
    }
}

fn m31_is_zero_gadget() -> Script {
    script! {
        0 OP_EQUALVERIFY
    }
}

fn m31_is_one_gadget() -> Script {
    script! {
        1 OP_EQUALVERIFY
    }
}

fn m31_trim_gadget(_: &mut Stack, options: &Options) -> Result<Script> {
    let logn = options.get_u32("logn")?;
    if logn == 31 {
        Ok(script! {})
    } else {
        Ok(script! {
            { 1u32 << logn }
            OP_MOD
        })
    }
}

#[cfg(test)]
mod test {
    use super::M31Var;
    use crate::dsl::primitives::table::utils::rand_m31;
    use crate::dsl::primitives::table::TableVar;
    use crate::treepp::*;
    use bitcoin_script_dsl::bvar::AllocVar;
    use bitcoin_script_dsl::constraint_system::ConstraintSystem;
    use bitcoin_script_dsl::test_program_with_op_mul as test_program;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn test_m31_mul() {
        let mut prng = ChaCha20Rng::seed_from_u64(0);

        let a_val = rand_m31(&mut prng);
        let b_val = rand_m31(&mut prng);

        let cs = ConstraintSystem::new_ref();

        let a = M31Var::new_constant(&cs, a_val).unwrap();
        let b = M31Var::new_constant(&cs, b_val).unwrap();
        let res = &a * &b;

        cs.set_program_output(&res).unwrap();

        test_program(
            cs,
            script! {
                { a_val * b_val }
            },
        )
        .unwrap();
    }

    #[test]
    fn test_m31_inverse() {
        let mut prng = ChaCha20Rng::seed_from_u64(0);

        let a_val = rand_m31(&mut prng);

        let cs = ConstraintSystem::new_ref();

        let a = M31Var::new_constant(&cs, a_val).unwrap();
        let table = TableVar::new_constant(&cs, ()).unwrap();

        let a_inv = a.inverse(&table);
        let res = &a * (&table, &a_inv);

        cs.set_program_output(&res).unwrap();

        test_program(
            cs,
            script! {
                1
            },
        )
        .unwrap();
    }

    #[test]
    fn test_m31_inverse_without_table() {
        let mut prng = ChaCha20Rng::seed_from_u64(0);

        let a_val = rand_m31(&mut prng);

        let cs = ConstraintSystem::new_ref();

        let a = M31Var::new_constant(&cs, a_val).unwrap();
        let a_inv = a.inverse_without_table();
        let res = &a * &a_inv;

        cs.set_program_output(&res).unwrap();

        test_program(
            cs,
            script! {
                1
            },
        )
        .unwrap();
    }
}
