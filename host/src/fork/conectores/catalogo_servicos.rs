//! Marketplace, fatia 1a: o catálogo de serviços como dado versionado no repositório.
//! Uma ficha descreve um serviço, os jeitos de conectar e os grupos de liberação. **Nunca guarda segredo**:
//! nem chave de usuário, nem token, nem `client_secret`, nem URL privada. O desenho aprovado está em
//! `docs/propostas/marketplace-20261113/conexoes-contas-skills.md`.
//!
//! O catálogo só diz o que é possível. Quem decide o que uma conta pode fazer é o Hub, a partir dos grupos que a
//! pessoa ligou ([`TipoConexao::liberacoes_validas`]): URL, escopos e ferramentas vêm sempre da ficha, nunca do
//! pedido do app.
use anyhow::{Result, bail, ensure};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Decisão do dono (16/11/2026): ficha conferida há mais de 90 dias não conecta e o teste do catálogo reprova.
pub(crate) const PRAZO_CONFERENCIA_DIAS: i64 = 90;

/// Jeitos de conectar do desenho. `OficialApp` e `Local` entram nas fatias 7 e seguintes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code, reason = "tipos do desenho aprovado que ainda não têm ficha")]
pub(crate) enum Tipo {
    OficialOauth,
    OficialChave,
    OficialApp,
    Local,
}

/// Como a pessoa entra. O registro OAuth é sempre sem segredo no binário (decisão de 16/11).
#[derive(Debug, Serialize)]
#[serde(tag = "modo", rename_all = "snake_case")]
#[allow(dead_code, reason = "a chave pelo cartão seguro ainda não tem ficha disponível")]
pub(crate) enum Auth {
    /// `registro`: "dcr" (o único que o Hub faz hoje).
    Oauth { registro: &'static str },
    /// `destino`: "header" ou "env"; `nome` é o nome do campo, nunca o valor.
    #[serde(rename_all = "camelCase")]
    /// `prefixo`: o que vem antes da chave no valor do cabeçalho (ex.: `Bearer `); o app o junta ao que a pessoa cola.
    Chave { destino: &'static str, nome: &'static str, prefixo: Option<&'static str>, como_criar_leitura: &'static str },
    /// O app OAuth da PRÓPRIA pessoa (ID e chave secreta dela, guardados por Space no cofre; nada nosso no código).
    /// `provedor` agrupa os serviços na tela ("google"); `emissor` é o servidor de login fixado pela ficha (o Hub só
    /// aceita esse); `guia` são os passos para a pessoa criar o app, com os links como dado.
    #[serde(rename_all = "camelCase")]
    AppProprio {
        provedor: &'static str,
        emissor: &'static str,
        guia: &'static [PassoDoGuia],
        #[serde(skip_serializing_if = "Option::is_none")]
        configuracao: Option<AppProprioConfig>,
    },
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppProprioConfig {
    pub cadastro: &'static str,
    #[serde(skip)]
    pub recurso: &'static str,
    #[serde(skip)]
    pub autorizacao: &'static str,
    #[serde(skip)]
    pub token: &'static str,
}

/// Um passo do guia "crie o seu app": o `id` escolhe o texto na tela; `url` é o link oficial (https), se houver.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PassoDoGuia {
    pub id: &'static str,
    pub url: Option<&'static str>,
}

/// Categorias do catálogo (lista fechada, decisão do dono de 18/11/2026). O app mostra nesta ordem.
pub(crate) const CATEGORIAS: &[&str] = &[
    "E-commerce",
    "Projetos",
    "Código",
    "Dados",
    "Comunicação",
    "Vendas e CRM",
    "Marketing",
    "Finanças e pagamentos",
    "Design e mídia",
    "Produtividade",
    "Pesquisa e web",
    "IA e automação",
    "Infraestrutura",
    "Agregadores",
];

/// Quem contém os efeitos do grupo: o fornecedor limita a leitura ou o Hub exige liberação de cada ação.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Imposicao {
    /// O endereço do MCP já é só de leitura (`/readonly`, `read_only=true`).
    Url,
    /// O Hub pede só escopos de leitura e o fornecedor recusa escrita sem eles.
    Escopo,
    /// O serviço não tem nenhuma ferramenta que escreve (ex.: só documentação ou busca).
    Natureza,
    Filtro,
}

