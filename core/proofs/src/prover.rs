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
use zpairing;
use rand::{Rand, Rng};
use scrypto::{
    jubjub::{
        JubjubEngine,
        JubjubParams,
        FixedGenerators,
        edwards,
        PrimeOrder,
    },
    redjubjub::{
        PublicKey,
    },
};
use polkadot_rs::Api;
use crate::circuit::Transfer;
use crate::elgamal::Ciphertext;
use crate::{
    EncryptionKey,
    ProofGenerationKey,
    SpendingKey,
    Nonce,
};
use crate::{MultiEncKeys, MultiCiphertexts, Confidential, CiphertextTrait};
use std::{
    io::{self, Write},
    path::Path,
    fs::File,
    marker::PhantomData,
};

pub trait ProofBuilder<E: JubjubEngine>: Sized {
    type Submitter: Submitter;

    fn setup<R: Rng>(rng: &mut R) -> Self;

    fn write_to_file<P: AsRef<Path>>(&self, pk_path: P, vk_path: P) -> io::Result<()>;

    fn read_from_path<P: AsRef<Path>>(pk_path: P, vk_path: P) -> io::Result<Self>;

    fn gen_proof<R: Rng>(
        &self,
        amount: u32,
        fee: u32,
        remaining_balance: u32,
        spending_key: &SpendingKey<E>,
        enc_keys: &MultiEncKeys<E>,
        encrypted_balance: &Ciphertext<E>,
        g_epoch: edwards::Point<E, PrimeOrder>,
        rng: &mut R,
        params: &E::Params,
    ) -> Result<Self::Submitter, SynthesisError>;
}

pub struct KeyContext<E: JubjubEngine> {
    proving_key: Parameters<E>,
    prepared_vk: PreparedVerifyingKey<E>,
}

impl<E: JubjubEngine> ProofBuilder<E> for KeyContext<E> {
    type Submitter = ConfidentialXt;

    fn setup<R: Rng>(rng: &mut R) -> Self {
        use crate::PARAMS;
        use pairing::bls12_381::Bls12;
        use bellman::groth16::{
            generate_random_parameters,
            prepare_verifying_key,
        };

        // // Create parameters for the confidential transfer circuit
        // let proving_key = {
        //     let c = Transfer::<E>::new(&*PARAMS);
        //     //     params: &<E as JubjubEngine>::Params,
        //     //     amount: None,
        //     //     remaining_balance: None,
        //     //     randomness: None,
        //     //     alpha: None,
        //     //     proof_generation_key: None,
        //     //     dec_key_sender: None,
        //     //     enc_key_recipient: None,
        //     //     encrypted_balance: None,
        //     //     fee: None,
        //     //     g_epoch: None,
        //     // };

        //     generate_random_parameters::<E, _, _>(c, rng).unwrap()
        // };

        // let prepared_vk = prepare_verifying_key::<E>(&proving_key.vk);
        // let mut v = vec![];
        // prepared_vk.write(&mut &mut v).unwrap();
        // println!("pvk: {:?}", v.len());

        // KeyContext::<E> {
        //     proving_key,
        //     prepared_vk,
        // }
        unimplemented!();
    }

    fn write_to_file<P: AsRef<Path>>(&self, pk_path: P, vk_path: P) -> io::Result<()> {
        let pk_file = File::create(&pk_path)?;
        let vk_file = File::create(&vk_path)?;

        let mut bw_pk = io::BufWriter::new(pk_file);
        let mut bw_vk = io::BufWriter::new(vk_file);

        let mut v_pk = vec![];
        let mut v_vk = vec![];

        self.proving_key.write(&mut &mut v_pk)?;
        self.prepared_vk.write(&mut &mut v_vk)?;

        bw_pk.write(&v_pk[..])?;
        bw_vk.write(&v_vk[..])?;

        bw_pk.flush()?;
        bw_vk.flush()?;

        Ok(())
    }

    fn read_from_path<P: AsRef<Path>>(pk_path: P, vk_path: P) -> io::Result<Self>{
        unimplemented!();
    }

