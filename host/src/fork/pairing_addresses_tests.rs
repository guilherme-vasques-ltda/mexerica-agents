use super::advertised;
use std::net::IpAddr;

// Somente entradas sintéticas; nenhum teste consulta interfaces ou o CLI Tailscale.
fn interface(name: &str, ip: &str) -> (String, IpAddr, bool) {
    let ip: IpAddr = ip.parse().unwrap();
    (name.to_owned(), ip, ip.is_loopback())
}

#[test]
fn dezenas_de_pontes_nao_ocupam_o_limite_e_lan_e_ordenada_antes_do_corte() {
    let mut interfaces = Vec::new();
    for n in 1..=40 {
        interfaces.push(interface(&format!("br-{n:04x}"), &format!("172.18.0.{n}")));
    }
    for n in (1..=20).rev() {
        interfaces.push(interface("eth0", &format!("10.0.0.{n}")));
    }
    interfaces.extend([
        interface("en0", "192.168.1.20"),
        interface("en0", "192.168.1.20"),
        interface("en1", "192.168.1.9"),
        interface("en2", "172.20.0.1"),
        interface("tailscale0", "100.64.0.2"),
        interface("tailscale0", "100.64.0.2"),
    ]);
    let urls = advertised(12345, interfaces, Some("host.example."));
    let mut expected: Vec<String> = [
        "http://100.64.0.2:12345",
        "http://host.example:12345",
        "http://192.168.1.9:12345",
        "http://192.168.1.20:12345",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    expected.extend((1..=12).map(|n| format!("http://10.0.0.{n}:12345")));
    assert_eq!(urls, expected);
    assert_eq!(urls.len(), 16);
}

#[test]
fn todos_os_prefixos_filtram_antes_da_classificacao_sem_bloquear_pontes_genericas() {
    let mut interfaces = Vec::new();
    for name in [
        "docker0",
        "docker_gwbridge",
        "br-abcd",
        "veth123",
        "virbr0",
        "lxcbr0",
        "lxdbr0",
        "cni0",
        "flannel.1",
        "cali123",
        "weave0",
        "podman0",
        "kube0",
    ] {
        interfaces.push(interface(name, "192.168.1.1"));
        interfaces.push(interface(name, "100.64.0.1"));
    }
    let allowed = ["eth0", "enp0s3", "en0", "wlan0", "br0", "bridge0", "tailscale0", "ens3"];
    for (n, name) in allowed.iter().enumerate() {
        interfaces.push(interface(name, &format!("192.168.1.{}", n + 2)));
    }
    let expected: Vec<String> = (2..=9).map(|n| format!("http://192.168.1.{n}:12345")).collect();
    assert_eq!(advertised(12345, interfaces, None), expected);
}

#[test]
fn preserva_faixas_ipv4_dns_e_exclusao_de_loopback_ipv6_e_publico() {
    let mut interfaces = vec![interface("lo", "10.0.0.99")];
    interfaces[0].2 = true;
    for ip in [
        "127.0.0.1",
        "::1",
        "fd00::1",
        "2001:db8::1",
        "192.0.2.1",
        "100.63.255.255",
        "100.128.0.1",
        "172.15.255.255",
        "172.32.0.1",
        "100.64.0.1",
        "100.127.255.254",
        "192.168.1.20",
        "10.0.0.10",
        "10.0.0.9",
        "172.16.0.1",
        "172.31.255.254",
    ] {
        interfaces.push(interface("eth0", ip));
    }
    let expected = [
        "http://100.127.255.254:12345",
        "http://100.64.0.1:12345",
        "http://192.168.1.20:12345",
        "http://10.0.0.9:12345",
        "http://10.0.0.10:12345",
        "http://172.16.0.1:12345",
        "http://172.31.255.254:12345",
    ];
    assert_eq!(advertised(12345, interfaces.clone(), None), expected);
    assert_eq!(advertised(12345, interfaces, Some("...")), expected);
    assert_eq!(advertised(12345, Vec::new(), None), [] as [String; 0]);
    assert_eq!(advertised(12345, Vec::new(), Some("host.example...")), ["http://host.example:12345"]);
}

#[test]
fn teto_global_inclui_dns_e_tailscale_com_dedup_antes_do_corte() {
    let mut interfaces = Vec::new();
    for n in 10..=30 {
        interfaces.push(interface("tailscale0", &format!("100.64.0.{n}")));
        interfaces.push(interface("tailscale0", &format!("100.64.0.{n}")));
    }
    interfaces.push(interface("eth0", "192.168.1.2"));
    let expected: Vec<String> = (10..=25).map(|n| format!("http://100.64.0.{n}:12345")).collect();
    assert_eq!(advertised(12345, interfaces, Some("host.example")), expected);
    // O DNS também participa da deduplicação histórica com os endereços Tailscale.
    assert_eq!(
        advertised(12345, [interface("tailscale0", "100.64.0.1")], Some("100.64.0.1.")),
        ["http://100.64.0.1:12345"]
    );
}
