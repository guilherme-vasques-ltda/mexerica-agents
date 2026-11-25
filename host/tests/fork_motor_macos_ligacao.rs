#[path = "../src/fork/fork-motor-macos/auditoria.rs"]
#[allow(dead_code)]
mod auditoria;
#[path = "../src/fork/fork-motor-macos/cofre.rs"]
#[allow(dead_code)]
mod cofre;
#[path = "../src/fork/fork-motor-macos/identidade.rs"]
#[allow(dead_code)]
mod identidade;
#[path = "../src/fork/fork-motor-macos/ligacao.rs"]
#[allow(dead_code)]
mod ligacao;
#[path = "../src/fork/fork-motor-macos/partida.rs"]
mod partida;
#[path = "../src/fork/fork-motor-macos/porta.rs"]
#[allow(dead_code)]
mod porta;
#[path = "../src/fork/fork-motor-macos/recusas.rs"]
#[allow(dead_code)]
mod recusas;

use std::num::NonZeroU64;

use cofre::{Item, Recusa as RecusaCofre};
use identidade::{CodigoAssinado, Observacao as ObservacaoIdentidade, ParValidado};
use ligacao::{IdentidadeEntrada, PartidaEntrada, UnidadeRc2};
use partida::{Capacidade, Membro, ObservacaoCenso, ObservacaoPerfil, Perfil};
use porta::{Acao, Alvo, Decisao, Escopo, Evidencia, Instancia, Parada, Recusa as RecusaPorta};
use recusas::Registro;
use rusqlite::Connection;

fn id(valor: u64) -> NonZeroU64 {
    NonZeroU64::new(valor).expect("ID da fixture deve ser diferente de zero")
}

fn escopo() -> Escopo {
    Escopo { titular: id(1), dispositivo: id(2), bot: id(3), sessao: id(4), geracao: id(5) }
}

fn acao() -> Acao {
    Acao::Apagar(Alvo::Autorizavel(id(6)))
}

fn codigo() -> CodigoAssinado<'static> {
    CodigoAssinado {
        equipe: "invalid.example.equipe",
        identificador: "invalid.example.motor",
        cdhash_sha256_20: [7; 20],
        entitlements: &[("com.apple.security.app-sandbox", Some(true))],
    }
}

fn manifesto() -> identidade::Artefato<'static> {
    identidade::Artefato { codigo: codigo(), sha256_macho: [9; 32] }
}

fn identidade_valida() -> IdentidadeEntrada<'static> {
    IdentidadeEntrada {
        manifesto: manifesto(),
        geracao: id(5),
        artefato: ObservacaoIdentidade::Arm64Validada(manifesto()),
        par: ObservacaoIdentidade::Arm64Validada(ParValidado { codigo: codigo(), geracao: id(5) }),
    }
}

fn perfil() -> Perfil<'static> {
    Perfil { identificador: "invalid.example.perfil", sha256: [3; 32] }
}

fn membro() -> Membro<'static> {
    Membro { pid: id(41), perfil: Some(perfil()) }
}

fn partida_valida(capacidades: Vec<Capacidade>) -> PartidaEntrada<'static> {
    PartidaEntrada {
        esperado: perfil(),
        perfil: ObservacaoPerfil::Carregado(perfil()),
        censo: ObservacaoCenso::Observado {
            motor: membro(),
            descendentes: vec![Membro { pid: id(42), perfil: Some(perfil()) }],
        },
        capacidades,
    }
}

fn registro() -> (tempfile::TempDir, Registro) {
    let dir = tempfile::tempdir().expect("criar diretório da fixture");
    let registro = Registro::abrir(&dir.path().join("hub.sqlite3")).expect("abrir registro durável");
    (dir, registro)
}

fn vinculo() -> cofre::Vinculo<'static> {
    cofre::Vinculo::compor("invalid.example.mexerica", escopo(), id(7), "invalid.example.inferencia")
        .expect("compor vínculo da fixture")
}

fn unidade(registro: &Registro) -> UnidadeRc2<'_> {
    ligacao::iniciar(
        &identidade_valida(),
        partida_valida(vec![Capacidade::Mediada]),
        escopo(),
        id(9),
        registro,
        vinculo(),
    )
    .expect("abrir unidade rc.2 da fixture")
}

