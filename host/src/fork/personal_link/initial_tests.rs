//! D70 focais do PR-A. Relógio injetado (`cfg(test)`) prova LÓGICA, não tempo físico; SQLite
//! reaberto não é queda de energia. Consumidores reais: escritor, Autoridade, instalação.
use super::{
    canon,
    installation::{self, Aberta},
    procedure::{Procedimento, Sessao},
    test_sync::Evento,
    time::{self, Amostra, Relogio, TimeSource},
    wire,
    writer::{self, Janela},
};
use crate::{
    LockExt,
    fork::autoridade::{self, Autoridade, personal_link::IntencaoInicial},
    hub::Hub,
};
use anyhow::{Context, Result};
use ed25519_dalek::SigningKey;
use rusqlite::TransactionBehavior;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const QUARENTENA: u64 = 86_486_404;

/// Variante de aceite adulterado usada no teste de trocas.
type Caso<'a> = Box<dyn Fn(&Value) -> Result<Value> + 'a>;

struct Fixture {
    dir: PathBuf,
    trava: std::fs::File,
}
impl Fixture {
    fn nova() -> Result<Self> {
        Self::nova_em(&std::env::temp_dir())
    }
    /// Base própria: o socket real exige pais sem escrita alheia (sob `$HOME`, nunca afrouxando a checagem).
    fn nova_em(base: &Path) -> Result<Self> {
        let dir = base.join(format!(".d430-pr-a-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let trava = std::fs::File::create(dir.join("host.lock"))?;
        installation::nascer(&dir, &trava)?;
        Ok(Self { dir, trava })
    }
    /// Mesma reabertura de produção + runtime novo registrado e observado.
    fn subir(&self, qualificado: bool) -> Result<(Arc<Procedimento>, Arc<Mutex<Amostra>>)> {
        let Aberta { store, identidade, k } = installation::reabrir(&self.dir, &self.trava)?;
        let (relogio, amostra) = Relogio::de_teste(qualificado, 1_000);
        let relogio = Arc::new(relogio);
        let runtime = canon::aleatorio::<32>();
        let hub = Hub::new(store, k.instalacao.clone(), identidade, "fixture".into(), 0);
        {
            let mut db = hub.store.db.locked();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            writer::registrar_runtime(&tx, &runtime, &relogio, 1_000)?;
            tx.commit()?;
        }
        if qualificado {
            relogio.iniciar()?;
        }
        let viva = Arc::new(std::sync::atomic::AtomicBool::new(true));
        Ok((
            Arc::new(Procedimento {
                hub,
                k,
                relogio,
                runtime,
                viva,
                sincronia: super::test_sync::Sincronia::default(),
            }),
            amostra,
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Aparelho {
    t: SigningKey,
    c: String,
}
impl Aparelho {
    fn novo() -> Self {
        Self { t: SigningKey::from_bytes(&crate::remote::crypto::random()), c: "c".repeat(64) }
    }
    fn chave(&self) -> String {
        crate::remote::crypto::b64(self.t.verifying_key().as_bytes())
    }
    fn assinar(&self, payload: &Value) -> Result<Value> {
        let sig = crate::remote::crypto::sign(&self.t, &canon::j(payload)?);
        Ok(json!([payload, crate::remote::crypto::b64(&sig)]))
    }
    fn aceite(&self, ch: &Value) -> Result<Value> {
        let d = ch[0].as_array().context("desafio")?;
        self.assinar(&json!([
            wire::ACEITE,
            1,
            d[2],
            d[3],
            d[4],
            d[5],
            d[6],
            d[7],
            d[8],
            d[9],
            canon::hash(ch)?,
            wire::p0()
        ]))
    }
    fn pc(&self, ra: &Value, resposta: &Value) -> Result<Value> {
        let r = ra[0].as_array().context("RA")?;
        self.assinar(&json!([
            wire::PERSISTIDO,
            1,
            r[2],
            r[3],
            r[4],
            resposta[2],
            r[6],
            r[7],
            r[10],
            canon::hash(ra)?,
            resposta[0],
            resposta[1]
        ]))
    }
}

fn pedir(p: &Procedimento, s: &mut Sessao, tipo: &str, args: &[Value]) -> Result<Value> {
    p.tratar(s, tipo, args)
}

fn sair_da_quarentena(a: &Mutex<Amostra>) {
    Relogio::avancar(a, QUARENTENA, 0);
}

/// Primeiro ato completo até RC; devolve (RA, RC).
fn raiz(p: &Procedimento, a: &Aparelho) -> Result<(Value, Value)> {
    let mut s = Sessao::nova();
    let ch = pedir(p, &mut s, "root-challenge", &[json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())])?;
    let ra = pedir(p, &mut s, "root-accept", &[a.aceite(&ch)?])?;
    let admission = ra[0][10].clone();
    let resposta = pedir(p, &mut s, "persist-challenge", &[json!(a.chave()), admission])?;
    let rc = pedir(p, &mut s, "persist-confirm", &[a.pc(&ra, &resposta)?])?;
    Ok((ra, rc))
}

#[test]
fn bordas_inteiras_de_l_e_u() {
    assert_eq!(time::minimo(86_486_403), Some(86_400_000));
    assert_eq!(time::minimo(86_486_404), Some(86_400_001));
    assert_eq!(time::minimo(0), Some(0));
    assert_eq!(time::minimo(2), Some(0));
    assert_eq!(time::maximo(29_967), Some(29_999));
    assert_eq!(time::maximo(29_968), Some(30_000));
    assert_eq!(time::maximo(u64::MAX), None);
    assert_eq!(time::minimo(u64::MAX), None);
}

#[test]
fn fonte_fechada_por_padrao_e_sem_perfil_embutido() {
    assert!(matches!(TimeSource::default(), TimeSource::Unqualified(time::Motivo::SemPerfilEmbutido)));
    let fatos = Relogio::fatos_ficticios(0, 10_000);
    assert!(matches!(
        Relogio::comparar_embutidos(Some(fatos.clone())),
        TimeSource::Unqualified(time::Motivo::SemPerfilEmbutido)
    ));
    assert!(matches!(Relogio::comparar_ficticio(Some(fatos)), TimeSource::Qualified { .. }));
    assert!(matches!(
        Relogio::comparar_ficticio(Some(Relogio::fatos_ficticios(1, 10_000))),
        TimeSource::Unqualified(time::Motivo::FatosDivergentes)
    ));
    assert!(matches!(
        Relogio::comparar_ficticio(Some(Relogio::fatos_ficticios(0, 11_000))),
        TimeSource::Unqualified(time::Motivo::FatosDivergentes)
    ));
    assert!(matches!(Relogio::comparar_ficticio(None), TimeSource::Unqualified(time::Motivo::FatosIndisponiveis)));
    // Produtor real do binário: sem perfil embutido nunca é Qualified.
    assert!(!Relogio::produzir().qualificada());
}

#[test]
fn unqualified_nunca_admite_nem_gera_prazo() -> Result<()> {
    let fixture = Fixture::nova()?;
    let (p, _) = fixture.subir(false)?;
    let a = Aparelho::novo();
    let mut sessao = Sessao::nova();
    let erro =
        pedir(&p, &mut sessao, "root-challenge", &[json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())]);
    assert!(erro.is_err());
    let total: i64 =
        p.hub.store.db.locked().query_row("SELECT count(*) FROM personal_link_journal", [], |r| r.get(0))?;
    assert_eq!(total, 1, "só a marca de nascimento");
    Ok(())
}

#[test]
fn quarentena_vale_para_todo_runtime_inclusive_o_primeiro() -> Result<()> {
    let f = Fixture::nova()?;
    let (p, relogio) = f.subir(true)?;
    let a = Aparelho::novo();
    let args = [json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())];
    Relogio::avancar(&relogio, QUARENTENA - 1, 0);
    assert!(pedir(&p, &mut Sessao::nova(), "root-challenge", &args).is_err());
    Relogio::avancar(&relogio, 1, 0);
    assert!(pedir(&p, &mut Sessao::nova(), "root-challenge", &args).is_ok());
    Ok(())
}

#[test]
fn dois_atos_separados_recibos_byte_exatos_e_reabertura() -> Result<()> {
    let fixture = Fixture::nova()?;
    let a = Aparelho::novo();
    let (ra, rc) = {
        let (p, relogio) = fixture.subir(true)?;
        sair_da_quarentena(&relogio);
        let (ra, rc) = raiz(&p, &a)?;
        // Raiz não concede gestão implícita: cabeça de direitos continua P0/rev1.
        let max: i64 = p.hub.store.db.locked().query_row(
            "SELECT MAX(revision) FROM personal_link_rights WHERE key=?",
            [a.chave()],
            |r| r.get(0),
        )?;
        assert_eq!(max, 1);
        // T D430 nunca autentica pelo canal genérico, mesmo existindo origem.
        assert!(autoridade::autenticar(&p.hub, &a.chave()).is_err());
        (ra, rc)
    };
    // Restart: reabre pelo procedimento protegido; gestão NÃO é admissão (vale em quarentena).
    let (p, _) = fixture.subir(true)?;
    let mut sessao = Sessao::nova();
    let digest = canon::hash(&ra)?;
    let payload = pedir(&p, &mut sessao, "management-challenge", &[json!(a.chave()), json!(digest)])?;
    assert_eq!(payload[10], wire::p1());
    assert_eq!(payload[9], 1);
    let gestao = a.assinar(&payload)?;
    let mr = pedir(&p, &mut sessao, "management-accept", std::slice::from_ref(&gestao))?;
    let reconciliado = pedir(&p, &mut sessao, "reconcile", &[json!(a.chave())])?;
    assert_eq!(reconciliado[0], "managed");
    for (i, original) in [(2, &ra), (3, &rc), (4, &gestao), (5, &mr)] {
        assert_eq!(canon::j(&reconciliado[i])?, canon::j(original)?, "envelope {i} byte-exato");
    }
    assert_eq!(reconciliado[6], canon::hash(&json!([ra, rc, gestao, mr]))?, "chainDigestT sem J duplo");
    drop(p);
    let (p, _) = fixture.subir(false)?;
    let total: i64 =
        p.hub.store.db.locked().query_row("SELECT count(*) FROM personal_link_reservations", [], |r| r.get(0))?;
    assert_eq!(total, 1, "gestão não cobra reserva");
    assert!(!fixture.dir.join("identity.json").exists(), "sem load_or_create");
    Ok(())
}

#[test]
fn trocas_de_h_t_c_nonce_p_e_replay_recusam() -> Result<()> {
    let f = Fixture::nova()?;
    let (p, relogio) = f.subir(true)?;
    sair_da_quarentena(&relogio);
    let a = Aparelho::novo();
    let outro = Aparelho::novo();
    let novo = |p: &Procedimento, s: &mut Sessao| {
        pedir(p, s, "root-challenge", &[json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())])
    };
    let casos: Vec<Caso<'_>> = vec![
        Box::new(|ch| outro.aceite(ch)),
        Box::new(|ch| {
            let mut x = ch.clone();
            x[0][9] = json!(canon::aleatorio::<32>());
            a.aceite(&x)
        }),
        Box::new(|ch| {
            let mut x = ch.clone();
            x[0][4] = json!("d".repeat(64));
            a.aceite(&x)
        }),
        Box::new(|ch| {
            let mut x = ch.clone();
            x[0][2] = json!(outro.chave());
            a.aceite(&x)
        }),
        Box::new(|ch| {
            let d = ch[0].as_array().context("x")?;
            a.assinar(&json!([
                wire::ACEITE,
                1,
                d[2],
                d[3],
                d[4],
                d[5],
                d[6],
                d[7],
                d[8],
                d[9],
                canon::hash(ch)?,
                wire::p1()
            ]))
        }),
    ];
    for (i, caso) in casos.into_iter().enumerate() {
        let mut s = Sessao::nova();
        let ch = novo(&p, &mut s)?;
        assert!(pedir(&p, &mut s, "root-accept", &[caso(&ch)?]).is_err(), "caso {i}");
        // Desafio é one-shot: recusado não volta, nem com aceite correto.
        assert!(pedir(&p, &mut s, "root-accept", &[a.aceite(&ch)?]).is_err(), "replay {i}");
    }
    // Desafio de outra sessão não serve nesta.
    let mut s1 = Sessao::nova();
    let ch = novo(&p, &mut s1)?;
    assert!(pedir(&p, &mut Sessao::nova(), "root-accept", &[a.aceite(&ch)?]).is_err());
    // Desafio vencido (U alcança 30 s) recusa.
    let mut s2 = Sessao::nova();
    let ch = novo(&p, &mut s2)?;
    Relogio::avancar(&relogio, 29_970, 0);
    assert!(pedir(&p, &mut s2, "root-accept", &[a.aceite(&ch)?]).is_err());
    // PC com N_H2 antigo/trocado recusa; com o atual conclui.
    let mut s = Sessao::nova();
    let ch = novo(&p, &mut s)?;
    let ra = pedir(&p, &mut s, "root-accept", &[a.aceite(&ch)?])?;
    let r1 = pedir(&p, &mut s, "persist-challenge", &[json!(a.chave()), ra[0][10].clone()])?;
    let r2 = pedir(&p, &mut s, "persist-challenge", &[json!(a.chave()), ra[0][10].clone()])?;
    assert!(pedir(&p, &mut s, "persist-confirm", &[a.pc(&ra, &r1)?]).is_err(), "N_H2 superado");
    let _ = r2;
    let r3 = pedir(&p, &mut s, "persist-challenge", &[json!(a.chave()), ra[0][10].clone()])?;
    assert!(pedir(&p, &mut s, "persist-confirm", &[outro.pc(&ra, &r3)?]).is_err(), "PC de outro T");
    let r4 = pedir(&p, &mut s, "persist-challenge", &[json!(a.chave()), ra[0][10].clone()])?;
    pedir(&p, &mut s, "persist-confirm", &[a.pc(&ra, &r4)?])?;
    // Segunda raiz nunca nasce.
    assert!(
        pedir(
            &p,
            &mut Sessao::nova(),
            "root-challenge",
            &[json!(outro.chave()), json!(outro.c), json!(canon::aleatorio::<32>())]
        )
        .is_err()
    );
    // Gestão com P trocada recusa.
    let mut s = Sessao::nova();
    let mut payload = pedir(&p, &mut s, "management-challenge", &[json!(a.chave()), json!(canon::hash(&ra)?)])?;
    payload[10] = wire::politica(&["devices.delegate", "devices.manage", "link.connect"], 2);
    assert!(pedir(&p, &mut s, "management-accept", &[a.assinar(&payload)?]).is_err());
    Ok(())
}

#[test]
fn d456_admissao_pendente_fecha_na_reabertura_e_exige_nova_ra() -> Result<()> {
    let fixture = Fixture::nova()?;
    let a = Aparelho::novo();
    let (ra, resposta_antiga) = {
        let (p, relogio) = fixture.subir(true)?;
        sair_da_quarentena(&relogio);
        let mut sessao = Sessao::nova();
        let args = [json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())];
        let ch = pedir(&p, &mut sessao, "root-challenge", &args)?;
        let ra = pedir(&p, &mut sessao, "root-accept", &[a.aceite(&ch)?])?;
        // Desafio de persistência obtido no runtime antigo; o PC nunca chega.
        let resposta = pedir(&p, &mut sessao, "persist-challenge", &[json!(a.chave()), ra[0][10].clone()])?;
        (ra, resposta)
    };
    let pc_antigo = a.pc(&ra, &resposta_antiga)?;
    // Reabertura (restart normal e restore coerente são indistinguíveis aqui), relógio cfg(test) QUALIFICADO e em
    // quarentena: AQUELA RA não conclui por nenhum caminho.
    let (p, relogio) = fixture.subir(true)?;
    let mut sessao = Sessao::nova();
    assert!(pedir(&p, &mut sessao, "persist-challenge", &[json!(a.chave()), ra[0][10].clone()]).is_err());
    assert!(pedir(&p, &mut sessao, "persist-confirm", std::slice::from_ref(&pc_antigo)).is_err());
    let atual = json!([canon::aleatorio::<32>(), wire::prazo(p.relogio.boot_id(), &p.runtime, 1, 30_000), p.runtime]);
    for pc in [pc_antigo.clone(), a.pc(&ra, &atual)?] {
        let pc = canon::verificar(&pc, wire::PERSISTIDO, 12, &a.chave())?;
        let mut db = p.hub.store.db.locked();
        let tx = db.transaction()?;
        assert!(writer::concluir(&tx, &p.hub.identity, &p.k, &pc, &p.runtime).is_err(), "barreira no escritor");
    }
    let reconciliado = pedir(&p, &mut sessao, "reconcile", &[json!(a.chave())])?;
    assert_eq!(reconciliado[0], "closed");
    assert_eq!(canon::j(&reconciliado[2])?, canon::j(&ra)?, "RA original preservada");
    // Refazer localmente: mesma chave nunca (origem imutável); nova chave só após a quarentena atual.
    let b = Aparelho::novo();
    let args_a = [json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())];
    let args_b = [json!(b.chave()), json!(b.c), json!(canon::aleatorio::<32>())];
    assert!(pedir(&p, &mut Sessao::nova(), "root-challenge", &args_b).is_err(), "quarentena vale para a nova RA");
    sair_da_quarentena(&relogio);
    assert!(pedir(&p, &mut Sessao::nova(), "root-challenge", &args_a).is_err(), "origem antiga não é reusada");
    let ch_b = pedir(&p, &mut sessao, "root-challenge", &args_b)?;
    let ra_b = pedir(&p, &mut sessao, "root-accept", &[b.aceite(&ch_b)?])?;
    let (estado, tombstone): (String, i64) = p.hub.store.db.locked().query_row(
        "SELECT r.state,o.tombstone FROM personal_link_roots r JOIN personal_link_origins o ON o.key=r.key WHERE r.key=?",
        [a.chave()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!((estado.as_str(), tombstone), ("closed", 1), "raiz antiga terminal, origem antiga encerrada");
    let reservas: i64 =
        p.hub.store.db.locked().query_row("SELECT count(*) FROM personal_link_reservations", [], |r| r.get(0))?;
    assert_eq!(reservas, 2, "reserva antiga preservada, sem apagar nem criar vaga");
    let resposta_b = pedir(&p, &mut sessao, "persist-challenge", &[json!(b.chave()), ra_b[0][10].clone()])?;
    pedir(&p, &mut sessao, "persist-confirm", &[b.pc(&ra_b, &resposta_b)?])?;
    assert!(pedir(&p, &mut sessao, "persist-challenge", &[json!(a.chave()), ra[0][10].clone()]).is_err());
    drop(p);
    // Reabertura positiva com a raiz encerrada no histórico e a nova completa.
    let (p, _) = fixture.subir(false)?;
    let reconciliado = pedir(&p, &mut Sessao::nova(), "reconcile", &[json!(b.chave())])?;
    assert_eq!(reconciliado[0], "complete");
    Ok(())
}

#[test]
fn autoridade_inicial_revalida_sessao_runtime_e_nao_alcanca_contas() -> Result<()> {
    let f = Fixture::nova()?;
    let (p, _) = f.subir(true)?;
    let sessao = Sessao::nova();
    let ingresso = p.ingresso_para_teste(&sessao);
    let autoridade = Autoridade::capturar_inicial(&p.hub, &ingresso, IntencaoInicial::AceiteRaiz)?;
    assert!(autoridade.exigir_inicial(IntencaoInicial::ConcedeGestao).is_err());
    let contas = crate::fork::engine_setup::access::Authority::Local;
    assert!(autoridade.with_db::<()>(&p.hub, Some(&contas), |_| panic!("contas")).is_err());
    assert!(autoridade.dono().is_none());
    // Runtime novo encerra a captura antiga mesmo com sessão viva.
    {
        let mut db = p.hub.store.db.locked();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        writer::registrar_runtime(&tx, &canon::aleatorio::<32>(), &p.relogio, 2_000)?;
        tx.commit()?;
    }
    assert!(autoridade.with_db::<()>(&p.hub, None, |_| panic!("runtime velho")).is_err());
    drop(sessao);
    assert!(Autoridade::capturar_inicial(&p.hub, &ingresso, IntencaoInicial::AceiteRaiz).is_err());
    Ok(())
}

#[test]
fn ingresso_d430_nao_alcanca_credenciais_setup_nem_cofre() -> Result<()> {
    let f = Fixture::nova()?;
    let (p, relogio) = f.subir(true)?;
    sair_da_quarentena(&relogio);
    for tipo in [
        "credentialList",
        "credentialSet",
        "setAgentEnv",
        "agentSetup",
        "engine.accounts",
        "subscriptions",
        "management.grant",
        "session.open",
        "invitation.create",
        "kit",
    ] {
        assert!(pedir(&p, &mut Sessao::nova(), tipo, &[]).is_err(), "{tipo}");
    }
    assert_eq!(p.hub.store.vault_unlock_attempts.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn reserva_null_atual_conta_e_anterior_sai_no_fim_da_quarentena() {
    let agora = |ms| {
        let (_, a) = Relogio::de_teste(true, ms);
        a.lock().map(|g| *g).expect("amostra")
    };
    let j = |q| Janela { runtime: "R2", boot_id: "b", agora: agora(200_000_000), quarentena_terminada: q };
    assert!(writer::conta("R1", None, None, &j(false)));
    assert!(writer::conta("R1", Some("b"), Some(1), &j(false)));
    assert!(!writer::conta("R1", None, None, &j(true)));
    assert!(writer::conta("R2", None, None, &j(true)), "NULL atual conta");
    assert!(writer::conta("R2", Some("b"), Some(200_000_000 - 86_486_403), &j(true)));
    assert!(!writer::conta("R2", Some("b"), Some(200_000_000 - 86_486_404), &j(true)));
    assert!(writer::conta("R2", Some("outro"), Some(1), &j(true)), "boot divergente conta");
}

#[test]
fn suspensao_sem_limite_fecha_o_runtime() -> Result<()> {
    let (r, a) = Relogio::de_teste(true, 10);
    r.iniciar()?;
    Relogio::avancar(&a, 5, 3);
    assert!(r.amostra().is_err());
    Relogio::avancar(&a, 1, 0);
    assert!(r.amostra().is_err(), "fechado até novo runtime");
    Ok(())
}

#[test]
fn nascimento_exclusivo_e_reabertura_sem_fallback() -> Result<()> {
    let f = Fixture::nova()?;
    assert!(installation::nascer(&f.dir, &f.trava).is_err(), "segundo nascimento");
    assert!(crate::fork::personal_bootstrap::provisioning::open_legacy_store(&f.dir).is_err(), "CLI legado");
    let birth = std::fs::read(f.dir.join("personal-link/birth"))?;
    // Marca ausente: preservado e fechado.
    std::fs::rename(f.dir.join("personal-link/birth"), f.dir.join("birth.bak"))?;
    assert!(installation::reabrir(&f.dir, &f.trava).is_err());
    std::fs::rename(f.dir.join("birth.bak"), f.dir.join("personal-link/birth"))?;
    // Master divergente fecha; nunca regenera.
    let master = f.dir.join("personal-link/master.key");
    let original = std::fs::read(&master)?;
    std::fs::remove_file(&master)?;
    assert!(installation::reabrir(&f.dir, &f.trava).is_err());
    assert!(!master.exists(), "sem regeneração");
    write_private(&master, &[7; 32])?;
    assert!(installation::reabrir(&f.dir, &f.trava).is_err());
    std::fs::remove_file(&master)?;
    write_private(&master, &original)?;
    // Identidade trocada fecha.
    let id = f.dir.join("personal-link/identity");
    let original_id = std::fs::read(&id)?;
    std::fs::remove_file(&id)?;
    assert!(installation::reabrir(&f.dir, &f.trava).is_err());
    write_private(&id, &original_id)?;
    assert!(installation::reabrir(&f.dir, &f.trava).is_ok());
    assert_eq!(std::fs::read(f.dir.join("personal-link/birth"))?, birth);
    // Lacuna no journal fecha (simula corrupção removendo a trava do SQL).
    let (p, relogio) = f.subir(true)?;
    sair_da_quarentena(&relogio);
    raiz(&p, &Aparelho::novo())?;
    drop(p);
    {
        let c = rusqlite::Connection::open(f.dir.join("codync.db"))?;
        c.execute_batch("DROP TRIGGER personal_link_journal_keep; DELETE FROM personal_link_journal WHERE seq=3;")?;
    }
    assert!(installation::reabrir(&f.dir, &f.trava).is_err());
    Ok(())
}

#[test]
fn conflito_pl1_na_mesma_tx_recusa_admissao() -> Result<()> {
    let fixture = Fixture::nova()?;
    let (p, relogio) = fixture.subir(true)?;
    sair_da_quarentena(&relogio);
    p.hub.store.db.locked().execute("INSERT INTO personal_local_birth(singleton,schema) VALUES(1,1)", [])?;
    let a = Aparelho::novo();
    let mut sessao = Sessao::nova();
    let ch =
        pedir(&p, &mut sessao, "root-challenge", &[json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())])?;
    assert!(pedir(&p, &mut sessao, "root-accept", &[a.aceite(&ch)?]).is_err());
    let total: i64 = p.hub.store.db.locked().query_row("SELECT count(*) FROM personal_link_roots", [], |r| r.get(0))?;
    assert_eq!(total, 0);
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    f.write_all(bytes)?;
    Ok(())
}

/// Bytes do banco e presença dos artefatos SQLite ao lado (WAL/SHM).
fn artefatos(dir: &Path) -> Result<(Vec<u8>, bool, bool)> {
    Ok((std::fs::read(dir.join("codync.db"))?, dir.join("codync.db-wal").exists(), dir.join("codync.db-shm").exists()))
}

#[test]
fn seg_a_r1_01_reabertura_recusa_db_incompleto_ou_alheio_sem_alterar_bytes() -> Result<()> {
    // birth/identidade/master externos válidos; só o banco é trocado.
    let alvo = Fixture::nova()?;
    let outra = Fixture::nova()?;
    let db = alvo.dir.join("codync.db");
    let proprio = std::fs::read(&db)?;
    {
        let c = rusqlite::Connection::open(&db)?;
        c.execute_batch("DROP TABLE personal_link_rights;")?;
    }
    let antes = artefatos(&alvo.dir)?;
    assert!(installation::reabrir(&alvo.dir, &alvo.trava).is_err(), "tabela D430 faltante recusa");
    assert_eq!(artefatos(&alvo.dir)?, antes, "tabela faltante: sem criação/migração/reparo");
    std::fs::write(&db, &proprio)?;
    std::fs::copy(outra.dir.join("codync.db"), &db)?;
    let antes = artefatos(&alvo.dir)?;
    assert!(installation::reabrir(&alvo.dir, &alvo.trava).is_err(), "banco de outra instalação recusa");
    assert_eq!(artefatos(&alvo.dir)?, antes, "banco alheio: bytes e artefatos intactos");
    Ok(())
}

fn linhas_journal(p: &Procedimento) -> Result<i64> {
    Ok(p.hub.store.db.locked().query_row("SELECT count(*) FROM personal_link_journal", [], |r| r.get(0))?)
}

type Vistos = Arc<Mutex<Vec<(i64, i64)>>>;

/// Observa, no `Store::from_connection` real, `synchronous` e o total de objetos logo antes da 1ª DDL.
fn observar_inicializador(vistos: &Vistos) {
    let vistos = vistos.clone();
    crate::store::personal_database::observador::observar(move |c| {
        let sincronia: i64 = c.pragma_query_value(None, "synchronous", |r| r.get(0)).unwrap_or(-1);
        let objetos: i64 = c.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0)).unwrap_or(-1);
        vistos.lock().expect("observador").push((sincronia, objetos));
    });
}