/// Um bloco de permissão que a pessoa liga ou desliga.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Grupo {
    pub id: &'static str,
    /// Texto da tela ("Ler", "Criar e editar").
    pub rotulo: &'static str,
    /// Escopos OAuth exatos que o Hub pede para este grupo; nunca a lista inteira do fornecedor.
    pub escopos: &'static [&'static str],
    /// Endereço do MCP que impõe este grupo (ex.: `/readonly`). Entre os grupos ligados vale o último declarado.
    pub url: Option<&'static str>,
    /// Ferramentas elegíveis. Vazio = o grupo não declara uma lista; não libera nenhuma ação por si só.
    pub ferramentas: &'static [&'static str],
    /// Leitura automática: só o par nome + digest curado aqui passa sem a pessoa ligar. O digest que o fornecedor
    /// mostrar tem de ser exatamente o curado; nome sem digest curado nunca passa sozinho. Não vai para a tela.
    #[serde(skip)]
    pub curadas: &'static [Curada],
    /// Pré-marcado ao conectar (o padrão seguro, em geral só leitura).
    pub padrao: bool,
    pub disponivel: bool,
    /// Contenção declarada, obrigatória em grupo disponível (conferida ao carregar a ficha).
    pub imposicao: Option<Imposicao>,
}

/// Definição curada de uma ferramenta de leitura: o nome e o `schema_digest` (nome, título, descrição, entrada, saída e
/// anotações) que a ficha conferiu. Mudou qualquer parte no fornecedor, deixa de passar sozinha.
#[derive(Debug)]
pub(crate) struct Curada {
    pub ferramenta: &'static str,
    pub digest: &'static str,
}

/// Ferramenta que só a pessoa liga, por conta e por bot, com confirmação (fatia 2).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code, reason = "as fichas com escrita entram depois do filtro de ações perigosas")]
pub(crate) struct Perigosa {
    pub ferramenta: &'static str,
    pub rotulo: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TipoConexao {
    /// Ex.: `linear.oficial_oauth`.
    pub id: &'static str,
    pub tipo: Tipo,
    pub rotulo: &'static str,
    pub mantido_por: &'static str,
    /// Endereço fixo do MCP oficial: a pessoa nunca o edita.
    pub url: &'static str,
    pub auth: Auth,
    pub grupos: &'static [Grupo],
    pub perigosas: &'static [Perigosa],
    pub recomendado: bool,
    pub disponivel: bool,
    pub motivo_indisponivel: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Links {
    pub privacidade: &'static str,
    pub termos: &'static str,
}

/// Skill que o fornecedor publica (só link: nada é instalado daqui). `aproveitavel`: licença MIT, Apache ou BSD, que a
/// Mexerica pode usar como base registrando em `THIRD_PARTY.md`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillOficial {
    pub url: &'static str,
    /// `fornecedor` (site ou repositório do próprio fornecedor) ou `skills_sh`.
    pub origem: &'static str,
    pub licenca: &'static str,
    pub aproveitavel: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Skill do catálogo curado que a ficha sugere depois de conectar: só texto, nada liga sozinho.
pub(crate) struct SkillRef {
    pub id: &'static str,
    pub titulo: &'static str,
    pub resumo: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Servico {
    pub id: &'static str,
    /// A chave da logo do serviço no app (arquivo local em `assets/marcas/`); o app cai no globo se não tiver arquivo.
    pub marca: &'static str,
    pub nome: &'static str,
    pub categoria: &'static str,
    pub descricao: &'static str,
    pub exemplos: &'static [&'static str],
    /// Página do fornecedor (https).
    pub site: &'static str,
    /// Documentação do conector do fornecedor, quando há uma página só dele.
    pub docs: Option<&'static str>,
    pub links: Links,
    /// `AAAA-MM-DD`: quando alguém leu as fontes oficiais pela última vez.
    pub conferida_em: &'static str,
    pub fontes: &'static [&'static str],
    /// Aviso que a ficha mostra antes de conectar (ex.: os dados passam por um intermediário).
    pub aviso: Option<&'static str>,
    pub tipos: &'static [TipoConexao],
    pub skills: &'static [SkillRef],
    /// Skills que o fornecedor publica: a ficha só mostra o link e a licença.
    pub skills_oficiais: &'static [SkillOficial],
    /// Serviço sem nenhum jeito de conectar hoje: o motivo curto ("em breve", sem data).
    pub em_breve: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Catalogo {
    pub versao: u8,
    pub prazo_conferencia_dias: i64,
    /// As categorias na ordem em que o app as mostra (lista fechada).
    pub categorias: &'static [&'static str],
    pub servicos: &'static [Servico],
}

pub(crate) fn publico() -> Catalogo {
    Catalogo {
        versao: 3,
        prazo_conferencia_dias: PRAZO_CONFERENCIA_DIAS,
        categorias: CATEGORIAS,
        servicos: super::catalogo_fichas::servicos(),
    }
}

#[cfg(test)]
pub(crate) fn fixture_filtro() -> &'static Servico {
    static FICHA: std::sync::LazyLock<Servico> = std::sync::LazyLock::new(|| {
        super::catalogo_json::ler(include_str!("../../../tests/fixtures/catalogo-filtro.json")).unwrap()
    });
    &FICHA
}

