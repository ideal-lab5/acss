/*
 * Copyright 2024 by Ideal Labs, LLC
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */
//! Hashed El Gamal Publicly Verifiable Encryption Scheme
//!
//! This is a Sigma protocol with a Fiat-Shamir Transform over a Hashed El Gamal
//! encryption scheme The scheme allows a prover to convince a verifier that:
//!    1) For a commitment c and (hashed-) El Gamal ciphertext ct that the
//!       preimage of the ciphertext was commited to by c
//!    2) An El Gamal ciphertext was encrypted for a specific recipient (do we
//!       want this? would be better if only the recipient could verify this
//!       aspect... let's consider that later0)
//!

use crate::proofs::ser::{ark_de, ark_se};
use alloc::borrow::ToOwned;
use ark_ec::CurveGroup;
use ark_ff::UniformRand;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_std::{rand::Rng, vec::Vec};
use core::marker::PhantomData;
use serde::{Deserialize, Serialize};
use sha2::Digest;

pub fn cross_product<const N: usize>(a: &[u8; N], b: &[u8; N]) -> [u8; N] {
	let mut o = a.to_owned();
	for (i, ri) in o.iter_mut().enumerate().take(N) {
		*ri ^= b[i];
	}
	o
}

/// the message type required for the hashed el gamal variant
pub type Message = [u8; 32];

/// the ciphertext type
#[derive(
	Clone,
	PartialEq,
	Debug,
	Serialize,
	Deserialize,
	CanonicalDeserialize,
	CanonicalSerialize,
)]
pub struct Ciphertext<C: CurveGroup> {
	#[serde(serialize_with = "ark_se", deserialize_with = "ark_de")]
	pub c1: C,
	pub c2: [u8; 32],
}

impl<C: CurveGroup> Ciphertext<C> {
	/// aggregate two ciphertexts C = <u, v> and C' = <u', v'> by
	/// calculating C'' = (u + u', v (+) v')
	///
	/// This is useful in the hashed el gamal sigma protocol
	pub fn add(self, ct: Ciphertext<C>) -> Self {
		Ciphertext {
			c1: self.c1 + ct.c1,
			c2: cross_product::<32>(&self.c2, &ct.c2),
		}
	}
}

#[derive(Debug, PartialEq)]
pub enum Error {
	InvalidBufferSize,
}

/// the hashed el gamal encryption scheme
pub struct HashedElGamal<C: CurveGroup> {
	_phantom_data: PhantomData<C>,
}

// I want to revisit this implementation later on and potentially modify it
// so that the decrypt function works on the secret key, rather than given by
// the HashedElGAmal type but this is fine for now

impl<C: CurveGroup> HashedElGamal<C> {
	/// Encrypt the hash of a message
	/// r <- Zp
	/// <c1, c2> = <rP, pk (+) H(message)>
	/// note that there is no MAC here, we produce that in the hashed el gamal
	/// sigma protocl impl
	pub fn encrypt<R: Rng + Sized>(
		message: Message,
		pk: C,
		generator: C,
		mut rng: R,
	) -> Result<Ciphertext<C>, Error> {
		let r = C::ScalarField::rand(&mut rng);
		let c1 = generator.mul(r);
		let inner = pk.mul(r);

		let c2: [u8; 32] = cross_product::<32>(
			&hash(inner).try_into().map_err(|_| Error::InvalidBufferSize)?, /*  but how can I test this? need to revist h2 impl */
			&message,
		);

		Ok(Ciphertext { c1, c2 })
	}

	/// decrypt a ciphertext using a secret key, recovered a scalar field
	/// element TODO: error handling
	pub fn decrypt(
		sk: C::ScalarField,
		ciphertext: Ciphertext<C>,
	) -> Result<Message, Error> {
		// s = sk * c1
		let s = ciphertext.c1.mul(sk);
		// m = s (+) c2
		Ok(cross_product::<32>(
			&hash(s).try_into().map_err(|_| Error::InvalidBufferSize)?,
			&ciphertext.c2,
		))
	}
}

