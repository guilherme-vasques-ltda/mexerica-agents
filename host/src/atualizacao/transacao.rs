//! Executor transacional: atualização, reversão e retomada por fase.
//!
//! Cada efeito privilegiado é precedido pelo registro durável da fase no
//! journal root-owned; toda função de retomada é idempotente e só consulta
//! journal, manifesto autenticado e snapshot root da última transação.
use super::Hex as _;
use super::{
    armazenamento::{Diretorio, sem_acl},
    certidao,
    chamador::Identidade,
    estado::{Estado, Fase},
    journal::{Etapa, Evento, Journal, Motivo, Operacao, Recuperacao, Resultado},
    manifesto::Artefato,
    politica::Politica,
    processos, recuperacao,
    release::ReleaseBaixada,
    slots::{self, Slot},
    snapshot::{self, Arquivo, NomeSlot, Par, Snapshot},
};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest as _, Sha256};
use std::{
    ffi::CStr,
    fs::File,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::fs::MetadataExt,
};

const HUB: &CStr = c"donnabot-host";
const AJUDANTE: &CStr = c"donnabot-motor-exec";

/// Diretórios root validados e política carregada sob a trava exclusiva.
pub struct Contexto {
    pub estado: Diretorio,
    pub publicacao: Diretorio,
    pub configuracao: Diretorio,
    pub politica: Politica,
    pub chamador: Identidade,
}

