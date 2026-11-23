//! Guard root separado: observa o journal e para a unidade de usuário no prazo.
#[cfg(target_os = "linux")]
#[path = "../atualizacao/mod.rs"]
mod atualizacao;
#[cfg(target_os = "linux")]
#[path = "../guarda_atualizacao/monitor.rs"]
mod monitor;
#[cfg(target_os = "linux")]
#[path = "../guarda_atualizacao/servico.rs"]
mod servico;

#[cfg(target_os = "linux")]
fn avaliar(
    estado: &atualizacao::armazenamento::Diretorio,
    publicacao: &atualizacao::armazenamento::Diretorio,
) -> anyhow::Result<monitor::Situacao> {
    use atualizacao::{
        estado::Fase,
        journal::{Etapa, Evento, Journal, Motivo, Operacao, Origem, Resultado},
    };
    use monitor::Situacao;
    let Some(trava) = estado.tentar_travar()? else {
        // O journal é gravado atomicamente por root. Durante a reversão, o guard
        // também preserva a partida enquanto o wrapper ainda detém a trava.
        if let Some(prazo) = reversao_na_janela(estado, publicacao) {
            return Ok(Situacao::Pendente(prazo));
        }
        return Ok(Situacao::Ocupado);
    };
    let agora = estado.hora_conferida(&trava);
    // Antes da primeira transação não há par gerido: o serviço legado segue livre.
    if !estado.existe("journal")? && publicacao.seletor()?.is_none() {
        publicacao.publicar_status(estado, &trava)?;
        return Ok(Situacao::Liberado);
    }
    let Ok(mut journal) = estado.ler::<Journal>("journal") else {
        estado.bloquear_por_falha(&trava)?;
        publicacao.publicar_status(estado, &trava)?;
        return Ok(Situacao::Bloqueado);
    };
    if matches!(journal.estado.fase, Fase::Expirada) {
        estado.expirar_journal(publicacao, &trava, &mut journal, 0)?;
        publicacao.publicar_status(estado, &trava)?;
        return Ok(Situacao::Bloqueado);
    }
    if let Ok(agora) = agora.as_ref()
        && matches!(journal.estado.fase, Fase::Pendente { prazo, .. } if *agora >= prazo)
    {
        let evento = |resultado| Evento {
            horario_unix: *agora,
            uid: 0,
            origem: Origem::GuardaRoot,
            operacao: Operacao::Recuperar,
            sequencia_atual: Some(journal.estado.marca),
            sequencia_anterior: journal.sequencia_anterior,
            resultado,
            motivo: Motivo::Expiracao,
        };
        let (inicio, mut fim) = (evento(Resultado::Inicio), evento(Resultado::Sucesso));
        let inicio = estado.inicio(&inicio);
        let desarme = estado.expirar_journal(publicacao, &trava, &mut journal, *agora);
        let status = publicacao.publicar_status(estado, &trava);
        if desarme.is_err() || status.is_err() {
            fim.resultado = Resultado::Falha;
        }
        let fim = estado.auditar(&fim);
        desarme?;
        status?;
        inicio?;
        fim?;
        return Ok(Situacao::Bloqueado);
    }
    if estado.exigir_sem_bloqueio().is_err() || publicacao.exigir_sem_emergencia().is_err() {
        publicacao.publicar_status(estado, &trava)?;
        return Ok(Situacao::Bloqueado);
    }
    let Ok(agora) = agora else {
        estado.bloquear_por_falha(&trava)?;
        publicacao.publicar_status(estado, &trava)?;
        return Ok(Situacao::Bloqueado);
    };
    let situacao = match (&journal.etapa, &journal.estado.fase, journal.publicado_em) {
        (Etapa::Pendente, Fase::Pendente { prazo, .. }, Some(inicio)) if inicio <= agora && agora < *prazo => {
            Situacao::Pendente(*prazo)
        }
        (Etapa::ReversaoIniciada | Etapa::SeletorRestaurado, Fase::Pendente { prazo, .. }, Some(inicio))
            if inicio <= agora && agora < *prazo =>
        {
            let valido = atualizacao::recuperacao::validar(estado, publicacao, &trava, agora)
                .is_ok_and(|plano| matches!(plano.acao, atualizacao::journal::Recuperacao::TerminarReversao));
            if valido {
                Situacao::Pendente(*prazo)
            } else {
                estado.bloquear_por_falha(&trava)?;
                Situacao::Bloqueado
            }
        }
        (Etapa::Concluida, Fase::Aceita | Fase::Revertida, _)
        | (Etapa::Cancelada, Fase::Livre | Fase::Aceita | Fase::Revertida, _) => Situacao::Liberado,
        // Um crash entre fases deixa o serviço parado e status bloqueado,
        // mas não cria um bloqueio irreversível quando o journal e seus
        // documentos permitem recuperação autenticada pelo wrapper.
        (
            Etapa::Preparada
            | Etapa::SlotPreparado
            | Etapa::DesativacaoIniciada
            | Etapa::AnteriorDesativado
            | Etapa::SeletorTrocado
            | Etapa::CancelamentoIniciado,
            _,
            _,
        ) => {
            use atualizacao::journal::Recuperacao;
            let recuperavel = atualizacao::recuperacao::validar(estado, publicacao, &trava, agora).is_ok_and(|plano| {
                matches!(
                    plano.acao,
                    Recuperacao::CancelarPreparacao
                        | Recuperacao::RestabelecerAnterior
                        | Recuperacao::ResolverPublicacao
                        | Recuperacao::TerminarReversao
                )
            });
            if !recuperavel && estado.exigir_sem_bloqueio().is_ok() {
                estado.bloquear_por_falha(&trava)?;
            }
            Situacao::Bloqueado
        }
        _ => {
            estado.bloquear_por_falha(&trava)?;
            Situacao::Bloqueado
        }
    };
    publicacao.publicar_status(estado, &trava)?;
    Ok(situacao)
}

