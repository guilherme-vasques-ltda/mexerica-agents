//! Endereços anunciados no pareamento e no hello; não configura a rede.
//! Adaptado de Codync 57537a1e5d52f27c595af7415cc4f69a79dff080 e bb242a7df4a9479fac622657a70f4007cb690fe3 (Apache-2.0); ver THIRD_PARTY.md.

use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;

const MAX_ADDRESSES: usize = 16;

pub(crate) fn addresses(port: u16) -> Vec<String> {
    let interfaces = if_addrs::get_if_addrs().unwrap_or_default();
    let dns = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .ok()
        .and_then(|out| serde_json::from_slice::<Value>(&out.stdout).ok())
        .and_then(|v| v["Self"]["DNSName"].as_str().map(str::to_owned));
    advertised(
        port,
        interfaces.into_iter().map(|iface| {
            let ip = iface.ip();
            let loopback = iface.is_loopback();
            (iface.name, ip, loopback)
        }),
        dns.as_deref(),
    )
}

/// Composição pura compartilhada pela produção e pelas entradas sintéticas dos testes.
fn advertised(
    port: u16,
    interfaces: impl IntoIterator<Item = (String, IpAddr, bool)>,
    dns: Option<&str>,
) -> Vec<String> {
    let mut tailscale = vec![];
    let mut lan = vec![];
    for (name, ip, loopback) in interfaces {
        if loopback || is_container_bridge(&name) {
            continue;
        }
        if let IpAddr::V4(ip) = ip {
            let o = ip.octets();
            // Conserva a classificação CGNAT do fork; não prova a origem ou alcançabilidade.
            if o[0] == 100 && (64..128).contains(&o[1]) {
                tailscale.push(format!("http://{ip}:{port}"));
            } else if ip.is_private() {
                lan.push(ip);
            }
        }
    }
    if let Some(dns) = dns {
        let dns = dns.trim_end_matches('.');
        if !dns.is_empty() {
            tailscale.push(format!("http://{dns}:{port}"));
        }
    }
    // Mantém a ordem textual histórica dentro de Tailscale, incluindo o DNS.
    tailscale.sort();
    tailscale.dedup();
    lan.sort_by_key(|ip| (lan_rank(*ip), *ip));
    lan.dedup();
    tailscale.into_iter().chain(lan.into_iter().map(|ip| format!("http://{ip}:{port}"))).take(MAX_ADDRESSES).collect()
}

/// Prioridade heurística antes do teto; não seleciona rota nem atesta confiança.
fn lan_rank(ip: Ipv4Addr) -> u8 {
    match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        _ => 2,
    }
}

/// Prefixos conhecidos de containers/VMs; pontes genéricas continuam elegíveis.
fn is_container_bridge(name: &str) -> bool {
    const PREFIXES: [&str; 12] =
        ["docker", "br-", "veth", "virbr", "lxcbr", "lxdbr", "cni", "flannel", "cali", "weave", "podman", "kube"];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

#[cfg(test)]
#[path = "pairing_addresses_tests.rs"]
mod tests;
