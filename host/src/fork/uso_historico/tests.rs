//! MX-260: limites de amostragem, retenção, agrupamento, previsão e o RPC do dono.
use super::*;
use crate::remote::identity::Identity;
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};

fn window(id: &str, percent: f64, resets_at: Option<i64>) -> Window {
    Window { id: id.into(), label: id.into(), percent, resets_at, resets_text: None, minutes: None }
}

fn store() -> Store {
    Store::open(Path::new(":memory:")).expect("operação da fixture")
}

fn hub(store: Store, dir: &Path) -> Arc<Hub> {
    Hub::new(store, "uso-historico".into(), Identity::load_or_create(dir).expect("operação da fixture"), "t".into(), 0)
}

fn dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uso-historico-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("operação da fixture");
    dir
}

fn conta(store: &Store, provider: &str, window: &str) -> i64 {
    store
        .db
        .locked()
        .query_row(
            "SELECT COUNT(*) FROM donna_usage_samples WHERE provider=?1 AND window_id=?2",
            params![provider, window],
            |r| r.get(0),
        )
        .expect("operação da fixture")
}

/// (`slot`, `at`, `percent`, `resets_at`) da linha única esperada.
fn amostra(store: &Store, provider: &str, window: &str) -> Option<(i64, i64, f64, Option<i64>)> {
    store
        .db
        .locked()
        .query_row(
            "SELECT slot,at,percent,resets_at FROM donna_usage_samples WHERE provider=?1 AND window_id=?2",
            params![provider, window],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .expect("operação da fixture")
}

/// Insere amostra direta: `slot` = instante que abriu a vaga, `at` = observação.
fn insere(store: &Store, provider: &str, window: &str, slot: i64, at: i64, percent: f64, resets_at: Option<i64>) {
    store
        .db
        .locked()
        .execute(
            "INSERT INTO donna_usage_samples(provider,window_id,slot,at,percent,resets_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![provider, window, slot, at, percent, resets_at],
        )
        .expect("operação da fixture");
}

const HORA: i64 = 60 * 60 * 1000;
const BASE: i64 = 1_800_000_000_000;
const DIA: i64 = 24 * HORA;

#[test]
fn no_maximo_uma_amostra_a_cada_dez_minutos_por_provedor_e_janela() {
    let s = store();
    let w = || vec![window("five_hour", 10.0, Some(BASE + HORA))];
    record_at(&s, "claude", &w(), BASE).expect("gravar");
    // Merges repetidos dentro da vaga de 10 min (barra de status) não gravam.
    for delta in [1, 60_000, INTERVALO_MS - 1] {
        record_at(&s, "claude", &w(), BASE + delta).expect("gravar");
    }
    assert_eq!(conta(&s, "claude", "five_hour"), 1);
    // Dedupe em memória não cega a garantia do banco: linha direta recente também trava.
    insere(&s, "codex", "primary", BASE, BASE, 5.0, None);
    record_at(&s, "codex", &[window("primary", 6.0, None)], BASE + 1).expect("gravar");
    assert_eq!(conta(&s, "codex", "primary"), 1);
    // Depois de 10 min da ABERTURA da vaga grava de novo; janela e provedor diferentes são independentes.
    record_at(&s, "claude", &w(), BASE + INTERVALO_MS + 1).expect("gravar");
    record_at(&s, "claude", &[window("seven_day", 40.0, None)], BASE + 1).expect("gravar");
    record_at(&s, "codex", &w(), BASE + 1).expect("gravar");
    assert_eq!(conta(&s, "claude", "five_hour"), 2);
    assert_eq!(conta(&s, "claude", "seven_day"), 1);
    assert_eq!(conta(&s, "codex", "five_hour"), 1);
}

#[test]
fn leitura_dentro_da_vaga_atualiza_a_observacao_e_conserva_a_vaga_e_o_reset() {
    let s = store();
    // 12:09 abre a vaga (slot); 12:11 atualiza a MESMA linha com at=12:11.
    record_at(&s, "claude", &[window("five_hour", 10.0, None)], BASE).expect("gravar");
    let reset = Some(BASE + 5 * HORA);
    record_at(&s, "claude", &[window("five_hour", 12.0, reset)], BASE + 120_000).expect("gravar");
    assert_eq!(conta(&s, "claude", "five_hour"), 1, "≤1 amostra por vaga");
    assert_eq!(amostra(&s, "claude", "five_hour"), Some((BASE, BASE + 120_000, 12.0, reset)));
    // Leitura sem reset não apaga o conhecido; slot conserva a abertura da vaga.
    record_at(&s, "claude", &[window("five_hour", 13.0, None)], BASE + 180_000).expect("gravar");
    assert_eq!(amostra(&s, "claude", "five_hour"), Some((BASE, BASE + 180_000, 13.0, reset)));
    // Alternância repetida CLI/statusline mantém a garantia do banco (linha direta também).
    insere(&s, "codex", "primary", BASE, BASE, 5.0, Some(BASE + HORA));
    record_at(&s, "codex", &[window("primary", 7.0, None)], BASE + 1).expect("gravar");
    assert_eq!(amostra(&s, "codex", "primary"), Some((BASE, BASE + 1, 7.0, Some(BASE + HORA))));
}

#[test]
fn observacao_que_trouxe_100_marca_o_bloco_e_o_exhaustsat() {
    let s = store();
    // 12:09 grava 99%; 12:11 (mesma vaga) traz os primeiros 100%: a observação vale.
    let b10 = 10 * 60 * 1000;
    let t_1209 = BASE - BASE.rem_euclid(b10); // borda de bloco qualquer
    record_at(&s, "claude", &[window("five_hour", 99.0, None)], t_1209).expect("gravar");
    record_at(&s, "claude", &[window("five_hour", 100.0, Some(t_1209 + 4 * HORA))], t_1209 + 120_000).expect("gravar");
    assert_eq!(amostra(&s, "claude", "five_hour"), Some((t_1209, t_1209 + 120_000, 100.0, Some(t_1209 + 4 * HORA))));
    let now = t_1209 + 120_000;
    let out = report(&s.db.locked(), Faixa::H24, now).expect("relatório");
    let buckets = out["windows"][0]["buckets"].as_array().expect("buckets");
    // A amostra aparece no bloco da OBSERVAÇÃO (12:11), não no da abertura (12:09).
    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0]["start"], serde_json::json!((t_1209 + 120_000).div_euclid(b10) * b10));
    // exhaustsAt = instante da observação que trouxe 100, nunca o da abertura da vaga.
    assert_eq!(out["windows"][0]["forecast"], serde_json::json!({"exhausted":true,"exhaustsAt":t_1209 + 120_000}));
}

