use bellman::{
        groth16::{
            create_random_proof,
            verify_proof,
            Parameters,
            PreparedVerifyingKey,
            Proof,
        },
        SynthesisError,
};
use pairing::Field;
use rand::{Rand, Rng};
use scrypto::{
    jubjub::{
        JubjubEngine,
        FixedGenerators,
    },
    redjubjub::{
        PublicKey,
    },
};
use crate::circuit::Transfer;
use crate::elgamal::Ciphertext;
use crate::{
    EncryptionKey,
    ProofGenerationKey,
    Nonce,
};

#[derive(Clone)]
pub struct MultiCiphertexts<E: JubjubEngine> {
    pub sender: Ciphertext<E>,
    pub recipient: Ciphertext<E>,
    pub decoys: Option<Vec<Ciphertext<E>>>,
    pub fee: Ciphertext<E>,
}

impl<E: JubjubEngine> MultiCiphertexts<E> {
    pub fn new_for_confidential(
        sender: Ciphertext<E>,
        recipient: Ciphertext<E>,
        fee: Ciphertext<E>,
    ) -> Self {
        MultiCiphertexts {
            sender,
            recipient,
            decoys: None,
            fee,
        }
    }

    pub fn new_for_anonymous(
        sender: Ciphertext<E>,
        recipient: Ciphertext<E>,
        decoys: Vec<Ciphertext<E>>,
        fee: Ciphertext<E>,
    ) -> Self {
        MultiCiphertexts {
            sender,
            recipient,
            decoys: Some(decoys),
            fee,
        }
    }
}

#[derive(Clone)]
pub struct MultiEncKeys<E: JubjubEngine> {
    pub recipient: EncryptionKey<E>,
    pub decoys: Option<Vec<EncryptionKey<E>>>,
}

impl<E: JubjubEngine> MultiEncKeys<E> {
    pub fn new_for_confidential(recipient: EncryptionKey<E>) -> Self {
        MultiEncKeys {
            recipient,
            decoys: None,
        }
    }

    pub fn new_for_anonymous(
        recipient: EncryptionKey<E>,
        decoys: Vec<EncryptionKey<E>>,
    ) -> Self {
        MultiEncKeys {
            recipient,
            decoys: Some(decoys),
        }
    }
}

pub struct AnonymousProof<E: JubjubEngine> {
    proof: Proof<E>,
    rvk: PublicKey<E>,
    enc_key_sender: EncryptionKey<E>,
    enc_keys: MultiEncKeys<E>,
    multi_ciphertexts: MultiCiphertexts<E>,
    cipher_balance: Ciphertext<E>,
}

impl<E: JubjubEngine> AnonymousProof<E> {
    pub fn gen_proof<R: Rng>(
        amount: u32,
        remaining_balance: u32,
        fee: u32,
        alpha: E::Fs,
        proving_key: &Parameters<E>,
        prepared_vk: &PreparedVerifyingKey<E>,
        proof_generation_key: &ProofGenerationKey<E>,
        enc_keys: &MultiEncKeys<E>,
        cipher_balance: Ciphertext<E>,
        nonce: Nonce<E>,
        rng: &mut R,
        params: &E::Params,
    ) -> Result<Self, SynthesisError>
    {

        unimplemented!();
    }
}

pub struct ConfidentialProof<E: JubjubEngine> {
    pub proof: Proof<E>,
    pub rvk: PublicKey<E>, // re-randomization sig-verifying key
    pub enc_key_sender: EncryptionKey<E>,
    pub enc_keys: MultiEncKeys<E>,
    pub multi_ciphertexts: MultiCiphertexts<E>,
    pub cipher_balance: Ciphertext<E>,
}

