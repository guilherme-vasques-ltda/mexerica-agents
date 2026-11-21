# Modelos por função

O Hub guarda seleções gerais para trabalhos auxiliares, separadas do modelo de
cada bot. O Bot de fundo usa as seleções de Memória e Títulos; as demais funções
ainda aguardam seus consumidores. O [painel Bot de fundo](bot-de-fundo.md) reúne
as escolhas e o histórico curto, conservando a autoridade de cada API.

São 13 funções: Visão, Compactar contexto, Mensagens durante o trabalho, Hub de
skills, Selecionar skills, Aprovações, Roteamento de ferramentas MCP, Organizar
memória, Títulos, Rascunho de novo bot, Conversa por voz, Ditado e Ler respostas.
As três de voz aparecem como **Gerenciado em Voz**, sem seleção editável aqui.
Aprovações guarda apenas uma seleção: o juiz começa observando; ativação e
promoção pertencem à política de regras, nunca ao ato de salvar um modelo.

## Seleção e disponibilidade

Automático conserva vínculos já definidos e completa uma seleção ainda sem modelo
quando a conta fica disponível. Para Memória e Títulos, prefere o login Claude
local verificado; havendo mais de uma conta equivalente, o ID estável desempata.
As escolhas explícitas são preservadas. Se Automático não tem conta nem vínculo,
a ausência de conta elegível não significa que uma conta foi apagada: o painel
orienta conectar a assinatura Claude no Pessoal para Memória e Títulos. Conta
explicitamente escolhida e ausente continua sendo uma referência não cadastrada;
a mensagem não altera escolhas ou revisões. Outras funções mantêm sua própria regra
de escolha entre contas. Contas pertencem ao dono. No personalizado,
`accountId`, `modelId` e `modelOptions` formam uma seleção indivisível. Esforço é
uma opção nativa em `modelOptions`, por exemplo `reasoning_effort`; `max` não é
convertido em `high`. Trocar o modelo não altera poderes, permissões ou sandbox.
Ao salvar, a conta precisa existir no cadastro do dono, o modelo no catálogo
daquela conta e cada opção/valor entre os anunciados. Os identificadores têm
formato e tamanho limitados; segredo, referência privada, URL e caminho são
recusados sem ecoar o valor. Quando uma conta é indicada no Automático, ela deve
existir e pertencer ao motor da receita. Uma referência só pode ficar inválida por remoção posterior.

Somente o rascunho em Automático declara a alternativa: Codex → Haiku da
conta vinculada, do mesmo titular, só por lentidão após 15 segundos, com
encerramento antes da alternativa e 25 segundos totais. Não vale por cota,
recusa ou autorização negada. Personalizado desliga essa alternativa. A execução
e a medição desses limites ainda serão implementadas.

O cadastro comum de contas é lido sem sondar motores, rede ou cofre. Estado
`disconnected`/`unavailable` e cobrança desconhecida são indisponíveis. Conta
no estado `unverified` com cobrança desconhecida informa `billing_unknown`; assinatura sem
comprovação informa `subscription_unverified`, inclusive no estado `unverified`.
Selecionar API grava uma referência, mas tarefas de fundo ainda não a executam:
falta comprovar o catálogo por revisão da conta. A resposta informa `api_coming_soon`. Não há troca automática para API.
Remover uma conta conserva a referência inválida até ação humana.

### Conta desconectada em Memória e Títulos

No modo automático de **Organizar memória** e **Nomes**, o Hub pula contas
desconectadas, indisponíveis, não confirmadas, de API paga ou sem modelo
comprovado (Haiku anunciado) e usa outra conta Claude do dono com assinatura
comprovada, conectada e com modelo anunciado. Essa troca vale só para o Pessoal, dono
do cadastro global de contas; quando houver contas por Space, a
escolha passa a filtrar pelo Space do bot; o teto de gasto continua conferido
antes de cada tarefa. Se nenhuma servir, a seleção guarda o motivo da conta
escolhida: `account_disconnected` quando ela está desconectada (nunca
`default_model_unknown`), e volta sozinha quando a conta reconecta ou o dono
escolhe outra. A escolha personalizada nunca é trocada.

Na tela, `functionModels` traz em `functions[]` (função `memory_consolidation`)
`resolution.unavailable.reason` e `resolution.unavailable.message`, este com o
texto pronto: "Organizar memória: a conta {rótulo} está desconectada; reconecte
em Contas de IA ou escolha outra em Bot de fundo > Modelos por função" ou, sem
modelo, "Organizar memória precisa de um modelo: escolha em Bot de fundo >
Modelos por função". O rótulo é `accounts[].label` da conta em
`selection.accountId`.

