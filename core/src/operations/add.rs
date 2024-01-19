use core::borrow::Borrow;
use core::borrow::BorrowMut;
use p3_air::AirBuilder;
use p3_field::Field;
use std::mem::size_of;
use valida_derive::AlignedBorrow;

use crate::air::CurtaAirBuilder;
use crate::air::Word;

use crate::bytes::ByteLookupEvent;
use crate::bytes::ByteOpcode;
use crate::runtime::Segment;
use p3_field::AbstractField;

/// A set of columns needed to compute the add of two words.
#[derive(AlignedBorrow, Default, Debug, Clone, Copy)]
#[repr(C)]
pub struct AddOperation<T> {
    /// The result of `a + b`.
    pub value: Word<T>,

    /// Trace.
    pub carry: [T; 3],
}

impl<F: Field> AddOperation<F> {
    pub fn populate(&mut self, segment: &mut Segment, a_u32: u32, b_u32: u32) -> u32 {
        let expected = a_u32.wrapping_add(b_u32);
        self.value = Word::from(expected);
        let a = a_u32.to_le_bytes();
        let b = b_u32.to_le_bytes();

        let mut carry = [0u8, 0u8, 0u8];
        if (a[0] as u32) + (b[0] as u32) > 255 {
            carry[0] = 1;
            self.carry[0] = F::one();
        }
        if (a[1] as u32) + (b[1] as u32) + (carry[0] as u32) > 255 {
            carry[1] = 1;
            self.carry[1] = F::one();
        }
        if (a[2] as u32) + (b[2] as u32) + (carry[1] as u32) > 255 {
            carry[2] = 1;
            self.carry[2] = F::one();
        }

        let base = 256u32;
        let overflow = a[0]
            .wrapping_add(b[0])
            .wrapping_sub(expected.to_le_bytes()[0]) as u32;
        debug_assert_eq!(overflow.wrapping_mul(overflow.wrapping_sub(base)), 0);

        // Range check
        {
            let bytes: Vec<u8> = a
                .iter()
                .chain(b.iter())
                .chain(expected.to_le_bytes().iter())
                .map(|x| *x)
                .collect();
            // The byte length is always even since each word has 4 bytes.
            assert_eq!(bytes.len() % 2, 0);

            // Pass two bytes to range check at a time.
            for i in (0..bytes.len()).step_by(2) {
                segment.add_byte_range_checks(bytes[i], bytes[i + 1]);
            }
        }
        expected
    }

    pub fn eval<AB: CurtaAirBuilder>(
        builder: &mut AB,
        a: Word<AB::Var>,
        b: Word<AB::Var>,
        cols: AddOperation<AB::Var>,
        is_real: AB::Var,
    ) {
        let one = AB::Expr::one();
        let base = AB::F::from_canonical_u32(256);

        let mut builder_is_real = builder.when(is_real);

        // For each limb, assert that difference between the carried result and the non-carried
        // result is either zero or the base.
        let overflow_0 = a[0] + b[0] - cols.value[0];
        let overflow_1 = a[1] + b[1] - cols.value[1] + cols.carry[0];
        let overflow_2 = a[2] + b[2] - cols.value[2] + cols.carry[1];
        let overflow_3 = a[3] + b[3] - cols.value[3] + cols.carry[2];
        builder_is_real.assert_zero(overflow_0.clone() * (overflow_0.clone() - base));
        builder_is_real.assert_zero(overflow_1.clone() * (overflow_1.clone() - base));
        builder_is_real.assert_zero(overflow_2.clone() * (overflow_2.clone() - base));
        builder_is_real.assert_zero(overflow_3.clone() * (overflow_3.clone() - base));

        // If the carry is one, then the overflow must be the base.
        builder_is_real.assert_zero(cols.carry[0] * (overflow_0.clone() - base.clone()));
        builder_is_real.assert_zero(cols.carry[1] * (overflow_1.clone() - base.clone()));
        builder_is_real.assert_zero(cols.carry[2] * (overflow_2.clone() - base.clone()));

        // If the carry is not one, then the overflow must be zero.
        builder_is_real.assert_zero((cols.carry[0] - one.clone()) * overflow_0.clone());
        builder_is_real.assert_zero((cols.carry[1] - one.clone()) * overflow_1.clone());
        builder_is_real.assert_zero((cols.carry[2] - one.clone()) * overflow_2.clone());

        // Assert that the carry is either zero or one.
        builder_is_real.assert_bool(cols.carry[0]);
        builder_is_real.assert_bool(cols.carry[1]);
        builder_is_real.assert_bool(cols.carry[2]);
        builder_is_real.assert_bool(is_real);

        // Range check each byte.
        {
            let bytes =
                a.0.iter()
                    .chain(b.0.iter())
                    .chain(cols.value.0.iter())
                    .map(|x| *x)
                    .collect::<Vec<_>>();
            for i in (0..bytes.len()).step_by(2) {
                builder.send_byte_pair(
                    AB::F::from_canonical_u32(ByteOpcode::Range as u32),
                    AB::F::zero(),
                    AB::F::zero(),
                    bytes[i],
                    bytes[i + 1],
                    is_real,
                );
            }
        }

        // Degree 3 constraint to avoid "OodEvaluationMismatch".
        builder.assert_zero(a[0] * b[0] * cols.value[0] - a[0] * b[0] * cols.value[0]);
    }
}