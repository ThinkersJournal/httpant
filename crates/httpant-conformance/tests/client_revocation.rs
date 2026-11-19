// SPDX-License-Identifier: MIT OR Apache-2.0
//! README: "rustls does not check revocation" (claim-to-command audit, ROADMAP row 19). A private CA issues a leaf for 127.0.0.1 and then
//! revokes it in a CRL. The control proves the revocation is real: a rustls verifier given that CRL refuses the leaf. The claim is then
//! run as written: the HTTPAnt client, given the same CA, still connects.

use std::sync::Arc;

use httpant_client::Client;
use rcgen::{
    BasicConstraints, CertificateParams, CertificateRevocationListParams, IsCa, Issuer,
    KeyIdMethod, KeyPair, KeyUsagePurpose, RevocationReason, RevokedCertParams, SerialNumber,
    date_time_ymd,
};
use rustls::RootCertStore;
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateRevocationListDer, PrivateKeyDer, ServerName};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

#[tokio::test]
async fn a_revoked_leaf_is_refused_by_a_crl_aware_verifier_and_accepted_by_the_client() {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);

    let leaf_key = KeyPair::generate().unwrap();
    let mut leaf_params = CertificateParams::new(vec!["127.0.0.1".to_owned()]).unwrap();
    leaf_params.serial_number = Some(SerialNumber::from(4242u64));
    let leaf = leaf_params.signed_by(&leaf_key, &issuer).unwrap();

    let crl = CertificateRevocationListParams {
        this_update: date_time_ymd(2020, 1, 1),
        next_update: date_time_ymd(2099, 1, 1),
        crl_number: SerialNumber::from(1u64),
        issuing_distribution_point: None,
        revoked_certs: vec![RevokedCertParams {
            serial_number: SerialNumber::from(4242u64),
            revocation_time: date_time_ymd(2021, 1, 1),
            reason_code: Some(RevocationReason::KeyCompromise),
            invalidity_date: None,
        }],
        key_identifier_method: KeyIdMethod::Sha256,
    }
    .signed_by(&issuer)
    .unwrap();

    // A TLS server presenting the revoked leaf, answering `ok` to any request.
    let key = PrivateKeyDer::try_from(leaf_key.serialize_der()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![leaf.der().clone()], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let _ = tls
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await;
                let _ = tls.shutdown().await;
            });
        }
    });

    // Control: the same CA, with the CRL, refuses the leaf; without the CRL it accepts it (so the CRL is what decides).
    let mut roots = RootCertStore::empty();
    roots.add(ca_cert.der().clone()).unwrap();
    let roots = Arc::new(roots);
    let handshake = |crls: Vec<CertificateRevocationListDer<'static>>| {
        let roots = Arc::clone(&roots);
        async move {
            let verifier = WebPkiServerVerifier::builder_with_provider(roots, provider())
                .with_crls(crls)
                .build()
                .unwrap();
            let config = rustls::ClientConfig::builder_with_provider(provider())
                .with_safe_default_protocol_versions()
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(verifier)
                .with_no_client_auth();
            let tcp = TcpStream::connect(addr).await.unwrap();
            TlsConnector::from(Arc::new(config))
                .connect(ServerName::try_from("127.0.0.1").unwrap(), tcp)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    };
    let refused = handshake(vec![crl.der().clone()]).await;
    assert!(
        refused.as_ref().is_err_and(|e| e.contains("Revoked")),
        "control: a CRL-aware verifier should refuse the revoked leaf, got {refused:?}"
    );
    assert_eq!(
        handshake(Vec::new()).await,
        Ok(()),
        "control: without the CRL the same leaf should verify"
    );

    // The claim: the HTTPAnt client checks no revocation, so it connects.
    let client = Client::builder()
        .add_root_pem(ca_cert.pem().as_bytes())
        .build()
        .unwrap();
    let text = client
        .get(format!("https://{addr}/"))
        .send()
        .await
        .expect("README says revocation is not checked: the revoked leaf should be accepted")
        .text()
        .await
        .unwrap();
    assert_eq!(text, "ok");
}
