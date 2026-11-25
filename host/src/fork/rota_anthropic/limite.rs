//! Teto de gasto da rota como política do cliente: a reserva entra no banco
//! ANTES do envio, cobre chamadas concorrentes e só é trocada pelo uso medido
//! depois da resposta. Uso incerto mantém a reserva cheia e fecha admissões
//! novas do mesmo período — não vira zero nem libera margem por otimismo.
use super::Falha;
use crate::{LockExt, store::Store};
use rusqlite::{Connection, OptionalExtension, params};

/// Orçamento por escopo (`bot:<id>`). Ausente = rota sem teto conferível:
/// recusa, não permissão.
pub(crate) fn migrate(c: &Connection) -> anyhow::Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS donna_rota_anthropic_orcamento(
          escopo TEXT PRIMARY KEY, limite_micros INTEGER NOT NULL CHECK(limite_micros > 0),
          atualizado INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS donna_rota_anthropic_reservas(
          id TEXT PRIMARY KEY, escopo TEXT NOT NULL, periodo TEXT NOT NULL,
          micros INTEGER NOT NULL CHECK(micros > 0),
          estado TEXT NOT NULL CHECK(estado IN ('retida','liquidada','incerta')),
          uso_micros INTEGER, criada INTEGER NOT NULL, encerrada INTEGER,
          CHECK((estado = 'liquidada') = (uso_micros IS NOT NULL)));
         CREATE INDEX IF NOT EXISTS donna_rota_anthropic_reservas_periodo
          ON donna_rota_anthropic_reservas(escopo, periodo, estado);",
    )?;
    Ok(())
}

/// Reserva viva de uma chamada. `micros` é a margem conservadora tomada antes
/// do envio; `id` é a chave da liquidação.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReservaDeGasto {
    pub(crate) id: String,
    pub(crate) micros: i64,
}

fn periodo(agora: i64) -> Result<String, Falha> {
    let instante = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(agora).ok_or(Falha::TetoIndisponivel)?;
    Ok(instante.format("%Y-%m").to_string())
}

fn limite_on(c: &Connection, escopo: &str) -> Result<i64, Falha> {
    c.query_row("SELECT limite_micros FROM donna_rota_anthropic_orcamento WHERE escopo = ?1", [escopo], |r| r.get(0))
        .optional()
        .map_err(|_| Falha::TetoIndisponivel)?
        .ok_or(Falha::SemTeto)
}

/// Soma o que já foi gasto no período: reserva retida e uso incerto contam
/// cheios; liquidação conta o uso medido.
fn gasto_on(c: &Connection, escopo: &str, periodo: &str) -> Result<i64, Falha> {
    c.query_row(
        "SELECT COALESCE(SUM(CASE WHEN estado = 'liquidada' THEN uso_micros ELSE micros END), 0)
           FROM donna_rota_anthropic_reservas WHERE escopo = ?1 AND periodo = ?2",
        params![escopo, periodo],
        |r| r.get(0),
    )
    .map_err(|_| Falha::TetoIndisponivel)
}

fn pendente_on(c: &Connection, escopo: &str, periodo: &str) -> Result<bool, Falha> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM donna_rota_anthropic_reservas
            WHERE escopo = ?1 AND periodo = ?2 AND estado = 'incerta')",
        params![escopo, periodo],
        |r| r.get::<_, bool>(0),
    )
    .map_err(|_| Falha::TetoIndisponivel)
}

/// Define o orçamento do escopo. Não apaga reserva nem uso já registrado.
pub(crate) fn configurar(store: &Store, escopo: &str, limite_micros: i64, agora: i64) -> Result<(), Falha> {
    if limite_micros <= 0 {
        return Err(Falha::TetoIndisponivel);
    }
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_rota_anthropic_orcamento(escopo, limite_micros, atualizado)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(escopo) DO UPDATE SET limite_micros = ?2, atualizado = ?3",
            params![escopo, limite_micros, agora],
        )
        .map_err(|_| Falha::TetoIndisponivel)?;
    Ok(())
}