#[cfg(target_os = "linux")]
fn parar_auditado(estado: &atualizacao::armazenamento::Diretorio, uid: u32) -> anyhow::Result<()> {
    use atualizacao::journal::{Evento, Motivo, Operacao, Origem, Resultado};
    use std::time::{SystemTime, UNIX_EPOCH};
    let agora = SystemTime::now().duration_since(UNIX_EPOCH).ok().and_then(|v| i64::try_from(v.as_secs()).ok());
    let evento = |horario_unix, resultado| Evento {
        horario_unix,
        uid: 0,
        origem: Origem::GuardaRoot,
        operacao: Operacao::Recuperar,
        sequencia_atual: None,
        sequencia_anterior: None,
        resultado,
        motivo: Motivo::ParadaSolicitada,
    };
    let inicio = agora
        .ok_or_else(|| anyhow::anyhow!("relógio indisponível"))
        .and_then(|hora| estado.inicio(&evento(hora, Resultado::Inicio)));
    // Parar também quando a auditoria falhar; nunca deixar serviço ativo por isso.
    let parada = servico::solicitar_parada(uid);
    let fim = match agora {
        Some(hora) => estado.auditar(&evento(hora, if parada.is_ok() { Resultado::Sucesso } else { Resultado::Falha })),
        None => Err(anyhow::anyhow!("relógio indisponível")),
    };
    inicio?;
    parada?;
    fim
}

/// Reversão dentro da janela, lida do journal root sem a trava: a mesma condição
/// em que o guard preserva a partida (regra: o guard não para durante a reversão).
#[cfg(target_os = "linux")]
fn reversao_na_janela(
    estado: &atualizacao::armazenamento::Diretorio,
    publicacao: &atualizacao::armazenamento::Diretorio,
) -> Option<i64> {
    use atualizacao::{
        estado::Fase,
        journal::{Etapa, Journal},
    };
    if estado.exigir_sem_bloqueio().is_err() || publicacao.exigir_sem_emergencia().is_err() {
        return None;
    }
    let journal = estado.ler::<Journal>("journal").ok()?;
    let (Etapa::ReversaoIniciada | Etapa::SeletorRestaurado, Fase::Pendente { prazo, .. }, Some(inicio)) =
        (journal.etapa, journal.estado.fase, journal.publicado_em)
    else {
        return None;
    };
    let agora =
        i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs()).ok()?;
    (inicio <= agora && agora < prazo).then_some(prazo)
}