fn linhas(db: &Connection) -> Vec<(i64, String, String, i64, i64)> {
    db.prepare("SELECT serie, motivo, caminho, instancia, instante_ms FROM recusas_motor_macos ORDER BY id")
        .expect("preparar consulta de recusas")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .expect("consultar recusas")
        .collect::<rusqlite::Result<_>>()
        .expect("juntar recusas")
}

fn leitor(dir: &tempfile::TempDir) -> Connection {
    Connection::open(dir.path().join("hub.sqlite3")).expect("abrir leitor da fixture")
}

/// Pino de compilação do empréstimo: a concessão só atravessa o efeito por
/// `&mut` emprestado — uma variante que entregue a partida por valor ao
/// chamador não compila neste pino, e nenhuma fixture guarda a concessão
/// fora da chamada.
fn exige_emprestimo<'u, F>(unidade: &mut UnidadeRc2<'u>, efeito: F) -> Result<(), anyhow::Error>
where
    F: for<'r> FnOnce(&'r mut partida::Partida<'u>),
{
    unidade.partir_para_o_motor(efeito)
}

fn inferencia_padrao() -> cofre::Pedido<'static> {
    cofre::Pedido::Inferencia(cofre::Inferencia {
        escopo: escopo(),
        provedor: id(7),
        item: Item { grupo: "invalid.example.mexerica", identificador: "invalid.example.inferencia" },
    })
}

#[test]
fn sudo_sempre_negado_antes_de_tudo() {
    let (_dir, registro) = registro();
    let erro = ligacao::iniciar(
        &identidade_valida(),
        partida_valida(vec![Capacidade::Mediada, Capacidade::Sudo]),
        escopo(),
        id(9),
        &registro,
        vinculo(),
    )
    .expect_err("sudo sempre negado");
    assert_eq!(erro.to_string(), "motor_macos_indisponivel: sudo sempre negado");
}

#[test]
fn shell_desligado_por_padrao_e_tela_desligada() {
    let (_dir, registro) = registro();
    for (capacidades, motivo) in [
        (vec![Capacidade::Mediada, Capacidade::Shell], "shell desligado por padrão"),
        (vec![Capacidade::Mediada, Capacidade::Tela], "tela desligada"),
    ] {
        let erro =
            ligacao::iniciar(&identidade_valida(), partida_valida(capacidades), escopo(), id(9), &registro, vinculo())
                .expect_err("capacidade proibida");
        assert_eq!(erro.to_string(), format!("motor_macos_indisponivel: {motivo}"));
    }
}

#[test]
fn ausencia_de_capacidade_mediada_nega() {
    let (_dir, registro) = registro();
    let erro = ligacao::iniciar(&identidade_valida(), partida_valida(vec![]), escopo(), id(9), &registro, vinculo())
        .expect_err("sem capacidade mediada");
    assert_eq!(erro.to_string(), "motor_macos_indisponivel: partida exige a capacidade mediada explícita");
}

#[test]
fn perfil_ausente_divergente_e_barreira_recusam_com_codigo_estavel() {
    let (_dir, registro) = registro();
    let casos = [
        (ObservacaoPerfil::FalhaDeCarga, "perfil não carregado"),
        (
            ObservacaoPerfil::Carregado(Perfil { identificador: "outro", sha256: [3; 32] }),
            "perfil carregado divergente do esperado",
        ),
        (ObservacaoPerfil::Carregado(perfil()), "barreira não comprovada"),
    ];
    for (perfil_observado, motivo) in casos {
        let entrada = PartidaEntrada {
            esperado: perfil(),
            perfil: perfil_observado,
            censo: ObservacaoCenso::SemCenso,
            capacidades: vec![Capacidade::Mediada],
        };
        let erro = ligacao::iniciar(&identidade_valida(), entrada, escopo(), id(9), &registro, vinculo())
            .expect_err("partida recusada");
        assert_eq!(erro.to_string(), format!("motor_macos_indisponivel: {motivo}"));
    }
}

#[test]
fn descendente_fora_do_perfil_comprovado_recusa() {
    let (_dir, registro) = registro();
    let entrada = PartidaEntrada {
        esperado: perfil(),
        perfil: ObservacaoPerfil::Carregado(perfil()),
        censo: ObservacaoCenso::Observado { motor: membro(), descendentes: vec![Membro { pid: id(42), perfil: None }] },
        capacidades: vec![Capacidade::Mediada],
    };
    let erro = ligacao::iniciar(&identidade_valida(), entrada, escopo(), id(9), &registro, vinculo())
        .expect_err("descendente fora");
    assert_eq!(erro.to_string(), "motor_macos_indisponivel: descendente fora do perfil comprovado");
}