/// a map from G -> {0, 1}^{32}
fn hash<G: CanonicalSerialize>(g: G) -> Vec<u8> {
	// let mut out = Vec::with_capacity(g.compressed_size());
	let mut out = Vec::new();
	g.serialize_compressed(&mut out)
		.expect("Enough space has been allocated in the buffer");

	let mut hasher = sha2::Sha256::new();
	hasher.update(&out);
	hasher.finalize().to_vec()
}

#[cfg(test)]
mod test {

	use super::*;
	use ark_bls12_381::{Fr, G1Projective as G1};
	use ark_ec::Group;
	use ark_ff::{One, UniformRand};
	use ark_std::{ops::Mul, test_rng};

	#[test]
	fn basic_encrypt_decrypt_works() {
		let sk = Fr::rand(&mut test_rng());
		let pk = G1::generator().mul(sk);

		let secret = Fr::rand(&mut test_rng());
		let mut secret_bytes = Vec::new();
		secret.serialize_compressed(&mut secret_bytes).unwrap();

		let ct = HashedElGamal::encrypt(
			secret_bytes.clone().try_into().unwrap(),
			pk,
			G1::generator(),
			&mut test_rng(),
		)
		.unwrap();
		let recovered_bytes = HashedElGamal::decrypt(sk, ct).unwrap();
		assert_eq!(recovered_bytes.to_vec(), secret_bytes);
	}

	#[test]
	fn can_add_ciphertexts() {
		let sk = Fr::rand(&mut test_rng());
		let pk = G1::generator().mul(sk);

		let secret = Fr::rand(&mut test_rng());
		let mut secret_bytes = Vec::new();
		secret.serialize_compressed(&mut secret_bytes).unwrap();

		let other_secret = Fr::one();
		let mut other_secret_bytes = Vec::new();
		other_secret.serialize_compressed(&mut other_secret_bytes).unwrap();

		let combined = secret + other_secret;
		let mut combined_bytes = Vec::new();
		combined.serialize_compressed(&mut combined_bytes).unwrap();

		let ct = HashedElGamal::encrypt(
			secret_bytes.clone().try_into().unwrap(),
			pk,
			G1::generator(),
			&mut test_rng(),
		)
		.unwrap();
		let other_ct = HashedElGamal::encrypt(
			other_secret_bytes.clone().try_into().unwrap(),
			pk,
			G1::generator(),
			&mut test_rng(),
		)
		.unwrap();

		let expected = Ciphertext {
			c1: ct.c1 + other_ct.c1,
			c2: cross_product::<32>(&ct.c2, &other_ct.c2).try_into().unwrap(),
		};
		assert_eq!(ct.add(other_ct), expected);
	}

	#[test]
	fn decryption_fails_with_bad_key() {
		let sk = Fr::rand(&mut test_rng());
		let bad_sk = Fr::one() + sk;
		let pk = G1::generator().mul(sk);

		let secret = Fr::rand(&mut test_rng());
		let mut secret_bytes = Vec::new();
		secret.serialize_compressed(&mut secret_bytes).unwrap();

		let ct = HashedElGamal::encrypt(
			secret_bytes.clone().try_into().unwrap(),
			pk,
			G1::generator(),
			&mut test_rng(),
		)
		.unwrap();
		let recovered_bytes = HashedElGamal::decrypt(bad_sk, ct).unwrap();
		assert!(recovered_bytes.to_vec() != secret_bytes);
	}

	#[test]
	fn decryption_fails_with_bad_ciphertext() {
		let sk = Fr::rand(&mut test_rng());
		let pk = G1::generator().mul(sk);

		let secret = Fr::rand(&mut test_rng());
		let mut secret_bytes = Vec::new();
		secret.serialize_compressed(&mut secret_bytes).unwrap();

		let mut ct = HashedElGamal::encrypt(
			secret_bytes.clone().try_into().unwrap(),
			pk,
			G1::generator(),
			&mut test_rng(),
		)
		.unwrap();
		ct.c2 = [1; 32];
		match HashedElGamal::decrypt(sk, ct) {
			Ok(recovered_bytes) => {
				assert!(recovered_bytes.to_vec() != secret_bytes);
			},
			Err(_) => {
				// assert_eq!(e, );
			},
		}
	}
}
