# Grupos de bots

Em Agentes, grupos organizam bots do Space ativo. Crie um grupo pelo menu +,
renomeie com dois cliques ou pelo menu, e arraste um bot ou use seu menu para
colocá-lo em um grupo. Apagar um grupo deixa seus bots fora de grupo; conversas
e rotinas continuam intactas. A ordem dos grupos é a de criação.

O recolhimento é local ao aparelho. A busca mostra correspondências sem mudar
essa preferência. Bots em Times conservam sua atribuição a grupo, mas aparecem
na equipe. Rotinas permite filtrar por grupo ou por bots fora de grupo.

O Hub guarda os dados por Space e sincroniza mudanças entre aparelhos. Com Hub
anterior ao contrato `botGroups`, o app mostra a lista plana e oculta controles
de grupo.

## Hub e transição

O Hub guarda grupos e seus membros por Space. `botGroups` devolve uma revisão
monotônica e a lista ordenada de grupos, cada um com seus IDs de bots. Bots sem
vínculo não ocupam um grupo persistido. `botGroupsEdit` aceita `add`, `rename`,
`remove` e `place`; a criação pode incluir um bot na mesma transação. Os métodos
exigem cliente local ou aparelho pareado com escopo de controle.

Nomes têm de 1 a 40 caracteres visíveis, com espaços normalizados. A unicidade
vale no Space e ignora maiúsculas e acentos. Apagar um grupo libera seus membros;
apagar um bot remove seu vínculo. Eventos de grupos só chegam a clientes inscritos
no mesmo Space.

Na migração, o Hub copia os dados das tabelas antigas para as novas e só então
apaga as antigas; se a cópia falhar, nada é apagado, a leitura retorna vazio e a
edição responde `migrating` até a próxima tentativa na abertura do Hub. Os demais
códigos estáveis são `name_invalid`, `name_taken`, `group_missing` e
`bot_missing`.

O rollback de N+1 para N conserva os dados novos; o rollback de N+1 para N−1 os
perde. Em N → N−1 → N, alterações feitas em N−1 depois da migração não são
importadas novamente, pois o marcador da cópia continua presente.
