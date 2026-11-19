//! Instalação limpa do Hub no layout gerido (#812), executada como o UID do Hub pela
//! etapa administrativa (`donnabot-instalar-atualizacao instalar --hub-novo`), depois que
//! o wrapper publicou a primeira release autenticada (TUF) e abriu a janela pendente.
//!
//! Não existe Hub legado: a unidade `systemd --user` nasce apontando para
//! `/opt/donnabot/releases/ativo/donnabot-host`, o Hub cria o próprio DB, a F3 roda dentro
//! da janela da primeira migração (raízes copiadas e marca `d330` gravada, como no
//! coordenador) e só então, saudável, a janela é aceita. Falha em qualquer etapa desfaz a
//! unidade e chama `--reverter`, sem Hub em execução.
use super::coordenador_atualizacao::{
    ATIVO, BIND_GERIDO, UNIDADE, conferir_binario_novo, conferir_hello, dir_unidade, diretorio_privado, exec_start,
    exigir_porta_livre, exigir_protocolo_wrapper, iniciar, migrar_raizes, parar, porta_exec_start,
    registrar_verificacao, release_esperada, saudavel, sudo, systemctl, uid,
};
use anyhow::{Context, Result, bail, ensure};
use std::{fs, path::Path};

/// Marca, no diretório privado do Hub, de que os dados nasceram nesta instalação limpa
/// (permite retomar uma tentativa anterior sem confundir com dados legados).
const MARCA: &str = "instalacao-gerida";

fn fase_publica() -> Result<String> {
    let bytes = super::publicacao_atualizacao::ler(super::publicacao_atualizacao::Nome::Status)
        .context("status público da atualização ausente; a etapa administrativa publica a primeira release antes")?;
    let status: serde_json::Value = serde_json::from_slice(&bytes)?;
    Ok(status["fase"].as_str().unwrap_or_default().to_owned())
}

fn unidade_gerida() -> Result<bool> {
    if !dir_unidade().join(UNIDADE).is_file() {
        return Ok(false);
    }
    Ok(exec_start()?.first().map(String::as_str) == Some(ATIVO))
}

fn marcada(raiz: &Path) -> bool {
    raiz.join(MARCA).is_file()
}

/// Unidades que esta instalação pode ter criado: a do layout gerido e, por defesa, a
/// do ambiente do binário (`preparar_executavel` usa o nome do ambiente).
fn candidatas() -> Vec<&'static str> {
    let ambiente = crate::environment::Environment::current().systemd_unit();
    if ambiente == UNIDADE { vec![UNIDADE] } else { vec![UNIDADE, ambiente] }
}

/// Estado inicial de cada candidata. A do layout gerido usa a visão do systemd (com
/// drop-ins); outra candidata só conta como desta instalação se não existia no início.
fn observar_candidatas() -> Vec<(&'static str, Result<Unidade>)> {
    candidatas()
        .into_iter()
        .map(|nome| {
            let estado = if nome == UNIDADE {
                observar_unidade()
            } else if dir_unidade().join(nome).exists() {
                Ok(Unidade::Outra)
            } else {
                Ok(Unidade::Ausente)
            };
            (nome, estado)
        })
        .collect()
}

