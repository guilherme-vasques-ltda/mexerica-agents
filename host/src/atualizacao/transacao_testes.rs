//! Provas da transação com diretórios root reais sob /var/lib (sem nosuid). Exigem root e
//! `fs.protected_hardlinks=1`; fora de VM descartável como root, são ignoradas.
use super::*;
use crate::atualizacao::{
    manifesto::{self, Manifesto},
    release::Release,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::{Signer as _, SigningKey};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::Command,
};

const GID_HUB: u32 = 65534;
/// UID do Hub distinto por ambiente: testes paralelos não enxergam os processos uns dos outros.
static PROXIMO_UID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(61_000);

struct Ambiente {
    raiz: PathBuf,
    ctx: Contexto,
    trava: File,
    chave: SigningKey,
    uid: u32,
}
impl Drop for Ambiente {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.raiz);
    }
}

fn criar(p: &Path, modo: u32) -> Result<()> {
    fs::DirBuilder::new().mode(modo).create(p)?;
    fs::set_permissions(p, fs::Permissions::from_mode(modo))?;
    Ok(())
}

fn ambiente() -> Result<Option<Ambiente>> {
    // SAFETY: consulta de UID sem efeitos colaterais.
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("teste de transação exige root em VM descartável; ignorado");
        return Ok(None);
    }
    let raiz = Path::new("/var/lib").join(format!("mx365-tx-{}", uuid::Uuid::new_v4()));
    criar(&raiz, 0o755)?;
    criar(&raiz.join("estado"), 0o700)?;
    criar(&raiz.join("estado/transacoes"), 0o700)?;
    criar(&raiz.join("publicacao"), 0o755)?;
    criar(&raiz.join("etc"), 0o755)?;
    for nome in ["lock", "auditoria"] {
        OpenOptions::new().create_new(true).write(true).mode(0o600).open(raiz.join("estado").join(nome))?;
    }
    let estado = Diretorio::abrir_com_modo(&raiz.join("estado"), 0o700)?;
    let publicacao = Diretorio::abrir_com_modo(&raiz.join("publicacao"), 0o755)?;
    let configuracao = Diretorio::abrir_com_modo(&raiz.join("etc"), 0o755)?;
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|_| anyhow::anyhow!("aleatoriedade indisponível"))?;
    let chave = SigningKey::from_bytes(&seed);
    let uid_hub = PROXIMO_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let politica = serde_json::json!({"endpoint":"https://exemplo.invalid/", "canal":"estavel", "arquitetura":std::env::consts::ARCH,
        "uid_hub":uid_hub,"gid_hub":GID_HUB,"chave_manifesto":B64.encode(chave.verifying_key().as_bytes()),
        "tamanho_maximo":64*1024*1024,"tempo_rede_segundos":10,"janela_minima":60,"janela_maxima":600,
        "cgroup_memoria":1024,"cgroup_processos":2,"cgroup_cpu_quota":1000,"cgroup_cpu_periodo":1000});
    estado.gravar("politica", &politica)?;
    let politica = Politica::ler(&estado)?;
    let trava = estado.travar()?;
    estado.inicializar_relogio(&trava)?;
    let ctx = Contexto { estado, publicacao, configuracao, politica, chamador: Identidade::AdminRoot };
    Ok(Some(Ambiente { raiz, ctx, trava, chave, uid: uid_hub }))
}

fn agora_bruto() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(0)
}

