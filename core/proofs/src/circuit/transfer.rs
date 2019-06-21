//! This module contains a circuit implementation for confidential payment.
//! The statement is following.
//! * Range proof of the transferred amount
//! * Range proof of the sender's balance
//! * Validity of public key
//! * Validity of encryption for transferred amount
//! * Validity of encryption for sender's balance
//! * Spend authority proof
//! * Some small order checks

use bellman::{
    SynthesisError,
    ConstraintSystem,
    Circuit,
};
use scrypto::jubjub::{
    JubjubEngine,
    FixedGenerators,
};
use crate::keys::{ProofGenerationKey, EncryptionKey, DecryptionKey};
use scrypto::circuit::{
    boolean::{self, Boolean},
    ecc::{self, EdwardsPoint},
    num::AllocatedNum,
};
use crate::{elgamal::Ciphertext, Assignment};

// An instance of the Transfer circuit.
pub struct Transfer<'a, E: JubjubEngine> {
    pub params: &'a E::Params,
    pub amount: Option<u32>,
    pub remaining_balance: Option<u32>,
    pub randomness: Option<&'a E::Fs>,
    pub alpha: Option<&'a E::Fs>,
    pub proof_generation_key: Option<&'a ProofGenerationKey<E>>,
    pub dec_key_sender: Option<&'a DecryptionKey<E>>,
    pub enc_key_recipient: Option< EncryptionKey<E>>,
    pub encrypted_balance: Option<&'a Ciphertext<E>>,
    pub fee: Option<u32>,
}