/// Remove só o que a tentativa criou (ausente no início e presente agora) ou a unidade
/// gerida desta instalação; unidade alheia ou de estado inicial ilegível nunca é tocada.
fn unidades_a_remover(
    iniciais: &[(&'static str, Result<Unidade>)],
    existe: impl Fn(&str) -> bool,
) -> Vec<&'static str> {
    iniciais
        .iter()
        .filter(|(nome, inicial)| matches!(inicial, Ok(Unidade::Ausente | Unidade::Gerida)) && existe(nome))
        .map(|(nome, _)| *nome)
        .collect()
}

/// Desfaz a tentativa depois que o wrapper publicou a primeira release: cada unidade
/// desta instalação (gerida, ou criada agora, inclusive com o nome do ambiente dev) é
/// desabilitada, parada e removida; uma unidade alheia (Hub legado) nunca é tocada.
/// Depois `--reverter` (primeira migração: o seletor some; ele recusa se um Hub ainda
/// estiver em execução). Os dados que o Hub criou ficam, marcados como desta instalação.
fn desfazer(remover: &[&str]) -> Result<()> {
    for &nome in remover {
        ensure!(
            systemctl(&["disable", "--now", nome])?.status.success(),
            "não foi possível desabilitar a unidade {nome} desta instalação"
        );
        if nome == UNIDADE {
            parar()?;
        }
        match fs::remove_file(dir_unidade().join(nome)) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    if !remover.is_empty() {
        let _ = systemctl(&["daemon-reload"]);
    }
    ensure!(sudo(&["--reverter"])?, "--reverter recusado; recuperação administrativa necessária");
    Ok(())
}

/// O que a instalação limpa faz a partir do estado observado.
#[derive(Debug, PartialEq, Eq)]
enum Passo {
    /// Já aceita: só conferir a unidade gerida e a saúde.
    Conferir,
    /// Janela pendente da primeira publicação: instalar (ou retomar) a unidade gerida.
    Instalar,
}

/// Unidade `systemd --user` do Hub encontrada no login.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unidade {
    Ausente,
    /// Já aponta para o seletor gerido (retomada desta instalação ou já instalada).
    Gerida,
    /// Aponta para outro executável: Hub legado.
    Outra,
}

/// Banco do Hub no diretório de dados.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dados {
    Ausentes,
    /// Nasceram nesta instalação limpa (marca gravada antes da primeira partida).
    DestaInstalacao,
    /// Existem sem a marca: dados de um Hub legado.
    Alheios,
}

struct Observado<'a> {
    /// Binário de produção (`CODYNC_ENV=main`): o layout gerido usa nomes fixos de `main`.
    main: bool,
    fase: &'a str,
    primeira_publicacao: bool,
    unidade: Unidade,
    dados: Dados,
}

/// Decide sem efeitos: recusa qualquer sinal de Hub legado (unidade fora do par gerido ou
/// dados que não nasceram desta instalação) e qualquer estado fora da primeira janela.
fn decidir(o: &Observado) -> Result<Passo> {
    // O layout gerido (unidade, drop-in do guard, seletor) é fixo em `donnabot-host.service`;
    // um build dev gravaria `donnabot-dev-host.service` fora dele.
    ensure!(
        o.main,
        "a instalação gerida exige o binário de produção (compile com CODYNC_ENV=main); nada foi instalado"
    );
    match o.fase {
        "liberado" => {
            ensure!(o.unidade == Unidade::Gerida, "release aceita, mas a unidade do Hub não aponta para o par gerido");
            return Ok(Passo::Conferir);
        }
        "pendente" => (),
        outra => bail!("estado da atualização `{outra}` não permite instalar o Hub"),
    }
    ensure!(o.primeira_publicacao, "a instalação limpa só acontece na primeira publicação; use coordenar-atualizacao");
    ensure!(
        o.unidade != Unidade::Outra,
        "já existe uma unidade do Hub fora do layout gerido; use coordenar-atualizacao para migrar"
    );
    ensure!(
        o.dados != Dados::Alheios,
        "já existem dados do Hub; use coordenar-atualizacao para migrar uma instalação antiga"
    );
    Ok(Passo::Instalar)
}

