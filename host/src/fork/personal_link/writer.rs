//! Escritor/validador D430 do PR-A. Funções síncronas que só usam a `Connection` recebida
//! (dentro da guarda da Autoridade ou do nascimento/abertura sob host.lock): sem Store,
//! Cloud, FS ou rede. Journal + origem + estado + reserva + evento no MESMO commit.
use super::{
    canon::{self, Assinado},
    time::{Amostra, Relogio},
    wire::{self, Contexto},
};
use crate::remote::identity::Identity;
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

/// Teto D433: 3 admissões por H e por T na janela conservadora.
const TETO: u64 = 3;

/// Conflitos PL1/ACS4/slot/`first_owner`/legado recusados na mesma Tx (D-1 do PR1).
pub(crate) fn conflitos(c: &Connection) -> Result<()> {
    let existe: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM personal_generation) OR EXISTS(SELECT 1 FROM personal_device_origins)
         OR EXISTS(SELECT 1 FROM bootstrap_generation) OR EXISTS(SELECT 1 FROM bootstrap_slot)
         OR EXISTS(SELECT 1 FROM first_owner) OR EXISTS(SELECT 1 FROM personal_local_birth)
         OR EXISTS(SELECT 1 FROM devices)",
        [],
        |r| r.get(0),
    )?;
    ensure!(!existe, "not_allowed: instalação com origem PL1/ACS4/legada; preservada sem migração");
    Ok(())
}

fn proximo_seq(c: &Connection) -> Result<i64> {
    Ok(c.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM personal_link_journal", [], |r| r.get(0))?)
}

fn registrar(c: &Connection, tipo: &str, a: &Assinado) -> Result<i64> {
    let seq = proximo_seq(c)?;
    c.execute("INSERT INTO personal_link_journal(seq,kind,bytes) VALUES(?,?,?)", params![seq, tipo, a.bytes()?])?;
    Ok(seq)
}

fn evento(c: &Connection, tipo: &str, sujeito: &str, runtime: &str) -> Result<()> {
    c.execute(
        "INSERT INTO personal_link_events(kind,subject,runtime_id) VALUES(?,?,?)",
        params![tipo, sujeito, runtime],
    )?;
    Ok(())
}

fn journal(c: &Connection, seq: i64) -> Result<(String, Value)> {
    let (tipo, bytes): (String, Vec<u8>) =
        c.query_row("SELECT kind,bytes FROM personal_link_journal WHERE seq=?", [seq], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok((tipo, canon::ler(&bytes)?))
}

/// Nascimento: instalação + journal seq1 com o `keyId` do master, na Tx da criação exclusiva.
pub(crate) fn nascer(c: &Connection, id: &Identity, k: &Contexto, nascimento: &Assinado, key_id: &str) -> Result<()> {
    conflitos(c)?;
    let vazio: bool = c.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM personal_link_installation) AND NOT EXISTS(SELECT 1 FROM personal_link_journal)
         AND NOT EXISTS(SELECT 1 FROM personal_link_origins)",
        [],
        |r| r.get(0),
    )?;
    ensure!(vazio, "not_allowed: base D430 não nasceu vazia");
    c.execute(
        "INSERT INTO personal_link_installation(singleton,install_id,hub_key,box_key,generation,install_revision,master_key_id,birth)
         VALUES(1,?,?,?,?,?,?,?)",
        params![
            k.instalacao,
            k.h,
            id.box_pub_b64(),
            k.g,
            i64::try_from(k.revisao_instalacao)?,
            key_id,
            nascimento.bytes()?
        ],
    )?;
    ensure!(registrar(c, "birth", nascimento)? == 1, "not_allowed: journal inicial fora de ordem");
    evento(c, "birth", &k.instalacao, "birth")
}

/// Runtime novo, append-only. O chamador observa o commit antes de iniciar a quarentena.
pub(crate) fn registrar_runtime(c: &Connection, runtime: &str, relogio: &Relogio, inicio_ms: u64) -> Result<()> {
    let revisao: i64 =
        c.query_row("SELECT COALESCE(MAX(revision),0)+1 FROM personal_link_runtimes", [], |r| r.get(0))?;
    c.execute(
        "INSERT INTO personal_link_runtimes(revision,runtime_id,boot_id,start_boottime_ms,time_source) VALUES(?,?,?,?,?)",
        params![revisao, runtime, relogio.boot_id(), i64::try_from(inicio_ms)?, relogio.descricao()],
    )?;
    evento(c, "runtime", runtime, runtime)
}