#[test]
fn seg_a_r1_01_full_antes_da_primeira_ddl_no_nascimento_e_na_reabertura() -> Result<()> {
    let vistos: Vistos = Arc::default();
    observar_inicializador(&vistos);
    let fixture = Fixture::nova()?;
    let objetos: i64 = rusqlite::Connection::open(fixture.dir.join("codync.db"))?.query_row(
        "SELECT count(*) FROM sqlite_master",
        [],
        |r| r.get(0),
    )?;
    observar_inicializador(&vistos);
    let _ = fixture.subir(false)?;
    observar_inicializador(&vistos);
    let _legado = crate::store::Store::open(Path::new(":memory:"))?;
    let vistos = vistos.lock().map_err(|_| anyhow::anyhow!("observador"))?.clone();
    assert_eq!(vistos.len(), 3, "um ponto por inicialização");
    assert_eq!(vistos[0], (2, 0), "nascimento: FULL e banco ainda sem nenhuma DDL");
    assert_eq!(vistos[1], (2, objetos), "reabertura: FULL antes de qualquer DDL nova");
    assert_eq!(vistos[2].0, 1, "perfil legado conserva NORMAL");
    Ok(())
}

async fn pedir_socket(stream: &mut tokio::net::UnixStream, tipo: &str, args: &[Value]) -> Result<Value> {
    let id = canon::aleatorio::<16>();
    let mut frame = vec![json!(super::procedure::PROTOCOLO), json!(1), json!(id), json!(tipo)];
    frame.extend_from_slice(args);
    super::procedure::escrever_frame(stream, &Value::Array(frame)).await?;
    super::procedure::ler_frame(stream).await
}

