// SPDX-FileCopyrightText: 2026 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

//! DANE TLSA validation for outbound SMTP delivery (RFC 7672).
//!
//! Looks up `_25._tcp.<mx-host>` TLSA records and validates the server's TLS
//! certificate against them.  Only DANE-EE (`cert_usage` 3) records are supported;
//! PKIX-TA/EE records require trust-chain verification which is out of scope here.
//!
//! **Important:** RFC 7672 §3 requires that TLSA records be DNSSEC-authenticated
//! before enforcement.  This implementation does not yet verify DNSSEC signatures;
//! it should only be deployed alongside a DNSSEC-validating resolver.

use {
    hickory_resolver::{
        proto::rr::{
            rdata::tlsa::{CertUsage, Matching, Selector},
            RData,
        },
        TokioResolver,
    },
    sha2::{Digest, Sha256, Sha512},
    tracing::{debug, warn},
};

/// TLSA record fields, cloned out of the DNS response.
#[derive(Debug, Clone)]
pub struct TlsaRecord {
    pub cert_usage: CertUsage,
    pub selector: Selector,
    pub matching: Matching,
    pub cert_data: Vec<u8>,
}

/// Looks up `_25._tcp.{mx_host}` TLSA records.
///
/// Returns an empty `Vec` when none are found or the lookup fails.
/// Per RFC 7672 §3, an empty result means DANE is not enforced for this host.
pub async fn fetch_tlsa_records(mx_host: &str, resolver: &TokioResolver) -> Vec<TlsaRecord> {
    let name = format!("_25._tcp.{mx_host}");
    match resolver.tlsa_lookup(name.as_str()).await {
        Ok(lookup) => {
            let records: Vec<TlsaRecord> = lookup
                .answers()
                .iter()
                .filter_map(|record| {
                    if let RData::TLSA(tlsa) = &record.data {
                        Some(TlsaRecord {
                            cert_usage: tlsa.cert_usage,
                            selector: tlsa.selector,
                            matching: tlsa.matching,
                            cert_data: tlsa.cert_data.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect();
            debug!("[DANE] {} TLSA record(s) for {}", records.len(), name);
            records
        }
        Err(e) => {
            debug!("[DANE] No TLSA records for {}: {}", name, e);
            Vec::new()
        }
    }
}

/// Returns `true` if `cert_der` matches at least one TLSA record, or if
/// `records` is empty (meaning DANE is not enforced for this host, RFC 7672 §3).
///
/// Only DANE-EE (usage 3) records are evaluated; other usage types are logged
/// and skipped because they require trust-chain verification.
#[allow(clippy::cognitive_complexity)]
pub fn validate_cert_against_tlsa(cert_der: &[u8], records: &[TlsaRecord]) -> bool {
    if records.is_empty() {
        return true;
    }

    for record in records {
        match record.cert_usage {
            CertUsage::DaneEe => {
                if let Some(matched) = check_dane_ee_record(cert_der, record) {
                    if matched {
                        return true;
                    }
                }
            }
            ref usage => {
                debug!(
                    "[DANE] Skipping TLSA record with cert_usage {:?} (only DANE-EE supported)",
                    usage
                );
            }
        }
    }
    false
}

/// Evaluate a single DANE-EE record against `cert_der`.
/// Returns `Some(true)` on match, `Some(false)` on mismatch, `None` when the
/// record cannot be processed (e.g., unsupported selector/matching type).
fn check_dane_ee_record(cert_der: &[u8], record: &TlsaRecord) -> Option<bool> {
    let data: Vec<u8> = match record.selector {
        Selector::Full => cert_der.to_vec(),
        Selector::Spki => extract_spki(cert_der).map(<[u8]>::to_vec)?,
        _ => {
            warn!(
                "[DANE] Unknown TLSA selector {:?}, skipping record",
                record.selector
            );
            return None;
        }
    };

    let matched = match record.matching {
        Matching::Raw => data == record.cert_data,
        Matching::Sha256 => {
            let hash = Sha256::digest(&data);
            hash.as_slice() == record.cert_data
        }
        Matching::Sha512 => {
            let hash = Sha512::digest(&data);
            hash.as_slice() == record.cert_data
        }
        _ => {
            warn!(
                "[DANE] Unknown TLSA matching type {:?}, skipping record",
                record.matching
            );
            return None;
        }
    };
    Some(matched)
}

/// Extract the `SubjectPublicKeyInfo` TLV bytes from a DER-encoded X.509 certificate.
///
/// Returns a sub-slice of `cert_der` covering the full SPKI TLV (tag + length +
/// content), or `None` if the certificate cannot be parsed.
fn extract_spki(cert_der: &[u8]) -> Option<&[u8]> {
    const SEQUENCE: u8 = 0x30;
    const CONTEXT_0: u8 = 0xa0; // [0] EXPLICIT for optional version field
    const INTEGER: u8 = 0x02;

    // Unwrap outer Certificate SEQUENCE
    let (tag, cert_inner, _) = der_parse_tlv(cert_der)?;
    if tag != SEQUENCE {
        return None;
    }

    // First element of Certificate is tbsCertificate SEQUENCE
    let (tag, tbs, _) = der_parse_tlv(cert_inner)?;
    if tag != SEQUENCE {
        return None;
    }

    let mut pos = tbs;

    // Skip optional version [0] EXPLICIT
    if pos.first() == Some(&CONTEXT_0) {
        let (_, _, rest) = der_parse_tlv(pos)?;
        pos = rest;
    }

    // serialNumber INTEGER
    if pos.first() != Some(&INTEGER) {
        return None;
    }
    let (_, _, rest) = der_parse_tlv(pos)?;
    pos = rest;

    // signature AlgorithmIdentifier SEQUENCE
    if pos.first() != Some(&SEQUENCE) {
        return None;
    }
    let (_, _, rest) = der_parse_tlv(pos)?;
    pos = rest;

    // issuer Name (SEQUENCE)
    if pos.first() != Some(&SEQUENCE) {
        return None;
    }
    let (_, _, rest) = der_parse_tlv(pos)?;
    pos = rest;

    // validity SEQUENCE
    if pos.first() != Some(&SEQUENCE) {
        return None;
    }
    let (_, _, rest) = der_parse_tlv(pos)?;
    pos = rest;

    // subject Name (SEQUENCE)
    if pos.first() != Some(&SEQUENCE) {
        return None;
    }
    let (_, _, rest) = der_parse_tlv(pos)?;
    pos = rest;

    // subjectPublicKeyInfo is the next TLV — return it including tag+length bytes
    if pos.first() != Some(&SEQUENCE) {
        return None;
    }
    let (_, _, rest) = der_parse_tlv(pos)?;
    let spki_len = pos.len() - rest.len();
    Some(&pos[..spki_len])
}

/// Parse a single DER TLV. Returns `(tag, content, remaining)`.
fn der_parse_tlv(data: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    if data.is_empty() {
        return None;
    }
    let tag = data[0];
    let (length, after_len) = der_parse_length(&data[1..])?;
    if after_len.len() < length {
        return None;
    }
    Some((tag, &after_len[..length], &after_len[length..]))
}

/// Decode a DER length field. Returns `(length, remaining_bytes)`.
fn der_parse_length(data: &[u8]) -> Option<(usize, &[u8])> {
    if data.is_empty() {
        return None;
    }
    if data[0] < 0x80 {
        return Some((usize::from(data[0]), &data[1..]));
    }
    // Long form: low 7 bits of data[0] say how many bytes encode the length.
    let n_bytes = usize::from(data[0] & 0x7f);
    if n_bytes == 0 || n_bytes > 4 || data.len() <= n_bytes {
        return None;
    }
    let mut length: usize = 0;
    for i in 0..n_bytes {
        length = (length << 8) | usize::from(data[1 + i]);
    }
    Some((length, &data[1 + n_bytes..]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use {
        hickory_resolver::proto::rr::rdata::tlsa::{CertUsage, Matching, Selector},
        sha2::{Digest, Sha256, Sha512},
    };

    fn make_record(
        usage: CertUsage,
        selector: Selector,
        matching: Matching,
        data: Vec<u8>,
    ) -> TlsaRecord {
        TlsaRecord {
            cert_usage: usage,
            selector,
            matching,
            cert_data: data,
        }
    }

    // ── RFC 7672 §3: no TLSA records → DANE not enforced → must pass ────────

    #[test]
    fn test_empty_records_passes() {
        assert!(validate_cert_against_tlsa(b"any-cert-der", &[]));
    }

    // ── DANE-EE + Full + Raw ─────────────────────────────────────────────────

    #[test]
    fn test_dane_ee_full_raw_match() {
        let cert = b"fake-cert-der";
        let record = make_record(
            CertUsage::DaneEe,
            Selector::Full,
            Matching::Raw,
            cert.to_vec(),
        );
        assert!(validate_cert_against_tlsa(cert, &[record]));
    }

    #[test]
    fn test_dane_ee_full_raw_mismatch() {
        let cert = b"fake-cert-der";
        let record = make_record(
            CertUsage::DaneEe,
            Selector::Full,
            Matching::Raw,
            b"different".to_vec(),
        );
        assert!(!validate_cert_against_tlsa(cert, &[record]));
    }

    // ── DANE-EE + Full + SHA-256 ─────────────────────────────────────────────

    #[test]
    fn test_dane_ee_full_sha256_match() {
        let cert = b"fake-cert-der";
        let hash = Sha256::digest(cert).to_vec();
        let record = make_record(CertUsage::DaneEe, Selector::Full, Matching::Sha256, hash);
        assert!(validate_cert_against_tlsa(cert, &[record]));
    }

    #[test]
    fn test_dane_ee_full_sha256_mismatch() {
        let cert = b"fake-cert-der";
        let hash = Sha256::digest(b"wrong-data").to_vec();
        let record = make_record(CertUsage::DaneEe, Selector::Full, Matching::Sha256, hash);
        assert!(!validate_cert_against_tlsa(cert, &[record]));
    }

    // ── DANE-EE + Full + SHA-512 ─────────────────────────────────────────────

    #[test]
    fn test_dane_ee_full_sha512_match() {
        let cert = b"fake-cert-der";
        let hash = Sha512::digest(cert).to_vec();
        let record = make_record(CertUsage::DaneEe, Selector::Full, Matching::Sha512, hash);
        assert!(validate_cert_against_tlsa(cert, &[record]));
    }

    // ── PKIX-TA/EE records are not supported and must not accidentally match ─

    #[test]
    fn test_pkixta_record_skipped_returns_false() {
        let cert = b"cert-data";
        // Even with raw cert data as TLSA data, PKIX-TA must be skipped
        let record = make_record(
            CertUsage::PkixTa,
            Selector::Full,
            Matching::Raw,
            cert.to_vec(),
        );
        assert!(!validate_cert_against_tlsa(cert, &[record]));
    }

    #[test]
    fn test_pkixee_record_skipped_returns_false() {
        let cert = b"cert-data";
        let record = make_record(
            CertUsage::PkixEe,
            Selector::Full,
            Matching::Raw,
            cert.to_vec(),
        );
        assert!(!validate_cert_against_tlsa(cert, &[record]));
    }

    // ── Multiple records: first miss, second hit ─────────────────────────────

    #[test]
    fn test_first_record_mismatch_second_matches() {
        let cert = b"cert-data";
        let wrong = make_record(
            CertUsage::DaneEe,
            Selector::Full,
            Matching::Raw,
            b"wrong".to_vec(),
        );
        let hash = Sha256::digest(cert).to_vec();
        let right = make_record(CertUsage::DaneEe, Selector::Full, Matching::Sha256, hash);
        assert!(validate_cert_against_tlsa(cert, &[wrong, right]));
    }

    // ── SPKI extraction ──────────────────────────────────────────────────────

    /// Build a minimal but structurally valid DER X.509 certificate containing
    /// a known `spki_bytes` block.  Used only for parser unit tests.
    fn build_minimal_cert_der(spki_bytes: &[u8]) -> Vec<u8> {
        let version_inner = encode_integer(&[2u8]);
        let version = encode_tlv(0xa0, &version_inner);
        let serial = encode_integer(&[1u8]);
        let oid = &[
            0x06u8, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b,
        ];
        let null = encode_tlv(0x05, &[]);
        let mut sig_alg_inner = oid.to_vec();
        sig_alg_inner.extend_from_slice(&null);
        let sig_alg = encode_tlv(0x30, &sig_alg_inner);
        let issuer = encode_tlv(0x30, &[]);
        let utc1 = encode_tlv(0x17, b"700101000000Z");
        let utc2 = encode_tlv(0x17, b"991231235959Z");
        let mut validity_inner = utc1.clone();
        validity_inner.extend_from_slice(&utc2);
        let validity = encode_tlv(0x30, &validity_inner);
        let subject = encode_tlv(0x30, &[]);

        let mut tbs_inner = version.clone();
        tbs_inner.extend_from_slice(&serial);
        tbs_inner.extend_from_slice(&sig_alg);
        tbs_inner.extend_from_slice(&issuer);
        tbs_inner.extend_from_slice(&validity);
        tbs_inner.extend_from_slice(&subject);
        tbs_inner.extend_from_slice(spki_bytes);
        let tbs = encode_tlv(0x30, &tbs_inner);

        let sig_value = encode_tlv(0x03, &[0x00]);
        let mut cert_inner = tbs;
        cert_inner.extend_from_slice(&sig_alg);
        cert_inner.extend_from_slice(&sig_value);
        encode_tlv(0x30, &cert_inner)
    }

    fn encode_tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        let len = content.len();
        if len < 0x80 {
            #[allow(clippy::cast_possible_truncation)]
            out.push(len as u8);
        } else if len < 0x100 {
            out.push(0x81);
            #[allow(clippy::cast_possible_truncation)]
            out.push(len as u8);
        } else {
            out.push(0x82);
            #[allow(clippy::cast_possible_truncation)]
            out.push((len >> 8) as u8);
            #[allow(clippy::cast_possible_truncation)]
            out.push(len as u8);
        }
        out.extend_from_slice(content);
        out
    }

    fn encode_integer(bytes: &[u8]) -> Vec<u8> {
        if bytes.first().copied().unwrap_or(0) >= 0x80 {
            let mut v = vec![0x00u8];
            v.extend_from_slice(bytes);
            encode_tlv(0x02, &v)
        } else {
            encode_tlv(0x02, bytes)
        }
    }

    #[test]
    fn test_extract_spki_returns_correct_bytes() {
        let spki_inner = b"\x02\x01\x01"; // INTEGER 1 — a trivially small payload
        let spki = encode_tlv(0x30, spki_inner);
        let cert = build_minimal_cert_der(&spki);
        let extracted = extract_spki(&cert).unwrap();
        assert_eq!(extracted, spki.as_slice());
    }

    #[test]
    fn test_extract_spki_invalid_input_returns_none() {
        assert!(extract_spki(b"not a certificate").is_none());
        assert!(extract_spki(b"").is_none());
    }

    // ── DANE-EE + SPKI + SHA-256 (most common real-world TLSA record type) ──

    #[test]
    fn test_dane_ee_spki_sha256_match() {
        let spki_inner = b"\x02\x08\x00\x01\x02\x03\x04\x05\x06\x07";
        let spki = encode_tlv(0x30, spki_inner);
        let cert = build_minimal_cert_der(&spki);
        let hash = Sha256::digest(&spki).to_vec();
        let record = make_record(CertUsage::DaneEe, Selector::Spki, Matching::Sha256, hash);
        assert!(validate_cert_against_tlsa(&cert, &[record]));
    }

    #[test]
    fn test_dane_ee_spki_sha256_mismatch() {
        let spki_inner = b"\x02\x08\x00\x01\x02\x03\x04\x05\x06\x07";
        let spki = encode_tlv(0x30, spki_inner);
        let cert = build_minimal_cert_der(&spki);
        let hash = Sha256::digest(b"different-spki").to_vec();
        let record = make_record(CertUsage::DaneEe, Selector::Spki, Matching::Sha256, hash);
        assert!(!validate_cert_against_tlsa(&cert, &[record]));
    }
}