#[test]
fn identidade_divergente_recusa_com_codigo_estavel() {
    let (_dir, registro) = registro();
    for (entrada, motivo) in [
        (
            &IdentidadeEntrada { artefato: ObservacaoIdentidade::Ausente, ..identidade_valida() },
            "artefato arm64 ausente",
        ),
        (
            &IdentidadeEntrada {
                par: ObservacaoIdentidade::Arm64Validada(ParValidado { codigo: codigo(), geracao: id(50) }),
                ..identidade_valida()
            },
            "geração divergente",
        ),
    ] {
        let erro =
            ligacao::iniciar(entrada, partida_valida(vec![Capacidade::Mediada]), escopo(), id(9), &registro, vinculo())
                .expect_err("identidade recusada");
        assert_eq!(erro.to_string(), format!("motor_macos_indisponivel: {motivo}"));
    }
}

#[test]
fn escopo_incoerente_com_a_admissao_recusa() {
    let (_dir, registro) = registro();
    // Geração do escopo diverge da geração admitida — e o vínculo acompanha
    // o escopo, isolando a divergência da identidade: sem a comparação com
    // a geração admitida, este caso passaria.
    let escopo_outro = Escopo { geracao: id(55), ..escopo() };
    let vinculo_do_escopo_outro = cofre::Vinculo::compor("invalid.example.mexerica", escopo_outro, id(7), "x.invalid")
        .expect("compor vínculo do escopo divergente da fixture");
    let erro = ligacao::iniciar(
        &identidade_valida(),
        partida_valida(vec![Capacidade::Mediada]),
        escopo_outro,
        id(9),
        &registro,
        vinculo_do_escopo_outro,
    )
    .expect_err("escopo incoerente");
    assert_eq!(erro.to_string(), "motor_macos_indisponivel: escopo incoerente com a admissão");
    // Vínculo de outro escopo também não abre unidade.
    let vinculo_outro = cofre::Vinculo::compor("invalid.example.mexerica", escopo_outro, id(7), "x.invalid")
        .expect("compor vínculo divergente da fixture");
    let erro = ligacao::iniciar(
        &identidade_valida(),
        partida_valida(vec![Capacidade::Mediada]),
        escopo(),
        id(9),
        &registro,
        vinculo_outro,
    )
    .expect_err("vínculo incoerente");
    assert_eq!(erro.to_string(), "motor_macos_indisponivel: escopo incoerente com a admissão");
}

#[test]
fn admissao_completa_entrega_partida_uma_unica_vez_e_segue_supervisora() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    // Partida exige aceite do aviso e motor registrado no censo — a
    // sequência real do runtime, sem processo não contabilizado.
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    assert!(unidade.registrar_motor(Instancia { pid: id(41), inicio: id(50) }));
    // O efeito de partida passa pela unidade: checagem e início na mesma
    // operação, a concessão EMPRESTADA e descartada dentro dela — nenhum
    // token escapa.
    let entregue = std::cell::Cell::new(false);
    unidade
        .partir_para_o_motor(|partida| {
            assert_eq!(partida.perfil(), perfil());
            entregue.set(true);
        })
        .expect("partir o motor");
    assert!(entregue.get());
    // Segunda partida não existe: nada parte duas vezes.
    assert_eq!(
        unidade.partir_para_o_motor(|_| panic!("segunda partida")).expect_err("segunda partida").to_string(),
        "motor_macos_indisponivel: partida já executada"
    );
    // A supervisão segue viva: a porta ainda aceita o caminho de pedir.
    let _pedido = unidade.pedir(acao(), 0).expect("abrir pedido após a partida");
}

#[test]
fn partida_recusada_apos_a_parada() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.parar_tudo(10).expect("parar tudo antes da partida");
    // Nada parte depois de "Parar tudo": a recusa é a mesma da porta fechada.
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida após a parada"))
            .expect_err("partida após a parada")
            .to_string(),
        "motor_macos_indisponivel: unidade parada ou porta fechada"
    );
}