struct Eventos(tokio::sync::mpsc::UnboundedReceiver<Evento>);
impl Eventos {
    async fn receber(&mut self) -> Result<Evento> {
        // Prazo só como watchdog de falha; nenhuma conclusão depende de tempo decorrido.
        tokio::time::timeout(std::time::Duration::from_secs(30), self.0.recv()).await?.context("observador fechado")
    }

    async fn esperar(&mut self, esperado: Evento) -> Result<()> {
        assert_eq!(self.receber().await.with_context(|| format!("esperado {esperado:?}"))?, esperado);
        Ok(())
    }

    async fn parada_observada(&mut self) -> Result<()> {
        // Os dois níveis de drain precisam estar realmente pendentes, com o worker ainda preso.
        // Servidor e conexão rodam em tarefas distintas; a ordem entre eles pode variar.
        let mut esperados = vec![Evento::DrainPendente, Evento::SessaoInvalidada(false), Evento::WorkerDrainPendente];
        while !esperados.is_empty() {
            let recebido = self.receber().await?;
            let Some(i) = esperados.iter().position(|e| *e == recebido) else {
                panic!("parada/drain: evento inesperado {recebido:?}");
            };
            esperados.swap_remove(i);
        }
        Ok(())
    }
}

type ServidorSocket =
    (tokio::net::UnixStream, tokio::sync::oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>, Eventos);