/// O que está em andamento quando o guard cai. Só sinais root contam: a trava do
/// estado (arquivo root 0600, que só wrapper e etapa administrativa obtêm) e o
/// journal root. Nada do lado do usuário do Hub (coordenador, trava do Hub) entra.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Andamento {
    /// Nada privilegiado em curso: a queda do guard é tratada como falha.
    Nenhum,
    /// Operação root com a trava do estado (publicação, reversão no início, aceite).
    Transacao,
    /// Reversão dentro da janela, ativa ou aguardando a retomada autenticada.
    Reversao,
}

#[cfg(target_os = "linux")]
fn andamento(
    estado: &atualizacao::armazenamento::Diretorio,
    publicacao: &atualizacao::armazenamento::Diretorio,
) -> Andamento {
    use atualizacao::journal::Recuperacao;
    match estado.tentar_travar() {
        Ok(None) if reversao_na_janela(estado, publicacao).is_some() => Andamento::Reversao,
        Ok(None) => Andamento::Transacao,
        // Trava livre: só uma reversão interrompida e válida pelo journal preserva a
        // partida, como em `avaliar` (o Hub anterior precisa iniciar para retomá-la).
        Ok(Some(trava)) => {
            let valida = reversao_na_janela(estado, publicacao).is_some()
                && estado.hora_conferida(&trava).is_ok_and(|agora| {
                    atualizacao::recuperacao::validar(estado, publicacao, &trava, agora)
                        .is_ok_and(|plano| matches!(plano.acao, Recuperacao::TerminarReversao))
                });
            if valida { Andamento::Reversao } else { Andamento::Nenhum }
        }
        Err(_) => Andamento::Nenhum,
    }
}

/// Contenção quando o guard para ou falha.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Contencao {
    /// Reversão em andamento: nem parada nem bloqueio; o guard reiniciado retoma o prazo.
    Preservar,
    /// Desligamento esperado ou transação root em curso: só para o serviço do Hub.
    Parar,
    /// Fora desses casos: emergência durável, parada e bloqueio (fail-closed).
    Bloquear,
}

#[cfg(target_os = "linux")]
fn contencao(encerrando: bool, andamento: Andamento) -> Contencao {
    match (encerrando, andamento) {
        (_, Andamento::Reversao) => Contencao::Preservar,
        (true, _) | (false, Andamento::Transacao) => Contencao::Parar,
        (false, Andamento::Nenhum) => Contencao::Bloquear,
    }
}