impl<E: JubjubEngine> ConfidentialProof<E> {
    pub fn gen_proof<R: Rng>(
        amount: u32,
        remaining_balance: u32,
        fee: u32,
        alpha: E::Fs,
        proving_key: &Parameters<E>,
        prepared_vk: &PreparedVerifyingKey<E>,
        proof_generation_key: &ProofGenerationKey<E>,
        enc_keys: &MultiEncKeys<E>,
        cipher_balance: &Ciphertext<E>,
        // nonce: Nonce<E>,
        rng: &mut R,
        params: &E::Params,
    ) -> Result<Self, SynthesisError>
    {
        let randomness = E::Fs::rand(rng);

        let dec_key_sender = proof_generation_key.into_decryption_key()?;
        let enc_key_sender = proof_generation_key.into_encryption_key(params)?;

        let rvk = PublicKey(proof_generation_key.0.clone().into())
            .randomize(
                alpha,
                FixedGenerators::NoteCommitmentRandomness,
                params,
        );

        let instance = Transfer {
            params: params,
            amount: Some(amount),
            remaining_balance: Some(remaining_balance),
            randomness: Some(&randomness),
            alpha: Some(&alpha),
            proof_generation_key: Some(&proof_generation_key),
            dec_key_sender: Some(&dec_key_sender),
            enc_key_recipient: Some(&enc_keys.recipient),
            encrypted_balance: Some(&cipher_balance),
            fee: Some(fee)
        };

        // Crate proof
        let proof = create_random_proof(instance, proving_key, rng)?;

        let mut public_input = [E::Fr::zero(); 18];
        let p_g = FixedGenerators::NoteCommitmentRandomness;

        let cipher_sender = Ciphertext::encrypt(
            amount,
            randomness,
            &enc_key_sender,
            p_g,
            params
        );

        let cipher_recipient = Ciphertext::encrypt(
            amount,
            randomness,
            &enc_keys.recipient,
            p_g,
            params
        );

        let cipher_fee = Ciphertext::encrypt(
            fee,
            randomness,
            &enc_key_sender,
            p_g,
            params
        );

        {
            let (x, y) = enc_key_sender.0.into_xy();
            public_input[0] = x;
            public_input[1] = y;
        }
        {
            let (x, y) = enc_keys.recipient.0.into_xy();
            public_input[2] = x;
            public_input[3] = y;
        }
        {
            let (x, y) = cipher_sender.left.into_xy();
            public_input[4] = x;
            public_input[5] = y;
        }
        {
            let (x, y) = cipher_recipient.left.into_xy();
            public_input[6] = x;
            public_input[7] = y;
        }
        {
            let (x, y) = cipher_sender.right.into_xy();
            public_input[8] = x;
            public_input[9] = y;
        }
        {
            let (x, y) = cipher_fee.left.into_xy();
            public_input[10] = x;
            public_input[11] = y;
        }
        {
            let (x, y) = cipher_balance.left.into_xy();
            public_input[12] = x;
            public_input[13] = y;
        }
        {
            let (x, y) = cipher_balance.right.into_xy();
            public_input[14] = x;
            public_input[15] = y;
        }
        {
            let (x, y) = rvk.0.into_xy();
            public_input[12] = x;
            public_input[13] = y;
        }

        // This verification is just an error handling, not validate if it returns `true`,
        // because public input of encrypted balance needs to be updated on-chain.
        if let Err(_) = verify_proof(prepared_vk, &proof, &public_input[..]) {
            return Err(SynthesisError::MalformedVerifyingKey)
        }

        let proof = ConfidentialProof {
            proof,
            rvk,
            enc_key_sender,
            enc_keys: MultiEncKeys::new_for_confidential(enc_keys.recipient.clone()),
            multi_ciphertexts: MultiCiphertexts::new_for_confidential(cipher_sender, cipher_recipient, cipher_fee),
            cipher_balance: cipher_balance.clone(),
        };

        Ok(proof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, XorShiftRng, Rng};
    use crate::keys::{ProofGenerationKey, EncryptionKey};
    use scrypto::jubjub::{fs, JubjubBls12};
    use pairing::bls12_381::Bls12;
    use std::path::Path;
    use std::fs::File;
    use std::io::{BufReader, Read};

    fn get_pk_and_vk() -> (Parameters<Bls12>, PreparedVerifyingKey<Bls12>) {
        let pk_path = Path::new("../../zface/proving.params");
        let vk_path = Path::new("../../zface/verification.params");

        let pk_file = File::open(&pk_path).unwrap();
        let vk_file = File::open(&vk_path).unwrap();

        let mut pk_reader = BufReader::new(pk_file);
        let mut vk_reader = BufReader::new(vk_file);

        let mut buf_pk = vec![];
        pk_reader.read_to_end(&mut buf_pk).unwrap();

        let mut buf_vk = vec![];
        vk_reader.read_to_end(&mut buf_vk).unwrap();

        let proving_key = Parameters::<Bls12>::read(&mut &buf_pk[..], true).unwrap();
        let prepared_vk = PreparedVerifyingKey::<Bls12>::read(&mut &buf_vk[..]).unwrap();

        (proving_key, prepared_vk)
    }

    #[test]
    fn test_gen_proof() {
        let params = &JubjubBls12::new();
        let p_g = FixedGenerators::NoteCommitmentRandomness;
        let rng = &mut XorShiftRng::from_seed([0x5dbe6259, 0x8d313d76, 0x3237db17, 0xe5bc0654]);
        let alpha = fs::Fs::rand(rng);

        let amount = 10 as u32;
        let remaining_balance = 89 as u32;
        let balance = 100 as u32;
        let fee = 1 as u32;

        let sender_seed: [u8; 32] = rng.gen();
        let recipient_seed: [u8; 32] = rng.gen();

        let proof_generation_key = ProofGenerationKey::<Bls12>::from_seed(&sender_seed, params);
        let enc_key_recipient = EncryptionKey::<Bls12>::from_seed(&recipient_seed, params).unwrap();

        let randomness = rng.gen();
        let enc_key = EncryptionKey::from_seed(&sender_seed[..], params).unwrap();
        let cipher_balance = Ciphertext::encrypt(balance, randomness, &enc_key, p_g, params);

        let (proving_key, prepared_vk) = get_pk_and_vk();

        let proofs = ConfidentialProof::gen_proof(
            amount,
            remaining_balance,
            fee,
            alpha,
            &proving_key,
            &prepared_vk,
            &proof_generation_key,
            &MultiEncKeys::new_for_confidential(enc_key_recipient),
            &cipher_balance,
            rng,
            params,
        );

        assert!(proofs.is_ok());
    }

    #[test]
    fn test_read_proving_key() {
        let pk_path = Path::new("../../zface/proving.params");

        let pk_file = File::open(&pk_path).unwrap();

        let mut pk_reader = BufReader::new(pk_file);
        println!("{:?}", pk_reader);
        let mut buf = vec![];

        pk_reader.read_to_end(&mut buf).unwrap();
        println!("{:?}", buf.len());

        let _proving_key = Parameters::<Bls12>::read(&mut &buf[..], true).unwrap();
    }
}