/// Toma a margem antes do envio. A transação é a fronteira: duas chamadas
/// concorrentes não passam juntas pela última margem, e um uso incerto no
/// período fecha a admissão até ser resolvido.
pub(crate) fn reservar(store: &Store, escopo: &str, micros: i64, agora: i64) -> Result<ReservaDeGasto, Falha> {
    if micros <= 0 {
        return Err(Falha::TetoIndisponivel);
    }
    let periodo = periodo(agora)?;
    let db = store.db.locked();
    let tx = db.unchecked_transaction().map_err(|_| Falha::TetoIndisponivel)?;
    let limite = limite_on(&tx, escopo)?;
    if pendente_on(&tx, escopo, &periodo)? {
        return Err(Falha::UsoIncerto);
    }
    let gasto = gasto_on(&tx, escopo, &periodo)?;
    if gasto.saturating_add(micros) > limite {
        return Err(Falha::TetoEsgotado);
    }
    let id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO donna_rota_anthropic_reservas(id, escopo, periodo, micros, estado, criada)
         VALUES(?1, ?2, ?3, ?4, 'retida', ?5)",
        params![id, escopo, periodo, micros, agora],
    )
    .map_err(|_| Falha::TetoIndisponivel)?;
    tx.commit().map_err(|_| Falha::TetoIndisponivel)?;
    Ok(ReservaDeGasto { id, micros })
}

/// Nada saiu: a reserva é apagada (não é uso, é margem devolvida).
pub(crate) fn liberar(store: &Store, reserva: &ReservaDeGasto) -> Result<(), Falha> {
    store
        .db
        .locked()
        .execute("DELETE FROM donna_rota_anthropic_reservas WHERE id = ?1 AND estado = 'retida'", [&reserva.id])
        .map_err(|_| Falha::TetoIndisponivel)?;
    Ok(())
}

/// A resposta chegou com uso medido: a reserva vira o gasto real.
pub(crate) fn liquidar(store: &Store, reserva: &ReservaDeGasto, uso_micros: i64, agora: i64) -> Result<(), Falha> {
    if uso_micros < 0 {
        return Err(Falha::TetoIndisponivel);
    }
    let mudou = store
        .db
        .locked()
        .execute(
            "UPDATE donna_rota_anthropic_reservas
                SET estado = 'liquidada', uso_micros = ?1, encerrada = ?2
              WHERE id = ?3 AND estado = 'retida'",
            params![uso_micros, agora, reserva.id],
        )
        .map_err(|_| Falha::TetoIndisponivel)?;
    if mudou != 1 {
        return Err(Falha::TetoIndisponivel);
    }
    Ok(())
}

/// O envio pode ter saído e o uso não foi medido: mantém a margem cheia e
/// fecha o período até haver prova. Nunca conta como zero.
pub(crate) fn incerta(store: &Store, reserva: &ReservaDeGasto, agora: i64) -> Result<(), Falha> {
    let mudou = store
        .db
        .locked()
        .execute(
            "UPDATE donna_rota_anthropic_reservas SET estado = 'incerta', encerrada = ?1
              WHERE id = ?2 AND estado = 'retida'",
            params![agora, reserva.id],
        )
        .map_err(|_| Falha::TetoIndisponivel)?;
    if mudou != 1 {
        return Err(Falha::TetoIndisponivel);
    }
    Ok(())
}

/// Resolve uma reserva incerta com o uso provado por fora (recibo do provedor),
/// única forma de reabrir o período.
pub(crate) fn resolver_incerta(store: &Store, id: &str, uso_micros: i64, agora: i64) -> Result<(), Falha> {
    if uso_micros < 0 {
        return Err(Falha::TetoIndisponivel);
    }
    let mudou = store
        .db
        .locked()
        .execute(
            "UPDATE donna_rota_anthropic_reservas
                SET estado = 'liquidada', uso_micros = ?1, encerrada = ?2
              WHERE id = ?3 AND estado = 'incerta'",
            params![uso_micros, agora, id],
        )
        .map_err(|_| Falha::TetoIndisponivel)?;
    if mudou != 1 {
        return Err(Falha::TetoIndisponivel);
    }
    Ok(())
}

/// Confere de novo, perto do efeito, que a reserva ainda cabe: baixar o teto
/// depois da reserva não deixa a chamada passar. Residual declarado: a janela
/// entre esta leitura e a escrita no soquete não é serializada; a cerca de
/// efeito da rota (fora desta fatia) é quem fecha essa janela.
pub(crate) fn revalidar(store: &Store, escopo: &str, agora: i64) -> Result<(), Falha> {
    let periodo = periodo(agora)?;
    let db = store.db.locked();
    let limite = limite_on(&db, escopo)?;
    if gasto_on(&db, escopo, &periodo)? > limite {
        return Err(Falha::TetoEsgotado);
    }
    Ok(())
}

/// Margem restante do período; negativa quando um uso medido passou do teto.
pub(crate) fn disponivel(store: &Store, escopo: &str, agora: i64) -> Result<i64, Falha> {
    let periodo = periodo(agora)?;
    let db = store.db.locked();
    let limite = limite_on(&db, escopo)?;
    Ok(limite.saturating_sub(gasto_on(&db, escopo, &periodo)?))
}