impl<'a, E: JubjubEngine> Circuit<E> for Transfer<'a, E> {
    fn synthesize<CS: ConstraintSystem<E>>(
        self,
        cs: &mut CS
    ) -> Result<(), SynthesisError>
    {
        let params = self.params;

        // Ensure the amount is u32.
        let amount_bits = u32_into_boolean_vec_le(
            cs.namespace(|| "range proof of amount"),
            self.amount
        )?;

        // Ensure the remaining balance is u32.
        let remaining_balance_bits = u32_into_boolean_vec_le(
            cs.namespace(|| "range proof of remaining_balance"),
            self.remaining_balance
        )?;

        //Ensure the fee is u32.
        let fee_bits = u32_into_boolean_vec_le(
            cs.namespace(|| "range proof of fee"),
            self.fee
        )?;

        // dec_key_sender in circuit
        let dec_key_sender_bits = boolean::field_into_boolean_vec_le(
            cs.namespace(|| format!("dec_key_sender")),
            self.dec_key_sender.map(|e| e.0)
        )?;

        // Ensure the validity of enc_key_sender
        let enc_key_sender_alloc = ecc::fixed_base_multiplication(
            cs.namespace(|| format!("compute enc_key_sender")),
            FixedGenerators::NoteCommitmentRandomness,
            &dec_key_sender_bits,
            params
        )?;

        // Expose the enc_key_sender publicly
        enc_key_sender_alloc.inputize(cs.namespace(|| format!("inputize enc_key_sender")))?;

        // Multiply the amount to the base point same as FixedGenerators::ElGamal.
        let amount_g = ecc::fixed_base_multiplication(
            cs.namespace(|| format!("compute the amount in the exponent")),
            FixedGenerators::NoteCommitmentRandomness,
            &amount_bits,
            params
        )?;

        // Multiply the fee to the base point same as FixedGenerators::ElGamal.
        let fee_g = ecc::fixed_base_multiplication(
            cs.namespace(|| format!("compute the fee in the exponent")),
            FixedGenerators::NoteCommitmentRandomness,
            &fee_bits,
            params
        )?;

        // Generate the randomness for elgamal encryption into the circuit
        let randomness_bits = boolean::field_into_boolean_vec_le(
            cs.namespace(|| format!("randomness_bits")),
            self.randomness.map(|e| *e)
        )?;

        // Generate the randomness * enc_key_sender in circuit
        let val_rls = enc_key_sender_alloc.mul(
            cs.namespace(|| format!("compute sender amount cipher")),
            &randomness_bits,
            params
        )?;

        let fee_rls = enc_key_sender_alloc.mul(
            cs.namespace(|| format!("compute sender fee cipher")),
            &randomness_bits,
            params
        )?;

        // Ensures recipient enc_key is on the curve
        let recipient_enc_key_bits = ecc::EdwardsPoint::witness(
            cs.namespace(|| "recipient enc_key witness"),
            self.enc_key_recipient.as_ref().map(|e| e.0.clone()),
            params
        )?;

        // Check the recipient enc_key is not small order
        recipient_enc_key_bits.assert_not_small_order(
            cs.namespace(|| "val_gl not small order"),
            params
        )?;

        // Generate the randomness * enc_key_recipient in circuit
        let val_rlr = recipient_enc_key_bits.mul(
            cs.namespace(|| format!("compute recipient amount cipher")),
            &randomness_bits,
            params
        )?;

        recipient_enc_key_bits.inputize(cs.namespace(|| format!("inputize enc_key_recipient")))?;


        // Generate the left elgamal component for sender in circuit
        let c_left_sender = amount_g.add(
            cs.namespace(|| format!("computation of sender's c_left")),
            &val_rls,
            params
        )?;

        // Generate the left elgamal component for recipient in circuit
        let c_left_recipient = amount_g.add(
            cs.namespace(|| format!("computation of recipient's c_left")),
            &val_rlr,
            params
        )?;

        // Multiply the randomness to the base point same as FixedGenerators::ElGamal.
        let c_right = ecc::fixed_base_multiplication(
            cs.namespace(|| format!("compute the right elgamal component")),
            FixedGenerators::NoteCommitmentRandomness,
            &randomness_bits,
            params
        )?;

        let f_left_sender = fee_g.add(
            cs.namespace(|| format!("computation of sender's f_left")),
            &fee_rls,
            params
        )?;

        // Expose the ciphertext publicly.
        c_left_sender.inputize(cs.namespace(|| format!("c_left_sender")))?;
        c_left_recipient.inputize(cs.namespace(|| format!("c_left_recipient")))?;
        c_right.inputize(cs.namespace(|| format!("c_right")))?;
        f_left_sender.inputize(cs.namespace(|| format!("f_left_sender")))?;


        // The balance encryption validity.
        // It is a bit complicated bacause we can not know the randomness of balance.
        // Enc_sender(sender_balance).cl - Enc_sender(amount).cl
        //     == (remaining_balance)G + dec_key_sender(Enc_sender(sender_balance).cr - (random)G)
        // <==> Enc_sender(sender_balance).cl + dec_key_sender * (random)G
        //       == (remaining_balance)G + dec_key_sender * Enc_sender(sender_balance).cr + Enc_sender(amount).cl
        //
        // Enc_sender(sender_balance).cl - Enc_sender(amount).cl - Enc_sender(fee).cl
        //  == (remaining_balance)G + dec_key_sender * (Enc_sender(sender_balance).cr - (random)G - (random)G)
        // <==> Enc_sender(sender_balance).cl + dec_key_sender * (random)G + dec_key_sender * (random)G
        //       == (remaining_balance)G + dec_key_sender * Enc_sender(sender_balance).cr + Enc_sender(amount).cl + Enc_sender(fee).cl
        {
            let bal_gl = ecc::EdwardsPoint::witness(
                cs.namespace(|| "balance left"),
                self.encrypted_balance.as_ref().map(|e| e.left.clone()),
                params
            )?;

            bal_gl.assert_not_small_order(
                cs.namespace(|| "bal_gl not small order"),
                params
            )?;

            let bal_gr = ecc::EdwardsPoint::witness(
                cs.namespace(|| "balance right"),
                self.encrypted_balance.as_ref().map(|e| e.right.clone()),
                params
            )?;

            bal_gr.assert_not_small_order(
                cs.namespace(|| "bal_gr not small order"),
                params
            )?;

            let left = self.encrypted_balance.clone().map(|e| e.left.into_xy());
            let right = self.encrypted_balance.map(|e| e.right.into_xy());

            let numxl = AllocatedNum::alloc(cs.namespace(|| "numxl"), || {
                Ok(left.get()?.0)
            })?;
            let numyl = AllocatedNum::alloc(cs.namespace(|| "numyl"), || {
                Ok(left.get()?.1)
            })?;
            let numxr = AllocatedNum::alloc(cs.namespace(|| "numxr"), || {
                Ok(right.get()?.0)
            })?;
            let numyr = AllocatedNum::alloc(cs.namespace(|| "numyr"), || {
                Ok(right.get()?.1)
            })?;

            let pointl = EdwardsPoint::interpret(
                cs.namespace(|| format!("interpret to pointl")),
                &numxl,
                &numyl,
                params
            )?;

            let pointr = EdwardsPoint::interpret(
                cs.namespace(|| format!("interpret to pointr")),
                &numxr,
                &numyr,
                params
            )?;

            //  dec_key_sender * (random)G
            let dec_key_sender_random = c_right.mul(
                cs.namespace(|| format!("c_right mul by dec_key_sender")),
                &dec_key_sender_bits,
                params
                )?;

            // Enc_sender(sender_balance).cl + dec_key_sender * (random)G
            let balance_dec_key_sender_random = pointl.add(
                cs.namespace(|| format!("pointl add dec_key_sender_pointl")),
                &dec_key_sender_random,
                params
                )?;

            // Enc_sender(sender_balance).cl + dec_key_sender * (random)G + dec_key_sender * (random)G
            let bi_left = balance_dec_key_sender_random.add(
                cs.namespace(|| format!("pointl readd dec_key_sender_pointl")),
                &dec_key_sender_random,
                params
                )?;

            // dec_key_sender * Enc_sender(sender_balance).cr
            let dec_key_sender_pointr = pointr.mul(
                cs.namespace(|| format!("c_right_sender mul by dec_key_sender")),
                &dec_key_sender_bits,
                params
                )?;

            // Compute (remaining_balance)G
            let rem_bal_g = ecc::fixed_base_multiplication(
                cs.namespace(|| format!("compute the remaining balance in the exponent")),
                FixedGenerators::NoteCommitmentRandomness,
                &remaining_balance_bits,
                params
                )?;

            // Enc_sender(amount).cl + (remaining_balance)G
            let val_rem_bal = c_left_sender.add(
                cs.namespace(|| format!("c_left_sender add rem_bal_g")),
                &rem_bal_g,
                params
                )?;

            // Enc_sender(amount).cl + (remaining_balance)G + dec_key_sender * Enc_sender(sender_balance).cr
            let val_rem_bal_balr = val_rem_bal.add(
                cs.namespace(|| format!("val_rem_bal add ")),
                &dec_key_sender_pointr,
                params
                )?;

            // Enc_sender(amount).cl + (remaining_balance)G + dec_key_sender * Enc_sender(sender_balance).cr + Enc_sender(fee).cl
            let bi_right = f_left_sender.add(
                cs.namespace(|| format!("f_left_sender add")),
                &val_rem_bal_balr,
                params
            )?;

            // The left hand for balance integrity into representation
            let bi_left_repr = bi_left.repr(
                cs.namespace(|| format!("bi_left into a representation"))
            )?;

            // The right hand for balance integrity into representation
            let bi_right_repr = bi_right.repr(
                cs.namespace(|| format!("bi_right into a representation"))
            )?;

            let iter = bi_left_repr.iter().zip(bi_right_repr.iter());

            // Ensure for the sender's balance integrity
            for (i, (a, b)) in iter.enumerate() {
                Boolean::enforce_equal(
                    cs.namespace(|| format!("bi_left equals bi_right {}", i)),
                    &a,
                    &b
                )?;
            }

            pointl.inputize(cs.namespace(|| format!("inputize pointl")))?;
            pointr.inputize(cs.namespace(|| format!("inputize pointr")))?;
        }


        // Ensure pgk on the curve.
        let pgk = ecc::EdwardsPoint::witness(
            cs.namespace(|| "pgk"),
            self.proof_generation_key.as_ref().map(|k| k.0.clone()),
            self.params
        )?;

        // Ensure pgk is large order.
        pgk.assert_not_small_order(
            cs.namespace(|| "pgk not small order"),
            self.params
        )?;

        // Re-randomized parameter for pgk
        let alpha = boolean::field_into_boolean_vec_le(
            cs.namespace(|| "alpha"),
            self.alpha.map(|e| *e)
        )?;

        // Make the alpha on the curve
        let alpha_g = ecc::fixed_base_multiplication(
            cs.namespace(|| "computation of randomiation for the signing key"),
            FixedGenerators::NoteCommitmentRandomness,
            &alpha,
            self.params
        )?;

        // Ensure randomaized sig-verification key is computed by the addition of ak and alpha_g
        let rvk = pgk.add(
            cs.namespace(|| "computation of rvk"),
            &alpha_g,
            self.params
        )?;

        // Ensure rvk is large order.
        rvk.assert_not_small_order(
            cs.namespace(|| "rvk not small order"),
            self.params
        )?;

        rvk.inputize(cs.namespace(|| "rvk"))?;

        Ok(())
    }
}