#[cfg(test)]
pub(crate) fn fixture_app_proprio() -> &'static Servico {
    static FICHA: std::sync::LazyLock<Servico> = std::sync::LazyLock::new(|| {
        super::catalogo_json::ler(include_str!("../../../tests/fixtures/catalogo-app-proprio.json")).unwrap()
    });
    &FICHA
}

pub(crate) fn servico(id: &str) -> Option<&'static Servico> {
    super::catalogo_fichas::servicos().iter().find(|s| s.id == id)
}

/// Dias entre a conferência e `hoje`. Data ilegível conta como vencida (fail-closed).
pub(crate) fn dias_desde_conferencia(conferida_em: &str, hoje: NaiveDate) -> i64 {
    NaiveDate::parse_from_str(conferida_em, "%Y-%m-%d").map_or(i64::MAX, |d| (hoje - d).num_days())
}

pub(crate) fn hoje() -> NaiveDate {
    chrono::Utc::now().date_naive()
}

impl Servico {
    pub(crate) fn conferencia_vigente(&self, hoje: NaiveDate) -> bool {
        (0..=PRAZO_CONFERENCIA_DIAS).contains(&dias_desde_conferencia(self.conferida_em, hoje))
    }

    pub(crate) fn tipo(&self, id: &str) -> Option<&'static TipoConexao> {
        self.tipos.iter().find(|t| t.id == id)
    }

    /// Tem algum jeito de conectar agora (tipo e ao menos um grupo disponíveis).
    pub(crate) fn conectavel(&self) -> bool {
        self.tipos.iter().any(TipoConexao::conectavel)
    }

    /// Recusa ficha vencida ou sem o jeito pedido, antes de qualquer rede ou cofre.
    pub(crate) fn exigir_conectavel(&self, tipo: &str, hoje: NaiveDate) -> Result<&'static TipoConexao> {
        ensure!(
            self.conferencia_vigente(hoje),
            "ficha_vencida: a ficha de {} passou de {PRAZO_CONFERENCIA_DIAS} dias sem conferência.",
            self.nome
        );
        let tipo =
            self.tipo(tipo).ok_or_else(|| anyhow::anyhow!("tipo_desconhecido: {} não tem esse jeito.", self.nome))?;
        ensure!(
            tipo.conectavel(),
            "tipo_indisponivel: {}",
            tipo.motivo_indisponivel.unwrap_or("este jeito de conectar ainda não está disponível.")
        );
        Ok(tipo)
    }
}

impl TipoConexao {
    pub(crate) fn conectavel(&self) -> bool {
        self.disponivel && self.grupos.iter().any(|g| g.disponivel)
    }

