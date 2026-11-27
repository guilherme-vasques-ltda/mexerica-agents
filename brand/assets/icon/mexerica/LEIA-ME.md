# Ícones de interface da Mexerica

159 ícones para o app de desktop, num traço só: grade de 24, traço de 2, pontas redondas.
São desenho próprio da Mexerica. Esta pasta é um catálogo: nenhum ícone está ligado ao app ainda.

## O que tem aqui

| Arquivo | Conteúdo |
| --- | --- |
| `svg/mx-*.svg` | Um ícone por arquivo, avulso, com `viewBox="0 0 24 24"` |
| `manifesto.json` | Nome, rótulo para leitor de tela, uso, controles do desktop e símbolos que cada ícone cobre |
| `HASHES.tsv` | SHA-256 e tamanho de cada arquivo desta pasta |

O nome do arquivo é o `id` do manifesto. Um ícone serve a vários símbolos do app quando a ação é a
mesma: por exemplo, `mx-baixar` cobre `arrow.down.to.line`, `square.and.arrow.down` e
`arrow.down.circle`. A lista fica em `simbolosDoApp`.

## Como usar

- O desenho usa `currentColor`. A cor vem do texto do botão, então o mesmo arquivo serve aos temas
  claro e escuro e a todos os estados.
- Tamanho de referência: 19 px. Mínimo: 16 px. Não mude a espessura do traço por ícone.
- A folha da mexerica aparece só nos cinco lugares da faixa lateral (`assinatura: true` no
  manifesto). Os controles comuns usam o símbolo que todo mundo já conhece.

## Estados

| Estado | Como fica |
| --- | --- |
| Neutro | Tinta de texto secundária do tema |
| Ativo | Tangerina (`#E26A00` no claro, `#F58A1F` no escuro) sobre o fundo de item ativo |
| Desativado | A tinta do neutro com 40% de opacidade |
| Foco | Contorno de foco no botão, nunca no desenho |

O estado nunca depende só da cor: o botão ativo também muda o fundo e leva `aria-current` ou
`aria-pressed`.

## Acessibilidade

- Botão só com ícone: `aria-label` com o `rotuloAcessivel` do manifesto, e o SVG com
  `aria-hidden="true"`.
- Ícone ao lado de texto: SVG com `aria-hidden="true"`; o texto já diz o que é.
- Ícone sozinho que informa algo (um aviso, por exemplo): `role="img"` e `aria-label`.

## Compositor: três botões que não se confundem

| Ação | Ícone | Desenho |
| --- | --- | --- |
| Enviar mensagem de texto | `mx-enviar` | Avião de papel |
| Gravar mensagem de áudio | `mx-gravar-audio` | Microfone |
| Conversa por voz em tempo real | `mx-voz-ao-vivo` | Telefone com ondas |

Não troque por `mx-reproduzir` (tocar um áudio), `mx-nota-musical` (anexo de áudio),
`mx-ondas-de-voz` (página Voz) nem `mx-seta-cima` (mover para cima).

## Movimento

O gesto de cada ícone é curto (menos de um segundo), toca uma vez e volta ao repouso. Com
"movimento reduzido" ligado no sistema, o ícone fica parado. O campo `gesto` do manifesto descreve
o movimento; ele é opcional.

## Regra de uso da arte

Esta pasta fica dentro de `brand/assets/icon/` e segue a regra que
[`brand/LICENCA-DA-ARTE.md`](../../../LICENCA-DA-ARTE.md) dá a essa pasta.

## O que não foi provado

- A conferência foi técnica: nomes, arquivos, bordas da grade e leitura a 19 px nos dois temas.
  Não houve teste com usuários.
- 8 ícones cobrem símbolos que o app declara mas nenhuma tela usa hoje (o manifesto marca
  "Reservado" em `controles`).
- Os símbolos de space são a versão em traço dos 30 desenhos em dois tons que o app usa hoje nos
  tiles; `simbolosDeSpace` diz qual `id` do Hub cada um cobre.
- Ficam fora: logos de serviços, avatares dos bots, o ícone do app e as bandeiras de idioma.
