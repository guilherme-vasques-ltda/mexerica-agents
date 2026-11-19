# Regra de uso da arte própria da Mexerica

Este arquivo diz, para cada grupo de imagens da Mexerica, o que quem baixa o repositório pode e não pode fazer. A licença Apache-2.0 do
repositório cobre o **código**. Para a arte valem as duas regras abaixo.

**Decisão do dono, 19/11/2026.** Perguntado se os fundos de chat, os papéis de parede e os personagens ficam "só da Mexerica ou livres para
reuso", o dono escolheu "só da Mexerica" ("isso"). Sobre logo, mascote, símbolo e ícone do app ficarem reservados como marca e sobre as
Mexeriquinhas e os 20 ícones opcionais já estarem registrados como livres para reuso, respondeu "ISSO". Antes disso, em 19/11/2026, o dono
tinha liberado a arte própria para esta cópia ("sim, libera tudo, as artes são nossas, são minhas..." e "sim, volte. tudo.").

Este texto registra essas decisões em linguagem simples. Nenhum advogado o leu, e ele não é parecer jurídico.

## Regra A: Apache-2.0 (reuso livre, com os avisos da licença)

A origem declara Apache-2.0 para estes grupos, e a declaração foi mantida.

| Grupo | Caminhos | Arquivos |
| --- | --- | ---: |
| Mexeriquinhas (pelúcia 3D, imagens e atlas) | `apps/desktop/src/renderer/lib/mexeriquinhas/assets/` | 595 |
| Ícones opcionais do app, as 19 opções das rodadas 2 e 3 | `apps/desktop/resources/app-icons/icone-01-*.png` a `icone-19-*.png` (texto da licença em `apps/desktop/resources/app-icons/LICENSE`) | 19 |

## Regra B: direitos reservados, uso só dentro da Mexerica

Estes grupos **não** são licenciados sob Apache-2.0. A origem não declara licença para eles, e esta cópia também não escolhe uma. Os direitos
ficam reservados ao titular (a Mexerica, por decisão do dono). O uso permitido é **dentro da Mexerica como distribuída**: ver, compilar e
rodar o app com essas imagens. Não é permitido reusar, republicar ou incluir essas imagens em outros produtos, nem usá-las como marca de
outro projeto. O nome, a logo, o mascote e o ícone são também marca reservada (ver [TRADEMARK.md](../TRADEMARK.md)). A permissão pode ser
ampliada depois pelo titular, sem voltar atrás; o contrário não vale.

| Grupo | Caminhos | Arquivos |
| --- | --- | ---: |
| Fundos de chat e papéis de parede | `brand/assets/fundos-chat/`, `apps/desktop/src/renderer/assets/wallpapers/` | 128 |
| Personagens por papel | `brand/assets/personagens/` | 11 |
| Logo, mascote, símbolo, ícone do app, favicons e ícone da barra de menus | `brand/assets/logo/`, `apps/desktop/resources/icon.png`, `apps/desktop/resources/icon.svg`, `apps/desktop/resources/icon-tray.png`, `apps/desktop/resources/Donnabot.icns`, `apps/desktop/resources/AppIcon.icon/`, `apps/desktop/resources/brand/`, `apps/desktop/resources/tray/`, `apps/desktop/src/renderer/assets/brand/` | 136 |
| Variantes e fontes do ícone do app | `brand/assets/icon/` | 185 |
| Ícone padrão da grade de Ajustes (é o ícone oficial reduzido) | `apps/desktop/resources/app-icons/padrao.png` | 1 |

Os textos que descrevem os arquivos dentro dessas pastas (`LEIA-ME`, tabelas de medidas) seguem a regra da pasta em que estão.

## O que não entra em nenhuma regra

Marcas de terceiros não fazem parte da arte da Mexerica e não são cobertas por este arquivo: as logos dos serviços do catálogo de conectores,
os 43 ícones de agentes, o logotipo da Apple, `google.svg` e `computer-linux.svg`. Veja [THIRD_PARTY.md](../THIRD_PARTY.md).

## Procedência

`brand/ARTE-HASHES.tsv` lista cada arquivo com o grupo, a regra de uso (`apache-2.0` ou `reservada`), se traz credenciais C2PA, o SHA-256 e o tamanho.

34 PNGs (os 20 fundos de chat, os 10 personagens e as 4 fontes originais do ícone) trazem credenciais de conteúdo C2PA assinadas pela OpenAI e
datadas de 07/11/2026, que dizem que foram geradas pelo ChatGPT (`gpt-image`). O dono não informou a ferramenta; este registro só repete o que
as credenciais afirmam. Não foram conferidos os termos de uso da ferramenta sobre titularidade ou uso comercial das imagens geradas. Detalhes em
[THIRD_PARTY.md](../THIRD_PARTY.md).