pub(crate) fn executar(porta: u16, saude_segundos: u64) -> Result<()> {
    ensure!(uid() != 0, "a instalação do Hub executa como o usuário do Hub, nunca root");
    ensure!(super::motor_exec::configured(), "motor.conf ausente: rode antes a etapa administrativa");
    exigir_protocolo_wrapper()?;
    let fase = fase_publica()?;
    let primeira_publicacao =
        fase != "pendente" || super::backup_atualizacao::ler_certidao(uid())?.transacao().sequencia_anterior.is_none();
    // Janela pendente da primeira publicação: foi o wrapper desta instalação que a abriu.
    // A partir daqui, qualquer falha (inclusive recusa) desfaz a publicação.
    let publicada_aqui = fase == "pendente" && primeira_publicacao;
    let iniciais = observar_candidatas();
    let resultado = instalar(&fase, primeira_publicacao, porta, saude_segundos);
    match resultado {
        Ok(()) => Ok(()),
        Err(erro) if !publicada_aqui => Err(erro),
        Err(erro) => {
            // Só remove unidades desta instalação (gerida, ou criada agora).
            let remover = unidades_a_remover(&iniciais, |nome| dir_unidade().join(nome).exists());
            Err(match desfazer(&remover) {
                Ok(()) if !remover.is_empty() => {
                    erro.context(format!("instalação desfeita: {remover:?} removida(s) e release revertida"))
                }
                Ok(()) => erro.context("release revertida; nenhuma unidade desta instalação a remover"),
                Err(e) => erro.context(format!("e a volta também falhou: {e:#}")),
            })
        }
    }
}

fn observar_unidade() -> Result<Unidade> {
    Ok(match (dir_unidade().join(UNIDADE).exists(), unidade_gerida()?) {
        (false, _) => Unidade::Ausente,
        (true, true) => Unidade::Gerida,
        (true, false) => Unidade::Outra,
    })
}