    fn gen_proof<R: Rng>(
        &self,
        amount: u32,
        fee: u32,
        remaining_balance: u32,
        spending_key: &SpendingKey<E>,
        enc_keys: &MultiEncKeys<E>,
        encrypted_balance: &Ciphertext<E>,
        g_epoch: edwards::Point<E, PrimeOrder>,
        rng: &mut R,
        params: &E::Params,
    ) -> Result<Self::Submitter, SynthesisError> {
        let randomness = E::Fs::rand(rng);
        let alpha = E::Fs::rand(rng);

        let proof_generation_key = ProofGenerationKey::<E>::from_spending_key(&spending_key, params);
        let dec_key_sender = proof_generation_key.into_decryption_key()?;
        let enc_key_sender = proof_generation_key.into_encryption_key(params)?;

        let rvk = PublicKey(proof_generation_key.0.into())
            .randomize(
                alpha,
                FixedGenerators::NoteCommitmentRandomness,
                params,
        );
        let nonce = g_epoch.mul(dec_key_sender.0, params);

        let instance = Transfer {
            params: params,
            amount: Some(amount),
            remaining_balance: Some(remaining_balance),
            randomness: Some(&randomness),
            alpha: Some(&alpha),
            proof_generation_key: Some(&proof_generation_key),
            dec_key_sender: Some(&dec_key_sender),
            enc_key_recipient: Some(&enc_keys.get_recipient()),
            encrypted_balance: Some(&encrypted_balance),
            fee: Some(fee),
            g_epoch: Some(&g_epoch),
        };

        // Crate proof
        let proof = create_random_proof(instance, &self.proving_key, rng)?;

        let multi_ciphertexts = MultiCiphertexts::<E, Confidential>::encrypt(
            amount,
            fee,
            &enc_key_sender,
            enc_keys,
            &randomness,
            params
        );

        ConfidentialProofContext::new(
            proof,
            rvk,
            enc_key_sender,
            *enc_keys,
            multi_ciphertexts,
            *encrypted_balance,
            g_epoch,
            nonce
        )
        .check_proof(&self.prepared_vk)?
        .gen_xt(&spending_key, alpha)
        .map_err(|e| SynthesisError::IoError(e))
    }

}

enum Unchecked { }
enum Checked { }

struct ConfidentialProofContext<E: JubjubEngine, IsChecked> {
    proof: Proof<E>,
    rvk: PublicKey<E>, // re-randomization sig-verifying key
    enc_key_sender: EncryptionKey<E>,
    enc_keys: MultiEncKeys<E>,
    multi_ciphertexts: MultiCiphertexts<E, Confidential>,
    encrypted_balance: Ciphertext<E>,
    g_epoch: edwards::Point<E, PrimeOrder>,
    nonce: edwards::Point<E, PrimeOrder>,
    _marker: PhantomData<IsChecked>,
}


impl<E: JubjubEngine, IsChecked> ConfidentialProofContext<E, IsChecked> {
    fn enc_amount_sender(&self) -> Ciphertext<E> {
        self.multi_ciphertexts.get_sender()
    }

    fn enc_amount_recipient(&self) -> Ciphertext<E> {
        self.multi_ciphertexts.get_recipient()
    }

    fn enc_fee(&self) -> Ciphertext<E> {
        self.multi_ciphertexts.get_fee()
    }

    fn recipient(&self) -> EncryptionKey<E> {
        self.enc_keys.get_recipient()
    }
}

impl<E: JubjubEngine> ConfidentialProofContext<E, Unchecked> {
    fn new(
        proof: Proof<E>,
        rvk: PublicKey<E>,
        enc_key_sender: EncryptionKey<E>,
        enc_keys: MultiEncKeys<E>,
        multi_ciphertexts: MultiCiphertexts<E, Confidential>,
        encrypted_balance: Ciphertext<E>,
        g_epoch: edwards::Point<E, PrimeOrder>,
        nonce: edwards::Point<E, PrimeOrder>,
    ) -> Self {
        ConfidentialProofContext {
            proof,
            rvk,
            enc_key_sender,
            enc_keys,
            multi_ciphertexts,
            encrypted_balance,
            g_epoch,
            nonce,
            _marker: PhantomData,
        }
    }