/// Aplica a contenção. Sem a classificação (estado ilegível), bloqueia.
#[cfg(target_os = "linux")]
fn conter(
    estado: Option<&atualizacao::armazenamento::Diretorio>,
    publicacao: &atualizacao::armazenamento::Diretorio,
    encerrando: bool,
    parar: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<Contencao> {
    let em_curso = estado.map_or(Andamento::Nenhum, |estado| andamento(estado, publicacao));
    aplicar(contencao(encerrando, em_curso), em_curso, estado, publicacao, parar)
}

#[cfg(target_os = "linux")]
fn aplicar(
    acao: Contencao,
    em_curso: Andamento,
    estado: Option<&atualizacao::armazenamento::Diretorio>,
    publicacao: &atualizacao::armazenamento::Diretorio,
    parar: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<Contencao> {
    if acao != Contencao::Bloquear {
        registrar(libc::LOG_WARNING, &format!("contenção sem emergência: {acao:?} ({em_curso:?})"));
        return if acao == Contencao::Parar { parar().map(|()| acao) } else { Ok(acao) };
    }
    // Bloquear só sob a trava do estado: um wrapper que a obteve depois da leitura
    // do journal é transação root em curso, e a emergência durável seria falsa.
    let trava = match estado.map(atualizacao::armazenamento::Diretorio::tentar_travar) {
        Some(Ok(None)) => {
            registrar(libc::LOG_WARNING, "contenção sem emergência: trava do estado obtida por operação root");
            return parar().map(|()| Contencao::Parar);
        }
        Some(Ok(Some(trava))) => Some(trava),
        // Erro na trava ou estado ilegível: sem evidência de transação, fail-closed.
        Some(Err(_)) | None => None,
    };
    let emergencia = publicacao.bloquear_emergencia();
    if let (Some(estado), Some(trava)) = (estado, &trava) {
        let _ = estado.bloquear_por_falha(trava);
        let _ = publicacao.publicar_status(estado, trava);
    }
    // A parada não segura a trava: o systemctl pode levar até 10 s.
    drop(trava);
    let parada = parar();
    emergencia?;
    parada.map(|()| Contencao::Bloquear)
}

/// Registra no journal do sistema; a normalização descarta stdout/stderr.
#[cfg(target_os = "linux")]
fn registrar(prioridade: libc::c_int, mensagem: &str) {
    if cfg!(test) {
        return;
    }
    // Mesmo teto do diagnóstico persistido.
    let curta: String = mensagem.chars().take(4000).map(|c| if c == '\0' { ' ' } else { c }).collect();
    let Ok(texto) = std::ffi::CString::new(curta) else { return };
    // SAFETY: identificador estático e formato fixo "%s" com string terminada em NUL.
    unsafe {
        libc::openlog(c"donnabot-guarda-atualizacao".as_ptr(), libc::LOG_PID, libc::LOG_DAEMON);
        libc::syslog(prioridade, c"%s".as_ptr(), texto.as_ptr());
    }
}

#[cfg(target_os = "linux")]
fn executar() -> anyhow::Result<()> {
    use atualizacao::{armazenamento::Diretorio, politica::Politica};
    use std::path::Path;
    // SAFETY: consulta de UID sem efeitos colaterais.
    anyhow::ensure!(unsafe { libc::geteuid() } == 0, "guard exige root");
    let argumentos: Vec<_> = std::env::args_os().skip(1).take(2).collect();
    let bloquear = match argumentos.as_slice() {
        [] => false,
        [arg] if arg == "--bloquear" => true,
        _ => anyhow::bail!("argumentos do guard inválidos"),
    };
    atualizacao::processo::normalizar()?;
    // SAFETY: libera SIGTERM para o systemd executar ExecStopPost no encerramento.
    unsafe {
        let mut sinais: libc::sigset_t = std::mem::zeroed();
        anyhow::ensure!(
            libc::sigemptyset(&raw mut sinais) == 0
                && libc::sigaddset(&raw mut sinais, libc::SIGTERM) == 0
                && libc::sigprocmask(libc::SIG_UNBLOCK, &raw const sinais, std::ptr::null_mut()) == 0,
            "não foi possível ajustar SIGTERM"
        );
    }
    let publicacao = Diretorio::abrir_publicacao()?;
    let estado = Diretorio::abrir(Path::new("/var/lib/donnabot/atualizacao"));
    if bloquear {
        // ExecStopPost: desligamento esperado, transação root ou reversão em
        // andamento evitam a emergência; fora disso, fail-closed como antes.
        let encerrando = servico::sistema_encerrando();
        let disponivel = estado.as_ref().ok();
        let acao = conter(disponivel, &publicacao, encerrando, || {
            let estado = disponivel.ok_or_else(|| anyhow::anyhow!("estado indisponível para parar o serviço"))?;
            parar_auditado(estado, Politica::ler(estado)?.uid_hub)
        });
        estado?;
        return acao.map(drop);
    }
    let estado = estado?;
    let politica = Politica::ler(&estado)?;
    let resultado = monitor::vigiar(|| avaliar(&estado, &publicacao), || parar_auditado(&estado, politica.uid_hub));
    // Falha do monitor (inclusive ao criá-lo): contenção conforme o que está em curso.
    if let Err(erro) = &resultado {
        registrar(libc::LOG_ERR, &format!("monitor do guard falhou: {erro:#}"));
        let _ = conter(Some(&estado), &publicacao, false, || parar_auditado(&estado, politica.uid_hub));
    }
    resultado
}

fn main() {
    #[cfg(target_os = "linux")]
    match executar() {
        Ok(()) => return,
        // O erro real vai ao journal; antes ele era descartado com a saída 1.
        Err(erro) => registrar(libc::LOG_ERR, &format!("guard encerrado com falha: {erro:#}")),
    }
    std::process::exit(1);
}

#[cfg(all(test, target_os = "linux"))]
mod tests_recuperacao {
    use super::*;
    use atualizacao::Hex as _;
    use atualizacao::{
        armazenamento::Diretorio,
        estado::{Estado, Fase},
        journal::{Etapa, Journal},
        slots::Slot,
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    use ed25519_dalek::{Signer as _, SigningKey};
    use sha2::{Digest as _, Sha256};
    use std::{
        fs::{self, OpenOptions},
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn fase_interrompida_integra_para_sem_bloqueio_irreversivel() -> anyhow::Result<()> {
        let raiz = std::env::temp_dir().join(format!("mx365-guard-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&raiz)?;
        for nome in ["estado", "publicacao"] {
            fs::DirBuilder::new().mode(0o700).create(raiz.join(nome))?;
        }
        let estado = Diretorio::abrir_teste(&raiz.join("estado"))?;
        let publicacao = Diretorio::abrir_teste(&raiz.join("publicacao"))?;
        for nome in ["lock", "auditoria"] {
            OpenOptions::new().create_new(true).write(true).mode(0o600).open(raiz.join("estado").join(nome))?;
        }
        let trava = estado.travar()?;
        estado.inicializar_relogio(&trava)?;
        let agora = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|_| anyhow::anyhow!("aleatoriedade indisponível"))?;
        let chave = SigningKey::from_bytes(&seed);
        let politica = serde_json::json!({"endpoint":"https://exemplo.invalid/", "canal":"estavel", "arquitetura":std::env::consts::ARCH,
            "uid_hub":1234,"gid_hub":1234,"chave_manifesto":B64.encode(chave.verifying_key().as_bytes()),
            "tamanho_maximo":1024,"tempo_rede_segundos":10,"janela_minima":10,"janela_maxima":100,
            "cgroup_memoria":1024,"cgroup_processos":2,"cgroup_cpu_quota":1000,"cgroup_cpu_periodo":1000});
        estado.gravar("politica", &politica)?;
        fs::DirBuilder::new().mode(0o700).create(raiz.join("estado/transacoes"))?;
        let transacao = estado.criar_transacao(&"c".repeat(64))?;
        let artefato = |nome: &str| format!(r#"{{"nome":"{nome}","tamanho":10,"sha256":"{}"}}"#, "a".repeat(64));
        let manifesto = format!(
            r#"{{"produto":"mexerica","canal":"estavel","arquitetura":"{}","versao":"1.0.0","sequencia":1,"inicio":{},"expiracao":{},"hub":{},"ajudante":{},"sequencia_minima_reversivel":0,"wrapper_minimo":"0.1.0"}}"#,
            std::env::consts::ARCH,
            agora - 1,
            agora + 1000,
            artefato("donnabot-host"),
            artefato("donnabot-motor-exec")
        );
        let mut mensagem = b"mexerica/atualizacao-privilegiada/manifesto/v1\0".to_vec();
        mensagem.extend_from_slice(manifesto.as_bytes());
        let snapshot = br#"{"anterior":null,"motor_conf":{"estado":"ausente"}}"#;
        transacao.gravar_bytes("manifesto", manifesto.as_bytes())?;
        transacao.gravar_bytes("assinatura", &chave.sign(&mensagem).to_bytes())?;
        transacao.gravar_bytes("snapshot", snapshot)?;
        let hash = |bytes: &[u8]| Sha256::digest(bytes).hex();
        let journal = Journal {
            estado: Estado { marca: 0, piso: 0, fase: Fase::Livre },
            etapa: Etapa::Preparada,
            transacao: "c".repeat(64),
            sequencia_nova: 1,
            publicado_em: None,
            sequencia_anterior: None,
            slot_anterior: None,
            slot_novo: Slot::A,
            manifesto_sha256: hash(manifesto.as_bytes()),
            snapshot_sha256: hash(snapshot),
        };
        estado.gravar("journal", &journal)?;
        drop(trava);
        assert!(matches!(avaliar(&estado, &publicacao)?, monitor::Situacao::Bloqueado));
        assert!(estado.exigir_sem_bloqueio().is_ok());
        let trava = estado.travar()?;
        let mut reversao = journal;
        reversao.estado = Estado { marca: 1, piso: 0, fase: Fase::Pendente { anterior: None, prazo: agora + 60 } };
        reversao.etapa = Etapa::ReversaoIniciada;
        reversao.publicado_em = Some(agora - 1);
        estado.gravar("journal", &reversao)?;
        assert!(matches!(avaliar(&estado, &publicacao)?, monitor::Situacao::Pendente(_)));
        drop(trava);
        assert!(matches!(avaliar(&estado, &publicacao)?, monitor::Situacao::Pendente(_)));
        assert!(estado.exigir_sem_bloqueio().is_ok());
        // Reversão interrompida e válida, trava livre: a queda do guard preserva a retomada.
        assert_eq!(andamento(&estado, &publicacao), Andamento::Reversao);
        let trava = estado.travar()?;
        reversao.etapa = Etapa::SeletorRestaurado;
        estado.gravar("journal", &reversao)?;
        drop(trava);
        assert!(matches!(avaliar(&estado, &publicacao)?, monitor::Situacao::Pendente(_)));
        transacao.gravar_bytes("snapshot", b"{}")?;
        assert!(matches!(avaliar(&estado, &publicacao)?, monitor::Situacao::Bloqueado));
        assert!(estado.exigir_sem_bloqueio().is_err());
        fs::remove_dir_all(raiz)?;
        Ok(())
    }

    /// SEG P3-2: com a trava ocupada e o prazo vencido, o guard não pode presumir
    /// reversão em andamento; `Ocupado` manda o monitor parar o serviço.
    #[test]
    fn trava_ocupada_com_prazo_vencido_para() -> anyhow::Result<()> {
        let raiz = std::env::temp_dir().join(format!("mx365-guard-ocupado-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&raiz)?;
        for nome in ["estado", "publicacao"] {
            fs::DirBuilder::new().mode(0o700).create(raiz.join(nome))?;
        }
        let estado = Diretorio::abrir_teste(&raiz.join("estado"))?;
        let publicacao = Diretorio::abrir_teste(&raiz.join("publicacao"))?;
        OpenOptions::new().create_new(true).write(true).mode(0o600).open(raiz.join("estado/lock"))?;
        let agora = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
        let journal = Journal {
            estado: Estado { marca: 1, piso: 0, fase: Fase::Pendente { anterior: Some(0), prazo: agora - 1 } },
            etapa: Etapa::ReversaoIniciada,
            transacao: "c".repeat(64),
            sequencia_nova: 1,
            publicado_em: Some(agora - 60),
            sequencia_anterior: None,
            slot_anterior: Some(Slot::B),
            slot_novo: Slot::A,
            manifesto_sha256: "a".repeat(64),
            snapshot_sha256: "b".repeat(64),
        };
        estado.gravar("journal", &journal)?;
        let _ocupante = estado.travar()?;
        assert!(matches!(avaliar(&estado, &publicacao)?, monitor::Situacao::Ocupado));
        fs::remove_dir_all(raiz)?;
        Ok(())
    }

    /// Reprodução da prova: o guard cai no meio da reversão coordenada
    /// (`--forcar-reversao`). A contenção do `ExecStopPost` não pode gravar
    /// `emergencia` nem `bloqueio` enquanto o wrapper detém a trava ou a reversão
    /// está na janela; senão o wrapper recusa a própria reversão (motivo=estado).
    #[test]
    fn queda_do_guard_durante_reversao_coordenada_nao_bloqueia() -> anyhow::Result<()> {
        let raiz = std::env::temp_dir().join(format!("mx365-guard-queda-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&raiz)?;
        for nome in ["estado", "publicacao"] {
            fs::DirBuilder::new().mode(0o700).create(raiz.join(nome))?;
        }
        let estado = Diretorio::abrir_teste(&raiz.join("estado"))?;
        let publicacao = Diretorio::abrir_teste(&raiz.join("publicacao"))?;
        OpenOptions::new().create_new(true).write(true).mode(0o600).open(raiz.join("estado/lock"))?;
        let agora = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
        let mut journal = Journal {
            estado: Estado { marca: 2, piso: 0, fase: Fase::Pendente { anterior: Some(1), prazo: agora + 600 } },
            etapa: Etapa::Pendente,
            transacao: "c".repeat(64),
            sequencia_nova: 2,
            publicado_em: Some(agora - 5),
            sequencia_anterior: Some(1),
            slot_anterior: Some(Slot::A),
            slot_novo: Slot::B,
            manifesto_sha256: "a".repeat(64),
            snapshot_sha256: "b".repeat(64),
        };
        estado.gravar("journal", &journal)?;
        let sem_bloqueio = || estado.exigir_sem_bloqueio().is_ok() && publicacao.exigir_sem_emergencia().is_ok();

        // 1. `--reverter` obtém a trava; o journal ainda está `pendente`.
        let wrapper = estado.travar()?;
        assert_eq!(andamento(&estado, &publicacao), Andamento::Transacao);
        let mut paradas = 0;
        let acao = conter(Some(&estado), &publicacao, false, || {
            paradas += 1;
            Ok(())
        })?;
        assert_eq!((acao, paradas), (Contencao::Parar, 1));
        assert!(sem_bloqueio(), "transação root em curso não gera emergência");

        // 2. O wrapper grava `reversao_iniciada`: o guard não para nem bloqueia.
        journal.etapa = Etapa::ReversaoIniciada;
        estado.gravar("journal", &journal)?;
        assert_eq!(andamento(&estado, &publicacao), Andamento::Reversao);
        let acao = conter(Some(&estado), &publicacao, false, || anyhow::bail!("não deve parar durante a reversão"))?;
        assert_eq!(acao, Contencao::Preservar);
        // Desligamento esperado também respeita a reversão em andamento.
        assert_eq!(contencao(true, andamento(&estado, &publicacao)), Contencao::Preservar);
        journal.etapa = Etapa::SeletorRestaurado;
        estado.gravar("journal", &journal)?;
        assert_eq!(conter(Some(&estado), &publicacao, false, || Ok(()))?, Contencao::Preservar);
        assert!(sem_bloqueio(), "reversão em andamento não gera emergência");
        drop(wrapper);

        // 3. Fora desses casos continua fail-closed: janela `pendente` sem transação
        // root nem desligamento bloqueia; reversão inválida sob a trava livre também.
        journal.etapa = Etapa::Pendente;
        estado.gravar("journal", &journal)?;
        assert_eq!(andamento(&estado, &publicacao), Andamento::Nenhum);
        assert_eq!(contencao(false, Andamento::Nenhum), Contencao::Bloquear);
        assert_eq!(contencao(true, Andamento::Nenhum), Contencao::Parar);
        journal.etapa = Etapa::ReversaoIniciada;
        estado.gravar("journal", &journal)?;
        // Sem relógio conferido e documentos autenticados a retomada não valida: sem preservação.
        assert_eq!(andamento(&estado, &publicacao), Andamento::Nenhum);
        // Corrida (P2 da revisão): `andamento` leu `pendente` com a trava livre e o
        // wrapper a obteve antes da gravação. Sem emergência nem bloqueio: só parada.
        journal.etapa = Etapa::Pendente;
        estado.gravar("journal", &journal)?;
        assert_eq!(andamento(&estado, &publicacao), Andamento::Nenhum);
        let corrida = estado.travar()?;
        let mut paradas = 0;
        let acao = aplicar(Contencao::Bloquear, Andamento::Nenhum, Some(&estado), &publicacao, || {
            paradas += 1;
            Ok(())
        })?;
        assert_eq!((acao, paradas), (Contencao::Parar, 1));
        assert!(sem_bloqueio(), "trava tomada após a leitura não gera emergência");
        drop(corrida);
        // Trava livre de fato: bloqueio gravado sob ela e parada pedida. A emergência
        // exige diretório de publicação root; fora de root o erro dela é devolvido.
        let mut paradas = 0;
        let resultado = aplicar(Contencao::Bloquear, Andamento::Nenhum, Some(&estado), &publicacao, || {
            paradas += 1;
            Ok(())
        });
        assert_eq!(paradas, 1);
        assert!(estado.exigir_sem_bloqueio().is_err());
        // SAFETY: consulta de UID sem efeitos colaterais.
        if unsafe { libc::geteuid() } == 0 {
            assert_eq!(resultado?, Contencao::Bloquear);
            assert!(publicacao.exigir_sem_emergencia().is_err());
        } else {
            assert!(resultado.is_err());
        }
        fs::remove_file(raiz.join("estado/bloqueio"))?;
        let _ = fs::remove_file(raiz.join("publicacao/emergencia"));
        // Janela vencida com o wrapper na trava: transação, nunca reversão preservada.
        journal.estado.fase = Fase::Pendente { anterior: Some(1), prazo: agora - 1 };
        estado.gravar("journal", &journal)?;
        let _wrapper = estado.travar()?;
        assert_eq!(andamento(&estado, &publicacao), Andamento::Transacao);
        fs::remove_dir_all(raiz)?;
        Ok(())
    }
}