#[test]
fn previsao_funciona_com_fontes_alternando_resets_at() {
    let s = store();
    // Quatro vagas; a primeira leitura de cada vaga é da CLI (None) e a statusline completa o reset.
    for (i, percent) in [40.0, 50.0, 60.0, 70.0].into_iter().enumerate() {
        let t = BASE + (INTERVALO_MS + 1) * i64::try_from(i).expect("fixture");
        record_at(&s, "claude", &[window("five_hour", percent, None)], t).expect("gravar");
        record_at(&s, "claude", &[window("five_hour", percent, Some(BASE + 10 * HORA))], t + 1).expect("gravar");
    }
    let amostras = {
        let db = s.db.locked();
        db.prepare("SELECT at,percent,resets_at FROM donna_usage_samples ORDER BY at")
            .expect("fixture")
            .query_map([], |r| Ok(Amostra { at: r.get(0)?, percent: r.get(1)?, resets_at: r.get(2)? }))
            .expect("fixture")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("fixture")
    };
    assert_eq!(amostras.len(), 4);
    assert!(amostras.iter().all(|a| a.resets_at == Some(BASE + 10 * HORA)));
    let now = BASE + 4 * INTERVALO_MS;
    // Reta 40→70 cobrindo ~30min com resíduo 0 e resets futuro → estimativa coerente.
    assert!(matches!(prever(&amostras, now), Previsao::Estimada { .. }));
}

#[test]
fn janela_sem_percent_finito_nao_grava() {
    let s = store();
    record_at(&s, "claude", &[window("five_hour", f64::NAN, None), window("seven_day", f64::INFINITY, None)], BASE)
        .expect("gravar");
    assert_eq!(conta(&s, "claude", "five_hour"), 0);
    assert_eq!(conta(&s, "claude", "seven_day"), 0);
}

#[test]
fn limpeza_apaga_em_lote_amostras_com_mais_de_31_dias() {
    let s = store();
    // Mais antigas que 31 dias em quantidade maior que o lote → 2 varreduras.
    for i in 0..(LIMPEZA_LOTE + 10) {
        let at = BASE - RETENCAO_MS - 10_000 + i;
        insere(&s, "claude", "five_hour", at, at, 90.0, None);
    }
    insere(&s, "claude", "five_hour", BASE - RETENCAO_MS / 2, BASE - RETENCAO_MS / 2, 10.0, None);
    record_at(&s, "claude", &[window("seven_day", 1.0, None)], BASE).expect("gravar");
    assert_eq!(conta(&s, "claude", "five_hour"), 11, "lote apaga no máximo LIMPEZA_LOTE");
    // A segunda gravação na mesma hora não repete a varredura (cache marcado pós-commit).
    assert!(s.uso_historico.locked().limpeza.is_some());
    s.uso_historico.locked().limpeza = Some(BASE - LIMPEZA_CADA_MS - 1);
    record_at(&s, "claude", &[window("seven_day", 2.0, None)], BASE + INTERVALO_MS + 1).expect("gravar");
    assert_eq!(conta(&s, "claude", "five_hour"), 1);
}

#[test]
fn rollback_pos_insert_nao_marca_cache_e_permite_nova_tentativa() {
    let s = store();
    // Linha antiga faz o DELETE da limpeza disparar o trigger (falha DEPOIS do
    // INSERT, dentro da mesma transação) → tx reverte a amostra também.
    insere(&s, "claude", "old", BASE - RETENCAO_MS - 1, BASE - RETENCAO_MS - 1, 90.0, None);
    s.db.locked()
        .execute_batch(
            "CREATE TRIGGER falha_limpeza BEFORE DELETE ON donna_usage_samples
             BEGIN SELECT RAISE(ABORT,'falha injetada'); END;",
        )
        .expect("fixture");
    assert!(record_at(&s, "claude", &[window("five_hour", 10.0, None)], BASE).is_err());
    assert_eq!(conta(&s, "claude", "five_hour"), 0, "insert revertido junto");
    {
        let cache = s.uso_historico.locked();
        assert!(cache.ultimas.is_empty() && cache.limpeza.is_none(), "rollback não marca cache nem limpeza");
    }
    s.db.locked().execute_batch("DROP TRIGGER falha_limpeza").expect("fixture");
    record_at(&s, "claude", &[window("five_hour", 10.0, None)], BASE).expect("gravar");
    assert_eq!(conta(&s, "claude", "five_hour"), 1, "nova tentativa grava normal");
    assert_eq!(conta(&s, "claude", "old"), 0, "e a limpeza completa");
}

#[test]
fn falha_de_gravacao_nao_derruba_a_leitura_de_uso() {
    let dir = dir();
    let hub = hub(store(), &dir);
    hub.store.db.locked().execute_batch("DROP TABLE donna_usage_samples").expect("fixture");
    crate::usage::ingest_statusline(
        &hub,
        &serde_json::json!({"rate_limits":{"five_hour":{"used_percentage":33.0,"resets_at":4_102_444_800_i64}}}),
    );
    let usage = hub.usage.locked();
    let claude = usage.providers.iter().find(|p| p.id == "claude").expect("provedor");
    assert!((claude.windows[0].percent - 33.0).abs() < f64::EPSILON);
    std::fs::remove_dir_all(dir).expect("fixture");
}