#[test]
fn parada_antes_da_partida_mantem_a_unidade_supervisora() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    assert!(unidade.registrar_motor(Instancia { pid: id(41), inicio: id(50) }));
    unidade.partir_para_o_motor(|_| {}).expect("partir o motor");
    // Supervisão após a partida: "Parar tudo" fecha porta e cofre da mesma
    // forma, e o estado da parada continua observável pela unidade.
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let _pedido = unidade.pedir(acao(), 0).expect("abrir pedido da fixture");
    unidade.parar_tudo(10).expect("parar tudo após a partida");
    assert_eq!(unidade.autorizar_inferencia(inferencia_padrao()), Err(RecusaCofre::Fechada));
    // O motor registrado segue contabilizado: sobrevivente visível enquanto
    // não confirma saída, e a parada só confirma com a saída dele.
    let motor = Instancia { pid: id(41), inicio: id(50) };
    assert_eq!(unidade.estado_da_parada(11, Evidencia::Viva(motor)), Parada::Sobrevivente(motor));
    assert_eq!(unidade.estado_da_parada(2_011, Evidencia::UnidadeFechada(escopo())), Parada::NaoConfirmada);
    assert!(unidade.confirmar_saida(motor));
    assert_eq!(unidade.estado_da_parada(2_010, Evidencia::UnidadeFechada(escopo())), Parada::ConfirmadaNoPrazo);
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida após a parada"))
            .expect_err("partida após a parada")
            .to_string(),
        "motor_macos_indisponivel: unidade parada ou porta fechada"
    );
}

#[test]
fn parar_tudo_fecha_o_cofre_antes_de_qualquer_gravacao() {
    let (dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let _pedido = unidade.pedir(acao(), 0).expect("abrir pedido da fixture");
    leitor(&dir).execute("DROP TABLE recusas_motor_macos", []).expect("derrubar tabela da fixture");
    // A gravação da pendência falha — e o fechamento não depende dela: o
    // cofre da geração e a porta estão fechados, e a partida recusada.
    assert_eq!(unidade.parar_tudo(10), Err(auditoria::Falha::Auditoria));
    assert_eq!(unidade.autorizar_inferencia(inferencia_padrao()), Err(RecusaCofre::Fechada));
    assert_eq!(unidade.pedir(acao(), 11), Err(auditoria::Falha::Recusa(porta::Recusa::Fechada)));
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida após a parada"))
            .expect_err("partida após a parada")
            .to_string(),
        "motor_macos_indisponivel: unidade parada ou porta fechada"
    );
}

#[test]
fn parar_tudo_fecha_porta_e_cofre_da_geracao_juntos() {
    let (dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let pedido = unidade.pedir(acao(), 0).expect("abrir pedido da fixture");
    // "Parar tudo" fecha porta e cofre na mesma geração, sem bloquear.
    unidade.parar_tudo(10).expect("parar tudo com pendência");
    assert_eq!(unidade.autorizar_inferencia(inferencia_padrao()), Err(RecusaCofre::Fechada));
    assert_eq!(unidade.pedir(acao(), 11), Err(auditoria::Falha::Recusa(RecusaPorta::Fechada)));
    assert_eq!(
        unidade.resolver(pedido, escopo(), Decisao::UmaVez, 12),
        Err(auditoria::Falha::Recusa(RecusaPorta::Fechada))
    );
    // A pendência cancelada fica registrada como `fechada` no caminho de parar.
    assert_eq!(linhas(&leitor(&dir)), vec![(1, "fechada".to_owned(), "parar".to_owned(), 9, 10)]);
}

#[test]
fn parada_confirmada_exige_evidencia_no_prazo_e_sobrevivente_e_visivel() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let lider = Instancia { pid: id(41), inicio: id(50) };
    let filho = Instancia { pid: id(42), inicio: id(51) };
    assert!(unidade.registrar(lider));
    assert!(unidade.registrar(filho));
    unidade.parar_tudo(10).expect("parar tudo da fixture");
    // Sobrevivente visível com identidade, dentro ou fora do prazo.
    assert!(unidade.confirmar_saida(lider));
    assert_eq!(unidade.estado_da_parada(1_000, Evidencia::Viva(filho)), Parada::Sobrevivente(filho));
    assert_eq!(unidade.estado_da_parada(2_009, Evidencia::SemCenso), Parada::EmCurso);
    // Confirmada no prazo só com unidade fechada e saídas confirmadas.
    assert!(unidade.confirmar_saida(filho));
    assert_eq!(unidade.estado_da_parada(2_010, Evidencia::UnidadeFechada(escopo())), Parada::ConfirmadaNoPrazo);
    // Sem evidência após o prazo é incerteza, nunca sucesso falso.
    unidade.parar_tudo(100_000).expect("parar tudo de novo da fixture");
    assert_eq!(unidade.estado_da_parada(102_011, Evidencia::SemCenso), Parada::NaoConfirmada);
}

