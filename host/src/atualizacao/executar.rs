//! Despacho privilegiado restrito; só opera sobre caminhos fixos root-owned.
use super::{
    armazenamento::Diretorio,
    cgroup,
    chamador::Identidade,
    entrada,
    journal::{Evento, Journal, Motivo, Operacao, Resultado},
    politica::Politica,
    transacao,
};
use anyhow::Result;
use std::{fs::File, path::Path};

pub const ESTADO: &str = "/var/lib/donnabot/atualizacao";

fn operacao(op: entrada::Operacao) -> Operacao {
    match op {
        entrada::Operacao::Atualizar => Operacao::Atualizar,
        entrada::Operacao::Reverter => Operacao::Reverter,
        entrada::Operacao::Aceitar => Operacao::Aceitar,
    }
}

pub fn executar(op: entrada::Operacao, chamador: Identidade, verificacao: Option<Vec<u8>>) -> Result<()> {
    let estado = Diretorio::abrir(Path::new(ESTADO))?;
    let publicacao = Diretorio::abrir_publicacao()?;
    let politica = Politica::ler(&estado)?;
    let trava = estado.travar()?;
    let operacao = operacao(op);
    let agora = match estado.hora_conferida(&trava) {
        Ok(agora) => agora,
        Err(erro) => {
            // O horário é apenas indicativo quando a qualificação falha.
            let horario = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|t| i64::try_from(t.as_secs()).ok())
                .unwrap_or(0);
            let evento = |resultado| Evento {
                horario_unix: horario,
                uid: chamador.uid(),
                origem: chamador.origem(),
                operacao,
                sequencia_atual: None,
                sequencia_anterior: None,
                resultado,
                motivo: Motivo::Relogio,
            };
            let inicio = estado.inicio(&evento(Resultado::Inicio));
            // Bloqueia mesmo se a auditoria não estiver disponível.
            estado.bloquear_por_falha(&trava)?;
            publicacao.publicar_status(&estado, &trava)?;
            let fim = estado.auditar(&evento(Resultado::Recusada));
            inicio?;
            fim?;
            return Err(erro);
        }
    };
    let sequencias =
        || estado.ler::<Journal>("journal").ok().map_or((None, None), |j| (Some(j.estado.marca), j.sequencia_anterior));
    let evento = |resultado, motivo, (sequencia_atual, sequencia_anterior)| Evento {
        horario_unix: agora,
        uid: chamador.uid(),
        origem: chamador.origem(),
        operacao,
        sequencia_atual,
        sequencia_anterior,
        resultado,
        motivo,
    };
    if let Err(erro) = chamador.autorizar(politica.uid_hub) {
        estado.inicio(&evento(Resultado::Inicio, Motivo::Politica, sequencias()))?;
        estado.auditar(&evento(Resultado::Recusada, Motivo::Politica, sequencias()))?;
        return Err(erro);
    }
    if matches!(op, entrada::Operacao::Aceitar) {
        return aceitar(&estado, &publicacao, &trava, chamador, verificacao, agora);
    }
    // Registro do início antes de qualquer alteração; sem ele, recusa.
    estado.inicio(&evento(Resultado::Inicio, Motivo::Solicitacao, sequencias()))?;
    let limites = cgroup::Limites {
        memoria: politica.cgroup_memoria,
        processos: politica.cgroup_processos,
        cpu_quota: politica.cgroup_cpu_quota,
        cpu_periodo: politica.cgroup_cpu_periodo,
    };
    let resultado = cgroup::entrar(&limites).and_then(|()| {
        let ctx = transacao::Contexto {
            estado,
            publicacao,
            configuracao: Diretorio::abrir_configuracao()?,
            politica,
            chamador,
        };
        let resultado = match op {
            entrada::Operacao::Reverter => transacao::reverter(&ctx, &trava, agora),
            _ => transacao::atualizar(&ctx, &trava, agora),
        };
        Ok((ctx, resultado))
    });
    let (estado, resultado) = match resultado {
        Ok((ctx, resultado)) => (ctx.estado, resultado),
        Err(erro) => (Diretorio::abrir(Path::new(ESTADO))?, Err(erro)),
    };
    let sequencias =
        estado.ler::<Journal>("journal").ok().map_or((None, None), |j| (Some(j.estado.marca), j.sequencia_anterior));
    let (saida, motivo) =
        if resultado.is_ok() { (Resultado::Sucesso, Motivo::Solicitacao) } else { (Resultado::Falha, Motivo::Estado) };
    estado.auditar(&evento(saida, motivo, sequencias))?;
    resultado
}