    fn grupo(&self, id: &str) -> Option<&'static Grupo> {
        self.grupos.iter().find(|g| g.id == id)
    }

    /// Liberações pedidas pelo app, conferidas contra a ficha. Sem pedido, vale o padrão da ficha.
    /// Devolve os ids na ordem da ficha, sem repetição; grupo desconhecido ou indisponível recusa tudo.
    pub(crate) fn liberacoes_validas(&self, pedidas: Option<&[String]>) -> Result<Vec<String>> {
        let escolhidas: BTreeSet<&str> = match pedidas {
            None => self.grupos.iter().filter(|g| g.padrao && g.disponivel).map(|g| g.id).collect(),
            Some(pedidas) => {
                let mut set = BTreeSet::new();
                for id in pedidas {
                    let Some(grupo) = self.grupo(id) else { bail!("liberacao_desconhecida: {id}") };
                    ensure!(grupo.disponivel, "liberacao_indisponivel: {}", grupo.rotulo);
                    set.insert(grupo.id);
                }
                set
            }
        };
        ensure!(!escolhidas.is_empty(), "liberacao_obrigatoria: escolha ao menos uma liberação.");
        Ok(self.grupos.iter().filter(|g| escolhidas.contains(g.id)).map(|g| g.id.to_owned()).collect())
    }

    fn ligados<'a>(&'a self, liberacoes: &'a [String]) -> impl Iterator<Item = &'static Grupo> + 'a {
        self.grupos.iter().filter(|g| liberacoes.iter().any(|l| l == g.id))
    }

    pub(crate) fn exige_filtro(&self, liberacoes: &[String]) -> bool {
        self.ligados(liberacoes).any(|g| g.imposicao == Some(Imposicao::Filtro))
    }

    /// Endereço do MCP para os grupos ligados: o do último grupo que declara um, senão o da ficha.
    pub(crate) fn url_efetiva(&self, liberacoes: &[String]) -> &'static str {
        self.ligados(liberacoes).filter_map(|g| g.url).last().unwrap_or(self.url)
    }

    /// Escopos OAuth a pedir: a união exata dos grupos ligados, na ordem da ficha. Nunca os do fornecedor inteiro.
    pub(crate) fn escopos_efetivos(&self, liberacoes: &[String]) -> Vec<String> {
        let mut vistos = BTreeSet::new();
        self.ligados(liberacoes)
            .flat_map(|g| g.escopos.iter())
            .filter(|escopo| vistos.insert(**escopo))
            .map(|escopo| (*escopo).to_owned())
            .collect()
    }

    /// O jeito de conectar e todos os grupos ligados continuam disponíveis na ficha vigente (e há ao menos um).
    /// Cada id salvo é conferido contra a ficha de agora: um grupo que sumiu ou ficou indisponível fecha o perfil
    /// inteiro, mesmo que outro grupo do mesmo perfil continue válido.
    pub(crate) fn perfil_disponivel(&self, liberacoes: &[String]) -> bool {
        self.disponivel
            && !liberacoes.is_empty()
            && liberacoes.iter().all(|id| self.grupo(id).is_some_and(|g| g.disponivel))
    }

    /// Pares nome + digest curados pelos grupos ligados. Com filtro, nem a curadoria de outro grupo do perfil se aplica.
    /// O mesmo nome com dois digests diferentes é ambíguo: nenhum dos dois passa.
    pub(crate) fn ferramentas_curadas(&self, liberacoes: &[String]) -> BTreeMap<&'static str, &'static str> {
        if self.exige_filtro(liberacoes) {
            return BTreeMap::new();
        }
        let mut vistas: BTreeMap<&'static str, Option<&'static str>> = BTreeMap::new();
        for curada in self.ligados(liberacoes).flat_map(|g| g.curadas.iter()) {
            let antiga = vistas.insert(curada.ferramenta, Some(curada.digest));
            if antiga.is_some_and(|d| d != Some(curada.digest)) {
                vistas.insert(curada.ferramenta, None);
            }
        }
        vistas.into_iter().filter_map(|(nome, digest)| digest.map(|d| (nome, d))).collect()
    }

    /// `None`: nenhum grupo ligado restringe nomes. `Some`: só estes nomes são elegíveis;
    /// um grupo sem lista misturado a grupos com lista não libera nada por conta própria (fail-closed).
    pub(crate) fn ferramentas_permitidas(&self, liberacoes: &[String]) -> Option<BTreeSet<&'static str>> {
        let ligados: Vec<_> = self.ligados(liberacoes).collect();
        if ligados.iter().all(|g| g.ferramentas.is_empty()) {
            return None;
        }
        Some(ligados.iter().flat_map(|g| g.ferramentas.iter().copied()).collect())
    }
}

/// Deixa passar só as ferramentas permitidas de um `tools/list`.
pub(crate) fn filtrar_ferramentas(
    tools: &[serde_json::Value],
    permitidas: Option<&BTreeSet<&str>>,
) -> Vec<serde_json::Value> {
    tools
        .iter()
        .filter(|t| permitidas.is_none_or(|set| t["name"].as_str().is_some_and(|name| set.contains(name))))
        .cloned()
        .collect()
}