    fn check_proof(
        &self,
        prepared_vk: &PreparedVerifyingKey<E>
    ) -> Result<ConfidentialProofContext<E, Checked>, SynthesisError> {
        let mut public_input = [E::Fr::zero(); 22];

        {
            let (x, y) = self.enc_key_sender.0.into_xy();
            public_input[0] = x;
            public_input[1] = y;
        }
        {
            let (x, y) = self.enc_keys.get_recipient().0.into_xy();
            public_input[2] = x;
            public_input[3] = y;
        }
        {
            let (x, y) = self.enc_amount_sender().left.into_xy();
            public_input[4] = x;
            public_input[5] = y;
        }
        {
            let (x, y) = self.enc_amount_recipient().left.into_xy();
            public_input[6] = x;
            public_input[7] = y;
        }
        {
            let (x, y) = self.enc_amount_sender().right.into_xy();
            public_input[8] = x;
            public_input[9] = y;
        }
        {
            let (x, y) = self.enc_fee().left.into_xy();
            public_input[10] = x;
            public_input[11] = y;
        }
        {
            let (x, y) = self.encrypted_balance.left.into_xy();
            public_input[12] = x;
            public_input[13] = y;
        }
        {
            let (x, y) = self.encrypted_balance.right.into_xy();
            public_input[14] = x;
            public_input[15] = y;
        }
        {
            let (x, y) = self.rvk.0.into_xy();
            public_input[16] = x;
            public_input[17] = y;
        }
        {
            let (x, y) = self.g_epoch.into_xy();
            public_input[18] = x;
            public_input[19] = y;
        }
        {
            let (x, y) = self.nonce.into_xy();
            public_input[20] = x;
            public_input[21] = y;
        }

        // This verification is just an error handling, not validate if it returns `true`,
        // because public input of encrypted balance needs to be updated on-chain.
        if let Err(_) = verify_proof(prepared_vk, &self.proof, &public_input[..]) {
            return Err(SynthesisError::MalformedVerifyingKey)
        }

        Ok(convert_to_checked::<E, Unchecked, Checked>(&self))
    }
}

fn convert_to_checked<E: JubjubEngine, C1, C2>(from: &ConfidentialProofContext<E, C1>) -> ConfidentialProofContext<E, C2> {
    ConfidentialProofContext {
        proof: from.proof,
        rvk: from.rvk,
        enc_key_sender: from.enc_key_sender,
        enc_keys: from.enc_keys,
        multi_ciphertexts: from.multi_ciphertexts,
        encrypted_balance: from.encrypted_balance,
        g_epoch: from.g_epoch,
        nonce: from.nonce,
        _marker: PhantomData,
    }
}

impl<E: JubjubEngine> ConfidentialProofContext<E, Checked> {
    fn gen_xt(&self, spending_key: &SpendingKey<E>, alpha: E::Fs) -> io::Result<ConfidentialXt> {

        // Generate the re-randomized sign key
		let mut rsk_bytes = [0u8; 32];
		spending_key
            .into_rsk(alpha)
            .write(&mut rsk_bytes[..])?;

		let mut rvk_bytes = [0u8; 32];
		self
			.rvk
			.write(&mut rvk_bytes[..])?;

		let mut proof_bytes = [0u8; 192];
		self
			.proof
			.write(&mut proof_bytes[..])?;

		let mut enc_key_sender = [0u8; 32];
		self
			.enc_key_sender
			.write(&mut enc_key_sender[..])?;

		let mut enc_key_recipient = [0u8; 32];
		self
			.recipient()
			.write(&mut enc_key_recipient[..])?;

		let mut enc_amount_recipient = [0u8; 64];
		self
            .enc_amount_recipient()
			.write(&mut enc_amount_recipient[..])?;

		let mut enc_amount_sender = [0u8; 64];
		self
			.enc_amount_sender()
			.write(&mut enc_amount_sender[..])?;

		let mut enc_fee = [0u8; 64];
		self
			.enc_fee()
			.write(&mut enc_fee[..])?;

		let mut enc_balance = [0u8; 64];
		self
            .encrypted_balance
			.write(&mut enc_balance[..])?;

		let mut nonce = [0u8; 32];
		self
			.nonce
			.write(&mut nonce[..])?;

		let tx = ConfidentialXt {
			proof: proof_bytes,
			rvk: rvk_bytes,
			enc_key_sender,
			enc_key_recipient,
			enc_amount_recipient,
			enc_amount_sender,
			rsk: rsk_bytes,
			enc_fee,
			enc_balance,
			nonce,
		};

		Ok(tx)
    }
}