#[test]
fn agrupamento_devolve_ultimo_percent_do_bloco_e_respeita_as_bordas() {
    let s = store();
    let now = BASE;
    // 24h: blocos de 10 min. Duas amostras no mesmo bloco → último percent; borda exata inicia bloco novo.
    let b10 = 10 * 60 * 1000;
    for (at, percent) in [(now - 15 * 60 * 1000, 10.0), (now - 14 * 60 * 1000, 12.0), (now - 10 * 60 * 1000, 20.0)] {
        insere(&s, "claude", "five_hour", at, at, percent, Some(now + HORA));
    }
    let out = report(&s.db.locked(), Faixa::H24, now).expect("relatório");
    let buckets = out["windows"][0]["buckets"].as_array().expect("buckets");
    let e10 = (now - 15 * 60 * 1000).div_euclid(b10) * b10;
    let e20 = (now - 10 * 60 * 1000).div_euclid(b10) * b10;
    assert_eq!(buckets.len(), 2, "bloco de 10 min agrupa pela borda, não pela distância");
    assert_eq!(buckets[0], serde_json::json!({"start":e10,"percent":12.0}));
    assert_eq!(buckets[1], serde_json::json!({"start":e20,"percent":20.0}));
    assert_eq!(out["windows"][0]["resetsAt"], serde_json::json!(now + HORA));
    assert_eq!(out["truncated"], serde_json::json!(false));

    // 7d: blocos de 1 hora, mesma regra de borda e último do bloco.
    let s = store();
    for (at, percent) in [(now - 90 * 60 * 1000, 30.0), (now - 30 * 60 * 1000, 31.0), (now - HORA + 1, 40.0)] {
        insere(&s, "codex", "secondary", at, at, percent, None);
    }
    let out = report(&s.db.locked(), Faixa::D7, now).expect("relatório");
    let buckets = out["windows"][0]["buckets"].as_array().expect("buckets");
    assert_eq!(buckets.len(), 2);
    // BASE é múltiplo de hora: now-90min e now-HORA+1ms caem em blocos distintos.
    assert_eq!(buckets[0]["start"], serde_json::json!((now - 2 * HORA).div_euclid(HORA) * HORA));
    assert_eq!(buckets[0]["percent"], serde_json::json!(30.0));
    assert_eq!(buckets[1]["start"], serde_json::json!(now - HORA));
    // O bloco [now-H, now) tem duas amostras (now-H+1ms → 40, now-30min → 31): vale a última.
    assert_eq!(buckets[1]["percent"], serde_json::json!(31.0));
    assert_eq!(out["windows"][0]["resetsAt"], Value::Null);

    // 30d: blocos de 6 horas; amostra fora do range não entra.
    let s = store();
    let b6 = 6 * HORA;
    insere(&s, "claude", "seven_day", now - Faixa::D30.span_ms() - 1, now - Faixa::D30.span_ms() - 1, 99.0, None);
    insere(&s, "claude", "seven_day", now - b6 + 1, now - b6 + 1, 50.0, None);
    let out = report(&s.db.locked(), Faixa::D30, now).expect("relatório");
    let buckets = out["windows"][0]["buckets"].as_array().expect("buckets");
    assert_eq!(buckets.len(), 1);
    assert_eq!(buckets[0]["percent"], serde_json::json!(50.0));
}

#[test]
fn corte_de_janelas_nunca_e_silencioso() {
    let s = store();
    // MAX_JANELAS+1 janelas distintas: a 65ª fica fora E o RPC declara o corte.
    {
        let mut db = s.db.locked();
        let tx = db.transaction().expect("fixture");
        {
            let mut st = tx.prepare("INSERT INTO donna_usage_samples VALUES(?1,?2,?3,?3,?4,NULL)").expect("fixture");
            for i in 0..=MAX_JANELAS {
                st.execute(params!["claude", format!("w{i:03}"), BASE - 60_000, 10.0]).expect("fixture");
            }
        }
        tx.commit().expect("fixture");
    }
    let out = report(&s.db.locked(), Faixa::H24, BASE).expect("relatório");
    assert_eq!(out["windows"].as_array().expect("windows").len(), MAX_JANELAS);
    assert_eq!(out["truncated"], serde_json::json!(true));
}

#[test]
fn recorte_de_amostras_nao_apaga_previsao_quando_a_vigente_e_conhecida() {
    let s = store();
    // Janela semanal: MAX_AMOSTRAS+1 amostras crescentes e com o mesmo resetsAt
    // (sem queda/troca): o recorte esconde o passado mas a vigente é conhecida.
    {
        let mut db = s.db.locked();
        let tx = db.transaction().expect("fixture");
        {
            let mut st = tx
                .prepare("INSERT INTO donna_usage_samples VALUES('claude','seven_day',?1,?1,?2,?3)")
                .expect("fixture");
            for i in 0..=MAX_AMOSTRAS {
                let at = BASE - i64::try_from(MAX_AMOSTRAS - i).expect("fixture") * 60_000;
                st.execute(params![at, 40.0 + f64::from(i32::try_from(i).expect("fixture")) * 0.1, BASE + 200 * HORA])
                    .expect("fixture");
            }
        }
        tx.commit().expect("fixture");
    }
    let out = report(&s.db.locked(), Faixa::D30, BASE).expect("relatório");
    let forecast = &out["windows"][0]["forecast"];
    assert!(forecast["exhaustsAt"].is_i64(), "previsão presente mesmo com recorte: {forecast}");
    assert_eq!(out["truncated"], serde_json::json!(false), "recorte de amostras não é corte real");
}

#[test]
fn ultimo_cem_com_recorte_devolve_esgotado_mesmo_assim() {
    let s = store();
    {
        let mut db = s.db.locked();
        let tx = db.transaction().expect("fixture");
        {
            let mut st = tx
                .prepare("INSERT INTO donna_usage_samples VALUES('claude','five_hour',?1,?1,?2,?3)")
                .expect("fixture");
            for i in 0..=MAX_AMOSTRAS {
                let at = BASE - i64::try_from(MAX_AMOSTRAS - i).expect("fixture") * 60_000;
                let percent = if i == MAX_AMOSTRAS { 100.0 } else { 50.0 };
                st.execute(params![at, percent, BASE + 4 * HORA]).expect("fixture");
            }
        }
        tx.commit().expect("fixture");
    }
    let out = report(&s.db.locked(), Faixa::D30, BASE).expect("relatório");
    let forecast = &out["windows"][0]["forecast"];
    assert_eq!(forecast["exhausted"], serde_json::json!(true), "fato observado não se perde no corte");
    assert_eq!(forecast["exhaustsAt"], serde_json::json!(BASE));
}

#[test]
fn incerteza_real_devolve_forecast_null() {
    let s = store();
    // Recorte + vigência alcançando a borda + resetsAt desconhecido: o que se vê
    // não permite concluir nada e pode haver mais da vigente fora de vista.
    {
        let mut db = s.db.locked();
        let tx = db.transaction().expect("fixture");
        {
            let mut st = tx
                .prepare("INSERT INTO donna_usage_samples VALUES('claude','five_hour',?1,?1,?2,NULL)")
                .expect("fixture");
            for i in 0..=MAX_AMOSTRAS {
                let at = BASE - i64::try_from(MAX_AMOSTRAS - i).expect("fixture") * 60_000;
                st.execute(params![at, 40.0 + f64::from(i32::try_from(i).expect("fixture")) * 0.1]).expect("fixture");
            }
        }
        tx.commit().expect("fixture");
    }
    let out = report(&s.db.locked(), Faixa::D30, BASE).expect("relatório");
    assert_eq!(out["windows"][0]["forecast"], Value::Null, "incerteza real é null, nunca silenciosa");
    assert_eq!(out["truncated"], serde_json::json!(false));
}