impl Ambiente {
    fn release(&self, sequencia: u64, piso: u64, hub: &[u8], ajudante: &[u8], validade: i64) -> Result<ReleaseBaixada> {
        let agora = agora_bruto();
        let artefato =
            |nome: &str, bytes: &[u8]| Artefato { nome: nome.into(), tamanho: bytes.len() as u64, sha256: hash(bytes) };
        let m = Manifesto {
            produto: "mexerica".into(),
            canal: "estavel".into(),
            arquitetura: std::env::consts::ARCH.into(),
            versao: format!("1.0.{sequencia}"),
            sequencia,
            inicio: agora - 10,
            expiracao: agora + validade,
            hub: artefato("donnabot-host", hub),
            ajudante: artefato("donnabot-motor-exec", ajudante),
            sequencia_minima_reversivel: piso,
            wrapper_minimo: "0.1.0".into(),
        };
        let bytes = serde_json::to_vec(&m)?;
        let mut mensagem = manifesto::DOMINIO.to_vec();
        mensagem.extend_from_slice(&bytes);
        let assinatura = self.chave.sign(&mensagem).to_bytes().to_vec();
        let manifesto = manifesto::verificar(&bytes, &assinatura, &self.chave.verifying_key().to_bytes())?;
        Ok(ReleaseBaixada { release: Release { manifesto, prazo_limite: 0 }, manifesto: bytes, assinatura })
    }

    fn atualizar(&self, sequencia: u64, piso: u64, validade: i64) -> Result<()> {
        let hub = format!("hub-{sequencia}").into_bytes();
        let ajudante = fs::read("/usr/bin/sleep")?;
        let release = self.release(sequencia, piso, &hub, &ajudante, validade)?;
        let mut fonte = |peca: Peca, destino: &mut File, artefato: &Artefato| -> Result<()> {
            let bytes = if peca == Peca::Hub { &hub } else { &ajudante };
            ensure!(hash(bytes) == artefato.sha256, "fixture divergente");
            destino.write_all(bytes)?;
            destino.sync_all()?;
            Ok(())
        };
        atualizar_com_release(&self.ctx, &self.trava, &release, &mut fonte)
    }

    fn agora(&self) -> Result<i64> {
        self.ctx.estado.hora_conferida(&self.trava)
    }
    fn aceitar(&self) -> Result<()> {
        self.ctx.estado.aceitar_transacao(&self.ctx.publicacao, &self.trava, Identidade::AdminRoot, self.agora()?)
    }
    fn reverter(&self) -> Result<()> {
        reverter(&self.ctx, &self.trava, self.agora()?)
    }
    fn recuperar(&self) -> Result<Option<Recuperacao>> {
        recuperar(&self.ctx, &self.trava, self.agora()?)
    }
    fn journal(&self) -> Result<Journal> {
        self.ctx.estado.ler("journal")
    }
    fn seletor(&self) -> Result<Option<Slot>> {
        self.ctx.publicacao.seletor()
    }
    fn caminho(&self, relativo: &str) -> PathBuf {
        self.raiz.join("publicacao").join(relativo)
    }
    fn modo(&self, relativo: &str) -> Result<u32> {
        Ok(fs::symlink_metadata(self.caminho(relativo))?.permissions().mode() & 0o7777)
    }
    fn capacidades(&self, slot: &str) -> Result<bool> {
        let caminho = std::ffi::CString::new(
            self.caminho(&format!("{slot}/donnabot-motor-exec")).into_os_string().into_encoded_bytes(),
        )?;
        // SAFETY: caminho de teste terminado em NUL; consulta somente o tamanho.
        let n = unsafe { libc::lgetxattr(caminho.as_ptr(), c"security.capability".as_ptr(), std::ptr::null_mut(), 0) };
        Ok(n > 0)
    }
    fn status(&self) -> Result<String> {
        Ok(fs::read_to_string(self.caminho("status.json"))?)
    }

    /// Par anterior ativo e armado, par novo inexistente e marca não avançada.
    fn conferir_cancelada(&self, marca: u64) -> Result<()> {
        let j = self.journal()?;
        ensure!(
            matches!(j.etapa, Etapa::Cancelada) && j.estado.marca == marca,
            "journal não cancelado: {:?} {}",
            j.etapa,
            j.estado.marca
        );
        ensure!(
            self.seletor()? == Some(Slot::A) && self.modo("slot-a")? == 0o755 && self.capacidades("slot-a")?,
            "par anterior não restabelecido"
        );
        ensure!(!self.caminho("slot-b").exists(), "par novo não descartado");
        ensure!(self.status()?.contains("\"liberado\"") && self.status()?.contains("slot-a"), "status divergente");
        Ok(())
    }