pub trait Submitter {
    fn submit<R: Rng>(&self, asset_id: Option<u32>, api: &Api, rng: &mut R);
}

/// Transaction components which is needed to create a signed `UncheckedExtrinsic`.
pub struct ConfidentialXt{
    pub proof: [u8; 192],                // 192 bytes
    pub enc_key_sender: [u8; 32],        // 32 bytes
    pub enc_key_recipient: [u8; 32],     // 32 bytes
    pub enc_amount_recipient: [u8; 64],  // 64 bytes
	pub enc_amount_sender: [u8; 64],     // 64 bytes
	pub enc_fee: [u8; 64],			     // 64 bytes
	pub rsk: [u8; 32],                   // 32 bytes
	pub rvk: [u8; 32],                   // 32 bytes
	pub enc_balance: [u8; 64],           // 32 bytes
	pub nonce: [u8; 32],
}

impl Submitter for ConfidentialXt {
    fn submit<R: Rng>(&self, asset_id: Option<u32>, api: &Api, rng: &mut R) {
        unimplemented!();
    }
}

// impl<E: JubjubEngine> ConfidentialProofContext<E> {
//     pub fn gen_proof<R: Rng>(
//         amount: u32,
//         remaining_balance: u32,
//         fee: u32,
//         alpha: E::Fs,
//         proving_key: &Parameters<E>,
//         prepared_vk: &PreparedVerifyingKey<E>,
//         proof_generation_key: &ProofGenerationKey<E>,
//         enc_keys: &MultiEncKeys<E>,
//         cipher_balance: &Ciphertext<E>,
//         g_epoch: &edwards::Point<E, PrimeOrder>,
//         rng: &mut R,
//         params: &E::Params,
//     ) -> Result<Self, SynthesisError>
//     {
//         let randomness = E::Fs::rand(rng);

//         let dec_key_sender = proof_generation_key.into_decryption_key()?;
//         let enc_key_sender = proof_generation_key.into_encryption_key(params)?;

//         let rvk = PublicKey(proof_generation_key.0.clone().into())
//             .randomize(
//                 alpha,
//                 FixedGenerators::NoteCommitmentRandomness,
//                 params,
//         );
//         let nonce = g_epoch.mul(dec_key_sender.0, params);

//         let instance = Transfer {
//             params: params,
//             amount: Some(amount),
//             remaining_balance: Some(remaining_balance),
//             randomness: Some(&randomness),
//             alpha: Some(&alpha),
//             proof_generation_key: Some(&proof_generation_key),
//             dec_key_sender: Some(&dec_key_sender),
//             enc_key_recipient: Some(&enc_keys.get_recipient()),
//             encrypted_balance: Some(&cipher_balance),
//             fee: Some(fee),
//             g_epoch: Some(g_epoch),
//         };

//         // Crate proof
//         let proof = create_random_proof(instance, proving_key, rng)?;

//         let mut public_input = [E::Fr::zero(); 22];
//         let p_g = FixedGenerators::NoteCommitmentRandomness;

//         let cipher_sender = Ciphertext::encrypt(
//             amount,
//             randomness,
//             &enc_key_sender,
//             p_g,
//             params
//         );