#[test]
fn at_para_no_primeiro_cem_mesmo_com_refrescos_na_mesma_vaga() {
    let s = store();
    let t0 = BASE;
    record_at(&s, "claude", &[window("five_hour", 99.0, None)], t0).expect("record");
    record_at(&s, "claude", &[window("five_hour", 100.0, Some(t0 + 4 * HORA))], t0 + 120_000).expect("record");
    assert_eq!(amostra(&s, "claude", "five_hour"), Some((t0, t0 + 120_000, 100.0, Some(t0 + 4 * HORA))));
    // Refresh ainda dentro da vaga com o provedor já a 100%: percent/reset
    // atualizam, mas `at` fica no instante da observação que trouxe o primeiro 100.
    record_at(&s, "claude", &[window("five_hour", 100.0, Some(t0 + 4 * HORA))], t0 + 180_000).expect("record");
    let linhas: Vec<(i64, i64, f64, Option<i64>)> =
        s.db.locked()
            .prepare("SELECT slot,at,percent,resets_at FROM donna_usage_samples")
            .expect("fixture")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .expect("fixture")
            .collect::<rusqlite::Result<_>>()
            .expect("fixture");
    assert_eq!(linhas, vec![(t0, t0 + 120_000, 100.0, Some(t0 + 4 * HORA))]);
    let out = report(&s.db.locked(), Faixa::H24, t0 + 180_000).expect("relatório");
    let forecast = &out["windows"][0]["forecast"];
    assert_eq!(forecast["exhausted"], serde_json::json!(true));
    assert_eq!(forecast["exhaustsAt"], serde_json::json!(t0 + 120_000));
}

#[test]
fn migracao_com_backfill_faz_o_dedupe_enxergar_linhas_antigas() {
    let s = store();
    // Recria o banco do commit anterior do PR: tabela sem a coluna `slot`.
    {
        let db = s.db.locked();
        db.execute_batch(&format!(
            "DROP TABLE donna_usage_samples;
             CREATE TABLE donna_usage_samples(provider TEXT NOT NULL, window_id TEXT NOT NULL,
               at INTEGER NOT NULL, percent REAL NOT NULL, resets_at INTEGER);
             CREATE INDEX donna_usage_samples_at ON donna_usage_samples(provider,window_id,at);
             INSERT INTO donna_usage_samples VALUES('claude','five_hour',{},50.0,NULL);",
            BASE - 60_000
        ))
        .expect("fixture");
        migrate(&db).expect("migração");
        let slot: i64 = db.query_row("SELECT slot FROM donna_usage_samples", [], |r| r.get(0)).expect("fixture");
        assert_eq!(slot, BASE - 60_000, "backfill abre a vaga legada na observação");
    }
    // Leitura segundos após a amostra legada, ainda na mesma vaga de 10 min:
    // atualiza a linha existente em vez de inserir uma segunda.
    record_at(&s, "claude", &[window("five_hour", 55.0, None)], BASE - 30_000).expect("record");
    let linhas: Vec<(i64, f64)> =
        s.db.locked()
            .prepare("SELECT at,percent FROM donna_usage_samples")
            .expect("fixture")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("fixture")
            .collect::<rusqlite::Result<_>>()
            .expect("fixture");
    assert_eq!(linhas, vec![(BASE - 30_000, 55.0)], "uma linha atualizada, não duas");
}

#[test]
fn reabertura_confere_identidade_e_falha_fechada_se_o_arquivo_mudou() {
    use crate::store::personal_database::identidade_da;
    let dir = std::env::temp_dir().join(format!("mx260-{}-{}", std::process::id(), BASE % 1_000_000));
    std::fs::create_dir_all(&dir).expect("fixture");
    let path = dir.join("history.db");
    let original = Connection::open(&path).expect("fixture");
    original.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES(1);").expect("fixture");
    let id = identidade_da(&original).expect("identidade").expect("banco tem arquivo");
    // Mesmo arquivo: reabre READ_ONLY e a conferência passa.
    {
        let leitura = Connection::open_with_flags(id.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).expect("reabre");
        id.conferir(&leitura).expect("arquivo intacto confere");
    }
    drop(original);
    // Arquivo substituído no mesmo caminho entre captura e leitura: fail-closed.
    // O substituto é criado ANTES do rename atômico para garantir inode diverso —
    // delete+create no mesmo caminho pode reutilizar o inode (dev+inode iguais).
    let substituto = dir.join("replacement.db");
    {
        let c = Connection::open(&substituto).expect("fixture");
        c.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES(2);").expect("fixture");
    }
    std::fs::rename(&substituto, &path).expect("fixture");
    let leitura = Connection::open_with_flags(id.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).expect("reabre");
    assert!(id.conferir(&leitura).is_err(), "arquivo trocado não pode ler");
    drop(leitura);
    std::fs::remove_file(&path).expect("fixture");
    std::fs::remove_dir(&dir).expect("fixture");
}

#[test]
fn captura_falha_fechada_se_o_arquivo_ja_foi_trocado_antes() {
    use crate::store::personal_database::identidade_da;
    let dir = std::env::temp_dir().join(format!("mx260-pre-{}-{}", std::process::id(), BASE % 1_000_000));
    std::fs::create_dir_all(&dir).expect("fixture");
    let path = dir.join("history.db");
    // Autoridade aberta no arquivo ORIGINAL; alguém o substitui antes da captura
    // (rename atômico → inode diverso). O handle detecta a troca e a captura falha.
    let autorizada = Connection::open(&path).expect("fixture");
    autorizada.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES(1);").expect("fixture");
    let substituto = dir.join("replacement.db");
    {
        let c = Connection::open(&substituto).expect("fixture");
        c.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES(2);").expect("fixture");
    }
    std::fs::rename(&substituto, &path).expect("fixture");
    assert!(
        identidade_da(&autorizada).is_err(),
        "handle que perdeu o arquivo não pode emprestar identidade a outro banco"
    );
    drop(autorizada);
    std::fs::remove_file(&path).expect("fixture");
    std::fs::remove_dir(&dir).expect("fixture");
}

#[test]
fn cem_porcento_antes_do_reset_nao_afirma_esgotamento_no_relatorio() {
    let s = store();
    insere(&s, "claude", "five_hour", BASE - 600_000, BASE - 600_000, 100.0, Some(BASE - 60_000));
    let out = report(&s.db.locked(), Faixa::H24, BASE).expect("relatório");
    let forecast = &out["windows"][0]["forecast"];
    assert_eq!(forecast["exhausted"], serde_json::json!(false), "100% de janela rolada não é esgotamento");
    assert!(!forecast["exhaustsAt"].is_i64() || forecast.is_null());
}