async fn subir_socket(f: &Fixture, p: &Arc<Procedimento>) -> Result<ServidorSocket> {
    use std::{future::Future, sync::atomic::Ordering};
    let trava = std::fs::File::open(f.dir.join("host.lock"))?;
    assert!(crate::lock_file(&trava, std::time::Duration::ZERO)?, "fixture detém host.lock");
    let mut eventos = Eventos(p.sincronia.observar());
    let (pronto_tx, pronto_rx) = tokio::sync::oneshot::channel();
    let (parar_tx, parar_rx) = tokio::sync::oneshot::channel::<()>();
    let servidor = tokio::spawn({
        let (p, dir) = (p.clone(), f.dir.clone());
        async move {
            let servir = installation::servir_socket(&dir, &p, async {
                // O futuro de parada só é consultado depois de o listener real estar pronto.
                let _ = pronto_tx.send(());
                let _ = parar_rx.await;
            });
            tokio::pin!(servir);
            let mut drain_observado = false;
            let r = std::future::poll_fn(|cx| {
                let estado = servir.as_mut().poll(cx);
                if estado.is_pending() && !p.viva.load(Ordering::Acquire) && !drain_observado {
                    // Observa Poll::Pending do servidor REAL após a parada. Sem drain, ele retorna Ready
                    // e emite ServidorConcluido: o teste falha mesmo se o worker ainda estiver bloqueado.
                    drain_observado = true;
                    p.sincronia.emitir(Evento::DrainPendente);
                }
                if estado.is_ready() {
                    p.sincronia.emitir(Evento::ServidorConcluido);
                }
                estado
            })
            .await;
            drop(trava);
            r
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(30), pronto_rx).await??;
    let cliente = tokio::net::UnixStream::connect(f.dir.join("personal-link/procedure.sock")).await?;
    eventos.esperar(Evento::ProntaParaPedido).await?;
    Ok((cliente, parar_tx, servidor, eventos))
}

/// Socket REAL da instância. O trabalho é bloqueado no próprio caminho consumidor (o relógio `cfg(test)` que
/// `aceite_raiz` consulta antes do efeito). Hooks `cfg(test)` só observam etapas; host.lock é um flock real.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(
    clippy::await_holding_lock,
    reason = "D70 SEG-A-R1-02: segura de propósito o relógio cfg(test) nos awaits para bloquear o worker real"
)]
async fn seg_a_r1_02_canal_fechado_ou_parada_nao_gravam_e_trava_so_solta_apos_drain() -> Result<()> {
    let home = std::env::var_os("HOME").context("HOME")?;
    for parada in [false, true] {
        let f = Fixture::nova_em(Path::new(&home))?;
        let (p, relogio) = f.subir(true)?;
        sair_da_quarentena(&relogio);
        let (mut cliente, parar_tx, servidor, mut eventos) = subir_socket(&f, &p).await?;
        let a = Aparelho::novo();
        let args = [json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())];
        let ch = pedir_socket(&mut cliente, "root-challenge", &args).await?;
        assert_eq!(ch[3], "ok");
        // A resposta pode sair pelo escritor antes de vigiar observar o fim do worker do desafio.
        // Só enviamos o aceite quando a sessão real está pronta para ler o próximo pedido.
        eventos.esperar(Evento::ProntaParaPedido).await?;
        let antes = linhas_journal(&p)?;
        let bloqueio = relogio.lock().map_err(|_| anyhow::anyhow!("relógio"))?;
        let id = canon::aleatorio::<16>();
        let aceite = a.aceite(&ch[5])?;
        let frame = vec![json!(super::procedure::PROTOCOLO), json!(1), json!(id), json!("root-accept"), aceite];
        super::procedure::escrever_frame(&mut cliente, &Value::Array(frame)).await?;
        eventos.esperar(Evento::CapturaIniciada).await?;
        eventos.esperar(Evento::CapturaConcluida).await?;
        eventos.esperar(Evento::TrabalhoIniciado).await?;
        if parada {
            let _ = parar_tx.send(());
            // O servidor foi efetivamente sondado até Pending com o trabalho ainda preso.
            eventos.parada_observada().await?;
            let outra = std::fs::File::open(f.dir.join("host.lock"))?;
            assert!(!crate::lock_file(&outra, std::time::Duration::ZERO)?, "host.lock retido enquanto há worker");
            assert!(!servidor.is_finished(), "servir_socket aguarda o drain");
            drop(bloqueio);
            eventos.esperar(Evento::TrabalhoConcluido).await?;
            servidor.await??;
            let outra = std::fs::File::open(f.dir.join("host.lock"))?;
            assert!(crate::lock_file(&outra, std::time::Duration::ZERO)?, "host.lock livre só após o drain");
            drop(cliente);
        } else {
            drop(cliente);
            eventos.esperar(Evento::SessaoInvalidada(false)).await?;
            eventos.esperar(Evento::WorkerDrainPendente).await?;
            drop(bloqueio);
            // Até um worker indevidamente destacado precisa terminar antes de lermos os efeitos.
            eventos.esperar(Evento::TrabalhoConcluido).await?;
            // Parada + drain ANTES das contagens: todo worker terminou quando elas são lidas.
            let _ = parar_tx.send(());
            servidor.await??;
        }
        let raizes: i64 =
            p.hub.store.db.locked().query_row("SELECT count(*) FROM personal_link_roots", [], |r| r.get(0))?;
        assert_eq!(raizes, 0, "parada={parada}: nenhum efeito depois da invalidação");
        assert_eq!(linhas_journal(&p)?, antes, "parada={parada}: journal intacto");
    }
    Ok(())
}