    /// Par novo publicado em janela pendente; o anterior fechado e desarmado.
    fn conferir_pendente(&self, sequencia: u64) -> Result<()> {
        let j = self.journal()?;
        ensure!(
            matches!(j.etapa, Etapa::Pendente)
                && matches!(j.estado.fase, Fase::Pendente { .. })
                && j.estado.marca == sequencia,
            "journal não pendente"
        );
        ensure!(self.seletor()? == Some(j.slot_novo), "seletor não aponta o par novo");
        let novo = j.slot_novo.nome();
        ensure!(
            self.modo(novo)? == 0o755
                && self.capacidades(novo)?
                && self.modo(&format!("{novo}/donnabot-host"))? == 0o755,
            "par novo incompleto"
        );
        if let Some(anterior) = j.slot_anterior {
            ensure!(
                self.modo(anterior.nome())? == 0o700 && !self.capacidades(anterior.nome())?,
                "par anterior exposto"
            );
        }
        ensure!(self.status()?.contains("\"pendente\""), "status não pendente");
        Ok(())
    }
}

#[test]
fn ciclo_completo_atualiza_aceita_reverte_e_recicla() -> Result<()> {
    let Some(amb) = ambiente()? else { return Ok(()) };
    amb.atualizar(1, 0, 3600)?;
    amb.conferir_pendente(1)?;
    let ajudante = fs::symlink_metadata(amb.caminho("slot-a/donnabot-motor-exec"))?;
    ensure!(ajudante.gid() == GID_HUB && ajudante.permissions().mode() & 0o7777 == 0o750, "ajudante sem dono/modo");
    amb.aceitar()?;
    ensure!(amb.status()?.contains("\"liberado\""), "aceite sem status liberado");
    amb.atualizar(2, 1, 3600)?;
    amb.conferir_pendente(2)?;
    // Janela pendente: nova atualização é recusada sem alterar nada.
    ensure!(amb.atualizar(3, 1, 3600).is_err(), "atualização concorrente aceita");
    amb.conferir_pendente(2)?;
    // Reversão: par anterior rearmado e o novo fechado e desarmado.
    fs::write(amb.raiz.join("etc/motor.conf"), b"alterado")?;
    fs::set_permissions(amb.raiz.join("etc/motor.conf"), fs::Permissions::from_mode(0o644))?;
    amb.reverter()?;
    let j = amb.journal()?;
    ensure!(
        matches!((j.etapa, &j.estado.fase), (Etapa::Concluida, Fase::Revertida)) && j.estado.marca == 2,
        "reversão não registrada"
    );
    ensure!(
        amb.seletor()? == Some(Slot::A) && amb.capacidades("slot-a")? && amb.modo("slot-a")? == 0o755,
        "anterior não reativado"
    );
    ensure!(amb.modo("slot-b")? == 0o700 && !amb.capacidades("slot-b")?, "par revertido exposto");
    ensure!(!amb.raiz.join("etc/motor.conf").exists(), "motor.conf não voltou à ausência registrada");
    ensure!(amb.status()?.contains("\"liberado\"") && amb.status()?.contains("slot-a"), "status após reversão");
    amb.reverter()?; // idempotente
    ensure!(amb.aceitar().is_err(), "aceite após reversão");
    // Nova atualização recicla o slot do par revertido; anterior é o par 1.
    amb.atualizar(3, 1, 3600)?;
    amb.conferir_pendente(3)?;
    ensure!(amb.journal()?.sequencia_anterior == Some(1), "par anterior errado após reversão");
    let auditoria = fs::read_to_string(amb.raiz.join("estado/auditoria"))?;
    ensure!(auditoria.lines().count() >= 4, "auditoria incompleta");
    Ok(())
}

