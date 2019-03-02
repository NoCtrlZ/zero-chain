#[cfg(feature = "std")]
use serde::{Serialize, Serializer, Deserialize, Deserializer};
use fixed_hash::construct_fixed_hash;
use jubjub::redjubjub;
use runtime_primitives::traits::{Verify, Lazy};
use crate::sig_vk::SigVerificationKey;
use jubjub::curve::FixedGenerators;
use crate::JUBJUB;

#[cfg(feature = "std")]
use substrate_primitives::bytes;

const SIZE: usize = 64;

construct_fixed_hash! {
    pub struct H512(SIZE);
}

pub type RedjubjubSignature = H512;

impl Verify for RedjubjubSignature {
    type Signer = SigVerificationKey;
    fn verify<L: Lazy<[u8]>>(&self, mut msg: L, signer: &Self::Signer) -> bool {
        let sig = match self.into_signature() {
            Some(s) => s,
            None => return false
        };

        let p_g = FixedGenerators::SpendingKeyGenerator;

        match signer.into_verification_key() {
            Some(vk) => return vk.verify(msg.get(), &sig, p_g, &JUBJUB),
            None => return false
        }
        
    }
}

#[cfg(feature = "std")]
impl Serialize for H512 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> 
        where S: Serializer
    {
        bytes::serialize(&self.0, serializer)
    }
}

#[cfg(feature = "std")]
impl<'de> Deserialize<'de> for H512 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de>
    {
        bytes::deserialize_check_len(deserializer, bytes::ExpectedLen::Exact(SIZE))
            .map(|x| H512::from_slice(&x))
    }
}

impl codec::Encode for H512 {
    fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
        self.0.using_encoded(f)
    }
}

impl codec::Decode for H512 {
    fn decode<I: codec::Input>(input: &mut I) -> Option<Self> {
        <[u8; SIZE] as codec::Decode>::decode(input).map(H512)
    }
}

impl H512 {
    pub fn into_signature(&self) -> Option<redjubjub::Signature> {   
        redjubjub::Signature::read(&self.0[..]).ok()        
    }

    pub fn from_signature(sig: &redjubjub::Signature) -> Self {
        let mut writer = [0u8; 64];
        sig.write(&mut writer[..]).unwrap();
        H512::from_slice(&writer)
    }
}

impl Into<RedjubjubSignature> for redjubjub::Signature {
    fn into(self) -> RedjubjubSignature {
        RedjubjubSignature::from_signature(&self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng, XorShiftRng};    
    use pairing::bls12_381::Bls12;
    use jubjub::curve::{FixedGenerators, JubjubBls12};
    use jubjub::redjubjub::PublicKey;
    use codec::{Encode, Decode};

    #[test]
    fn test_sig_into_from() {
        let mut rng = &mut XorShiftRng::from_seed([0x3dbe6259, 0x8d313d76, 0x3237db17, 0xe5bc0654]);
        let p_g = FixedGenerators::SpendingKeyGenerator;
        let params = &JubjubBls12::new();

        let sk = redjubjub::PrivateKey::<Bls12>(rng.gen());
        let vk = PublicKey::from_private(&sk, p_g, params);

        let msg = b"Foo bar";
        let sig1 = sk.sign(msg, &mut rng, p_g, params);
        
        assert!(vk.verify(msg, &sig1, p_g, params));

        let sig_b = Signature::from_signature(&sig1);        
        let sig2 = sig_b.into_signature().unwrap();

        assert!(sig1 == sig2);
    }

    #[test]
    fn test_sig_encode_decode() {
        let mut rng = &mut XorShiftRng::from_seed([0x3dbe6259, 0x8d313d76, 0x3237db17, 0xe5bc0654]);
        let p_g = FixedGenerators::SpendingKeyGenerator;
        let params = &JubjubBls12::new();

        let sk = redjubjub::PrivateKey::<Bls12>(rng.gen());
        let vk = PublicKey::from_private(&sk, p_g, params);

        let msg = b"Foo bar";
        let sig1 = sk.sign(msg, &mut rng, p_g, params);
        
        assert!(vk.verify(msg, &sig1, p_g, params));
        let sig_b = Signature::from_signature(&sig1);
        
        let encoded_sig = sig_b.encode();        
        
        let decoded_sig = Signature::decode(&mut encoded_sig.as_slice()).unwrap();
        assert_eq!(sig_b, decoded_sig);
    }    
}
