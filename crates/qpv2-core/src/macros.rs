#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        eprintln!($($arg)*);
    }
}

#[macro_export]
macro_rules! spx_keygen {
    ($kg:ty, $n:expr, $seed:expr, $index:expr) => {{
        const N: usize = $n;
        let path = format!("{}{}", KDF_PATH_PREFIX, $index);

        // Spliting the master seed into three length-equal parts.
        let sk_seed: &[u8; N] = $seed[0..N].try_into().expect("Invalid sk seed length");
        let sk_prf: &[u8; N] = $seed[N..2 * N].try_into().expect("Invalid sk prf length");
        let pk_seed: &[u8; N] = $seed[2 * N..3 * N]
            .try_into()
            .expect("Invalid pk seed length");

        let sk_seed_kd: SecureVec = utilities::derive_hkdf_key(sk_seed, path.as_bytes(), N)?;
        let sk_prf_kd: SecureVec = utilities::derive_hkdf_key(sk_prf, path.as_bytes(), N)?;
        let pk_seed_kd: SecureVec = utilities::derive_hkdf_key(pk_seed, path.as_bytes(), N)?;

        let sk_seed_kd_ref: &[u8; N] = sk_seed_kd
            .as_ref()
            .try_into()
            .map_err(|_| "Invalid sk seed length")?;
        let sk_prf_kd_ref: &[u8; N] = sk_prf_kd
            .as_ref()
            .try_into()
            .map_err(|_| "Invalid sk prf length")?;
        let pk_seed_kd_ref: &[u8; N] = pk_seed_kd
            .as_ref()
            .try_into()
            .map_err(|_| "Invalid pk seed length")?;

        let (pub_key, pri_key) =
            <$kg>::keygen_with_seeds(sk_seed_kd_ref, sk_prf_kd_ref, pk_seed_kd_ref);

        let mut pub_bytes = pub_key.into_bytes();
        let mut pri_bytes = pri_key.into_bytes();

        let result = Ok((
            SecureVec::from_vec(pub_bytes.to_vec()),
            SecureVec::from_vec(pri_bytes.to_vec()),
        ));

        pub_bytes.zeroize();
        pri_bytes.zeroize();

        result
    }};
}

#[macro_export]
macro_rules! ckb_spx_sign {
    ($module:ident, $pri_key:expr, $message_vec:expr, $variant:expr) => {{
        let pri_key_ref: &[u8; $module::SK_LEN] = $pri_key
            .as_ref()
            .try_into()
            .map_err(|_| "Invalid private key length".to_string())?;

        let signing_key = $module::PrivateKey::try_from_bytes(&pri_key_ref)
            .map_err(|e| format!("Unable to construct private key: {:?}", e))?;
        let signature = signing_key
            .try_sign($message_vec, &[], true)
            .map_err(|e| format!("Signing error: {:?}", e))?;

        let all_in_one_config: [u8; 4] = [
            MULTISIG_RESERVED_FIELD_VALUE,
            REQUIRED_FIRST_N,
            THRESHOLD,
            PUBKEY_NUM,
        ];
        let param_id_and_sign_flag: u8 = ($variant << 1) | 1;

        // The sphincs+ public key is the second half of the private key
        // [sk_seed][sk_prf] [pk_seed][pk_root]
        let pub_key_slice: &[u8] = &pri_key_ref[$module::PK_LEN..$module::SK_LEN];
        let ckb_qr_full_signature = [
            &all_in_one_config[..],
            &[param_id_and_sign_flag],
            &pub_key_slice[..],
            signature.as_slice(),
        ]
        .concat();

        Ok(ckb_qr_full_signature)
    }};
}

#[macro_export]
macro_rules! raw_spx_sign {
    ($module:ident, $pri_key:expr, $message_vec:expr, $variant:expr) => {{
        let pri_key_ref: &[u8; $module::SK_LEN] = $pri_key
            .as_ref()
            .try_into()
            .map_err(|_| "Invalid private key length".to_string())?;

        let signing_key = $module::PrivateKey::try_from_bytes(&pri_key_ref)
            .map_err(|e| format!("Unable to construct private key: {:?}", e))?;
        let signature = signing_key
            .try_sign($message_vec, &[], true)
            .map_err(|e| format!("Signing error: {:?}", e))?;

        // The sphincs+ public key is the second half of the private key
        // [sk_seed][sk_prf] [pk_seed][pk_root]
        let pub_key_slice: &[u8] = &pri_key_ref[$module::PK_LEN..$module::SK_LEN];

        Ok((signature.as_slice().to_vec(), pub_key_slice.to_vec()))
    }};
}

#[macro_export]
macro_rules! raw_spx_verify {
    ($module:ident, $public_key:expr, $message:expr, $signature:expr) => {{
        use $module::{PublicKey, PK_LEN, SIG_LEN};
        if $public_key.len() != PK_LEN {
            return Err(format!(
                "Invalid public key length: expected {}, got {}",
                PK_LEN,
                $public_key.len()
            ));
        }
        if $signature.len() != SIG_LEN {
            return Err(format!(
                "Invalid signature length: expected {}, got {}",
                SIG_LEN,
                $signature.len()
            ));
        }
        let pk_array: &[u8; PK_LEN] = $public_key.try_into().unwrap();
        let sig_array: &[u8; SIG_LEN] = $signature.try_into().unwrap();

        let pk = PublicKey::try_from_bytes(pk_array)
            .map_err(|e| format!("Failed to deserialize public key: {:?}", e))?;

        let is_valid = pk.verify($message, sig_array, &[]);
        Ok(is_valid)
    }};
}
