//! Filtro de saída do login: strings de controle de 7 e de 8 bits, e as listas de sites oficiais.
use super::{official_url, tests::login};

#[test]
fn osc_de_8_bits_c1_009d_e_descartado_inclusive_entre_blocos() {
    let login = login();
    assert_eq!(login.output(b"antes\xc2\x9d52;c;SEGREDO\x07depois").unwrap(), b"antesdepois");
    let login = super::tests::login();
    let mut clean = Vec::new();
    for chunk in [&b"a\xc2"[..], b"\x9d52;c;SEG", b"RETO\xc2", b"\x9cok"] {
        clean.extend(login.output(chunk).unwrap());
    }
    assert_eq!(clean, b"aok");
}

#[test]
fn dcs_apc_pm_sos_em_7_e_8_bits_sao_descartados_e_texto_comum_passa() {
    for (input, expected) in [
        (&b"x\x1bPsegredo\x1b\\y"[..], &b"xy"[..]),
        (b"x\x1b_segredo\x07texto\x1b\\y", b"xy"),
        (b"x\x1b^segredo\x1b\\y", b"xy"),
        (b"x\x1bXsegredo\x07texto\x1b\\y", b"xy"),
        (b"x\x1b]52;c;segredo\x1b\\y", b"xy"),
        (b"x\xc2\x90segredo\xc2\x9cy", b"xy"),
        (b"x\xc2\x9fsegredo\xc2\x9cy", b"xy"),
        (b"x\xc2\x9e\x1b\x1b\\y", b"xy"),
        (b"\x1b[31mvermelho\x1b[0m", b"\x1b[31mvermelho\x1b[0m"),
        ("ação ñ\u{a0}ok é".as_bytes(), "ação ñ\u{a0}ok é".as_bytes()),
    ] {
        let login = login();
        assert_eq!(login.output(input).unwrap(), expected, "{:?}", String::from_utf8_lossy(input));
    }
}

#[test]
fn recomeco_apos_esc_pendente_nao_abre_string_as_claras() {
    // ESC ESC ]: o segundo ESC continua pendente e o ] abre OSC descartado.
    assert_eq!(login().output(b"a\x1b\x1b]52;c;SEGREDO\x07b").unwrap(), b"ab");
    // ESC seguido de C1 em UTF-8: o ESC pendente é descartado e o C2 9D abre OSC descartado.
    assert_eq!(login().output(b"a\x1b\xc2\x9d52;c;SEGREDO\x07b").unwrap(), b"ab");
    // Recomeço dentro da string: ESC ] no meio de um DCS abre OSC novo, descartado até o BEL.
    assert_eq!(login().output(b"a\x1bPabc\x1b]52;c;SEGREDO\x07b").unwrap(), b"ab");
}

#[test]
fn dcs_nao_termina_em_bel_e_blocos_divididos_mantem_o_estado() {
    // BEL no meio do DCS não libera o texto seguinte: só o ST encerra.
    let login = login();
    assert_eq!(login.output(b"a\x1bPsegredo\x07texto\x1b\\b").unwrap(), b"ab");
    // Byte a byte, inclusive o ESC partido do terminador e texto UTF-8 comum no fim.
    let login = super::tests::login();
    let mut clean = Vec::new();
    for chunk in [b"a\x1b"[..].iter(), b"Pseg".iter(), b"redo\x07tex".iter(), b"to\x1b".iter(), b"\\b \xc3\xa9".iter()]
    {
        let bytes: Vec<u8> = chunk.copied().collect();
        clean.extend(login.output(&bytes).unwrap());
    }
    assert_eq!(clean, "ab é".as_bytes());
}

