use std::borrow::Borrow;

use anyhow::Result;
use p3_baby_bear::BabyBear;
use p3_field::AbstractField;
use sp1_core::{
    air::PublicValues,
    stark::{MachineProof, MachineVerificationError, StarkGenericConfig},
    utils::BabyBearPoseidon2,
};
use sp1_recursion_core::{air::RecursionPublicValues, stark::config::BabyBearPoseidon2Outer};

use crate::{
    CoreSC, HashableKey, OuterSC, SP1CoreProofData, SP1Prover, SP1ReduceProof, SP1VerifyingKey,
};

impl SP1Prover {
    /// Verify a core proof by verifying the shards, verifying lookup bus, verifying that the
    /// shards are contiguous and complete.
    pub fn verify(
        &self,
        proof: &SP1CoreProofData,
        vk: &SP1VerifyingKey,
    ) -> Result<(), MachineVerificationError<CoreSC>> {
        let mut challenger = self.core_machine.config().challenger();
        let machine_proof = MachineProof {
            shard_proofs: proof.0.to_vec(),
        };
        self.core_machine
            .verify(&vk.vk, &machine_proof, &mut challenger)?;

        // Verify shard transitions
        for (i, shard_proof) in proof.0.iter().enumerate() {
            let public_values = PublicValues::from_vec(shard_proof.public_values.clone());
            // Verify shard transitions
            if i == 0 {
                // If it's the first shard, index should be 1.
                if public_values.shard != BabyBear::one() {
                    return Err(MachineVerificationError::InvalidPublicValues(
                        "first shard not 1",
                    ));
                }
                if public_values.start_pc != vk.vk.pc_start {
                    return Err(MachineVerificationError::InvalidPublicValues(
                        "wrong pc_start",
                    ));
                }
            } else {
                let prev_shard_proof = &proof.0[i - 1];
                let prev_public_values =
                    PublicValues::from_vec(prev_shard_proof.public_values.clone());
                // For non-first shards, the index should be the previous index + 1.
                if public_values.shard != prev_public_values.shard + BabyBear::one() {
                    return Err(MachineVerificationError::InvalidPublicValues(
                        "non incremental shard index",
                    ));
                }
                // Start pc should be what the next pc declared in the previous shard was.
                if public_values.start_pc != prev_public_values.next_pc {
                    return Err(MachineVerificationError::InvalidPublicValues("pc mismatch"));
                }
                // Digests and exit code should be the same in all shards.
                if public_values.committed_value_digest != prev_public_values.committed_value_digest
                    || public_values.deferred_proofs_digest
                        != prev_public_values.deferred_proofs_digest
                    || public_values.exit_code != prev_public_values.exit_code
                {
                    return Err(MachineVerificationError::InvalidPublicValues(
                        "digest or exit code mismatch",
                    ));
                }
                // The last shard should be halted. Halt is signaled with next_pc == 0.
                if i == proof.0.len() - 1 && public_values.next_pc != BabyBear::zero() {
                    return Err(MachineVerificationError::InvalidPublicValues(
                        "last shard isn't halted",
                    ));
                }
                // All non-last shards should not be halted.
                if i != proof.0.len() - 1 && public_values.next_pc == BabyBear::zero() {
                    return Err(MachineVerificationError::InvalidPublicValues(
                        "non-last shard is halted",
                    ));
                }
            }
        }

        Ok(())
    }

    /// Verify a compressed proof.
    pub fn verify_compressed(
        &self,
        proof: &SP1ReduceProof<BabyBearPoseidon2>,
        vk: &SP1VerifyingKey,
    ) -> Result<(), MachineVerificationError<CoreSC>> {
        let mut challenger = self.compress_machine.config().challenger();
        let machine_proof = MachineProof {
            shard_proofs: vec![proof.proof.clone()],
        };
        self.compress_machine
            .verify(&self.compress_vk, &machine_proof, &mut challenger)?;

        // Validate public values
        let public_values: &RecursionPublicValues<_> =
            proof.proof.public_values.as_slice().borrow();

        // `is_complete` should be 1. In the reduce program, this ensures that the proof is fully reduced.
        if public_values.is_complete != BabyBear::one() {
            return Err(MachineVerificationError::InvalidPublicValues(
                "is_complete is not 1",
            ));
        }

        // Verify that the proof is for the sp1 vkey we are expecting.
        let vkey_hash = vk.hash_babybear();
        if public_values.sp1_vk_digest != vkey_hash {
            return Err(MachineVerificationError::InvalidPublicValues(
                "sp1 vk hash mismatch",
            ));
        }

        // Verify that the reduce program is the one we are expecting.
        let recursion_vkey_hash = self.compress_vk.hash_babybear();
        if public_values.compress_vk_digest != recursion_vkey_hash {
            return Err(MachineVerificationError::InvalidPublicValues(
                "recursion vk hash mismatch",
            ));
        }

        Ok(())
    }

    /// Verify a shrink proof.
    pub fn verify_shrink(
        &self,
        proof: &SP1ReduceProof<BabyBearPoseidon2>,
        vk: &SP1VerifyingKey,
    ) -> Result<(), MachineVerificationError<CoreSC>> {
        let mut challenger = self.shrink_machine.config().challenger();
        let machine_proof = MachineProof {
            shard_proofs: vec![proof.proof.clone()],
        };
        self.shrink_machine
            .verify(&self.shrink_vk, &machine_proof, &mut challenger)?;

        // Validate public values
        let public_values: &RecursionPublicValues<_> =
            proof.proof.public_values.as_slice().borrow();

        // `is_complete` should be 1. In the reduce program, this ensures that the proof is fully reduced.
        if public_values.is_complete != BabyBear::one() {
            return Err(MachineVerificationError::InvalidPublicValues(
                "is_complete is not 1",
            ));
        }

        // Verify that the proof is for the sp1 vkey we are expecting.
        let vkey_hash = vk.hash_babybear();
        if public_values.sp1_vk_digest != vkey_hash {
            return Err(MachineVerificationError::InvalidPublicValues(
                "sp1 vk hash mismatch",
            ));
        }

        Ok(())
    }

    /// Verify a wrap bn254 proof.
    pub fn verify_wrap_bn254(
        &self,
        proof: &SP1ReduceProof<BabyBearPoseidon2Outer>,
        vk: &SP1VerifyingKey,
    ) -> Result<(), MachineVerificationError<OuterSC>> {
        let mut challenger = self.wrap_machine.config().challenger();
        let machine_proof = MachineProof {
            shard_proofs: vec![proof.proof.clone()],
        };
        self.wrap_machine
            .verify(&self.wrap_vk, &machine_proof, &mut challenger)?;

        // Validate public values
        let public_values: &RecursionPublicValues<_> =
            proof.proof.public_values.as_slice().borrow();

        // `is_complete` should be 1. In the reduce program, this ensures that the proof is fully reduced.
        if public_values.is_complete != BabyBear::one() {
            return Err(MachineVerificationError::InvalidPublicValues(
                "is_complete is not 1",
            ));
        }

        // Verify that the proof is for the sp1 vkey we are expecting.
        let vkey_hash = vk.hash_babybear();
        if public_values.sp1_vk_digest != vkey_hash {
            return Err(MachineVerificationError::InvalidPublicValues(
                "sp1 vk hash mismatch",
            ));
        }

        Ok(())
    }
}
