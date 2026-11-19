//! Comprovante root-owned separado: preserva o formato do journal do guard anterior.
use super::{
    armazenamento::Diretorio,
    journal::{Etapa, Journal, Recuperacao},
    snapshot::{self, Arquivo},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::File;

const NOME: &str = "verificacao-partida";

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pedido {
    pub transacao: String,
    pub hub_sha256: String,
    pub versao: String,
    pub porta: u16,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Registro {
    pedido: Pedido,
    instante: i64,
}

fn conferir_vinculo(pedido: &Pedido, transacao: &str, hash: &str, versao: &str) -> Result<()> {
    ensure!(pedido.transacao == transacao, "transação da verificação divergente");
    ensure!(pedido.hub_sha256 == hash, "hash da verificação divergente");
    ensure!(pedido.versao == versao && pedido.porta != 0, "versão ou porta da verificação inválida");
    Ok(())
}

fn conferir_pedido(pedido: &Pedido, journal: &Journal, manifesto: &super::manifesto::Manifesto) -> Result<()> {
    conferir_vinculo(pedido, &journal.transacao, &manifesto.hub.sha256, &manifesto.versao)
}

fn conferir_slot(publicacao: &Diretorio, journal: &Journal, manifesto: &super::manifesto::Manifesto) -> Result<()> {
    ensure!(publicacao.seletor()? == Some(journal.slot_novo), "slot selecionado divergente");
    let slot = publicacao.abrir_slot(journal.slot_novo)?;
    let mut hub = super::transacao::abrir_fixo(&slot, c"donnabot-host")?.context("Hub selecionado ausente")?;
    let esperado = Arquivo { tamanho: manifesto.hub.tamanho, sha256: manifesto.hub.sha256.clone() };
    snapshot::verificar_arquivo(&mut hub, &esperado, 0, 0o755)
}

pub fn registrar(estado: &Diretorio, publicacao: &Diretorio, trava: &File, pedido: Pedido, agora: i64) -> Result<()> {
    let plano = super::recuperacao::validar(estado, publicacao, trava, agora)?;
    ensure!(
        matches!(plano.acao, Recuperacao::Pendente) && matches!(plano.journal.etapa, Etapa::Pendente),
        "transação não está pendente"
    );
    let manifesto = estado.manifesto_transacao(trava, &plano.journal, agora)?;
    conferir_pedido(&pedido, &plano.journal, &manifesto)?;
    conferir_slot(publicacao, &plano.journal, &manifesto)?;
    estado.abrir_transacao(&plano.journal.transacao)?.gravar(NOME, &Registro { pedido, instante: agora })
}

pub fn conferir(estado: &Diretorio, publicacao: &Diretorio, trava: &File, journal: &Journal, agora: i64) -> Result<()> {
    let manifesto = estado.manifesto_transacao(trava, journal, agora)?;
    let registro: Registro =
        estado.abrir_transacao(&journal.transacao)?.ler(NOME).context("verificação de partida ausente ou inválida")?;
    conferir_pedido(&registro.pedido, journal, &manifesto)?;
    let inicio = journal.publicado_em.context("início da transação ausente")?;
    ensure!(registro.instante >= inicio && registro.instante <= agora, "instante da verificação inválido");
    conferir_slot(publicacao, journal, &manifesto)
}

pub fn limpar(estado: &Diretorio, journal: &Journal) -> Result<()> {
    estado.abrir_transacao(&journal.transacao)?.remover_regular(NOME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    };

    fn pedido() -> Pedido {
        Pedido { transacao: "a".repeat(64), hub_sha256: "b".repeat(64), versao: "1.2.3".into(), porta: 7777 }
    }

    #[test]
    fn vinculo_exige_transacao_hash_versao_e_porta() {
        let p = pedido();
        assert!(conferir_vinculo(&p, &"a".repeat(64), &"b".repeat(64), "1.2.3").is_ok());
        assert!(conferir_vinculo(&p, &"c".repeat(64), &"b".repeat(64), "1.2.3").is_err());
        assert!(conferir_vinculo(&p, &"a".repeat(64), &"c".repeat(64), "1.2.3").is_err());
        assert!(conferir_vinculo(&p, &"a".repeat(64), &"b".repeat(64), "1.2.4").is_err());
        let mut p = pedido();
        p.porta = 0;
        assert!(conferir_vinculo(&p, &"a".repeat(64), &"b".repeat(64), "1.2.3").is_err());
    }

    #[test]
    fn registro_recusa_modo_link_e_conteudo_adulterado() -> Result<()> {
        let raiz = std::env::temp_dir().join(format!("mx-verificacao-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&raiz)?;
        let dir = Diretorio::abrir_teste(&raiz)?;
        dir.gravar(NOME, &Registro { pedido: pedido(), instante: 1 })?;
        let registro: Registro = dir.ler(NOME)?;
        assert_eq!(registro.pedido, pedido());
        fs::set_permissions(raiz.join(NOME), fs::Permissions::from_mode(0o644))?;
        assert!(dir.ler::<Registro>(NOME).is_err());
        fs::set_permissions(raiz.join(NOME), fs::Permissions::from_mode(0o600))?;
        fs::write(raiz.join(NOME), b"{\"pedido\":{}}")?;
        assert!(dir.ler::<Registro>(NOME).is_err());
        dir.remover_regular(NOME)?;
        symlink("outro", raiz.join(NOME))?;
        assert!(dir.ler::<Registro>(NOME).is_err());
        fs::remove_file(raiz.join(NOME))?;
        fs::remove_dir_all(raiz)?;
        Ok(())
    }

    /// Só VM como root: o inode deve ser recusado quando pertence a outro UID.
    #[test]
    fn registro_recusa_dono_distinto_somente_root() -> Result<()> {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: consulta sem efeitos colaterais.
        if unsafe { libc::geteuid() } != 0 {
            return Ok(());
        }
        let raiz = std::env::temp_dir().join(format!("mx-verificacao-dono-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&raiz)?;
        let dir = Diretorio::abrir_teste(&raiz)?;
        dir.gravar(NOME, &Registro { pedido: pedido(), instante: 1 })?;
        let nome = std::ffi::CString::new(raiz.join(NOME).as_os_str().as_bytes())?;
        // SAFETY: caminho próprio criado exclusivamente neste teste.
        ensure!(unsafe { libc::chown(nome.as_ptr(), 65534, 0) } == 0, "chown de teste falhou");
        assert!(dir.ler::<Registro>(NOME).is_err());
        fs::remove_file(raiz.join(NOME))?;
        fs::remove_dir_all(raiz)?;
        Ok(())
    }
}