#[test]
fn sites_oficiais_so_dos_motores_com_terminal_remoto() {
    for (backend, url) in [
        ("cursor", "https://cursor.com/loginDeepControl"),
        ("grok", "https://auth.x.ai/device"),
        ("grok", "https://accounts.x.ai/device"),
        ("antigravity", "https://accounts.google.com/o/oauth2/v2/auth"),
    ] {
        assert!(official_url(backend, url), "{backend}: {url}");
    }
    for (backend, url) in [
        ("opencode", "https://opencode.ai/auth"),
        ("opencode", "https://auth.openai.com/oauth"),
        ("kimi", "https://www.kimi.com/login"),
        ("cline", "https://app.cline.bot/auth"),
        ("gemini", "https://accounts.google.com/o/oauth2/v2/auth"),
        ("claude", "https://claude.ai/login"),
        ("codex", "https://auth.openai.com/oauth"),
        ("cursor", "https://accounts.google.com/o/oauth2/v2/auth"),
    ] {
        assert!(!official_url(backend, url), "{backend}: {url}");
    }
}

#[test]
fn esc_ou_c2_pendente_nao_se_junta_ao_texto_depois_de_string_descartada() {
    // Ataques simulados na revisão r2 do #965: nenhum vira OSC 52 nem DCS na saída.
    for input in [
        &b"\x1b\xc2\x9dx\x07]52;c;SEGREDO\x07"[..],
        b"\xc2\xc2\x9dx\x07\x9d52;c;SEGREDO\x07",
        b"\x1b\x1bPx\x1b\\]52;c;SEGREDO\x07",
        b"\xc2\x1b]x\x07P1$r\x1b\\",
        b"\x1b\xc2\x90x\xc2\x9cP1$r\x1b\\",
    ] {
        let saida = login().output(input).unwrap();
        let junto = |a: &[u8]| saida.windows(a.len()).any(|w| w == a);
        assert!(
            !junto(b"\x1b]") && !junto(b"\x1bP") && !junto(b"\xc2\x9d") && !junto(b"\xc2\x90"),
            "{:?} -> {:?}",
            String::from_utf8_lossy(input),
            String::from_utf8_lossy(&saida)
        );
    }
    // Picado em blocos: o estado atravessa a fronteira.
    let picado = login();
    let mut saida = Vec::new();
    for chunk in [&b"a\x1b"[..], b"\xc2", b"\x9dx\x07", b"]52;c;S\x07b"] {
        saida.extend(picado.output(chunk).unwrap());
    }
    assert!(!saida.windows(2).any(|w| w == b"\x1b]"), "{:?}", String::from_utf8_lossy(&saida));
    // Texto UTF-8 comum com C2 (©, °, NBSP) e cores CSI continuam passando.
    assert_eq!(login_texto("© 20° \u{a0}ok"), "© 20° \u{a0}ok");
    assert_eq!(login().output(b"\x1b[1mok\x1b[0m").unwrap(), b"\x1b[1mok\x1b[0m");
}

fn login_texto(texto: &str) -> String {
    String::from_utf8(login().output(texto.as_bytes()).unwrap()).unwrap()
}

/// Todos os pedaços de uma entrada, de uma vez e picada em dois blocos em cada posição.
fn saidas(input: &[u8]) -> Vec<Vec<u8>> {
    let mut todas = vec![login().output(input).unwrap()];
    for corte in 1..input.len() {
        let l = login();
        let mut saida = l.output(&input[..corte]).unwrap();
        saida.extend(l.output(&input[corte..]).unwrap());
        todas.push(saida);
    }
    todas
}

#[test]
fn c1_cru_sem_prefixo_c2_e_tratado_como_o_c1_em_utf8() {
    // 0x90/0x98/0x9D/0x9E/0x9F abrem uma string de controle, descartada até BEL (só o OSC) ou ST.
    for (input, esperado) in [
        (&b"a\x9d52;c;SEGREDO\x07b"[..], &b"ab"[..]),
        (b"a\x9d52;c;SEGREDO\x1b\\b", b"ab"),
        (b"a\x9d52;c;SEGREDO\xc2\x9cb", b"ab"),
        (b"a\x90segredo\x07texto\x1b\\b", b"ab"),
        (b"a\x98segredo\x1b\\b", b"ab"),
        (b"a\x9esegredo\xc2\x9cb", b"ab"),
        (b"a\x9fsegredo\x1b\\b", b"ab"),
        // Os demais C1 caem sozinhos: o texto em volta fica.
        (b"a\x80b\x85c\x8dd\x9be\x9cf", b"abcdef"),
        // C1 cru depois de ESC: o ESC é descartado e a string também (nada de ESC solto na saída).
        (b"a\x1b\x9dx\x07]52;c;SEGREDO\x07b", b"a]52;c;SEGREDO\x07b"),
        (b"a\x1b\x85b", b"ab"),
        // O ataque da r2 do #965 sem prefixo C2, com C1 cru no recomeço.
        (b"\xc2\xc2\x9dx\x07\x9d52;c;SEGREDO\x07", b""),
    ] {
        for saida in saidas(input) {
            assert_eq!(saida, esperado, "{:?}", String::from_utf8_lossy(input));
        }
    }
}