fn instalar(fase: &str, primeira_publicacao: bool, porta: u16, saude_segundos: u64) -> Result<()> {
    let data = crate::service::data_dir();
    let raiz = data.join("atualizacao-privilegiada");
    let unidade = observar_unidade()?;
    let dados = match (data.join("codync.db").exists(), marcada(&raiz)) {
        (false, _) => Dados::Ausentes,
        (true, true) => Dados::DestaInstalacao,
        (true, false) => Dados::Alheios,
    };
    let main = crate::environment::Environment::current() == crate::environment::Environment::Main;
    let passo = decidir(&Observado { main, fase, primeira_publicacao, unidade, dados })?;
    if passo == Passo::Conferir {
        ensure!(saudavel(saude_segundos, true)?, "Hub gerido instalado, mas não está saudável");
        println!("instalação: Hub gerido já instalado e saudável");
        return Ok(());
    }
    crate::fork::private_fs::Directory::open(&data, true)?;
    diretorio_privado(&raiz)?;
    let identidade = std::env::var("MEXERICA_INSTALACAO_ID").context("identidade da tentativa ausente")?;
    ensure!(
        identidade.len() == 32 && identidade.bytes().all(|b| b.is_ascii_hexdigit()),
        "identidade da tentativa inválida"
    );
    fs::write(raiz.join(MARCA), identidade)?;
    fs::File::open(raiz.join(MARCA))?.sync_all()?;
    fs::File::open(&raiz)?.sync_all()?;
    // A unidade nasce no par gerido, só em loopback; o Hub cria o DB na primeira partida.
    crate::service::preparar_executavel(porta, ATIVO, Some(BIND_GERIDO))?;
    let argv = exec_start()?;
    ensure!(
        argv.first().map(String::as_str) == Some(ATIVO) && porta_exec_start(&argv)? == porta,
        "unidade instalada não corresponde ao seletor ou à porta esperada"
    );
    let certidao = super::backup_atualizacao::ler_certidao(uid())?;
    let release = release_esperada(&certidao.transacao().id)?;
    exigir_porta_livre(porta)?;
    conferir_binario_novo(&release)?;
    iniciar()?;
    ensure!(saudavel(saude_segundos, true)?, "Hub gerido não ficou saudável na primeira partida");
    conferir_hello(porta, &release.versao)?;
    // F3 na janela da primeira migração, com o serviço parado e a host.lock mantida.
    parar()?;
    let trava = crate::service::lock_host_at(&data.join("host.lock"))?;
    migrar_raizes(&data, &trava)?;
    super::motor_migration::migrar_com_trava(&data, &trava)?;
    drop(trava);
    exigir_porta_livre(porta)?;
    conferir_binario_novo(&release)?;
    iniciar()?;
    ensure!(saudavel(saude_segundos, true)?, "Hub gerido não ficou saudável depois da F3");
    conferir_hello(porta, &release.versao)?;
    registrar_verificacao(&release, porta)?;
    ensure!(sudo(&["--aceitar"])?, "aceite recusado");
    println!("instalação: Hub gerido instalado em {ATIVO}, saudável e aceito");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limpo(fase: &str) -> Observado<'_> {
        Observado { main: true, fase, primeira_publicacao: true, unidade: Unidade::Ausente, dados: Dados::Ausentes }
    }

    #[test]
    fn instalacao_limpa_so_sem_legado_e_na_primeira_janela() {
        assert_eq!(decidir(&limpo("pendente")).ok(), Some(Passo::Instalar));
        // Retomada de uma tentativa desta instalação (unidade gerida e dados marcados).
        let retomada = Observado { unidade: Unidade::Gerida, dados: Dados::DestaInstalacao, ..limpo("pendente") };
        assert_eq!(decidir(&retomada).ok(), Some(Passo::Instalar));
        // Hub legado: unidade própria ou dados que não nasceram aqui.
        assert!(decidir(&Observado { unidade: Unidade::Outra, ..limpo("pendente") }).is_err());
        assert!(decidir(&Observado { dados: Dados::Alheios, ..limpo("pendente") }).is_err());
        // Fora da primeira janela ou com bloqueio.
        assert!(decidir(&Observado { primeira_publicacao: false, ..limpo("pendente") }).is_err());
        assert!(decidir(&limpo("bloqueado")).is_err());
        // Já aceita: só confere, e exige a unidade gerida.
        assert!(decidir(&limpo("liberado")).is_err());
        let aceita = Observado { unidade: Unidade::Gerida, ..limpo("liberado") };
        assert_eq!(decidir(&aceita).ok(), Some(Passo::Conferir));
    }

    /// Build dev: recusa antes de criar unidade, para qualquer fase.
    #[test]
    fn instalacao_gerida_fora_de_main_recusa() {
        for fase in ["pendente", "liberado"] {
            let dev = Observado { main: false, unidade: Unidade::Gerida, ..limpo(fase) };
            let erro = decidir(&dev).err().map(|e| e.to_string()).unwrap_or_default();
            assert!(erro.contains("CODYNC_ENV=main"), "{fase}: {erro}");
        }
    }

    /// Prova da VM: a unidade dev criada nesta tentativa precisa sair na volta; unidade
    /// alheia ou de estado inicial ilegível nunca; ausente que segue ausente não conta.
    #[test]
    fn volta_remove_toda_unidade_criada_nesta_tentativa() {
        const DEV: &str = "donnabot-dev-host.service";
        let existe = |nomes: &'static [&'static str]| move |nome: &str| nomes.contains(&nome);
        let criada_dev = [(UNIDADE, Ok(Unidade::Ausente)), (DEV, Ok(Unidade::Ausente))];
        assert_eq!(unidades_a_remover(&criada_dev, existe(&[DEV])), vec![DEV]);
        assert_eq!(unidades_a_remover(&criada_dev, existe(&[UNIDADE, DEV])), vec![UNIDADE, DEV]);
        assert_eq!(unidades_a_remover(&criada_dev, existe(&[])), Vec::<&str>::new());
        let alheia = [(UNIDADE, Ok(Unidade::Gerida)), (DEV, Ok(Unidade::Outra))];
        assert_eq!(unidades_a_remover(&alheia, existe(&[UNIDADE, DEV])), vec![UNIDADE]);
        let ilegivel = [(UNIDADE, Err(anyhow::anyhow!("systemctl indisponível"))), (DEV, Ok(Unidade::Ausente))];
        assert_eq!(unidades_a_remover(&ilegivel, existe(&[UNIDADE, DEV])), vec![DEV]);
    }
}