//         let cipher_recipient = Ciphertext::encrypt(
//             amount,
//             randomness,
//             &enc_keys.get_recipient(),
//             p_g,
//             params
//         );

//         let cipher_fee = Ciphertext::encrypt(
//             fee,
//             randomness,
//             &enc_key_sender,
//             p_g,
//             params
//         );

//         {
//             let (x, y) = enc_key_sender.0.into_xy();
//             public_input[0] = x;
//             public_input[1] = y;
//         }
//         {
//             let (x, y) = enc_keys.get_recipient().0.into_xy();
//             public_input[2] = x;
//             public_input[3] = y;
//         }
//         {
//             let (x, y) = cipher_sender.left.into_xy();
//             public_input[4] = x;
//             public_input[5] = y;
//         }
//         {
//             let (x, y) = cipher_recipient.left.into_xy();
//             public_input[6] = x;
//             public_input[7] = y;
//         }
//         {
//             let (x, y) = cipher_sender.right.into_xy();
//             public_input[8] = x;
//             public_input[9] = y;
//         }
//         {
//             let (x, y) = cipher_fee.left.into_xy();
//             public_input[10] = x;
//             public_input[11] = y;
//         }
//         {
//             let (x, y) = cipher_balance.left.into_xy();
//             public_input[12] = x;
//             public_input[13] = y;
//         }
//         {
//             let (x, y) = cipher_balance.right.into_xy();
//             public_input[14] = x;
//             public_input[15] = y;
//         }
//         {
//             let (x, y) = rvk.0.into_xy();
//             public_input[16] = x;
//             public_input[17] = y;
//         }
//         {
//             let (x, y) = g_epoch.into_xy();
//             public_input[18] = x;
//             public_input[19] = y;
//         }
//         {
//             let (x, y) = nonce.into_xy();
//             public_input[20] = x;
//             public_input[21] = y;
//         }

//         // This verification is just an error handling, not validate if it returns `true`,
//         // because public input of encrypted balance needs to be updated on-chain.
//         if let Err(_) = verify_proof(prepared_vk, &proof, &public_input[..]) {
//             return Err(SynthesisError::MalformedVerifyingKey)
//         }

//         let proof = ConfidentialProof {
//             proof,
//             rvk,
//             enc_key_sender,
//             enc_keys: MultiEncKeys::new_for_confidential(enc_keys.get_recipient().clone()),
//             multi_ciphertexts: MultiCiphertexts::new_for_confidential(cipher_sender, cipher_recipient, cipher_fee),
//             cipher_balance: cipher_balance.clone(),
//             nonce,
//         };

//         Ok(proof)
//     }
// }

// pub struct AnonymousProof<E: JubjubEngine> {
//     proof: Proof<E>,
//     rvk: PublicKey<E>,
//     enc_key_sender: EncryptionKey<E>,
//     enc_keys: MultiEncKeys<E>,
//     multi_ciphertexts: MultiCiphertexts<E>,
//     cipher_balance: Ciphertext<E>,
// }

// impl<E: JubjubEngine> AnonymousProof<E> {
//     pub fn gen_proof<R: Rng>(
//         amount: u32,
//         remaining_balance: u32,
//         fee: u32,
//         alpha: E::Fs,
//         proving_key: &Parameters<E>,
//         prepared_vk: &PreparedVerifyingKey<E>,
//         proof_generation_key: &ProofGenerationKey<E>,
//         enc_keys: &MultiEncKeys<E>,
//         cipher_balance: Ciphertext<E>,
//         g_epoch: &edwards::Point<E, PrimeOrder>,
//         rng: &mut R,
//         params: &E::Params,
//     ) -> Result<Self, SynthesisError>
//     {

//         unimplemented!();
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, XorShiftRng, Rng};
    use crate::{ProofGenerationKey, EncryptionKey};
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

        let g_epoch = edwards::Point::rand(rng, params).mul_by_cofactor(params);

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
            &g_epoch,
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