#[test]
fn pedido_de_chaveiro_de_outro_app_e_negado_pela_unidade() {
    let (_dir, registro) = registro();
    let unidade = unidade(&registro);
    let pedido_alheio = cofre::Pedido::Inferencia(cofre::Inferencia {
        escopo: escopo(),
        provedor: id(7),
        item: Item { grupo: "invalid.example.outro", identificador: "invalid.example.segredo-alheio" },
    });
    assert_eq!(unidade.autorizar_inferencia(pedido_alheio), Err(RecusaCofre::OutroApp));
}

#[test]
fn sempre_permitir_nunca_existira_para_shell_pela_ligacao() {
    assert_eq!(ligacao::regra_permanente(Capacidade::Shell), Err(cofre::RecusaRegra::SempreShell));
}

#[test]
fn autorizacao_da_unidade_so_e_redimida_pelo_cofre_aberto() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    let autorizacao = unidade.autorizar_inferencia(inferencia_padrao()).expect("autorizar pela unidade");
    assert_eq!(unidade.redimir(autorizacao).expect("redimir pela unidade"), id(5));
    // Emitida antes da parada: morre com o cofre, redimida por valor.
    let autorizacao = unidade.autorizar_inferencia(inferencia_padrao()).expect("autorizar de novo");
    unidade.parar_tudo(10).expect("parar tudo da fixture");
    assert_eq!(unidade.redimir(autorizacao), Err(RecusaCofre::Fechada));
}

#[test]
fn recusa_da_ligacao_e_estavel_em_todos_os_caminhos() {
    let (_dir, registro) = registro();
    // Toda recusa da admissão carrega o mesmo código antes de Motor/MCP.
    let erros = [
        ligacao::iniciar(
            &identidade_valida(),
            partida_valida(vec![Capacidade::Mediada, Capacidade::Sudo]),
            escopo(),
            id(9),
            &registro,
            vinculo(),
        )
        .expect_err("sudo"),
        ligacao::iniciar(
            &IdentidadeEntrada { artefato: ObservacaoIdentidade::Ausente, ..identidade_valida() },
            partida_valida(vec![Capacidade::Mediada]),
            escopo(),
            id(9),
            &registro,
            vinculo(),
        )
        .expect_err("identidade"),
        ligacao::iniciar(
            &identidade_valida(),
            partida_valida(vec![Capacidade::Mediada]),
            Escopo { geracao: id(55), ..escopo() },
            id(9),
            &registro,
            vinculo(),
        )
        .expect_err("incoerência"),
    ];
    for erro in &erros {
        assert!(erro.to_string().starts_with("motor_macos_indisponivel: "), "recusa sem código estável: {erro}");
    }
    let motivos: Vec<String> =
        erros.iter().map(|e| e.to_string().split_once(": ").expect("motivo após o código").1.to_owned()).collect();
    assert_eq!(motivos, vec!["sudo sempre negado", "artefato arm64 ausente", "escopo incoerente com a admissão"]);
}

#[test]
fn falha_de_auditoria_em_pedir_fecha_porta_cofre_e_barr_a_partida() {
    let (dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    assert!(unidade.registrar(Instancia { pid: id(41), inicio: id(50) }));
    let _pedido = unidade.pedir(acao(), 0).expect("abrir pedido da fixture");
    leitor(&dir).execute("DROP TABLE recusas_motor_macos", []).expect("derrubar tabela da fixture");
    // A expiração no caminho de pedir falha ao gravar: a porta fecha pela
    // auditoria — e o fechamento unificado fecha o cofre e barra a partida.
    assert_eq!(unidade.pedir(acao(), 300_000), Err(auditoria::Falha::Recusa(porta::Recusa::Auditoria)));
    assert_eq!(unidade.autorizar_inferencia(inferencia_padrao()), Err(RecusaCofre::Fechada));
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida com porta fechada por auditoria"))
            .expect_err("partida com porta fechada por auditoria")
            .to_string(),
        "motor_macos_indisponivel: unidade parada ou porta fechada"
    );
}

