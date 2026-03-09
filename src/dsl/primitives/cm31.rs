use super::m31::M31Var;
use super::table::TableVar;
use anyhow::Result;
use bitcoin_script_dsl::bvar::{AllocVar, AllocationMode, BVar};
use bitcoin_script_dsl::constraint_system::ConstraintSystemRef;
use std::ops::{Add, Mul, Neg, Sub};
use stwo_prover::core::fields::cm31::CM31;
use stwo_prover::core::fields::FieldExpOps;

#[derive(Clone)]
pub struct CM31Var {
    pub imag: M31Var,
    pub real: M31Var,
}

impl BVar for CM31Var {
    type Value = CM31;

    fn cs(&self) -> ConstraintSystemRef {
        self.real.cs.and(&self.imag.cs)
    }

    fn variables(&self) -> Vec<usize> {
        vec![self.imag.variable, self.real.variable]
    }

    fn length() -> usize {
        2
    }

    fn value(&self) -> Result<Self::Value> {
        Ok(CM31::from_m31(self.real.value, self.imag.value))
    }
}

impl AllocVar for CM31Var {
    fn new_variable(
        cs: &ConstraintSystemRef,
        data: <Self as BVar>::Value,
        mode: AllocationMode,
    ) -> Result<Self> {
        let imag = M31Var::new_variable(cs, data.1, mode)?;
        let real = M31Var::new_variable(cs, data.0, mode)?;

        Ok(Self { imag, real })
    }
}

impl Add for &CM31Var {
    type Output = CM31Var;

    fn add(self, rhs: Self) -> Self::Output {
        let imag = &self.imag + &rhs.imag;
        let real = &self.real + &rhs.real;

        CM31Var { imag, real }
    }
}

impl Add<&M31Var> for &CM31Var {
    type Output = CM31Var;

    fn add(self, rhs: &M31Var) -> Self::Output {
        let imag = self.imag.copy().unwrap();
        let real = &self.real + rhs;

        CM31Var { imag, real }
    }
}

impl Sub for &CM31Var {
    type Output = CM31Var;

    fn sub(self, rhs: Self) -> Self::Output {
        let imag = &self.imag - &rhs.imag;
        let real = &self.real - &rhs.real;

        CM31Var { imag, real }
    }
}

impl Sub<&M31Var> for &CM31Var {
    type Output = CM31Var;

    fn sub(self, rhs: &M31Var) -> Self::Output {
        let imag = self.imag.copy().unwrap();
        let real = &self.real - rhs;

        CM31Var { imag, real }
    }
}

impl Mul for &CM31Var {
    type Output = CM31Var;

    fn mul(self, rhs: Self) -> Self::Output {
        // Karatsuba: (a+bi)(c+di) = (ac-bd) + ((a+b)(c+d) - ac - bd)i
        let ac = &self.real * &rhs.real;
        let bd = &self.imag * &rhs.imag;
        let a_plus_b = &self.real + &self.imag;
        let c_plus_d = &rhs.real + &rhs.imag;
        let abcd = &a_plus_b * &c_plus_d;

        let real = &ac - &bd;
        let imag = &(&abcd - &ac) - &bd;

        CM31Var { real, imag }
    }
}

impl Mul<(&TableVar, &CM31Var)> for &CM31Var {
    type Output = CM31Var;

    fn mul(self, rhs: (&TableVar, &CM31Var)) -> Self::Output {
        let rhs = rhs.1;
        self * rhs
    }
}

impl Mul<(&TableVar, &M31Var)> for &CM31Var {
    type Output = CM31Var;

    fn mul(self, rhs: (&TableVar, &M31Var)) -> Self::Output {
        let rhs = rhs.1;
        let real = &self.real * rhs;
        let imag = &self.imag * rhs;
        CM31Var { real, imag }
    }
}

impl Neg for &CM31Var {
    type Output = CM31Var;

    fn neg(self) -> Self::Output {
        let real = -&self.real;
        let imag = -&self.imag;

        CM31Var { imag, real }
    }
}

impl CM31Var {
    pub fn is_one(&self) {
        assert_eq!(self.value().unwrap(), CM31::from_u32_unchecked(1, 0));
        self.real.is_one();
        self.imag.is_zero();
    }

    pub fn is_zero(&self) {
        assert_eq!(self.value().unwrap(), CM31::from_u32_unchecked(0, 0));
        self.real.is_zero();
        self.imag.is_zero();
    }

    pub fn inverse(&self, _table: &TableVar) -> Self {
        self.inverse_without_table()
    }

    pub fn inverse_without_table(&self) -> Self {
        let cs = self.cs();
        let res = self.value().unwrap().inverse();

        let res_var = CM31Var::new_hint(&cs, res).unwrap();
        let expected_one = &res_var * self;
        expected_one.is_one();

        res_var
    }

    pub fn shift_by_i(&self) -> Self {
        let imag = self.real.copy().unwrap();
        let real = -&self.imag;

        Self { imag, real }
    }
}

#[cfg(test)]
mod test {
    use crate::dsl::primitives::cm31::CM31Var;
    use crate::dsl::primitives::table::utils::rand_cm31;
    use crate::dsl::primitives::table::TableVar;
    use crate::treepp::*;
    use bitcoin_script_dsl::bvar::AllocVar;
    use bitcoin_script_dsl::constraint_system::ConstraintSystem;
    use bitcoin_script_dsl::test_program_with_op_mul as test_program;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn cm31_inverse() {
        let mut prng = ChaCha20Rng::seed_from_u64(0);

        let a_val = rand_cm31(&mut prng);

        let cs = ConstraintSystem::new_ref();

        let a = CM31Var::new_constant(&cs, a_val).unwrap();
        let table = TableVar::new_constant(&cs, ()).unwrap();

        let a_inv = a.inverse(&table);
        let res = &a * (&table, &a_inv);

        cs.set_program_output(&res).unwrap();

        test_program(
            cs,
            script! {
                0
                1
            },
        )
        .unwrap();
    }

    #[test]
    fn cm31_inverse_without_table() {
        let mut prng = ChaCha20Rng::seed_from_u64(0);

        let a_val = rand_cm31(&mut prng);

        let cs = ConstraintSystem::new_ref();

        let a = CM31Var::new_constant(&cs, a_val).unwrap();

        let a_inv = a.inverse_without_table();
        let res = &a * &a_inv;

        cs.set_program_output(&res).unwrap();

        test_program(
            cs,
            script! {
                0
                1
            },
        )
        .unwrap();
    }
}