const PONTOS_ATUALIZACAO: [&str; 12] = [
    "preparada",
    "download_hub",
    "artefatos",
    "slot_preparado",
    "desativacao_iniciada",
    "anterior_fechado",
    "anterior_desativado",
    "novo_exposto",
    "seletor_gravado",
    "seletor_trocado",
    "pendente",
    "nenhum",
];

#[test]
fn crash_em_cada_fase_da_atualizacao_retoma_sem_estado_misto() -> Result<()> {
    for ponto in PONTOS_ATUALIZACAO {
        let Some(amb) = ambiente()? else { return Ok(()) };
        amb.atualizar(1, 0, 3600)?;
        amb.aceitar()?;
        FALHA.with(|f| f.set(Some(ponto)));
        let primeira = amb.atualizar(2, 1, 3600);
        FALHA.with(|f| f.set(None));
        ensure!(primeira.is_err() == (ponto != "nenhum"), "{ponto}: falha simulada não interrompeu");
        // Novo processo: a retomada só lê journal, manifesto e snapshot root.
        let acao = amb.recuperar().map_err(|e| anyhow::anyhow!("{ponto}: {e:#}"))?;
        match ponto {
            "seletor_gravado" | "seletor_trocado" => {
                ensure!(acao == Some(Recuperacao::ResolverPublicacao), "{ponto}: ação {acao:?}");
                amb.conferir_pendente(2)?;
            }
            "pendente" | "nenhum" => {
                ensure!(acao.is_none(), "{ponto}: ação {acao:?}");
                amb.conferir_pendente(2)?;
            }
            _ => {
                amb.conferir_cancelada(1).map_err(|e| anyhow::anyhow!("{ponto}: {e:#}"))?;
                // O estado retomado aceita nova transação normalmente.
                amb.atualizar(3, 1, 3600)?;
                amb.conferir_pendente(3)?;
            }
        }
    }
    Ok(())
}

#[test]
fn crash_durante_a_propria_recuperacao_tambem_retoma() -> Result<()> {
    let Some(amb) = ambiente()? else { return Ok(()) };
    amb.atualizar(1, 0, 3600)?;
    amb.aceitar()?;
    FALHA.with(|f| f.set(Some("novo_exposto")));
    ensure!(amb.atualizar(2, 1, 3600).is_err(), "falha simulada não interrompeu");
    for ponto in ["cancelamento_iniciado", "novo_desativado", "anterior_restabelecido"] {
        FALHA.with(|f| f.set(Some(ponto)));
        ensure!(amb.recuperar().is_err(), "{ponto}: retomada não interrompida");
    }
    FALHA.with(|f| f.set(None));
    ensure!(amb.recuperar()? == Some(Recuperacao::RestabelecerAnterior), "retomada final");
    amb.conferir_cancelada(1)
}