#[test]
fn migracao_backfill_tambem_cobre_linhas_com_slot_null_e_o_dedupe_olha_at() {
    let s = store();
    // Banco da rodada 3: a coluna `slot` JÁ existe, mas a linha antiga ficou NULL.
    {
        let db = s.db.locked();
        db.execute_batch(&format!(
            "DROP TABLE donna_usage_samples;
             CREATE TABLE donna_usage_samples(provider TEXT NOT NULL, window_id TEXT NOT NULL,
               slot INTEGER, at INTEGER NOT NULL, percent REAL NOT NULL, resets_at INTEGER);
             CREATE INDEX donna_usage_samples_at ON donna_usage_samples(provider,window_id,at);
             INSERT INTO donna_usage_samples VALUES('claude','five_hour',NULL,{},50.0,NULL);
             INSERT INTO donna_usage_samples VALUES('codex','primary',{}, {},60.0,NULL);",
            BASE - 60_000,
            (BASE - 120_001) / 600_000 * 600_000,
            BASE - 120_001
        ))
        .expect("fixture");
        migrate(&db).expect("migração");
        // Rodada 3 (NULL) e rodada 4 (slot alinhado ≠ at): os dois viram vaga
        // aberta na observação.
        let linhas: Vec<i64> = db
            .prepare("SELECT at - slot FROM donna_usage_samples ORDER BY provider")
            .expect("fixture")
            .query_map([], |r| r.get(0))
            .expect("fixture")
            .collect::<rusqlite::Result<_>>()
            .expect("fixture");
        assert_eq!(linhas, vec![0, 0], "slot backfilled = at em todos os formatos");
    }
    // Legado "12:09" + leitura "12:10" (60s depois): o slot alinhado permitiria
    // outra linha, mas o contrato é ≥10 min entre observações → atualiza a mesma.
    record_at(&s, "claude", &[window("five_hour", 55.0, None)], BASE).expect("record");
    let linhas: Vec<(i64, f64)> =
        s.db.locked()
            .prepare("SELECT at,percent FROM donna_usage_samples WHERE provider='claude'")
            .expect("fixture")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("fixture")
            .collect::<rusqlite::Result<_>>()
            .expect("fixture");
    assert_eq!(linhas, vec![(BASE, 55.0)], "observação <10 min depois não duplica");
}

#[test]
fn queda_ou_reset_novo_destrava_o_at_travado_no_primeiro_cem() {
    let s = store();
    let t0 = BASE;
    record_at(&s, "claude", &[window("five_hour", 100.0, Some(t0 + 4 * HORA))], t0).expect("record");
    // Janela NOVA dentro da mesma vaga: percent caiu e o reset mudou — `at` avança.
    record_at(&s, "claude", &[window("five_hour", 10.0, Some(t0 + 8 * HORA))], t0 + 120_000).expect("record");
    assert_eq!(
        amostra(&s, "claude", "five_hour"),
        Some((t0, t0 + 120_000, 10.0, Some(t0 + 8 * HORA))),
        "at acompanha a observação da nova janela"
    );
    // Mesmo 100%, MAS resets_at trocou → também destrava (janela diferente).
    record_at(&s, "claude", &[window("seven_day", 100.0, Some(t0 + HORA))], t0).expect("record");
    record_at(&s, "claude", &[window("seven_day", 100.0, Some(t0 + 8 * HORA))], t0 + 60_000).expect("record");
    assert_eq!(amostra(&s, "claude", "seven_day"), Some((t0, t0 + 60_000, 100.0, Some(t0 + 8 * HORA))));
}

#[test]
fn captura_revalida_e_falha_se_a_troca_cai_no_intervalo() {
    use crate::store::personal_database::identidade_da_injetando;
    let dir = std::env::temp_dir().join(format!("mx260-mid-{}-{}", std::process::id(), BASE % 1_000_000));
    std::fs::create_dir_all(&dir).expect("fixture");
    let path = dir.join("history.db");
    let autorizada = Connection::open(&path).expect("fixture");
    autorizada.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES(1);").expect("fixture");
    let substituto = dir.join("replacement.db");
    {
        let c = Connection::open(&substituto).expect("fixture");
        c.execute_batch("CREATE TABLE t(x INTEGER); INSERT INTO t VALUES(2);").expect("fixture");
    }
    // Troca DEPOIS da checagem do handle e do stat, antes da revalidação:
    // sem a segunda `unchanged_handle`, a identidade registrada era a do
    // substituto e a reabertura aceitaria ler outro banco.
    let dentro = substituto.clone();
    let destino = path.clone();
    assert!(
        identidade_da_injetando(&autorizada, move || {
            std::fs::rename(&dentro, &destino)?;
            Ok(())
        })
        .is_err(),
        "troca dentro da janela da captura falha fechada"
    );
    drop(autorizada);
    std::fs::remove_file(&path).expect("fixture");
    std::fs::remove_dir(&dir).expect("fixture");
}

#[test]
fn reset_nulo_vira_conhecido_sem_mover_o_primeiro_cem() {
    let s = store();
    let t0 = BASE;
    // 12:01: a CLI observa 100% sem resetsAt; 12:02: a statusline confirma 100%
    // trazendo o resetsAt — é a PRIMEIRA informação de reset, não troca de janela.
    record_at(&s, "claude", &[window("five_hour", 100.0, None)], t0).expect("record");
    record_at(&s, "claude", &[window("five_hour", 100.0, Some(t0 + 4 * HORA))], t0 + 60_000).expect("record");
    assert_eq!(
        amostra(&s, "claude", "five_hour"),
        Some((t0, t0, 100.0, Some(t0 + 4 * HORA))),
        "at fica no primeiro 100 observado e o reset é absorvido"
    );
}

#[test]
fn exhaustsat_devolve_o_primeiro_cem_mesmo_fora_do_recorte() {
    let s = store();
    // Esgotamento ~67h no passado (400 amostras > MAX_AMOSTRAS): o recorte das
    // recentes não pode esconder o início — MIN(at) sai direto do banco.
    {
        let mut db = s.db.locked();
        let tx = db.transaction().expect("fixture");
        {
            let mut st = tx
                .prepare("INSERT INTO donna_usage_samples VALUES('claude','five_hour',?1,?1,100.0,?2)")
                .expect("fixture");
            for i in 0..400_i64 {
                st.execute(params![BASE - (400 - i) * 600_000, BASE + HORA]).expect("fixture");
            }
        }
        tx.commit().expect("fixture");
    }
    for faixa in [Faixa::H24, Faixa::D7, Faixa::D30] {
        let out = report(&s.db.locked(), faixa, BASE).expect("relatório");
        let forecast = &out["windows"][0]["forecast"];
        assert_eq!(forecast["exhausted"], serde_json::json!(true));
        assert_eq!(forecast["exhaustsAt"], serde_json::json!(BASE - 400 * 600_000));
    }
}

