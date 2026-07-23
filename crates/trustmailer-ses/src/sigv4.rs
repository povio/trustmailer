use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub struct SigningInput<'a> {
    pub method: &'a str,
    pub uri: &'a str,
    pub host: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    pub amz_date: &'a str,
    pub body: &'a [u8],
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
    pub session_token: Option<&'a str>,
}

/// Computes the `Authorization` header value for AWS Signature Version 4.
pub fn sign(input: &SigningInput) -> String {
    let date_stamp = &input.amz_date[..8];
    let payload_hash = hex::encode(Sha256::digest(input.body));

    let mut signed_header_names = vec!["host", "x-amz-date"];
    if input.session_token.is_some() {
        signed_header_names.push("x-amz-security-token");
    }
    signed_header_names.sort_unstable();

    let mut canonical_headers = String::new();
    for name in &signed_header_names {
        let value = match *name {
            "host" => input.host,
            "x-amz-date" => input.amz_date,
            "x-amz-security-token" => input.session_token.unwrap_or_default(),
            _ => "",
        };
        canonical_headers.push_str(name);
        canonical_headers.push(':');
        canonical_headers.push_str(value);
        canonical_headers.push('\n');
    }
    let signed_headers = signed_header_names.join(";");

    let canonical_request = format!(
        "{}\n{}\n\n{}\n{}\n{}",
        input.method, input.uri, canonical_headers, signed_headers, payload_hash
    );
    let hashed_canonical_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));

    let credential_scope = format!(
        "{date_stamp}/{}/{}/aws4_request",
        input.region, input.service
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        input.amz_date, credential_scope, hashed_canonical_request
    );

    let k_date = hmac_sha256(
        format!("AWS4{}", input.secret_access_key).as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, input.region.as_bytes());
    let k_service = hmac_sha256(&k_region, input.service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        input.access_key_id
    )
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts key of any length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}