fn aceitar(
    estado: &Diretorio,
    publicacao: &Diretorio,
    trava: &File,
    chamador: Identidade,
    verificacao: Option<Vec<u8>>,
    agora: i64,
) -> Result<()> {
    let operacao = Operacao::Aceitar;
    let sequencias =
        || estado.ler::<Journal>("journal").ok().map_or((None, None), |j| (Some(j.estado.marca), j.sequencia_anterior));
    let evento = |resultado, motivo, (sequencia_atual, sequencia_anterior)| Evento {
        horario_unix: agora,
        uid: chamador.uid(),
        origem: chamador.origem(),
        operacao,
        sequencia_atual,
        sequencia_anterior,
        resultado,
        motivo,
    };
    if let Some(bytes) = verificacao {
        estado.inicio(&evento(Resultado::Inicio, Motivo::Solicitacao, sequencias()))?;
        let resultado = (|| -> Result<()> {
            let pedido: super::verificacao::Pedido = serde_json::from_slice(&bytes)?;
            super::verificacao::registrar(estado, publicacao, trava, pedido, agora)
        })();
        let auditoria = estado.auditar(&evento(
            if resultado.is_ok() { Resultado::Sucesso } else { Resultado::Recusada },
            Motivo::Solicitacao,
            sequencias(),
        ));
        if auditoria.is_err() && resultado.is_ok() {
            let journal: Journal = estado.ler("journal")?;
            if super::verificacao::limpar(estado, &journal).is_err() {
                estado.bloquear_por_falha(trava)?;
            }
        }
        auditoria?;
        return resultado;
    }
    let journal: Journal = match estado.ler("journal") {
        Ok(journal) => journal,
        Err(erro) => {
            // Aceite recusado com journal ausente ou ilegível também é auditado.
            estado.inicio(&evento(Resultado::Inicio, Motivo::Solicitacao, sequencias()))?;
            estado.auditar(&evento(Resultado::Recusada, Motivo::Estado, sequencias()))?;
            return Err(erro);
        }
    };
    if matches!(journal.etapa, super::journal::Etapa::Pendente)
        && let Err(erro) = super::verificacao::conferir(estado, publicacao, trava, &journal, agora)
    {
        estado.inicio(&evento(Resultado::Inicio, Motivo::Solicitacao, sequencias()))?;
        estado.auditar(&evento(Resultado::Recusada, Motivo::Integridade, sequencias()))?;
        return Err(erro);
    }
    estado.aceitar_transacao(publicacao, trava, chamador, agora)?;
    super::verificacao::limpar(estado, &journal).map_err(|erro| {
        erro.context("aceite já registrado de forma durável; a limpeza do comprovante será retomada na recuperação")
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        fs::{self, OpenOptions},
        os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    };

    #[test]
    fn aceite_sem_journal_audita_inicio_e_recusa() -> Result<()> {
        let raiz = std::env::temp_dir().join(format!("mx365-exec-{}", uuid::Uuid::new_v4()));
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
        let agora = i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs())?;
        let resultado = aceitar(&estado, &publicacao, &trava, Identidade::AdminRoot, None, agora);
        assert!(resultado.is_err());
        let linhas = fs::read_to_string(raiz.join("estado").join("auditoria"))?;
        let eventos: Vec<serde_json::Value> = linhas.lines().map(serde_json::from_str).collect::<Result<_, _>>()?;
        assert_eq!(eventos.len(), 2);
        assert_eq!(eventos[0]["resultado"], "inicio");
        assert_eq!(eventos[1]["resultado"], "recusada");
        assert_eq!(eventos[1]["motivo"], "estado");
        assert!(eventos.iter().all(|evento| evento["operacao"] == "aceitar"));
        fs::remove_dir_all(raiz)?;
        Ok(())
    }
}