#[test]
fn esgotamento_preserva_inicio_com_fontes_alternadas_em_vagas_distintas() {
    let s = store();
    let reset = BASE + 4 * HORA;
    for (i, r) in [None, Some(reset), None].into_iter().enumerate() {
        let at = BASE + i64::try_from(i).expect("fixture") * INTERVALO_MS;
        record_at(&s, "claude", &[window("five_hour", 100.0, r)], at).expect("gravar");
        let out = report(&s.db.locked(), Faixa::H24, at).expect("relatório");
        assert_eq!(out["windows"][0]["forecast"], serde_json::json!({"exhausted":true,"exhaustsAt":BASE}));
    }
}

#[test]
fn reset_conhecido_divergente_ou_vencido_interrompe_alternancia() {
    let s = store();
    let reset_antigo = BASE + 2 * INTERVALO_MS;
    for (i, r) in [Some(reset_antigo), None, None, Some(BASE + 4 * HORA)].into_iter().enumerate() {
        let at = BASE + i64::try_from(i).expect("fixture") * INTERVALO_MS;
        record_at(&s, "claude", &[window("five_hour", 100.0, r)], at).expect("gravar");
    }
    let out = report(&s.db.locked(), Faixa::H24, BASE + 3 * INTERVALO_MS).expect("relatório");
    assert_eq!(out["windows"][0]["forecast"]["exhaustsAt"], serde_json::json!(BASE + 2 * INTERVALO_MS));

    let s = store();
    for (i, r) in [Some(BASE + HORA), None, Some(BASE + 2 * HORA)].into_iter().enumerate() {
        let at = BASE + i64::try_from(i).expect("fixture") * INTERVALO_MS;
        record_at(&s, "claude", &[window("five_hour", 100.0, r)], at).expect("gravar");
    }
    let out = report(&s.db.locked(), Faixa::H24, BASE + 2 * INTERVALO_MS).expect("relatório");
    assert_eq!(out["windows"][0]["forecast"]["exhaustsAt"], serde_json::json!(BASE + 2 * INTERVALO_MS));
}

#[test]
fn reset_antigo_fora_do_recorte_ainda_impede_esgotamento_apos_vencer() {
    let s = store();
    let reset = BASE - INTERVALO_MS;
    for i in 0..320_i64 {
        let at = BASE - (321 - i) * INTERVALO_MS;
        insere(&s, "claude", "five_hour", at, at, 100.0, (i == 0).then_some(reset));
    }
    let out = report(&s.db.locked(), Faixa::H24, BASE).expect("relatório");
    assert_eq!(out["windows"][0]["forecast"], serde_json::json!({"exhausted":false,"exhaustsAt":null}));
    assert_eq!(out["windows"][0]["resetsAt"], serde_json::json!(reset));
}

#[test]
fn leitura_sem_reset_nao_herda_reset_vencido_da_vaga() {
    let s = store();
    // Vaga aberta com 100% e resetsAt já VENCIDO (janela rolada); a observação
    // nova de 100% chega sem reset (CLI) e NÃO pode herdar o reset morto —
    // senão o relatório responderia exhausted:false para um esgotamento atual.
    insere(&s, "claude", "five_hour", BASE - 60_000, BASE - 60_000, 100.0, Some(BASE - 30_000));
    record_at(&s, "claude", &[window("five_hour", 100.0, None)], BASE).expect("record");
    assert_eq!(
        amostra(&s, "claude", "five_hour"),
        Some((BASE - 60_000, BASE, 100.0, None)),
        "reset vencido é descartado, não herdado"
    );
    let out = report(&s.db.locked(), Faixa::H24, BASE).expect("relatório");
    let forecast = &out["windows"][0]["forecast"];
    assert_eq!(forecast["exhausted"], serde_json::json!(true));
    assert_eq!(forecast["exhaustsAt"], serde_json::json!(BASE));
}

#[test]
fn primeiro_cem_respeita_queda() {
    let s = store();
    insere(&s, "claude", "five_hour", BASE - 4 * HORA, BASE - 4 * HORA, 100.0, Some(BASE + HORA));
    insere(&s, "claude", "five_hour", BASE - 3 * HORA, BASE - 3 * HORA, 20.0, Some(BASE + HORA));
    insere(&s, "claude", "five_hour", BASE - 2 * HORA, BASE - 2 * HORA, 100.0, Some(BASE + HORA));
    let out = report(&s.db.locked(), Faixa::H24, BASE).expect("relatório");
    assert_eq!(out["windows"][0]["forecast"]["exhaustsAt"], serde_json::json!(BASE - 2 * HORA));
}

#[test]
fn migracao_nao_desloca_vaga_genuina_em_aberturas_posteriores() {
    let s = store();
    let slot = BASE / 600_000 * 600_000;
    insere(&s, "claude", "five_hour", slot, slot + 60_000, 20.0, None);
    let db = s.db.locked();
    migrate(&db).expect("reabertura");
    let salvo: i64 = db.query_row("SELECT slot FROM donna_usage_samples", [], |r| r.get(0)).expect("slot");
    assert_eq!(salvo, slot);
}

fn plano(c: &Connection, sql: &str, args: &[i64]) -> Vec<String> {
    c.prepare(sql)
        .expect("fixture")
        .query_map(rusqlite::params_from_iter(args.iter()), |r| r.get::<_, String>(3))
        .expect("fixture")
        .collect::<rusqlite::Result<_>>()
        .expect("fixture")
}