#[test]
fn terminador_cru_restaura_saida_sem_confundir_continuacao_utf8() {
    for input in [
        &b"antes\x9dtexto\x9cdepois"[..],
        b"antes\x90texto\x9cdepois",
        b"antes\x1b]texto\x1b\x9cdepois",
        b"antes\x1bPtexto\xc2\x9cdepois",
        b"antes\x9dtexto\xe2\x80\x9coculto\x9cdepois",
        b"antes\x90texto\xed\x95\x9coculto\x9cdepois",
    ] {
        for saida in saidas(input) {
            assert_eq!(saida, b"antesdepois", "{input:x?}");
        }
    }
}

#[test]
fn utf8_comum_com_continuacao_na_faixa_c1_passa_inteiro_inclusive_picado() {
    // “ ” … (E2 80 9C/9D/A6), emoji (F0 9F 98 80), Ý (C3 9D), 한 (ED 95 9C), © ° NBSP e cores CSI.
    let texto = "“ok” … 😀 Ý 한 © 20° \u{a0} \u{1b}[31mx\u{1b}[0m \u{fffd}";
    for saida in saidas(texto.as_bytes()) {
        assert_eq!(String::from_utf8_lossy(&saida), texto);
    }
    // Sequência quebrada: E0 não aceita 9D depois dele, então o 9D é C1 cru e abre OSC descartado.
    assert_eq!(login().output(b"a\xe0\x9d52;c;S\x07b").unwrap(), b"a\xe0b");
    // Lead interrompido por ASCII: o 9D seguinte já é cru.
    assert_eq!(login().output(b"a\xc3b\x9d52;c;S\x07c").unwrap(), b"a\xc3bc");
}

/// Varredura: toda combinação curta dos bytes que importam, inteira ou picada em dois blocos, nunca deixa
/// na saída um iniciador de string (ESC ] P X ^ _, C2 e C1 cru de string) fora de uma sequência UTF-8 válida.
#[test]
fn varredura_nenhuma_combinacao_deixa_iniciador_de_string_na_saida() {
    const ALFABETO: [u8; 12] = [0x1b, 0xc2, 0x9d, 0x90, 0x9c, 0xe0, 0x07, b']', b'P', b'\\', b'x', 0xa9];
    fn confere(saida: &[u8], entrada: &[u8]) {
        for pedaco in saida.utf8_chunks() {
            assert!(
                !pedaco.invalid().iter().any(|b| (0x80..=0x9f).contains(b)),
                "C1 cru na saída: {entrada:x?} -> {saida:x?}"
            );
            let texto = pedaco.valid();
            assert!(
                !texto.chars().any(|c| matches!(c, '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}'))
                    && !["\u{1b}]", "\u{1b}P", "\u{1b}X", "\u{1b}^", "\u{1b}_"].iter().any(|i| texto.contains(i)),
                "iniciador na saída: {entrada:x?} -> {saida:x?}"
            );
        }
    }
    fn gerar(entrada: &mut Vec<u8>, falta: usize) {
        if !entrada.is_empty() {
            for saida in saidas(entrada) {
                confere(&saida, entrada);
            }
        }
        if falta == 0 {
            return;
        }
        for byte in ALFABETO {
            entrada.push(byte);
            gerar(entrada, falta - 1);
            entrada.pop();
        }
    }
    let mut entrada = Vec::new();
    gerar(&mut entrada, 5);
}