fn u32_into_boolean_vec_le<E, CS>(
    mut cs: CS,
    amount: Option<u32>
) -> Result<Vec<Boolean>, SynthesisError>
    where E: JubjubEngine, CS: ConstraintSystem<E>
{
    let amounts = match amount {
        Some(ref amount) => {
            let mut tmp = Vec::with_capacity(32);
            for i in 0..32 {
                tmp.push(Some(*amount >> i & 1 == 1));
            }
            tmp
        },

        None => {
            vec![None; 32]
        }
    };

    let bits = amounts.into_iter()
            .enumerate()
            .map(|(i, v)| {
                Ok(boolean::Boolean::from(boolean::AllocatedBit::alloc(
                    cs.namespace(|| format!("bit {}", i)),
                    v
                )?))
            })
            .collect::<Result<Vec<_>, SynthesisError>>()?;

    Ok(bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pairing::{bls12_381::{Bls12, Fr}, Field};
    use rand::{SeedableRng, Rng, XorShiftRng, Rand};
    use crate::circuit::TestConstraintSystem;
    use scrypto::jubjub::{JubjubBls12, fs, JubjubParams};
    use crate::keys::EncryptionKey;

    #[test]
    fn test_circuit_transfer() {
        let params = &JubjubBls12::new();
        let rng = &mut XorShiftRng::from_seed([0x3dbe6258, 0x8d313d76, 0x3237db17, 0xe5bc0654]);

        let sk_fs_s: [u8; 32] = rng.gen();
        let sk_fs_r: [u8; 32] = rng.gen();

        let proof_generation_key_s = ProofGenerationKey::<Bls12>::from_seed(&sk_fs_s[..], params);
        let proof_generation_key_r = ProofGenerationKey::<Bls12>::from_seed(&sk_fs_r[..], params);

        let decryption_key_s = proof_generation_key_s.into_decryption_key().unwrap();
        let decryption_key_r = proof_generation_key_r.into_decryption_key().unwrap();

        let address_recipient = EncryptionKey::from_seed(&sk_fs_r, params).unwrap();
        let address_sender_xy = proof_generation_key_s.into_encryption_key(params).unwrap().0.into_xy();
        let address_recipient_xy = address_recipient.0.into_xy();

        let alpha: fs::Fs = rng.gen();

        let amount = 10 as u32;
        let remaining_balance = 16 as u32;
        let current_balance = 27 as u32;
        let fee = 1 as u32;

        let r_fs_b = fs::Fs::rand(rng);
        let r_fs_v = fs::Fs::rand(rng);

        let p_g = FixedGenerators::NoteCommitmentRandomness;
        let public_key_s = EncryptionKey(params.generator(p_g).mul(decryption_key_s.0, params));
        let ciphetext_balance = Ciphertext::encrypt(current_balance, r_fs_b, &public_key_s, p_g, params);

        let c_bal_left = ciphetext_balance.left.into_xy();
        let c_bal_right = ciphetext_balance.right.into_xy();

        let ciphertext_amount_sender = Ciphertext::encrypt(amount, r_fs_v, &public_key_s, p_g, params);
        let c_val_s_left = ciphertext_amount_sender.left.into_xy();
        let c_val_right = ciphertext_amount_sender.right.into_xy();

        let ciphertext_fee_sender = Ciphertext::encrypt(fee, r_fs_v, &public_key_s, p_g, params);
        let c_fee_s_left = ciphertext_fee_sender.left.into_xy();

        let public_key_r = EncryptionKey(params.generator(p_g).mul(decryption_key_r.0, params));
        let ciphertext_amount_recipient = Ciphertext::encrypt(amount, r_fs_v, &public_key_r, p_g, params);
        let c_val_r_left = ciphertext_amount_recipient.left.into_xy();

        let rvk = proof_generation_key_s.into_rvk(alpha, params).0.into_xy();

        let mut cs = TestConstraintSystem::<Bls12>::new();

        let instance = Transfer {
            params: params,
            amount: Some(amount),
            remaining_balance: Some(remaining_balance),
            randomness: Some(&r_fs_v),
            alpha: Some(&alpha),
            proof_generation_key: Some(&proof_generation_key_s),
            dec_key_sender: Some(&decryption_key_s),
            enc_key_recipient: Some(address_recipient.clone()),
            encrypted_balance: Some(&ciphetext_balance),
            fee: Some(fee),
        };

        instance.synthesize(&mut cs).unwrap();

        assert!(cs.is_satisfied());
        assert_eq!(cs.num_constraints(), 21687);
        assert_eq!(cs.hash(), "006d0e0175bc1154278d7ef3f0e53514840b478ad6db2540d7910cd94a38da24");

        assert_eq!(cs.num_inputs(), 19);
        assert_eq!(cs.get_input(0, "ONE"), Fr::one());
        assert_eq!(cs.get_input(1, "inputize enc_key_sender/x/input variable"), address_sender_xy.0);
        assert_eq!(cs.get_input(2, "inputize enc_key_sender/y/input variable"), address_sender_xy.1);
        assert_eq!(cs.get_input(3, "inputize enc_key_recipient/x/input variable"), address_recipient_xy.0);
        assert_eq!(cs.get_input(4, "inputize enc_key_recipient/y/input variable"), address_recipient_xy.1);
        assert_eq!(cs.get_input(5, "c_left_sender/x/input variable"), c_val_s_left.0);
        assert_eq!(cs.get_input(6, "c_left_sender/y/input variable"), c_val_s_left.1);
        assert_eq!(cs.get_input(7, "c_left_recipient/x/input variable"), c_val_r_left.0);
        assert_eq!(cs.get_input(8, "c_left_recipient/y/input variable"), c_val_r_left.1);
        assert_eq!(cs.get_input(9, "c_right/x/input variable"), c_val_right.0);
        assert_eq!(cs.get_input(10, "c_right/y/input variable"), c_val_right.1);
        assert_eq!(cs.get_input(11, "f_left_sender/x/input variable"), c_fee_s_left.0);
        assert_eq!(cs.get_input(12, "f_left_sender/y/input variable"), c_fee_s_left.1);
        assert_eq!(cs.get_input(13, "inputize pointl/x/input variable"), c_bal_left.0);
        assert_eq!(cs.get_input(14, "inputize pointl/y/input variable"), c_bal_left.1);
        assert_eq!(cs.get_input(15, "inputize pointr/x/input variable"), c_bal_right.0);
        assert_eq!(cs.get_input(16, "inputize pointr/y/input variable"), c_bal_right.1);
        assert_eq!(cs.get_input(17, "rvk/x/input variable"), rvk.0);
        assert_eq!(cs.get_input(18, "rvk/y/input variable"), rvk.1);

    }
}