#[test]
fn limpeza_e_relatorio_usam_indice_e_o_custo_e_limitado() {
    let s = store();
    // 31 dias de amostras sintéticas: 8 janelas × 4464 (1/10 min) ≈ o dobro do
    // pior caso real (Claude e Codex 5h/7d) — o teto MAX_JANELAS do relatório.
    {
        let mut db = s.db.locked();
        let tx = db.transaction().expect("fixture");
        {
            let mut st = tx.prepare("INSERT INTO donna_usage_samples VALUES(?1,?2,?3,?3,?4,NULL)").expect("fixture");
            for window in [
                "five_hour",
                "seven_day",
                "seven_day_sonnet",
                "seven_day_opus",
                "extra_a",
                "extra_b",
                "extra_c",
                "extra_d",
            ] {
                for i in 0..4464_i64 {
                    st.execute(params!["claude", window, BASE - 31 * DIA + i * 600_000, 10.0]).expect("fixture");
                }
            }
        }
        tx.commit().expect("fixture");
    }
    let db = s.db.locked();
    let linhas: i64 = db.query_row("SELECT COUNT(*) FROM donna_usage_samples", [], |r| r.get(0)).expect("fixture");
    // Limpeza: subselect por `at` usa o índice `quando` (SEARCH, não SCAN).
    let limpeza_plano = plano(
        &db,
        "EXPLAIN QUERY PLAN SELECT rowid FROM donna_usage_samples WHERE at < ?1 LIMIT ?2",
        &[0, LIMPEZA_LOTE],
    );
    assert!(limpeza_plano.iter().any(|l| l.contains("donna_usage_samples_quando")), "{limpeza_plano:?}");
    // Relatório — lista de janelas: range por `at` no índice `quando`.
    let janelas_plano = plano(
        &db,
        "EXPLAIN QUERY PLAN SELECT provider,window_id FROM donna_usage_samples
         INDEXED BY donna_usage_samples_quando
         WHERE at>=?1 GROUP BY provider,window_id ORDER BY provider,window_id LIMIT ?2",
        &[BASE - DIA, i64::try_from(MAX_JANELAS + 1).expect("fixture")],
    );
    assert!(
        janelas_plano.iter().any(|l| l.contains("SEARCH") && l.contains("donna_usage_samples_quando")),
        "{janelas_plano:?}"
    );
    // Relatório — blocos e amostras por janela: SEARCH no índice (provider,window_id,at).
    let blocos_plano = plano(
        &db,
        "EXPLAIN QUERY PLAN SELECT (at/?3)*?3 AS start, percent, MAX(at) FROM donna_usage_samples
         WHERE provider=?1 AND window_id=?2 AND at>=?4 GROUP BY start ORDER BY start LIMIT ?5",
        &[0, 0, 0, 0, 0],
    );
    assert!(
        blocos_plano.iter().any(|l| l.contains("SEARCH donna_usage_samples USING INDEX donna_usage_samples_at")),
        "{blocos_plano:?}"
    );
    let amostras_plano = plano(
        &db,
        "EXPLAIN QUERY PLAN SELECT at,percent,resets_at FROM donna_usage_samples
         WHERE provider=?1 AND window_id=?2 AND at>=?3 ORDER BY at DESC LIMIT ?4",
        &[0, 0, 0, 0],
    );
    assert!(
        amostras_plano.iter().any(|l| l.contains("SEARCH donna_usage_samples USING INDEX donna_usage_samples_at")),
        "{amostras_plano:?}"
    );
    eprintln!("EXPLAIN limpeza: {limpeza_plano:?}");
    eprintln!("EXPLAIN janelas: {janelas_plano:?}");
    eprintln!("EXPLAIN blocos: {blocos_plano:?}");
    eprintln!("EXPLAIN amostras: {amostras_plano:?}");
    // Range 24h: devolve só agregados — 4 janelas × ≤144 blocos; previsão lê ≤65/janela.
    // Tempo dentro da transação autorizada, medido no pior caso (8 janelas ×
    // 31 dias = ~35.7k linhas) e nas faixas curtas. Impresso para documentação
    // no PR — o assert é só uma rede de segurança frouxa contra regressão.
    let t = std::time::Instant::now();
    let out = report(&db, Faixa::H24, BASE).expect("relatório");
    let h24 = t.elapsed();
    let t = std::time::Instant::now();
    report(&db, Faixa::D7, BASE).expect("relatório");
    let d7 = t.elapsed();
    let t = std::time::Instant::now();
    report(&db, Faixa::D30, BASE).expect("relatório");
    let d30 = t.elapsed();
    eprintln!("RELATORIO linhas={linhas} 24h={}ms 7d={}ms 30d={}ms", h24.as_millis(), d7.as_millis(), d30.as_millis());
    assert!(d30 < std::time::Duration::from_secs(5), "relatório 30d lento demais: {d30:?}");
    let janelas = out["windows"].as_array().expect("windows");
    assert_eq!(janelas.len(), 8);
    for j in janelas {
        assert!(j["buckets"].as_array().expect("buckets").len() <= 144);
    }
}

fn amostras(serie: &[(i64, f64)], resets_at: Option<i64>) -> Vec<Amostra> {
    serie.iter().map(|&(at, percent)| Amostra { at, percent, resets_at }).collect()
}

#[test]
fn previsao_so_projeta_janela_vigente_crescente_antes_do_reset() {
    let resets_ms = BASE + 10 * HORA;
    let resets = Some(resets_ms);
    // Crescente 10%/h cobrindo 4h: a reta cruza 100% ~5h após a última amostra → antes do reset.
    let crescente =
        amostras(&[(BASE, 40.0), (BASE + HORA, 50.0), (BASE + 2 * HORA, 60.0), (BASE + 4 * HORA, 80.0)], resets);
    let Previsao::Estimada { at: esgota } = prever(&crescente, BASE + 4 * HORA) else {
        panic!("previsão esperada");
    };
    assert!(esgota > BASE + 4 * HORA && esgota < resets_ms);
    // Cruzando depois do reset → sem previsão.
    let lenta =
        amostras(&[(BASE, 40.0), (BASE + HORA, 40.5), (BASE + 2 * HORA, 41.0), (BASE + 4 * HORA, 42.0)], resets);
    assert_eq!(prever(&lenta, BASE + 4 * HORA), Previsao::Sem);
    // Plano → sem previsão.
    let plano =
        amostras(&[(BASE, 40.0), (BASE + HORA, 40.0), (BASE + 2 * HORA, 40.0), (BASE + 4 * HORA, 40.0)], resets);
    assert_eq!(prever(&plano, BASE + 4 * HORA), Previsao::Sem);
    // Poucas amostras / cobertura curta → sem previsão.
    let curtas =
        amostras(&[(BASE, 40.0), (BASE + 60_000, 50.0), (BASE + 120_000, 60.0), (BASE + 180_000, 70.0)], resets);
    assert_eq!(prever(&curtas, BASE), Previsao::Sem);
    let poucas = amostras(&[(BASE, 40.0), (BASE + HORA, 50.0), (BASE + 2 * HORA, 60.0)], resets);
    assert_eq!(prever(&poucas, BASE), Previsao::Sem);
    // Reinício por queda de percent: só o sufixo vigente conta (2 amostras → sem previsão).
    let reinicio =
        amostras(&[(BASE, 80.0), (BASE + HORA, 90.0), (BASE + 2 * HORA, 10.0), (BASE + 3 * HORA, 20.0)], resets);
    assert_eq!(prever(&reinicio, BASE + 3 * HORA), Previsao::Sem);
    // Reinício por mudança de resets_at idem.
    let mut troca_reset = amostras(&[(BASE, 40.0), (BASE + HORA, 50.0)], Some(BASE + 10 * HORA));
    troca_reset.extend(amostras(&[(BASE + 2 * HORA, 60.0), (BASE + 4 * HORA, 70.0)], Some(BASE + 20 * HORA)));
    assert_eq!(prever(&troca_reset, BASE + 4 * HORA), Previsao::Sem);
    // Sem resets_at vigente não há como comparar → sem previsão.
    let sem_reset =
        amostras(&[(BASE, 40.0), (BASE + HORA, 50.0), (BASE + 2 * HORA, 60.0), (BASE + 4 * HORA, 70.0)], None);
    assert_eq!(prever(&sem_reset, BASE + 4 * HORA), Previsao::Sem);
}