pub(crate) fn runtime_atual(c: &Connection) -> Result<Option<String>> {
    Ok(c.query_row("SELECT runtime_id FROM personal_link_runtimes ORDER BY revision DESC LIMIT 1", [], |r| r.get(0))
        .optional()?)
}

/// Visão de tempo da decisão: runtime/boot atuais e amostra qualificada já tomada.
pub(crate) struct Janela<'a> {
    pub runtime: &'a str,
    pub boot_id: &'a str,
    pub agora: Amostra,
    pub quarentena_terminada: bool,
}

/// P3 (a) conservadora: NULL do runtime atual conta; âncora conta enquanto `L(idade) <= 24 h`;
/// reserva de runtime ANTERIOR conta até terminar a quarentena atual e então sai da janela.
pub(crate) fn conta(runtime: &str, boot: Option<&str>, ancora: Option<i64>, j: &Janela<'_>) -> bool {
    if runtime != j.runtime {
        return !j.quarentena_terminada;
    }
    match (boot, ancora.and_then(|a| u64::try_from(a).ok())) {
        (Some(boot), Some(ancora)) if boot == j.boot_id => Relogio::dentro_da_janela(ancora, j.agora),
        _ => true,
    }
}

pub(crate) fn contar(c: &Connection, coluna: &str, valor: &str, j: &Janela<'_>) -> Result<u64> {
    ensure!(matches!(coluna, "hub_key" | "key"), "not_allowed: coluna de quota inválida");
    let mut stmt = c.prepare(&format!(
        "SELECT runtime_id,anchor_boot_id,anchor_boottime_ms FROM personal_link_reservations WHERE {coluna}=?"
    ))?;
    let linhas = stmt.query_map([valor], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i64>>(2)?))
    })?;
    let mut total = 0u64;
    for linha in linhas {
        let (runtime, boot, ancora) = linha?;
        if conta(&runtime, boot.as_deref(), ancora, j) {
            total = total.checked_add(1).context("not_allowed: quota")?;
        }
    }
    Ok(total)
}

/// Housekeeping: âncora posterior ao commit observado; não admite nem libera vaga.
pub(crate) fn ancorar(c: &Connection, runtime: &str, boot_id: &str, agora: Amostra) -> Result<()> {
    c.execute(
        "UPDATE personal_link_reservations SET anchor_boot_id=?,anchor_boottime_ms=? WHERE runtime_id=? AND anchor_boottime_ms IS NULL",
        params![boot_id, i64::try_from(agora.boottime_ms)?, runtime],
    )?;
    Ok(())
}

fn instalacao_confere(c: &Connection, k: &Contexto) -> Result<()> {
    let ok: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM personal_link_installation WHERE install_id=? AND hub_key=? AND generation=? AND install_revision=?)",
        params![k.instalacao, k.h, k.g, i64::try_from(k.revisao_instalacao)?],
        |r| r.get(0),
    )?;
    ensure!(ok, "not_allowed: instalação D430 divergente");
    Ok(())
}

/// D-2: tombstone conferido explicitamente antes de qualquer positivo.
fn origem_aberta(c: &Connection, t: &str) -> Result<()> {
    let tombstone: Option<i64> =
        c.query_row("SELECT tombstone FROM personal_link_origins WHERE key=?", [t], |r| r.get(0)).optional()?;
    ensure!(tombstone == Some(0), "not_allowed: origem D430 ausente ou encerrada");
    Ok(())
}