#[test]
fn partida_sem_aceite_ou_com_descendente_sozinho_no_censo_e_recusada() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    // Sem aceite do aviso, a partida não sai.
    assert_eq!(
        unidade.partir_para_o_motor(|_| panic!("partida sem aceite")).expect_err("partida sem aceite").to_string(),
        "motor_macos_indisponivel: partida exige o aceite do aviso"
    );
    // Com aceite mas com apenas um descendente no censo, a partida não
    // sai: registrar somente um descendente não basta.
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    assert!(unidade.registrar(Instancia { pid: id(42), inicio: id(51) }));
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida com descendente sozinho no censo"))
            .expect_err("partida com descendente sozinho no censo")
            .to_string(),
        "motor_macos_indisponivel: partida exige o motor registrado no censo"
    );
    // Registrado o motor, a sequência real executa a partida.
    assert!(unidade.registrar_motor(Instancia { pid: id(41), inicio: id(50) }));
    assert!(unidade.partir_para_o_motor(|_| {}).is_ok());
}

#[test]
fn concessao_da_partida_nao_escapa_do_efeito() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    assert!(unidade.registrar_motor(Instancia { pid: id(41), inicio: id(50) }));
    // O efeito só vê a concessão por empréstimo, dentro da chamada: lê o
    // perfil admitido e nada sai dela — reter a partida fora não compila
    // (pino `exige_emprestimo`).
    let emprestimo_visto = std::cell::Cell::new(false);
    exige_emprestimo(&mut unidade, |partida| {
        assert_eq!(partida.perfil(), perfil());
        emprestimo_visto.set(true);
    })
    .expect("partir o motor por empréstimo");
    assert!(emprestimo_visto.get());
    // A concessão foi descartada dentro da operação: nada parte duas vezes
    // e "Parar tudo" segue valendo depois dela.
    assert_eq!(
        unidade.partir_para_o_motor(|_| panic!("segunda partida")).expect_err("segunda partida").to_string(),
        "motor_macos_indisponivel: partida já executada"
    );
    unidade.parar_tudo(10).expect("parar tudo após a partida");
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida após a parada"))
            .expect_err("partida após a parada")
            .to_string(),
        "motor_macos_indisponivel: unidade parada ou porta fechada"
    );
}

#[test]
fn saida_confirmada_do_motor_e_terminal_para_a_marca_do_censo() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    let motor = Instancia { pid: id(41), inicio: id(50) };
    assert!(unidade.registrar_motor(motor));
    unidade.partir_para_o_motor(|_| {}).expect("partir o motor");
    // A saída confirmada do motor retira a marca que abre a partida: a
    // recusa seguinte é do censo, antes do descarte único da concessão.
    assert!(unidade.confirmar_saida(motor));
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida com motor fora do censo"))
            .expect_err("partida com motor fora do censo")
            .to_string(),
        "motor_macos_indisponivel: partida exige o motor registrado no censo"
    );
    // Saída confirmada é TERMINAL: nenhuma remarcação re-cria a marca —
    // nem a própria identidade, nem PID reutilizado.
    assert!(!unidade.registrar_motor(Instancia { pid: id(43), inicio: id(52) }));
    assert!(!unidade.registrar_motor(Instancia { pid: id(41), inicio: id(51) }));
    assert!(!unidade.registrar_motor(motor));
    // Sem a marca, a partida segue recusada pelo censo: motor declarado
    // morto não parte de novo.
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida com motor declarado morto"))
            .expect_err("partida com motor declarado morto")
            .to_string(),
        "motor_macos_indisponivel: partida exige o motor registrado no censo"
    );
}

#[test]
fn descendente_da_mesma_instancia_nao_e_promovido_a_motor() {
    let (_dir, registro) = registro();
    // O caminho real é marcar o motor direto, sem descendente prévio com
    // a mesma instância.
    let mut unidade_limpa = unidade(&registro);
    unidade_limpa.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    assert!(unidade_limpa.registrar_motor(Instancia { pid: id(41), inicio: id(50) }));
    assert!(unidade_limpa.partir_para_o_motor(|_| {}).is_ok());

    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    // A instância admitida registrada como descendente não vira motor:
    // promoção de descendente não existe nesta API.
    assert!(unidade.registrar(Instancia { pid: id(41), inicio: id(50) }));
    assert!(!unidade.registrar_motor(Instancia { pid: id(41), inicio: id(50) }));
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida com descendente promovido"))
            .expect_err("partida com descendente promovido")
            .to_string(),
        "motor_macos_indisponivel: partida exige o motor registrado no censo"
    );
}