#[test]
fn esgotado_e_fato_observado_e_a_vigente_nunca_e_vazia() {
    let resets = Some(BASE + 10 * HORA);
    // Única amostra 100%: a vigente inclui a última → esgotada na observação.
    let unica = amostras(&[(BASE, 100.0)], resets);
    assert_eq!(prever(&unica, BASE), Previsao::Esgotada { at: BASE });
    // 1ª amostra depois de mudança de reset já em 100%: idem.
    let mut apos_reset = amostras(&[(BASE, 40.0), (BASE + HORA, 50.0)], Some(BASE + 10 * HORA));
    apos_reset.extend(amostras(&[(BASE + 2 * HORA, 100.0)], Some(BASE + 20 * HORA)));
    assert_eq!(prever(&apos_reset, BASE + 2 * HORA), Previsao::Esgotada { at: BASE + 2 * HORA });
    // 1ª amostra depois de queda já em 100%: idem.
    let apos_queda = amostras(&[(BASE, 90.0), (BASE + HORA, 80.0), (BASE + 2 * HORA, 100.0)], resets);
    assert_eq!(prever(&apos_queda, BASE + 2 * HORA), Previsao::Esgotada { at: BASE + 2 * HORA });
    // 0,0,0,100 a cada 10 min: esgotada no instante da observação de 100 (min 30), não da reta.
    let esgotada =
        amostras(&[(BASE, 0.0), (BASE + 60_000, 0.0), (BASE + 120_000, 0.0), (BASE + 180_000, 100.0)], resets);
    assert_eq!(prever(&esgotada, BASE + 180_000), Previsao::Esgotada { at: BASE + 180_000 });
    // 100 constante: esgotada desde a primeira amostra.
    let cheia = amostras(&[(BASE, 100.0), (BASE + HORA, 100.0), (BASE + 2 * HORA, 100.0)], resets);
    assert_eq!(prever(&cheia, BASE + 2 * HORA), Previsao::Esgotada { at: BASE });
}

#[test]
fn a_reta_nunca_afirma_esgotamento_e_exige_explicar_a_ultima_observacao() {
    let resets = Some(BASE + 10 * HORA);
    // 0,99,99,99 a cada 10 min: a reta cruzaria no min ~23,7 mas a leitura do min 30
    // diz 99 → a reta não explica o dado mais recente (resíduo > 2pp) → null.
    let salto = amostras(&[(BASE, 0.0), (BASE + 60_000, 99.0), (BASE + 120_000, 99.0), (BASE + 180_000, 99.0)], resets);
    assert_eq!(prever(&salto, BASE + 180_000), Previsao::Sem);
    // 99,9 subindo devagar com a reta consistente → estimativa FUTURA coerente.
    let quase =
        amostras(&[(BASE, 99.0), (BASE + HORA, 99.2), (BASE + 2 * HORA, 99.5), (BASE + 4 * HORA, 99.9)], resets);
    match prever(&quase, BASE + 4 * HORA) {
        Previsao::Estimada { at } => assert!(at > BASE + 4 * HORA, "exhaustsAt nunca no passado"),
        Previsao::Sem => {}
        Previsao::Esgotada { .. } => panic!("sem fato observado, a reta não afirma esgotamento"),
    }
    // Dente-de-serra: queda de percent quebra a vigência → sufixo curto → sem previsão.
    let serra =
        amostras(&[(BASE, 50.0), (BASE + HORA, 60.0), (BASE + 2 * HORA, 55.0), (BASE + 4 * HORA, 70.0)], resets);
    assert_eq!(prever(&serra, BASE + 4 * HORA), Previsao::Sem);
}

#[test]
fn cem_porcento_antigo_nao_sobrevive_ao_reset_sem_nova_leitura() {
    let resets = Some(BASE + 10 * HORA);
    let amostras = amostras(&[(BASE, 90.0), (BASE + HORA, 100.0)], resets);
    // Antes do reset: esgotada (fato observado). Depois do resetsAt vigente sem
    // leitura da janela nova: janela rolou → exhausted:false / forecast null.
    assert_eq!(prever(&amostras, BASE + HORA), Previsao::Esgotada { at: BASE + HORA });
    assert_eq!(prever(&amostras, BASE + 10 * HORA), Previsao::Sem, "reset no instante = janela rolada");
    assert_eq!(prever(&amostras, BASE + 11 * HORA), Previsao::Sem);
}

#[test]
fn rpc_recusa_range_invalido_e_campos_desconhecidos() {
    let dir = dir();
    let hub = hub(store(), &dir);
    for body in [serde_json::json!({"range":"1h"}), serde_json::json!({"range":"24h","extra":1}), serde_json::json!({})]
    {
        assert!(call(&hub, &Caller::Local, body.clone()).is_err(), "{body}");
    }
    let out = call(&hub, &Caller::Local, serde_json::json!({"range":"30d"})).expect("relatório");
    assert_eq!(out["range"], serde_json::json!("30d"));
    assert_eq!(out["bucketMs"], serde_json::json!(6 * HORA));
    assert_eq!(out["windows"].as_array().expect("windows").as_slice(), &[] as &[Value]);
    assert_eq!(out["truncated"], serde_json::json!(false));
    std::fs::remove_dir_all(dir).expect("fixture");
}