#[test]
fn crash_em_cada_fase_da_reversao_conclui_ao_repetir() -> Result<()> {
    for ponto in [
        "reversao_iniciada",
        "anterior_armado",
        "novo_fechado",
        "seletor_revertido",
        "seletor_restaurado",
        "configuracao_restaurada",
        "revertida",
    ] {
        let Some(amb) = ambiente()? else { return Ok(()) };
        amb.atualizar(1, 0, 3600)?;
        amb.aceitar()?;
        fs::write(amb.raiz.join("etc/motor.conf"), b"motor=1\n")?;
        fs::set_permissions(amb.raiz.join("etc/motor.conf"), fs::Permissions::from_mode(0o644))?;
        amb.atualizar(2, 1, 3600)?;
        fs::write(amb.raiz.join("etc/motor.conf"), b"motor=2\n")?;
        FALHA.with(|f| f.set(Some(ponto)));
        ensure!(amb.reverter().is_err(), "{ponto}: falha simulada não interrompeu");
        // Queda dentro da reversão: o status público distingue a volta retomável (dentro do
        // prazo) do bloqueio definitivo, para o Hub agendar a retomada em vez de desistir.
        ensure!(amb.status()?.contains("\"revertendo\""), "{ponto}: reversão interrompida publicada como bloqueio");
        FALHA.with(|f| f.set(None));
        amb.reverter().map_err(|e| anyhow::anyhow!("{ponto}: {e:#}"))?;
        let j = amb.journal()?;
        ensure!(
            matches!((j.etapa, &j.estado.fase), (Etapa::Concluida, Fase::Revertida)),
            "{ponto}: reversão incompleta"
        );
        ensure!(
            amb.seletor()? == Some(Slot::A)
                && amb.capacidades("slot-a")?
                && !amb.capacidades("slot-b")?
                && amb.modo("slot-b")? == 0o700,
            "{ponto}: pares inconsistentes"
        );
        ensure!(fs::read(amb.raiz.join("etc/motor.conf"))? == b"motor=1\n", "{ponto}: motor.conf não restaurado");
    }
    Ok(())
}

#[test]
fn validade_insuficiente_na_publicacao_restabelece_anterior() -> Result<()> {
    let Some(amb) = ambiente()? else { return Ok(()) };
    amb.atualizar(1, 0, 3600)?;
    amb.aceitar()?;
    // Janela mínima da política é 60 s; a release vence antes disso.
    ensure!(amb.atualizar(2, 1, 30).is_err(), "publicação sem validade mínima");
    amb.conferir_cancelada(1)
}

#[test]
fn ajudante_anterior_em_execucao_e_encerrado_e_descritor_aberto_falha_fechado() -> Result<()> {
    let Some(amb) = ambiente()? else { return Ok(()) };
    amb.atualizar(1, 0, 3600)?;
    amb.aceitar()?;
    // Processo executando o inode privilegiado do par ativo.
    let mut executando = Command::new(amb.caminho("slot-a/donnabot-motor-exec")).arg("300").spawn()?;
    std::thread::sleep(std::time::Duration::from_millis(300));
    amb.atualizar(2, 1, 3600)?;
    let status = executando.wait()?;
    ensure!(status.signal() == Some(libc::SIGKILL), "processo do ajudante anterior não foi encerrado: {status:?}");
    amb.conferir_pendente(2)?;
    amb.aceitar()?;
    // Descritor aberto para o inode do par ativo (slot-b): troca recusada e volta ao par 2.
    let descritor = File::open(amb.caminho("slot-b/donnabot-motor-exec"))?;
    let mut segurando = Command::new("/usr/bin/sleep").arg("300").stdin(descritor).spawn()?;
    std::thread::sleep(std::time::Duration::from_millis(300));
    let resultado = amb.atualizar(3, 1, 3600);
    let _ = segurando.kill();
    let _ = segurando.wait();
    ensure!(resultado.is_err(), "troca com descritor aberto aceita");
    ensure!(amb.recuperar()? == Some(Recuperacao::RestabelecerAnterior), "retomada após recusa");
    let j = amb.journal()?;
    ensure!(
        matches!(j.etapa, Etapa::Cancelada)
            && j.estado.marca == 2
            && amb.seletor()? == Some(Slot::B)
            && amb.capacidades("slot-b")?,
        "par 2 não restabelecido"
    );
    Ok(())
}