/// R1-02 (captura): a captura original roda antes da fila de negócio e é VIGIADA. Com a Connection presa durante a
/// captura, fechar o canal desliga a sessão antes de a captura terminar; ao soltar o banco nada é gravado.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "D70 SEG-A-R1-02: segura de propósito a Connection nos awaits para bloquear a captura real"
)]
async fn seg_a_r1_02_canal_fechado_durante_captura_bloqueada_nao_grava() -> Result<()> {
    let home = std::env::var_os("HOME").context("HOME")?;
    let f = Fixture::nova_em(Path::new(&home))?;
    let (p, relogio) = f.subir(true)?;
    sair_da_quarentena(&relogio);
    let (mut cliente, parar_tx, servidor, mut eventos) = subir_socket(&f, &p).await?;
    let a = Aparelho::novo();
    let args = [json!(a.chave()), json!(a.c), json!(canon::aleatorio::<32>())];
    let ch = pedir_socket(&mut cliente, "root-challenge", &args).await?;
    assert_eq!(ch[3], "ok");
    eventos.esperar(Evento::ProntaParaPedido).await?;
    let antes = linhas_journal(&p)?;
    let banco = p.hub.store.db.locked();
    let id = canon::aleatorio::<16>();
    let aceite = a.aceite(&ch[5])?;
    let frame = vec![json!(super::procedure::PROTOCOLO), json!(1), json!(id), json!("root-accept"), aceite];
    super::procedure::escrever_frame(&mut cliente, &Value::Array(frame)).await?;
    eventos.esperar(Evento::CapturaIniciada).await?;
    drop(cliente);
    eventos.esperar(Evento::SessaoInvalidada(false)).await?;
    eventos.esperar(Evento::WorkerDrainPendente).await?;
    drop(banco);
    eventos.esperar(Evento::CapturaConcluida).await?;
    let _ = parar_tx.send(());
    servidor.await??;
    let raizes: i64 =
        p.hub.store.db.locked().query_row("SELECT count(*) FROM personal_link_roots", [], |r| r.get(0))?;
    assert_eq!(raizes, 0, "canal fechado durante a captura: nenhum efeito");
    assert_eq!(linhas_journal(&p)?, antes, "journal intacto");
    Ok(())
}

#[path = "channel_tests.rs"]
mod channel_tests;
