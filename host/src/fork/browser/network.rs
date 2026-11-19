//! Política de destinos para o futuro proxy do navegador.
//!
//! Este módulo ainda não abre sessões nem constitui contenção de rede por si só.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{Result, ensure};

/// Resolve uma única vez e devolve apenas endereços já validados para conexão direta.
/// Uma resposta mista é recusada integralmente, sem escolher um endereço alternativo.
#[allow(dead_code)] // A ativação depende do proxy e do supervisor, ainda não entregues.
pub(super) async fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    ensure!(matches!(port, 80 | 443), "browser_network_port_denied");
    let addresses: Vec<_> = tokio::net::lookup_host((host, port)).await?.collect();
    validate_addresses(&addresses, port)?;
    Ok(addresses)
}

pub(super) fn validate_addresses(addresses: &[SocketAddr], port: u16) -> Result<()> {
    ensure!(matches!(port, 80 | 443), "browser_network_port_denied");
    ensure!(!addresses.is_empty(), "browser_network_dns_empty");
    ensure!(
        addresses.iter().all(|address| address.port() == port && public_ip(address.ip())),
        "browser_network_address_denied"
    );
    Ok(())
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => public_v6(ip),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    // Recusa também faixas especiais sem roteamento público, além das privadas.
    // AS112 é roteável publicamente e permanece permitido nesta política.
    !matches!(
        (a, b, c),
        (0 | 10 | 127 | 224..=255, _, _)
            | (100, 64..=127, _)
            | (169, 254, _)
            | (172, 16..=31, _)
            | (192, 0, 0 | 2)
            | (192, 88, 99)
            | (192, 168, _)
            | (198, 18 | 19 | 51, _)
            | (203, 0, 113)
    )
}

fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return public_v4(mapped);
    }
    let segments = ip.segments();
    // Apenas unicast global 2000::/3; endereços especiais 2001::/23,
    // documentação 2001:db8::/32 e 6to4 são recusados conservadoramente.
    (segments[0] & 0xe000) == 0x2000
        && !(segments[0] == 0x2001 && (segments[1] <= 0x01ff || segments[1] == 0x0db8))
        && segments[0] != 0x2002
        && !(segments[0] == 0x3fff && segments[1] <= 0x0fff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recusa_enderecos_especiais_inclusive_mapeados() {
        for value in [
            "127.0.0.1",
            "10.0.0.1",        // vazamento-ok: endereço reservado usado em teste de recusa
            "172.31.1.1",      // vazamento-ok: endereço reservado usado em teste de recusa
            "192.168.1.1",     // vazamento-ok: endereço reservado usado em teste de recusa
            "169.254.169.254", // vazamento-ok: endereço reservado usado em teste de recusa
            "100.64.0.1",      // vazamento-ok: endereço reservado usado em teste de recusa
            "224.0.0.1",
            "::1",
            "fc00::1", // vazamento-ok: endereço reservado usado em teste de recusa
            "fe80::1", // vazamento-ok: endereço reservado usado em teste de recusa
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254", // vazamento-ok: endereço reservado usado em teste de recusa
            "2001:db8::1",
            "2002::1",
        ] {
            assert!(!public_ip(value.parse().unwrap()), "{value}");
        }
        for value in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111", "::ffff:8.8.8.8"] {
            assert!(public_ip(value.parse().unwrap()), "{value}");
        }
    }

    #[test]
    fn recusa_dns_misto_vazio_e_porta_nao_permitida() {
        let public: SocketAddr = "8.8.8.8:443".parse().unwrap();
        let private: SocketAddr = "127.0.0.1:443".parse().unwrap();
        assert!(validate_addresses(&[public], 443).is_ok());
        let public_http: SocketAddr = "8.8.8.8:80".parse().unwrap();
        assert!(validate_addresses(&[public_http], 80).is_ok());
        assert!(validate_addresses(&[public, private], 443).is_err());
        assert!(validate_addresses(&[], 443).is_err());
        assert!(validate_addresses(&[public], 8080).is_err());
        assert!(validate_addresses(&[public], 80).is_err());
    }
}