impl Contexto {
    fn exigir_hub_parado(&self) -> Result<()> {
        super::servico_hub::exigir_parado(&self.publicacao, self.politica.uid_hub, self.chamador)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peca {
    Hub,
    Ajudante,
}

/// Escreve os bytes do artefato no inode exclusivo recém-criado no slot 0700.
pub type Fonte<'a> = dyn FnMut(Peca, &mut File, &Artefato) -> Result<()> + 'a;

#[cfg(test)]
thread_local! {
    /// Interrompe a execução após o ponto nomeado, simulando crash nos testes.
    pub(super) static FALHA: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

#[cfg_attr(not(test), expect(clippy::unnecessary_wraps, reason = "falha só é injetada nos testes"))]
fn ponto(nome: &'static str) -> Result<()> {
    #[cfg(test)]
    if FALHA.with(std::cell::Cell::get) == Some(nome) {
        bail!("falha simulada em {nome}");
    }
    let _ = nome;
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).hex()
}
fn arquivo(a: &Artefato) -> Arquivo {
    Arquivo { tamanho: a.tamanho, sha256: a.sha256.clone() }
}
fn nome_slot(slot: Slot) -> NomeSlot {
    match slot {
        Slot::A => NomeSlot::A,
        Slot::B => NomeSlot::B,
    }
}
fn slot_de(nome: NomeSlot) -> Slot {
    match nome {
        NomeSlot::A => Slot::A,
        NomeSlot::B => Slot::B,
    }
}

fn gravar(ctx: &Contexto, trava: &File, journal: &Journal) -> Result<()> {
    ctx.estado.validar_trava(trava)?;
    ctx.estado.gravar("journal", journal)
}

fn ler_journal(ctx: &Contexto) -> Result<Option<Journal>> {
    if !ctx.estado.existe("journal")? {
        return Ok(None);
    }
    Ok(Some(ctx.estado.ler("journal")?))
}

fn chmod(file: &File, modo: libc::mode_t) -> Result<()> {
    // SAFETY: altera somente o inode já aberto e validado pelo chamador.
    ensure!(unsafe { libc::fchmod(file.as_raw_fd(), modo) } == 0, "ajuste de modo recusado");
    file.sync_all()?;
    Ok(())
}

/// Nome fixo do slot, sem seguir links; `None` quando ausente.
pub(super) fn abrir_fixo(dir: &Diretorio, nome: &CStr) -> Result<Option<File>> {
    // SAFETY: nome fixo relativo a slot root validado; sem seguir links nem bloquear.
    let fd = unsafe {
        libc::openat(
            dir.descritor().as_raw_fd(),
            nome.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let erro = std::io::Error::last_os_error();
        ensure!(erro.raw_os_error() == Some(libc::ENOENT), "objeto de slot inacessível");
        return Ok(None);
    }
    // SAFETY: openat retornou descritor novo, cuja posse passa ao File.
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    ensure!(meta.is_file() && meta.uid() == 0 && meta.nlink() == 1, "objeto de slot inseguro");
    sem_acl(&file)?;
    Ok(Some(file))
}

fn remover_fixo(dir: &Diretorio, nome: &CStr) -> Result<()> {
    // SAFETY: remove só o nome fixo em slot root 0700 já conferido.
    if unsafe { libc::unlinkat(dir.descritor().as_raw_fd(), nome.as_ptr(), 0) } != 0 {
        ensure!(
            std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT),
            "remoção de objeto do slot recusada"
        );
    }
    Ok(())
}

/// Fecha o slot (0700), zera as capabilities do ajudante e encerra quem ainda
/// o executa ou mantém privilégio sob o UID do Hub. Idempotente.
fn desativar(ctx: &Contexto, slot: Slot) -> Result<Diretorio> {
    ctx.exigir_hub_parado()?;
    let dir = ctx.publicacao.fechar_slot(slot)?;
    if let Some(ajudante) = abrir_fixo(&dir, AJUDANTE)? {
        slots::limpar_capacidades(&ajudante)?;
        processos::verificar_sem_referencias(&ajudante, ctx.politica.uid_hub)?;
    }
    Ok(dir)
}

/// Remove par não selecionado: somente os dois nomes fixos, depois o diretório.
fn limpar_slot(ctx: &Contexto, slot: Slot) -> Result<()> {
    ensure!(ctx.publicacao.seletor()? != Some(slot), "slot ativo não pode ser descartado");
    if !ctx.publicacao.existe(slot.nome())? {
        return Ok(());
    }
    let dir = desativar(ctx, slot)?;
    for nome in [HUB, AJUDANTE] {
        // Confere tipo e dono antes de remover o nome fixo.
        if abrir_fixo(&dir, nome)?.is_some() {
            remover_fixo(&dir, nome)?;
        }
    }
    dir.descritor().sync_all()?;
    drop(dir);
    let componente = std::ffi::CString::new(slot.nome())?;
    ensure!(
        // SAFETY: remove só o diretório do slot enumerado; falha se ainda tiver outros objetos.
        unsafe { libc::unlinkat(ctx.publicacao.descritor().as_raw_fd(), componente.as_ptr(), libc::AT_REMOVEDIR) } == 0,
        "slot contém objetos inesperados ou não pode ser removido"
    );
    ctx.publicacao.descritor().sync_all()?;
    Ok(())
}

/// Confere o par exposto sem alterar nada: Hub 0755 e ajudante com as capabilities exatas.
fn conferir_par_ativo(dir: &Diretorio, hub: &Arquivo, ajudante: &Arquivo, gid_hub: u32) -> Result<()> {
    let mut binario = abrir_fixo(dir, HUB)?.context("binário do par ausente")?;
    snapshot::verificar_arquivo(&mut binario, hub, 0, 0o755)?;
    let mut exec = abrir_fixo(dir, AJUDANTE)?.context("ajudante do par ausente")?;
    let meta = exec.metadata()?;
    ensure!(meta.gid() == gid_hub && meta.mode() & 0o7777 == 0o750, "atributos do ajudante divergentes");
    slots::exigir_capacidades(&exec)?;
    snapshot::conferir_hash(&mut exec, ajudante)
}

fn reativar_com(
    estado: &Diretorio,
    trava: &File,
    journal: &Journal,
    efetivar: impl FnOnce(Slot, &Par) -> Result<()>,
) -> Result<()> {
    estado.validar_trava(trava)?;
    ensure!(!matches!(journal.estado.fase, Fase::Expirada), "janela expirada; reativação recusada");
    estado.exigir_sem_bloqueio()?;
    let snapshot = estado.snapshot_transacao(trava, journal)?;
    let (Some(slot), Some(par)) = (journal.slot_anterior, snapshot.anterior.as_ref()) else {
        bail!("par anterior ausente");
    };
    efetivar(slot, par)
}

/// Rearma o par anterior somente após conferir conteúdo e atributos contra o snapshot.
fn reativar(ctx: &Contexto, trava: &File, journal: &Journal) -> Result<()> {
    ctx.exigir_hub_parado()?;
    reativar_com(&ctx.estado, trava, journal, |slot, par| {
        let gid = ctx.politica.gid_hub;
        let dir = ctx.publicacao.fechar_slot(slot)?;
        let mut binario = abrir_fixo(&dir, HUB)?.context("binário anterior ausente")?;
        snapshot::verificar_arquivo(&mut binario, &par.hub, 0, 0o755)?;
        let mut exec = abrir_fixo(&dir, AJUDANTE)?.context("ajudante anterior ausente")?;
        slots::limpar_capacidades(&exec)?;
        slots::armar_ajudante(&mut exec, &par.ajudante, gid, gid, 0o750)?;
        chmod(dir.descritor(), 0o755)?;
        dir.descritor().sync_all()?;
        ctx.publicacao.descritor().sync_all()?;
        Ok(())
    })
}

fn apontar(ctx: &Contexto, alvo: Option<Slot>) -> Result<()> {
    ctx.exigir_hub_parado()?;
    let atual = ctx.publicacao.seletor()?;
    if atual == alvo {
        return Ok(());
    }
    match alvo {
        Some(slot) => ctx.publicacao.trocar_seletor(atual, slot),
        None => ctx.publicacao.remover_seletor(atual.context("seletor ausente")?),
    }
}

/// Descreve o par ativo apenas por documentos autenticados da última transação.
fn par_ativo(ctx: &Contexto, trava: &File, journal: Option<&Journal>) -> Result<Option<Par>> {
    let Some(j) = journal else { return Ok(None) };
    match (j.etapa, &j.estado.fase) {
        (Etapa::Concluida, Fase::Aceita) => {
            let m = ctx.estado.manifesto_assinado(trava, j)?;
            Ok(Some(Par {
                sequencia: j.sequencia_nova,
                slot: nome_slot(j.slot_novo),
                hub: arquivo(&m.hub),
                ajudante: arquivo(&m.ajudante),
            }))
        }
        (Etapa::Concluida, Fase::Revertida) | (Etapa::Cancelada, _) => {
            Ok(ctx.estado.snapshot_transacao(trava, j)?.anterior)
        }
        _ => bail!("transação anterior não concluída"),
    }
}

/// Executa a transação a partir de uma release já autenticada (TUF + Ed25519).
pub fn atualizar_com_release(
    ctx: &Contexto,
    trava: &File,
    baixada: &ReleaseBaixada,
    fonte: &mut Fonte<'_>,
) -> Result<()> {
    // Antes de qualquer efeito: serviço rodando recusa sem alterar nada.
    ctx.exigir_hub_parado()?;
    ctx.estado.validar_trava(trava)?;
    ctx.estado.exigir_sem_bloqueio()?;
    ctx.publicacao.exigir_sem_emergencia()?;
    let atual = ler_journal(ctx)?;
    let estado = atual.as_ref().map_or(Estado { marca: 0, piso: 0, fase: Fase::Livre }, |j| j.estado.clone());
    ensure!(
        !matches!(estado.fase, Fase::Pendente { .. } | Fase::Expirada),
        "transação pendente impede nova atualização"
    );
    let anterior = par_ativo(ctx, trava, atual.as_ref())?;
    let slot_anterior = anterior.as_ref().map(|p| slot_de(p.slot));
    ensure!(ctx.publicacao.seletor()? == slot_anterior, "seletor não corresponde ao par ativo");
    let m = &baixada.release.manifesto;
    ensure!(hash(&baixada.manifesto) == hash(&serde_json::to_vec(m)?), "manifesto baixado divergente");
    ensure!(
        m.sequencia > estado.marca
            && m.sequencia_minima_reversivel >= estado.piso
            && anterior.as_ref().is_none_or(|p| p.sequencia < m.sequencia),
        "sequência da release não avança"
    );
    let slot_novo = slot_anterior.map_or(Slot::A, Slot::outro);
    // Par inativo de transação concluída anteriormente: descartado antes de reutilizar o nome.
    limpar_slot(ctx, slot_novo)?;
    let snapshot = Snapshot { anterior, motor_conf: ctx.configuracao.capturar_motor_conf()? };
    let bytes_snapshot = serde_json::to_vec(&snapshot)?;
    let id = certidao::gerar_id()?;
    let transacao = ctx.estado.criar_transacao(&id)?;
    transacao.gravar_bytes("manifesto", &baixada.manifesto)?;
    transacao.gravar_bytes("assinatura", &baixada.assinatura)?;
    transacao.gravar_bytes("snapshot", &bytes_snapshot)?;
    let mut journal = Journal {
        estado,
        etapa: Etapa::Preparada,
        transacao: id,
        sequencia_nova: m.sequencia,
        publicado_em: None,
        sequencia_anterior: snapshot.anterior.as_ref().map(|p| p.sequencia),
        slot_anterior,
        slot_novo,
        manifesto_sha256: hash(&baixada.manifesto),
        snapshot_sha256: hash(&bytes_snapshot),
    };
    gravar(ctx, trava, &journal)?;
    // Durante a troca o status público fica bloqueado: nenhum par gerido inicia.
    ctx.publicacao.publicar_status(&ctx.estado, trava)?;
    ponto("preparada")?;
    let slot = ctx.publicacao.criar_slot_inativo(slot_novo)?;
    let mut hub = slot.criar_regular("donnabot-host")?;
    fonte(Peca::Hub, &mut hub, &m.hub)?;
    ponto("download_hub")?;
    let mut ajudante = slot.criar_regular("donnabot-motor-exec")?;
    fonte(Peca::Ajudante, &mut ajudante, &m.ajudante)?;
    // Hash do manifesto conferido de novo a partir do disco, pelo mesmo descritor.
    snapshot::verificar_arquivo(&mut hub, &arquivo(&m.hub), 0, 0o600)?;
    chmod(&hub, 0o755)?;
    slots::preparar_ajudante(&mut ajudante, &arquivo(&m.ajudante), ctx.politica.gid_hub)?;
    slot.descritor().sync_all()?;
    ponto("artefatos")?;
    journal.etapa = Etapa::SlotPreparado;
    gravar(ctx, trava, &journal)?;
    ponto("slot_preparado")?;
    publicar(ctx, trava, journal)
}

fn publicar(ctx: &Contexto, trava: &File, mut journal: Journal) -> Result<()> {
    ensure!(matches!(journal.etapa, Etapa::SlotPreparado), "fase inválida para publicar");
    let snapshot = ctx.estado.snapshot_transacao(trava, &journal)?;
    let gid = ctx.politica.gid_hub;
    // Conferência do par anterior antes de desativá-lo: falha aqui não altera nada.
    if let (Some(slot), Some(par)) = (journal.slot_anterior, &snapshot.anterior) {
        conferir_par_ativo(&ctx.publicacao.abrir_slot(slot)?, &par.hub, &par.ajudante, gid)?;
    }
    journal.etapa = Etapa::DesativacaoIniciada;
    gravar(ctx, trava, &journal)?;
    ponto("desativacao_iniciada")?;
    if let Some(slot) = journal.slot_anterior {
        desativar(ctx, slot)?;
    }
    ponto("anterior_fechado")?;
    journal.etapa = Etapa::AnteriorDesativado;
    gravar(ctx, trava, &journal)?;
    ponto("anterior_desativado")?;
    let manifesto = ctx.estado.manifesto_assinado(trava, &journal)?;
    let novo = ctx.publicacao.abrir_slot_inativo(journal.slot_novo)?;
    conferir_par_ativo(&novo, &arquivo(&manifesto.hub), &arquivo(&manifesto.ajudante), gid)?;
    chmod(novo.descritor(), 0o755)?;
    ctx.publicacao.descritor().sync_all()?;
    ponto("novo_exposto")?;
    ctx.exigir_hub_parado()?;
    ctx.publicacao.trocar_seletor(journal.slot_anterior, journal.slot_novo)?;
    // Pós-condição: um Hub iniciado entre a checagem e a troca falha a transação;
    // a volta exige o serviço parado e, até lá, o guard o mantém parado.
    ctx.exigir_hub_parado()?;
    ponto("seletor_gravado")?;
    journal.etapa = Etapa::SeletorTrocado;
    gravar(ctx, trava, &journal)?;
    ponto("seletor_trocado")?;
    concluir_publicacao(ctx, trava, journal)
}

/// Abre a janela pendente somente se ainda resta a validade mínima no instante
/// da publicação durável; caso contrário volta ao par anterior pelo journal.
fn concluir_publicacao(ctx: &Contexto, trava: &File, mut journal: Journal) -> Result<()> {
    ensure!(matches!(journal.etapa, Etapa::SeletorTrocado), "fase inválida para concluir publicação");
    let agora = ctx.estado.hora_conferida(trava)?;
    let m = ctx.estado.manifesto_assinado(trava, &journal)?;
    let p = &ctx.politica;
    let prazo = m.expiracao.min(agora.saturating_add(p.janela_maxima));
    let mut estado = journal.estado.clone();
    let valido = m.inicio <= agora
        && m.expiracao.checked_sub(agora).is_some_and(|r| r >= p.janela_minima)
        && estado.iniciar(m.sequencia, m.sequencia_minima_reversivel, journal.sequencia_anterior, prazo, agora).is_ok();
    if !valido {
        restabelecer_anterior(ctx, trava, journal)?;
        bail!("validade restante insuficiente na publicação; par anterior restabelecido");
    }
    journal.estado = estado;
    journal.publicado_em = Some(agora);
    journal.etapa = Etapa::Pendente;
    gravar(ctx, trava, &journal)?;
    ponto("pendente")?;
    ctx.publicacao.publicar_status(&ctx.estado, trava)?;
    ctx.publicacao.publicar_certidao(&ctx.estado, trava)
}

/// Antes da desativação do par anterior: só descarta o slot novo não exposto.
fn cancelar_preparacao(ctx: &Contexto, trava: &File, mut journal: Journal) -> Result<()> {
    ensure!(matches!(journal.etapa, Etapa::Preparada | Etapa::SlotPreparado), "fase inválida para cancelar");
    limpar_slot(ctx, journal.slot_novo)?;
    ponto("cancelamento_limpo")?;
    journal.etapa = Etapa::Cancelada;
    gravar(ctx, trava, &journal)?;
    ctx.publicacao.publicar_status(&ctx.estado, trava)
}

/// Volta anterior à publicação: desativa o par novo, rearma o anterior, aponta
/// o seletor de volta e descarta o novo. A marca monotônica não avança.
fn restabelecer_anterior(ctx: &Contexto, trava: &File, mut journal: Journal) -> Result<()> {
    recuperacao::exigir_pre_publicacao(&journal)?;
    if !matches!(journal.etapa, Etapa::CancelamentoIniciado) {
        ensure!(
            matches!(journal.etapa, Etapa::DesativacaoIniciada | Etapa::AnteriorDesativado | Etapa::SeletorTrocado),
            "fase inválida para restabelecer"
        );
        journal.etapa = Etapa::CancelamentoIniciado;
        gravar(ctx, trava, &journal)?;
        ctx.publicacao.publicar_status(&ctx.estado, trava)?;
        ponto("cancelamento_iniciado")?;
    }
    let snapshot = ctx.estado.snapshot_transacao(trava, &journal)?;
    if ctx.publicacao.existe(journal.slot_novo.nome())? {
        desativar(ctx, journal.slot_novo)?;
    }
    ponto("novo_desativado")?;
    if journal.slot_anterior.is_some() || snapshot.anterior.is_some() {
        reativar(ctx, trava, &journal)?;
    }
    apontar(ctx, journal.slot_anterior)?;
    ponto("anterior_restabelecido")?;
    limpar_slot(ctx, journal.slot_novo)?;
    journal.etapa = Etapa::Cancelada;
    gravar(ctx, trava, &journal)?;
    ctx.publicacao.publicar_status(&ctx.estado, trava)
}

/// Reversão dentro da janela; idempotente a partir de qualquer fase de reversão.
fn executar_reversao(ctx: &Contexto, trava: &File, mut journal: Journal, agora: i64) -> Result<()> {
    ctx.exigir_hub_parado()?;
    ensure!(
        matches!(journal.etapa, Etapa::Pendente | Etapa::ReversaoIniciada | Etapa::SeletorRestaurado),
        "fase inválida para reverter"
    );
    let mut final_ = journal.estado.clone();
    final_.reverter(agora)?;
    let manifesto = ctx.estado.manifesto_transacao(trava, &journal, agora)?;
    let snapshot = ctx.estado.snapshot_transacao(trava, &journal)?;
    if let Some(par) = &snapshot.anterior {
        ensure!(
            par.sequencia >= manifesto.sequencia_minima_reversivel && par.sequencia >= journal.estado.piso,
            "piso impede reativar o par anterior"
        );
    }
    let gid = ctx.politica.gid_hub;
    if matches!(journal.etapa, Etapa::Pendente) {
        // Conferência sem efeitos: o par anterior precisa estar íntegro e desarmado.
        if let (Some(slot), Some(par)) = (journal.slot_anterior, &snapshot.anterior) {
            let dir = ctx.publicacao.abrir_slot(slot)?;
            let mut binario = abrir_fixo(&dir, HUB)?.context("binário anterior ausente")?;
            snapshot::verificar_arquivo(&mut binario, &par.hub, 0, 0o755)?;
            let mut exec = abrir_fixo(&dir, AJUDANTE)?.context("ajudante anterior ausente")?;
            snapshot::verificar_arquivo(&mut exec, &par.ajudante, gid, 0o750)?;
        }
        journal.etapa = Etapa::ReversaoIniciada;
        gravar(ctx, trava, &journal)?;
        ctx.publicacao.publicar_status(&ctx.estado, trava)?;
        ponto("reversao_iniciada")?;
    }
    if matches!(journal.etapa, Etapa::ReversaoIniciada) {
        if let (Some(slot), Some(par)) = (journal.slot_anterior, &snapshot.anterior) {
            if ctx.publicacao.seletor()? == Some(slot) {
                let dir = ctx.publicacao.abrir_slot(slot)?;
                conferir_par_ativo(&dir, &par.hub, &par.ajudante, gid)?;
            } else {
                reativar(ctx, trava, &journal)?;
            }
        }
        ponto("anterior_armado")?;
        apontar(ctx, journal.slot_anterior)?;
        ponto("seletor_revertido")?;
        desativar(ctx, journal.slot_novo)?;
        ponto("novo_fechado")?;
        journal.etapa = Etapa::SeletorRestaurado;
        gravar(ctx, trava, &journal)?;
        ponto("seletor_restaurado")?;
    }
    ctx.configuracao.restaurar_motor_conf(&ctx.estado, &ctx.publicacao, trava, agora)?;
    ponto("configuracao_restaurada")?;
    journal.estado = final_;
    journal.etapa = Etapa::Concluida;
    gravar(ctx, trava, &journal)?;
    super::verificacao::limpar(&ctx.estado, &journal)?;
    ponto("revertida")?;
    ctx.publicacao.publicar_status(&ctx.estado, trava)
}

fn auditar(ctx: &Contexto, operacao: Operacao, agora: i64, resultado: Resultado, motivo: Motivo) -> Result<()> {
    let journal = ler_journal(ctx).ok().flatten();
    let evento = Evento {
        horario_unix: agora,
        uid: ctx.chamador.uid(),
        origem: ctx.chamador.origem(),
        operacao,
        sequencia_atual: journal.as_ref().map(|j| j.estado.marca),
        sequencia_anterior: journal.as_ref().and_then(|j| j.sequencia_anterior),
        resultado,
        motivo,
    };
    if matches!(resultado, Resultado::Inicio) { ctx.estado.inicio(&evento) } else { ctx.estado.auditar(&evento) }
}

/// Retoma a última transação interrompida antes de qualquer nova operação.
/// Devolve a ação executada; `None` quando nada estava interrompido.
pub fn recuperar(ctx: &Contexto, trava: &File, agora: i64) -> Result<Option<Recuperacao>> {
    if ler_journal(ctx)?.is_none() {
        return Ok(None);
    }
    let plano = recuperacao::validar(&ctx.estado, &ctx.publicacao, trava, agora)?;
    if matches!(plano.acao, Recuperacao::Concluida | Recuperacao::Pendente) {
        if matches!(plano.acao, Recuperacao::Concluida) && matches!(plano.journal.etapa, Etapa::Concluida) {
            super::verificacao::limpar(&ctx.estado, &plano.journal)?;
        }
        // Um crash após o journal pode ter deixado o status público antigo.
        ctx.publicacao.publicar_status(&ctx.estado, trava)?;
        return Ok(None);
    }
    auditar(ctx, Operacao::Recuperar, agora, Resultado::Inicio, Motivo::Estado)?;
    let acao = plano.acao;
    let mut journal = plano.journal;
    let resultado = match acao {
        Recuperacao::CancelarPreparacao => cancelar_preparacao(ctx, trava, journal),
        Recuperacao::RestabelecerAnterior => restabelecer_anterior(ctx, trava, journal),
        Recuperacao::ResolverPublicacao => {
            // Com o seletor já no par novo, registrar a troca antes de concluir.
            if matches!(journal.etapa, Etapa::AnteriorDesativado) {
                journal.etapa = Etapa::SeletorTrocado;
                gravar(ctx, trava, &journal)?;
            }
            concluir_publicacao(ctx, trava, journal)
        }
        Recuperacao::TerminarReversao => executar_reversao(ctx, trava, journal, agora),
        Recuperacao::Concluida | Recuperacao::Pendente => Ok(()),
    };
    let saida = if resultado.is_ok() { Resultado::Sucesso } else { Resultado::Falha };
    auditar(ctx, Operacao::Recuperar, agora, saida, Motivo::Estado)?;
    resultado.map(|()| Some(acao))
}

/// Falha no meio da troca: volta ao par anterior imediatamente, pelo journal.
fn desfazer(ctx: &Contexto, trava: &File) -> Result<()> {
    let Some(journal) = ler_journal(ctx)? else { return Ok(()) };
    let agora = ctx.estado.hora_conferida(trava)?;
    let plano = recuperacao::validar(&ctx.estado, &ctx.publicacao, trava, agora)?;
    ensure!(plano.journal.transacao == journal.transacao, "journal mudou durante a falha");
    match plano.journal.etapa {
        Etapa::Preparada | Etapa::SlotPreparado => cancelar_preparacao(ctx, trava, plano.journal),
        Etapa::DesativacaoIniciada
        | Etapa::AnteriorDesativado
        | Etapa::SeletorTrocado
        | Etapa::CancelamentoIniciado => restabelecer_anterior(ctx, trava, plano.journal),
        _ => Ok(()),
    }
}

/// Fluxo completo de `atualizar`: retomada, release autenticada, download no
/// slot inativo e publicação com janela de aceite.
pub fn atualizar(ctx: &Contexto, trava: &File, agora: i64) -> Result<()> {
    use super::{
        rede::{self, Recurso, Transporte},
        release,
    };
    slots::protected_hardlinks()?;
    if matches!(recuperar(ctx, trava, agora)?, Some(Recuperacao::ResolverPublicacao)) {
        // A publicação interrompida foi concluída agora: é o resultado pedido.
        return Ok(());
    }
    let atual = ler_journal(ctx)?;
    let (marca, piso) = atual.as_ref().map_or((0, 0), |j| (j.estado.marca, j.estado.piso));
    ensure!(
        !atual.as_ref().is_some_and(|j| matches!(j.estado.fase, Fase::Pendente { .. } | Fase::Expirada)),
        "transação pendente impede nova atualização"
    );
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let baixada = runtime.block_on(release::baixar_verificada(
        &ctx.estado,
        &ctx.publicacao,
        trava,
        &ctx.politica,
        agora,
        marca,
        piso,
    ))?;
    let transporte =
        Transporte::novo(&ctx.politica.endpoint, ctx.politica.tamanho_maximo, ctx.politica.tempo_rede_segundos)?;
    let mut fonte = |peca: Peca, destino: &mut File, artefato: &Artefato| {
        let recurso = match peca {
            Peca::Hub => Recurso::Hub,
            Peca::Ajudante => Recurso::Ajudante,
        };
        runtime.block_on(rede::baixar_artefato(&transporte, recurso, destino, artefato))
    };
    let resultado = atualizar_com_release(ctx, trava, &baixada, &mut fonte);
    if resultado.is_err() {
        // A falha original prevalece; se a volta também falhar, o journal fica
        // na fase gravada e a próxima chamada retoma (o guard mantém o serviço parado).
        let _ = desfazer(ctx, trava);
    }
    resultado
}

/// `--reverter`: só journal, manifesto e snapshot root da última transação.
pub fn reverter(ctx: &Contexto, trava: &File, agora: i64) -> Result<()> {
    slots::protected_hardlinks()?;
    let plano = recuperacao::validar(&ctx.estado, &ctx.publicacao, trava, agora)?;
    match plano.acao {
        Recuperacao::Concluida => {
            ensure!(
                matches!((plano.journal.etapa, &plano.journal.estado.fase), (Etapa::Concluida, Fase::Revertida)),
                "sem transação reversível"
            );
            ctx.publicacao.publicar_status(&ctx.estado, trava)
        }
        Recuperacao::Pendente | Recuperacao::TerminarReversao => executar_reversao(ctx, trava, plano.journal, agora),
        // Troca interrompida antes da janela: a volta é o cancelamento pelo journal.
        Recuperacao::CancelarPreparacao => cancelar_preparacao(ctx, trava, plano.journal),
        Recuperacao::RestabelecerAnterior | Recuperacao::ResolverPublicacao => {
            restabelecer_anterior(ctx, trava, plano.journal)
        }
    }
}

#[cfg(test)]
#[path = "transacao_testes.rs"]
mod testes;