#[test]
fn reverter_primeira_migracao_volta_ao_legado_sem_componente_privilegiado() -> Result<()> {
    let Some(amb) = ambiente()? else { return Ok(()) };
    amb.atualizar(1, 0, 3600)?;
    amb.reverter()?;
    let j = amb.journal()?;
    ensure!(
        matches!((j.etapa, &j.estado.fase), (Etapa::Concluida, Fase::Revertida)) && j.estado.marca == 1,
        "reversão não registrada"
    );
    ensure!(amb.seletor()?.is_none(), "seletor permanece após voltar ao legado");
    ensure!(amb.modo("slot-a")? == 0o700 && !amb.capacidades("slot-a")?, "par novo exposto ou armado");
    ensure!(amb.status()?.contains("\"bloqueado\""), "status deveria impedir início de par gerido");
    amb.reverter()?;
    // Nova tentativa recicla o slot revertido e volta a ser a primeira migração.
    amb.atualizar(2, 0, 3600)?;
    amb.conferir_pendente(2)?;
    ensure!(amb.journal()?.sequencia_anterior.is_none(), "par anterior inexistente esperado");
    Ok(())
}

#[test]
fn hub_em_execucao_recusa_a_troca_sem_alterar_nada() -> Result<()> {
    use std::os::unix::process::CommandExt;
    let Some(amb) = ambiente()? else { return Ok(()) };
    amb.atualizar(1, 0, 3600)?;
    amb.aceitar()?;
    // Processo chamado donnabot-host sob o UID do Hub da política deste ambiente.
    let copia = amb.raiz.join("donnabot-host");
    fs::copy("/usr/bin/sleep", &copia)?;
    fs::set_permissions(&amb.raiz, fs::Permissions::from_mode(0o755))?;
    let mut hub = Command::new(&copia).arg("300").uid(amb.uid).gid(GID_HUB).spawn()?;
    std::thread::sleep(std::time::Duration::from_millis(300));
    let antes = fs::read(amb.raiz.join("estado/journal"))?;
    let atualizacao = amb.atualizar(2, 1, 3600);
    let _ = hub.kill();
    let _ = hub.wait();
    let erro = format!("{:#}", atualizacao.err().context("troca com Hub em execução aceita")?);
    ensure!(erro.contains("Hub em execução"), "motivo inesperado: {erro}");
    ensure!(fs::read(amb.raiz.join("estado/journal"))? == antes, "journal alterado");
    ensure!(amb.seletor()? == Some(Slot::A) && !amb.caminho("slot-b").exists(), "estado alterado");
    // Parado o Hub, a mesma atualização segue.
    amb.atualizar(2, 1, 3600)?;
    amb.conferir_pendente(2)
}

/// Só VM como root: prova o registro contra manifesto, slot e o estado pendente real.
#[test]
fn aceite_exige_verificacao_vinculada_e_reversao_a_remove() -> Result<()> {
    use crate::atualizacao::verificacao::{self, Pedido};
    let Some(amb) = ambiente()? else { return Ok(()) };
    amb.atualizar(1, 0, 3600)?;
    let journal = amb.journal()?;
    let agora = amb.agora()?;
    let manifesto = amb.ctx.estado.manifesto_transacao(&amb.trava, &journal, agora)?;
    ensure!(
        verificacao::conferir(&amb.ctx.estado, &amb.ctx.publicacao, &amb.trava, &journal, agora).is_err(),
        "aceite sem registro"
    );
    let pedido = |hash: &str| Pedido {
        transacao: journal.transacao.clone(),
        hub_sha256: hash.into(),
        versao: manifesto.versao.clone(),
        porta: 7777,
    };
    ensure!(
        verificacao::registrar(&amb.ctx.estado, &amb.ctx.publicacao, &amb.trava, pedido(&"0".repeat(64)), agora)
            .is_err(),
        "hash adulterado aceito"
    );
    verificacao::registrar(&amb.ctx.estado, &amb.ctx.publicacao, &amb.trava, pedido(&manifesto.hub.sha256), agora)?;
    verificacao::conferir(&amb.ctx.estado, &amb.ctx.publicacao, &amb.trava, &journal, agora)?;
    amb.reverter()?;
    ensure!(
        verificacao::conferir(&amb.ctx.estado, &amb.ctx.publicacao, &amb.trava, &journal, agora).is_err(),
        "registro sobreviveu à reversão"
    );
    Ok(())
}