/// Primeiro ato: grava RA pendente, origem connect-only imutável, direitos rev1, reserva NULL e evento.
pub(crate) fn admitir(
    c: &Connection,
    id: &Identity,
    k: &Contexto,
    desafio: &Assinado,
    aceite: &Assinado,
    janela: &Janela<'_>,
) -> Result<Assinado> {
    conflitos(c)?;
    instalacao_confere(c, k)?;
    let campos = desafio.payload();
    let texto = |i: usize| campos[i].as_str().context("not_allowed: desafio inválido");
    let (t, op, root_id) = (texto(7)?, texto(6)?, texto(11)?);
    ensure!(janela.quarentena_terminada, "not_allowed: quarentena de novas admissões");
    ensure!(!nova_raiz_bloqueada(c, k, janela.runtime, t)?, "not_allowed: raiz D430 já existe");
    ensure!(
        contar(c, "hub_key", &k.h, janela)? < TETO && contar(c, "key", t, janela)? < TETO,
        "not_allowed: teto de 3 admissões em 24 h"
    );
    // D456: a nova RA local encerra, na MESMA Tx, as pendentes de runtimes anteriores. Journal, recibos e reservas
    // antigos ficam; a origem antiga só ganha tombstone (monotônico), nunca é reescrita nem reaberta.
    let antigas: Vec<(String, String)> = {
        let mut stmt = c.prepare("SELECT admission_id,key FROM personal_link_roots WHERE state='pending'")?;
        stmt.query_map([], |linha| Ok((linha.get(0)?, linha.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    for (antiga, chave) in &antigas {
        let alteradas = c.execute(
            "UPDATE personal_link_roots SET state='closed' WHERE admission_id=? AND state='pending'",
            [antiga],
        )?;
        ensure!(alteradas == 1, "not_allowed: encerramento concorrente");
        c.execute("UPDATE personal_link_origins SET tombstone=1 WHERE key=?", [chave])?;
        evento(c, "root-closed-reopen", antiga, janela.runtime)?;
    }
    let admission_id = canon::aleatorio::<16>();
    let ra = wire::admissao(id, &wire::Admissao { desafio, aceite, admission_id: &admission_id })?;
    let s_desafio = registrar(c, "challenge", desafio)?;
    let s_aceite = registrar(c, "accept", aceite)?;
    let s_ra = registrar(c, "admission", &ra)?;
    c.execute(
        "INSERT INTO personal_link_roots(admission_id,root_id,key,challenge_seq,accept_seq,admission_seq,state)
         VALUES(?,?,?,?,?,?,'pending')",
        params![admission_id, root_id, t, s_desafio, s_aceite, s_ra],
    )?;
    let contexto = campos[4].as_str().context("not_allowed: contexto inválido")?;
    let origem = super::origem_raiz(&k.h, contexto, &k.g, t, &ra.digest()?, root_id, &aceite.digest()?)?;
    c.execute("INSERT INTO personal_link_origins(key,binding) VALUES(?,?)", params![t, origem])?;
    c.execute(
        "INSERT INTO personal_link_rights(key,revision,policy,grant_seq) VALUES(?,1,?,?)",
        params![t, canon::j(&wire::p0())?, s_ra],
    )?;
    c.execute(
        "INSERT INTO personal_link_reservations(admission_id,hub_key,key,op,runtime_id) VALUES(?,?,?,?,?)",
        params![admission_id, k.h, t, op, janela.runtime],
    )?;
    evento(c, "root-admitted", &admission_id, janela.runtime)?;
    Ok(ra)
}

/// Raiz registrada para T: `admissionId`, `signedRA`, `signedRC` e gestão opcionais.
pub(crate) struct Raiz {
    pub admission_id: String,
    pub ra: Assinado,
    pub rc: Option<Assinado>,
    pub gestao: Option<(Assinado, Assinado)>,
    /// D456: pendente encerrada por uma nova RA local depois da reabertura (terminal).
    pub fechada: bool,
}

impl Raiz {
    /// D456: RA pendente nascida em OUTRO runtime fica incerta/fechada; só o mesmo runtime conclui.
    pub(crate) fn pendente_de_outro_runtime(&self, runtime: &str) -> bool {
        self.rc.is_none() && (self.fechada || self.ra.payload()[5] != runtime)
    }
}

/// Há raiz que impede NOVA admissão: completa, pendente do runtime atual, ou origem já usada por T.
pub(crate) fn nova_raiz_bloqueada(c: &Connection, k: &Contexto, runtime: &str, t: &str) -> Result<bool> {
    let origem: bool =
        c.query_row("SELECT EXISTS(SELECT 1 FROM personal_link_origins WHERE key=?)", [t], |linha| linha.get(0))?;
    if origem {
        return Ok(true);
    }
    let mut stmt = c.prepare("SELECT key FROM personal_link_roots WHERE state<>'closed'")?;
    let chaves: Vec<String> = stmt.query_map([], |linha| linha.get(0))?.collect::<rusqlite::Result<_>>()?;
    for chave in chaves {
        let registro = raiz(c, k, &chave)?.context("not_allowed: raiz D430")?;
        if !registro.pendente_de_outro_runtime(runtime) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `(admission_id, admission_seq, persisted_seq, complete_seq, state)`.
type LinhaRaiz = (String, i64, Option<i64>, Option<i64>, String);

pub(crate) fn raiz(c: &Connection, k: &Contexto, t: &str) -> Result<Option<Raiz>> {
    let linha: Option<LinhaRaiz> = c
        .query_row(
            "SELECT admission_id,admission_seq,persisted_seq,complete_seq,state FROM personal_link_roots WHERE key=?",
            [t],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((admission_id, seq_admissao, _, seq_completo, estado)) = linha else { return Ok(None) };
    let ra = wire::ler_admissao(&journal(c, seq_admissao)?.1, &k.h)?;
    let rc = match (estado.as_str(), seq_completo) {
        ("complete", Some(s)) => Some(canon::verificar(&journal(c, s)?.1, wire::COMPLETO, 10, &k.h)?),
        ("pending" | "closed", None) => None,
        _ => bail!("not_allowed: estado da raiz D430 inconsistente"),
    };
    let gestao: Option<(i64, Option<i64>)> = c
        .query_row("SELECT grant_seq,consent_seq FROM personal_link_rights WHERE key=? AND revision=2", [t], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let gestao = match gestao {
        None => None,
        Some((seq_recibo, Some(seq_gestao))) => Some((
            canon::verificar(&journal(c, seq_gestao)?.1, wire::GESTAO, 14, t)?,
            canon::verificar(&journal(c, seq_recibo)?.1, wire::RECIBO_GESTAO, 12, &k.h)?,
        )),
        Some(_) => bail!("not_allowed: gestão D430 inconsistente"),
    };
    ensure!(estado != "closed" || gestao.is_none(), "not_allowed: raiz D430 encerrada com gestão");
    Ok(Some(Raiz { admission_id, ra, rc, gestao, fechada: estado == "closed" }))
}

/// Conclusão única da raiz pelos MESMOS IDs/bytes; repetição devolve o RC original.
pub(crate) fn concluir(c: &Connection, id: &Identity, k: &Contexto, pc: &Assinado, runtime: &str) -> Result<Assinado> {
    conflitos(c)?;
    instalacao_confere(c, k)?;
    let campos = pc.payload();
    let t = campos[7].as_str().context("not_allowed: PC inválido")?;
    origem_aberta(c, t)?;
    let registro = raiz(c, k, t)?.context("not_allowed: admissão D430 ausente")?;
    ensure!(
        campos[8] == registro.admission_id && campos[9] == registro.ra.digest()?,
        "not_allowed: PC de outra admissão"
    );
    // D456 (barreira no escritor): pendente de outro runtime não conclui, nem com PC antigo nem com desafio novo.
    ensure!(!registro.pendente_de_outro_runtime(runtime), "not_allowed: admissão reaberta fica fechada (D456)");
    if let Some(rc) = registro.rc {
        return Ok(rc);
    }
    let rc = wire::completo(id, &registro.ra, pc)?;
    let seq_persistido = registrar(c, "persisted", pc)?;
    let seq_completo = registrar(c, "complete", &rc)?;
    let alteradas = c.execute(
        "UPDATE personal_link_roots SET persisted_seq=?,complete_seq=?,state='complete' WHERE admission_id=? AND state='pending'",
        params![seq_persistido, seq_completo, registro.admission_id],
    )?;
    ensure!(alteradas == 1, "not_allowed: conclusão concorrente");
    evento(c, "root-completed", &registro.admission_id, runtime)?;
    Ok(rc)
}

/// Segundo ato separado: direitos rev2 (P1) encadeados à raiz; emissor = autoridade administrativa.
pub(crate) fn conceder(
    c: &Connection,
    id: &Identity,
    k: &Contexto,
    consentimento: &Assinado,
    runtime: &str,
) -> Result<Assinado> {
    conflitos(c)?;
    instalacao_confere(c, k)?;
    let campos = consentimento.payload();
    let t = campos[7].as_str().context("not_allowed: gestão inválida")?;
    origem_aberta(c, t)?;
    let registro = raiz(c, k, t)?.context("not_allowed: raiz D430 ausente")?;
    ensure!(registro.rc.is_some(), "not_allowed: raiz ainda não concluída");
    ensure!(campos[8] == registro.ra.digest()? && campos[9] == 1, "not_allowed: gestão sobre outra raiz ou revisão");
    if let Some((anterior, mr)) = registro.gestao {
        ensure!(anterior.bytes()? == consentimento.bytes()?, "not_allowed: gestão já concedida por outra operação");
        return Ok(mr);
    }
    let cabeca: i64 =
        c.query_row("SELECT MAX(revision) FROM personal_link_rights WHERE key=?", [t], |linha| linha.get(0))?;
    ensure!(cabeca == 1, "not_allowed: cabeça de direitos mudou");
    let mr = wire::recibo_gestao(id, &registro.ra, consentimento)?;
    let seq_gestao = registrar(c, "management", consentimento)?;
    let seq_recibo = registrar(c, "management-receipt", &mr)?;
    c.execute(
        "INSERT INTO personal_link_rights(key,revision,policy,grant_seq,consent_seq) VALUES(?,2,?,?,?)",
        params![t, canon::j(&wire::p1())?, seq_recibo, seq_gestao],
    )?;
    evento(c, "management-granted", &registro.admission_id, runtime)?;
    Ok(mr)
}

/// Reabertura: H/G/instalação exatas, journal contíguo e sem órfãos, assinaturas e vínculos.
/// Divergência fecha e preserva; nunca recria, nunca cai em PL1/legado.
pub(crate) fn validar(c: &Connection, k: &Contexto, nascimento: &[u8], key_id: &str) -> Result<()> {
    conflitos(c)?;
    let (birth, master): (Vec<u8>, String) = c
        .query_row("SELECT birth,master_key_id FROM personal_link_installation WHERE singleton=1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?
        .context("not_allowed: instalação D430 ausente na base")?;
    ensure!(birth == nascimento && master == key_id, "not_allowed: marca/custódia D430 divergente");
    instalacao_confere(c, k)?;
    let (total, maximo): (i64, i64) =
        c.query_row("SELECT count(*),COALESCE(MAX(seq),0) FROM personal_link_journal", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    ensure!(total == maximo && total >= 1, "not_allowed: lacuna no journal D430");
    let (tipo, primeiro) = journal(c, 1)?;
    ensure!(tipo == "birth" && canon::j(&primeiro)? == nascimento, "not_allowed: journal inicial divergente");
    let mut usados = 1i64;
    let mut stmt = c.prepare("SELECT key FROM personal_link_roots")?;
    let chaves: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    // D456: raízes encerradas ficam no histórico; no máximo uma não encerrada.
    let abertas: i64 =
        c.query_row("SELECT count(*) FROM personal_link_roots WHERE state<>'closed'", [], |r| r.get(0))?;
    ensure!(abertas <= 1, "not_allowed: mais de uma raiz D430 aberta");
    for t in &chaves {
        let (seq_desafio, seq_aceite, seq_admissao, seq_persistido): (i64, i64, i64, Option<i64>) = c.query_row(
            "SELECT challenge_seq,accept_seq,admission_seq,persisted_seq FROM personal_link_roots WHERE key=?",
            [t],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let (t1, desafio) = journal(c, seq_desafio)?;
        let desafio = canon::verificar(&desafio, wire::DESAFIO, 14, &k.h)?;
        let (t2, aceite) = journal(c, seq_aceite)?;
        let aceite = wire::aceite(&aceite, &desafio)?;
        ensure!(
            t1 == "challenge" && t2 == "accept" && journal(c, seq_admissao)?.0 == "admission",
            "not_allowed: journal D430"
        );
        let registro = raiz(c, k, t)?.context("not_allowed: raiz D430")?;
        if registro.fechada {
            let tombstone: i64 =
                c.query_row("SELECT tombstone FROM personal_link_origins WHERE key=?", [t], |r| r.get(0))?;
            ensure!(tombstone == 1 && seq_persistido.is_none(), "not_allowed: raiz D430 encerrada inconsistente");
        }
        let campos_ra = registro.ra.payload();
        let campos_desafio = desafio.payload();
        ensure!(
            campos_ra[2..10] == [&campos_desafio[2..8], &campos_desafio[11..13]].concat()[..]
                && campos_ra[11] == desafio.digest()?
                && campos_ra[12] == aceite.digest()?
                && campos_ra[10] == registro.admission_id
                && campos_desafio[2] == k.h
                && campos_desafio[3] == k.g,
            "not_allowed: admissão D430 desvinculada"
        );
        let origem: Vec<u8> =
            c.query_row("SELECT binding FROM personal_link_origins WHERE key=?", [t], |r| r.get(0))?;
        let contexto = campos_desafio[4].as_str().context("not_allowed: contexto")?;
        let root_id = campos_desafio[11].as_str().context("not_allowed: rootId")?;
        ensure!(
            origem == super::origem_raiz(&k.h, contexto, &k.g, t, &registro.ra.digest()?, root_id, &aceite.digest()?)?,
            "not_allowed: origem D430 divergente do journal"
        );
        let (pol1, seq_rev1): (Vec<u8>, i64) =
            c.query_row("SELECT policy,grant_seq FROM personal_link_rights WHERE key=? AND revision=1", [t], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        ensure!(pol1 == canon::j(&wire::p0())? && seq_rev1 == seq_admissao, "not_allowed: direitos D430 rev1");
        usados += 3;
        if let Some(rc) = &registro.rc {
            let seq_persistido = seq_persistido.context("not_allowed: PC ausente")?;
            let (tp, pc) = journal(c, seq_persistido)?;
            let pc = canon::verificar(&pc, wire::PERSISTIDO, 12, t)?;
            let campos_pc = pc.payload();
            ensure!(
                tp == "persisted"
                    && campos_pc[8] == registro.admission_id
                    && campos_pc[9] == registro.ra.digest()?
                    && rc.payload()[7] == registro.ra.digest()?
                    && rc.payload()[8] == pc.digest()?
                    && rc.payload()[6] == registro.admission_id,
                "not_allowed: conclusão D430 desvinculada"
            );
            usados += 2;
        }
        if let Some((consentimento, mr)) = &registro.gestao {
            ensure!(registro.rc.is_some(), "not_allowed: gestão antes da conclusão");
            ensure!(
                consentimento.payload()[8] == registro.ra.digest()?
                    && mr.payload()[9] == registro.ra.digest()?
                    && mr.payload()[10] == consentimento.digest()?
                    && mr.payload()[7] == *t,
                "not_allowed: gestão D430 desvinculada"
            );
            let pol2: Vec<u8> =
                c.query_row("SELECT policy FROM personal_link_rights WHERE key=? AND revision=2", [t], |r| r.get(0))?;
            ensure!(pol2 == canon::j(&wire::p1())?, "not_allowed: direitos D430 rev2");
            usados += 2;
        }
    }
    let extras: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM personal_link_rights WHERE revision>2 OR key NOT IN (SELECT key FROM personal_link_roots))
         OR EXISTS(SELECT 1 FROM personal_link_origins WHERE key NOT IN (SELECT key FROM personal_link_roots))
         OR EXISTS(SELECT 1 FROM personal_link_reservations WHERE admission_id NOT IN (SELECT admission_id FROM personal_link_roots))",
        [],
        |r| r.get(0),
    )?;
    ensure!(!extras && usados == total, "not_allowed: registro D430 órfão ou desconhecido");
    Ok(())
}
