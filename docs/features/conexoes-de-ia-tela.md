# Conexões de IA: tela «Adicionar conexão»

Em Ajustes → Contas de IA, o botão «Adicionar» abre uma tela só, com busca e dois grupos:

- **Planos**: assinatura ou plano de código, ligado pela tela, sem terminal. Hoje: Codex e Claude Code.
- **Chaves de API**: uma linha por fornecedor que o Hub lista (Anthropic, OpenAI, Google, xAI, OpenRouter e Meta).
  Só aparece com o motor Mexerica ligado e no Hub deste computador.

Só entra o que já funciona. Item que ainda não existe fica fora da tela: não há botão sem ação nem «em breve».

## Como um item novo entra

- **Plano:** o motor entra em `ENGINES` (`contas-ia/regras.ts`) e a tela o mostra sozinha (`conexoes.ts`).
- **Chave:** o fornecedor entra na lista do Hub (`motorChaves`); a tela monta a linha a partir dela.
- **Selo no seletor de modelo:** o fornecedor entra em `FORNECEDORES` (`seletor-modelo/selo.ts`).

## Edição Open

Na Open do Mac a tela mostra só o Codex, sem o grupo de chaves, e a seção de chaves salvas não aparece nem consulta o
Hub. A edição vem da ponte do app (`appScheme`); sem a ponte, vale a edição completa. Há testes para isso em
`contas-ia/conexoes.test.ts` e `contas-ia/adicionar-conexao-render.test.ts`.

## «Testar acesso» (chaves)

O teste só lê a lista de modelos do fornecedor e nunca gera texto, então não afirma que «está tudo certo». Quatro estados:

| Estado | Quando |
|---|---|
| Conta reconhecida | O fornecedor aceitou a chave, mas não listou nenhum modelo de conversa. |
| Modelos no catálogo | O fornecedor aceitou a chave e listou modelos. A tela lembra que a lista não prova saldo nem permissão de cada modelo. |
| Acesso ainda não confirmado | O fornecedor respondeu, mas limite de uso ou falta de crédito impedem confirmar. |
| Não foi possível verificar | Sem resposta a tempo, resposta quebrada, erro do fornecedor ou sem permissão de listar. Não é chave inválida. |

Só a recusa explícita do fornecedor (chave inválida) mostra «Chave recusada». A chave nunca volta à tela depois de salva.

## Selo no seletor de modelo

Cada modelo diz por qual conexão roda: «Chave · Anthropic» no motor Mexerica (pelo prefixo do id do modelo);
«Plano · Codex» ou «Chave · Claude Code» quando a conta do motor é de assinatura ou paga por uso. Se a cobrança da conta
não é conhecida (ou a conta não está à vista), o selo mostra só o nome da conexão. Fornecedor desconhecido não ganha selo.
