use super::Falha;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};

const LIMITE: usize = 3;
const JANELA: Duration = Duration::from_hours(24);

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub(super) enum Space<'a> {
    Pessoal(&'a str),
    Compartilhado,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub(super) struct Vinculo<'a> {
    pub principal: &'a str,
    pub emissor: &'a str,
    pub conta: &'a str,
    pub cliente: &'a str,
    pub dispositivo: &'a str,
    pub space: Space<'a>,
    pub conversa: &'a str,
    pub geracao: u64,
    pub hub_local_do_titular: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registro {
    epoca: u64,
    consentimento: Option<Consentimento>,
    ultima_reserva: u64,
    reservas: Vec<u64>,
    pausado_por_cota: bool,
    em_voo: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Consentimento {
    versao: u8,
    vinculo: serde_json::Value,
}

pub(super) struct Admissao<'a> {
    db: Connection,
    _trava: File,
    ativa: Option<(Vinculo<'a>, u64)>,
    falhou: bool,
    primeira_leitura: BTreeMap<u64, Instant>,
    ultimo_instante: Option<Instant>,
}

impl<'a> Admissao<'a> {
    pub(super) fn abrir(path: &Path, criar: bool) -> Result<Self, Falha> {
        let trava = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .map_err(|_| Falha::Registro)?;
        fs2::FileExt::try_lock_exclusive(&trava).map_err(|_| Falha::Registro)?;
        if criar {
            OpenOptions::new().write(true).create_new(true).open(path).map_err(|_| Falha::Registro)?;
        }
        let mut db =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|_| Falha::Registro)?;
        db.pragma_update(None, "synchronous", "FULL").map_err(|_| Falha::Registro)?;
        db.pragma_update(None, "fullfsync", true).map_err(|_| Falha::Registro)?;
        if criar {
            db.pragma_update(None, "journal_mode", "WAL").map_err(|_| Falha::Registro)?;
            let tx = Self::transacao(&mut db)?;
            tx.execute_batch("CREATE TABLE admissao (id INTEGER PRIMARY KEY CHECK (id = 1), estado TEXT NOT NULL)")
                .map_err(|_| Falha::Registro)?;
            let estado = serde_json::to_string(&Registro::default()).map_err(|_| Falha::Registro)?;
            tx.execute("INSERT INTO admissao (id, estado) VALUES (1, ?1)", [estado]).map_err(|_| Falha::Registro)?;
            tx.commit().map_err(|_| Falha::Registro)?;
        }
        Self::ler(&db)?;
        Ok(Self {
            db,
            _trava: trava,
            ativa: None,
            falhou: false,
            primeira_leitura: BTreeMap::new(),
            ultimo_instante: None,
        })
    }

    fn transacao(db: &mut Connection) -> Result<Transaction<'_>, Falha> {
        let sync: i64 = db.pragma_query_value(None, "synchronous", |row| row.get(0)).map_err(|_| Falha::Registro)?;
        let journal: String =
            db.pragma_query_value(None, "journal_mode", |row| row.get(0)).map_err(|_| Falha::Registro)?;
        if sync != 2 || journal != "wal" {
            return Err(Falha::Registro);
        }
        db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|_| Falha::Registro)
    }

    fn ler(db: &Connection) -> Result<Registro, Falha> {
        let estado: String =
            db.query_row("SELECT estado FROM admissao WHERE id = 1", [], |r| r.get(0)).map_err(|_| Falha::Registro)?;
        let registro: Registro = serde_json::from_str(&estado).map_err(|_| Falha::Registro)?;
        if registro.reservas.len() > LIMITE
            || registro.reservas.iter().any(|&id| id == 0 || id > registro.ultima_reserva)
            || !registro.reservas.windows(2).all(|ids| ids[0] < ids[1])
            || registro.consentimento.as_ref().is_some_and(|c| c.versao != 1 || registro.epoca == 0)
            || (registro.em_voo && (registro.reservas.is_empty() || registro.consentimento.is_none()))
        {
            return Err(Falha::Registro);
        }
        Ok(registro)
    }

    fn gravar(tx: Transaction<'_>, registro: &Registro) -> Result<(), Falha> {
        let estado = serde_json::to_string(registro).map_err(|_| Falha::Registro)?;
        if tx.execute("UPDATE admissao SET estado = ?1 WHERE id = 1", params![estado]).map_err(|_| Falha::Registro)?
            != 1
        {
            return Err(Falha::Registro);
        }
        tx.commit().map_err(|_| Falha::Registro)
    }

    pub(super) fn armar(&mut self, vinculo: Vinculo<'a>) -> Result<(), Falha> {
        self.ativa = None;
        if self.falhou {
            return Err(Falha::Registro);
        }
        let resultado = self.armar_verificado(vinculo);
        self.falhou = resultado == Err(Falha::Registro);
        resultado
    }

    fn armar_verificado(&mut self, vinculo: Vinculo<'a>) -> Result<(), Falha> {
        let Space::Pessoal(space) = vinculo.space else { return Err(Falha::Recusado) };
        if !vinculo.hub_local_do_titular
            || vinculo.geracao == 0
            || [
                vinculo.principal,
                vinculo.emissor,
                vinculo.conta,
                vinculo.cliente,
                vinculo.dispositivo,
                vinculo.conversa,
                space,
            ]
            .iter()
            .any(|id| id.is_empty() || id.chars().any(char::is_whitespace))
        {
            return Err(Falha::Recusado);
        }
        let tx = Self::transacao(&mut self.db)?;
        let mut registro = Self::ler(&tx)?;
        if registro.pausado_por_cota {
            return Err(Falha::Cota);
        }
        if registro.em_voo {
            return Err(Falha::Registro);
        }
        registro.epoca = registro.epoca.checked_add(1).ok_or(Falha::Registro)?;
        registro.consentimento =
            Some(Consentimento { versao: 1, vinculo: serde_json::to_value(vinculo).map_err(|_| Falha::Registro)? });
        Self::gravar(tx, &registro)?;
        self.ativa = Some((vinculo, registro.epoca));
        Ok(())
    }

    pub(super) fn sessao(&self, origem: Vinculo<'_>) -> Result<u64, Falha> {
        self.ativa.filter(|(vinculo, _)| *vinculo == origem).map(|(_, epoca)| epoca).ok_or(Falha::Recusado)
    }

    pub(super) fn desligar(&mut self) -> Result<(), Falha> {
        self.invalidar(false)
    }

    pub(super) fn pausar_por_cota(&mut self) -> Result<(), Falha> {
        self.invalidar(true)
    }

    fn invalidar(&mut self, cota: bool) -> Result<(), Falha> {
        self.ativa = None;
        let resultado = self.gravar_invalidacao(cota);
        self.falhou |= resultado.is_err();
        resultado
    }

    fn gravar_invalidacao(&mut self, cota: bool) -> Result<(), Falha> {
        let tx = Self::transacao(&mut self.db)?;
        let mut registro = Self::ler(&tx)?;
        registro.epoca = registro.epoca.checked_add(1).ok_or(Falha::Registro)?;
        registro.consentimento = None;
        registro.pausado_por_cota |= cota || registro.em_voo;
        registro.em_voo = false;
        Self::gravar(tx, &registro)
    }

    pub(super) fn reservar(&mut self, epoca: u64, agora: Instant) -> Result<(), Falha> {
        let vinculo = self.ativa.filter(|(_, atual)| *atual == epoca).map(|(v, _)| v).ok_or(Falha::Recusado)?;
        let resultado = self.reservar_ativa(vinculo, epoca, agora);
        if resultado == Err(Falha::Registro) {
            self.ativa = None;
            self.falhou = true;
        }
        resultado
    }

    fn reservar_ativa(&mut self, vinculo: Vinculo<'_>, epoca: u64, agora: Instant) -> Result<(), Falha> {
        if self.ultimo_instante.is_some_and(|anterior| agora < anterior) {
            return Err(Falha::Registro);
        }
        self.ultimo_instante = Some(agora);
        let tx = Self::transacao(&mut self.db)?;
        let mut registro = Self::ler(&tx)?;
        let esperado = serde_json::to_value(vinculo).map_err(|_| Falha::Registro)?;
        if registro.epoca != epoca || !registro.consentimento.as_ref().is_some_and(|c| c.vinculo == esperado) {
            return Err(Falha::Registro);
        }
        if registro.em_voo || registro.pausado_por_cota {
            return Err(Falha::Registro);
        }
        self.primeira_leitura.retain(|id, _| registro.reservas.contains(id));
        registro
            .reservas
            .retain(|id| agora.duration_since(*self.primeira_leitura.entry(*id).or_insert(agora)) < JANELA);
        if registro.reservas.len() == LIMITE {
            return Err(Falha::Cota);
        }
        registro.ultima_reserva = registro.ultima_reserva.checked_add(1).ok_or(Falha::Registro)?;
        registro.reservas.push(registro.ultima_reserva);
        registro.em_voo = true;
        Self::gravar(tx, &registro)?;
        self.primeira_leitura.insert(registro.ultima_reserva, agora);
        Ok(())
    }

    pub(super) fn terminar(&mut self, epoca: u64) -> Result<(), Falha> {
        let resultado = (|| {
            let tx = Self::transacao(&mut self.db)?;
            let mut registro = Self::ler(&tx)?;
            if registro.epoca != epoca || !registro.em_voo {
                return Err(Falha::Registro);
            }
            registro.em_voo = false;
            Self::gravar(tx, &registro)
        })();
        if resultado.is_err() {
            self.ativa = None;
            self.falhou = true;
        }
        resultado
    }
}