O cadastro não comprova o modelo padrão Codex. A descoberta do catálogo de fundo
e os jobs de Memória/Títulos usam o conector auditado fixado em 0.94.0, com nome
e versão exatos, sem fallback; isso não se estende às conversas normais. O pin
não substitui a comprovação de identidade, assinatura, cota ou modelos. Assinatura
com prova do plano, sem identidade de cota, verificação e modelos, continua
inelegível. Veja os [limites do conector de fundo](bot-de-fundo.md#conector-e-conta-indisponíveis).
Para tarefas de fundo Claude, o conector anuncia o catálogo e confirma modelo e
esforço antes do pedido; a conta
e seu uso são conferidos antes de cada execução. Quando anunciado, Haiku é
um alias com versão administrada pelo provedor. A receita pede esforço baixo
somente quando anunciado; do contrário usa Padrão do modelo. Os demais resolvedores exigem evidências confiáveis de capacidade e cota; dados
enviados na consulta nunca servem de comprovação. A consulta administrativa
não executa tarefas nem garante que a próxima tentativa terá cota disponível.

## API administrativa local

Os métodos são exclusivos do canal administrativo local do dono, fora do MCP.
Não aceitam `human`, titular, origem, consentimento ou evidências do modelo:

- `functionModels {}`: documento, catálogo, seleções e estados, sem segredos,
  caminhos de perfil, referência de cofre ou identidade privada de cota.
  `accountsIssue` informa `accounts_unavailable` quando o cadastro está ilegível
  (normalmente é `null`). A seleção já gravada é preservada; sem documento inicial,
  `document` e seleções ficam `null`. O Hub continua abrindo e uma próxima leitura
  tenta inicializar novamente, sem substituir cadastro ou seleção.
- `saveFunctionModel {expectedRevision, functionId, selection}`: grava
  `{mode:"custom", accountId, modelId, modelOptions}` ou
  `{mode:"automatic", accountId?, alternativeAccountId?}`. A alternativa só é
  aceita no rascunho. O servidor calcula os vínculos automáticos; ausência de
  comprovação fica visível, sem execução. O campo de conta permite a escolha
  humana entre contas da mesma marca.
- `restoreFunctionModel {expectedRevision, functionId}`: recalcula uma receita.
- `restoreFunctionModels {expectedRevision}`: recalcula todas as dez receitas
  editáveis; mantém a voz gerenciada no lugar próprio.
- `checkFunctionModel {functionId}`: só resolve; devolve seleção conferida ou
  indisponibilidade tipada. Não lança processo, não usa rede e não testa chamada.

Para Memória e Títulos, `consumer` é `background_worker` e `execution` é
`partial`. Uma seleção elegível é conferida novamente no início do trabalho.
Assinatura Codex retorna `codex_subscription_unavailable`; chave API retorna
`api_coming_soon`, ambos com `message` simples; Memória com conta desconectada ou sem modelo também traz `message` (ver acima). `backgroundHistory {}` é local,
limitado e mostra resultados, falhas, modelos e uso informados, sem conversas.
Não existe `testFunctionModel`. Mutações incrementam a revisão
com CAS em transação SQLite e emitem `functionModelsChanged {revision}` depois
da gravação. Revisão antiga é recusada, sem sobrescrever edição de outra janela.
O documento inclui `schemaVersion`, `revision`, `defaultsRevision`, `updatedBy`
e `updatedAt`. A migração inicial é idempotente e não altera contas ou bots.
Configuração inválida ou erro de leitura falham explicitamente; não restauram
padrões nem selecionam outra conta. Cadastro ilegível adia a migração sem impedir
o boot; salvar exige cadastro legível para validar a intenção. O estado fica no
SQLite do Hub, na chave
`fork.function-models.v1`, fora do mirror do app.

**Limitação conhecida:** `Caller::Local` não prova pessoa. Um bot com shell sob o mesmo
usuário do Hub pode alcançar banco e token administrativo. Restringir a API e
não anunciá-la no MCP evita alteração acidental; isolamento exige a separação
prevista dos usuários do sistema. Esta etapa não implementa login por pessoa.
O cadastro contém somente contas do único dono: a conferência atual é do
contexto administrativo e da conta exata por ID, sem teste de isolamento entre titulares.
