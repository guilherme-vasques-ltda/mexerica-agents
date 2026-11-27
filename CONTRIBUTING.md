# Como contribuir

Obrigado por ajudar. Este arquivo diz o que um PR precisa ter para ser aceito. As regras de
código, segurança e licenças estão no [AGENTS.md](AGENTS.md), que vale para pessoas e para agentes.

## Antes de começar

- **Dúvida ou ideia:** abra uma issue. Mudança grande também começa por uma issue, para
  combinar o caminho antes do código.
- **Falha de segurança:** nunca em issue pública. Veja [SECURITY.md](SECURITY.md).
- **Convivência:** vale o [Código de Conduta](CODE_OF_CONDUCT.md).

## Montar o ambiente

Requisitos e comandos de build e teste de cada parte estão em
[AGENTS.md → Construir e testar](AGENTS.md#construir-e-testar). Detalhes do app e do Hub em
[docs/guides/development.md](docs/guides/development.md); gerar o app no Mac em
[docs/guides/app-mac-local.md](docs/guides/app-mac-local.md).

Rode só a verificação da parte que você mudou.

## O PR

- **Pequeno e com um assunto só.** Um PR que corrige um erro, muda uma tela e reorganiza
  arquivos vai voltar pedindo para dividir.
- **Preencha o modelo** de [.github/pull_request_template.md](.github/pull_request_template.md):
  problema, solução, como foi comprovado e o que ficou sem provar.
- **Texto publicado** (título, descrição, commit, branch, comentário) segue a regra do
  [AGENTS.md](AGENTS.md#convenções-de-código). Antes de publicar, rode
  `node tools/ci-bastidor.mjs local` ([guia](docs/guides/bastidor.md)).
- **Mudança de tela:** prints de antes e depois na descrição, com dados fictícios. Se a mudança
  tem movimento, um vídeo curto.
- **Prove que funciona** seguindo o [roteiro de verificação](docs/verification/README.md).
- **Dependência nova:** diga por que ela é necessária e confira a licença antes
  ([AGENTS.md → licenças](AGENTS.md#código-de-terceiros-licenças-e-créditos)).

## Testes

- Mudou comportamento, ganha teste que falharia sem a mudança. Não enfraqueça nem apague
  teste para ele passar.
- **Sem sleep para esperar.** Espere o evento que prova o comportamento (a resposta, a linha
  gravada, o canal fechado). Teste novo que só passa com um tempo de espera está errado.
- **Nunca use a pasta nem os dados reais do usuário.** Teste e verificação usam uma pasta
  temporária em `MEXERICA_HOME` e outra porta que não a do Hub instalado (19422). Os testes
  de unidade do Hub já caem numa pasta temporária quando `MEXERICA_HOME` não está definida;
  mantenha assim.
- Dados de exemplo só fictícios, com domínios reservados (`*.example.invalid`).

## Mudar como os dados são gravados

**A regra: o que a versão N grava, a versão N-1 consegue ler.** Quem volta uma versão (à mão,
por backup ou porque a nova não subiu) abre com o Hub antigo dados que o novo já tocou. Isso
precisa funcionar.

Como é hoje: o Hub guarda o estado num banco SQLite na pasta de dados. A versão do esquema
fica na chave `schema` da tabela `kv` (`SCHEMA` em [host/src/store/schema.rs](host/src/store/schema.rs)),
e as migrações só andam para a frente, na abertura do banco. Um Hub mais antigo que encontra
um esquema maior que o seu **não recusa** abrir: ele segue como se estivesse atualizado. Não há
outra trava. Por isso a regra é de quem escreve a mudança.

Cabem em uma versão só, porque a anterior não percebe:

- tabela, índice ou coluna nova que aceita vazio (`NULL`);
- campo novo e opcional num JSON, quando quem lê o JSON antigo ignora campo que não conhece
  (confira: struct com `#[serde(deny_unknown_fields)]` não ignora);
- um preenchimento único de campo novo, feito só quando o campo ainda não existe.

Precisam de **duas versões** as mudanças com que a versão anterior falharia, perderia dado ou
desfaria a mudança sem avisar: renomear ou mover tabela, coluna, arquivo ou pasta; apagar
coluna; mudar o sentido de um campo que o código antigo usa; um valor que o código antigo
descarta por não reconhecer.

1. **Primeiro a que lê.** A versão N passa a entender o formato novo ao lado do antigo e
   continua gravando o antigo. Sai assim.
2. **Depois a que grava.** Na versão seguinte, ela começa a gravar o formato novo. A anterior
   já sabe ler.

A regra vale entre uma versão e a seguinte. Para voltar mais de uma versão, o caminho é o
backup do Hub ([packaging/hub-backup](packaging/hub-backup/README.md)). O cache que o app guarda
do Hub tem regra própria (`MIRROR_SCHEMA`, no [AGENTS.md](AGENTS.md#convenções-de-código)).

## Licença da contribuição

O projeto é Apache-2.0 ([LICENSE](LICENSE)). Ao enviar um PR, você concorda em licenciar sua
contribuição nos mesmos termos. Mande só código seu ou de fonte com licença compatível, com o
crédito em [THIRD_PARTY.md](THIRD_PARTY.md) quando copiar algo.