#[test]
fn marca_do_motor_e_fixada_a_identidade_admitida() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    // PID divergente do motor admitido no censo da admissão: não marca —
    // e descendente registrado antes não é promovido a motor.
    assert!(unidade.registrar(Instancia { pid: id(42), inicio: id(51) }));
    assert!(!unidade.registrar_motor(Instancia { pid: id(42), inicio: id(51) }));
    assert!(!unidade.registrar_motor(Instancia { pid: id(99), inicio: id(60) }));
    assert_eq!(
        unidade
            .partir_para_o_motor(|_| panic!("partida com marca divergente"))
            .expect_err("partida com marca divergente")
            .to_string(),
        "motor_macos_indisponivel: partida exige o motor registrado no censo"
    );
    // O PID admitido marca e abre a partida.
    assert!(unidade.registrar_motor(Instancia { pid: id(41), inicio: id(50) }));
    assert!(unidade.partir_para_o_motor(|_| {}).is_ok());
}

#[test]
fn segunda_marca_divergente_nao_substitui_a_marca_do_censo() {
    let (_dir, registro) = registro();
    let mut unidade = unidade(&registro);
    unidade.aceitar_aviso(escopo()).expect("aceitar aviso da fixture");
    assert!(unidade.registrar_motor(Instancia { pid: id(41), inicio: id(50) }));
    // Marca existente não é substituída: mesmo PID com início divergente,
    // ou instância diferente, não valem como segunda marca.
    assert!(!unidade.registrar_motor(Instancia { pid: id(41), inicio: id(51) }));
    assert!(!unidade.registrar_motor(Instancia { pid: id(43), inicio: id(52) }));
    // A marca original segue valendo: a partida abre com ela.
    assert!(unidade.partir_para_o_motor(|_| {}).is_ok());
}

#[test]
fn ids_fora_do_intervalo_do_registro_recusam_na_admissao() {
    let (_dir, registro) = registro();
    // Instância 2^63 não cabe em i64: o registro de recusas não a grava, e
    // a recusa acontece na admissão, antes de a unidade existir — nunca no
    // efeito aprovado.
    let instancia_grande = NonZeroU64::new(1u64 << 63).expect("instância não-zero da fixture");
    let erro = ligacao::iniciar(
        &identidade_valida(),
        partida_valida(vec![Capacidade::Mediada]),
        escopo(),
        instancia_grande,
        &registro,
        vinculo(),
    )
    .expect_err("instância fora do intervalo");
    assert_eq!(erro.to_string(), "motor_macos_indisponivel: ids fora do intervalo do registro");
    // O mesmo vale para qualquer campo do escopo.
    let escopo_grande = Escopo { titular: instancia_grande, ..escopo() };
    let vinculo_grande = cofre::Vinculo::compor("invalid.example.mexerica", escopo_grande, id(7), "x.invalid")
        .expect("compor vínculo da fixture");
    let erro2 = ligacao::iniciar(
        &identidade_valida(),
        partida_valida(vec![Capacidade::Mediada]),
        escopo_grande,
        id(9),
        &registro,
        vinculo_grande,
    )
    .expect_err("escopo fora do intervalo");
    assert_eq!(erro2.to_string(), "motor_macos_indisponivel: ids fora do intervalo do registro");
    // No limite representável (i64::MAX), a admissão continua aceita.
    let limite = NonZeroU64::new(i64::MAX as u64).expect("limite não-zero da fixture");
    let escopo_limite = Escopo { titular: limite, dispositivo: limite, bot: limite, sessao: limite, geracao: limite };
    let mut identidade_limite = identidade_valida();
    identidade_limite.geracao = limite;
    let par_limite = ParValidado { codigo: codigo(), geracao: limite };
    identidade_limite.par = ObservacaoIdentidade::Arm64Validada(par_limite);
    let vinculo_limite = cofre::Vinculo::compor("invalid.example.mexerica", escopo_limite, id(7), "x.invalid")
        .expect("compor vínculo do limite da fixture");
    ligacao::iniciar(
        &identidade_limite,
        partida_valida(vec![Capacidade::Mediada]),
        escopo_limite,
        limite,
        &registro,
        vinculo_limite,
    )
    .expect("ids no limite do i64 são representáveis e aceitos");
}
