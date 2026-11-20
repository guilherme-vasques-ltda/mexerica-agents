# Atualizar os binários ACP verificados

O Hub compila [fixados.json](fixados.json) no binário. Para Cursor e Devin, o
catálogo ACP precisa indicar a mesma versão e URL da plataforma. O Hub usa o
executável, a chamada ACP e o hash fixados localmente, sem recorrer a outro
distribuidor ou buscar checksums em tempo de instalação.

## Procedência desta revisão

Em 19/11/2026, lemos o [catálogo ACP](https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json)
e baixamos as 12 URLs por HTTPS, sem executar os binários. Calculamos
SHA-256 sobre cada arquivo completo e comparamos com a fonte indicada em
`url_checksum_fornecedor`: o arquivo `.sha256` do próprio Cursor ou o manifesto
versionado do Devin, onde também conferimos versão e URL. Todos os 12 conferiram.
`bytes` é o tamanho do download; `bytes_extraidos` é o tamanho do fluxo TAR
com cabeçalhos e preenchimento, ou a soma dos tamanhos descomprimidos no ZIP.
Os limites são por pacote verificado e não alteram os tetos dos demais agentes.
O extrator do Hub publicou e revalidou o cache dos 12 pacotes em sandbox Linux,
sem iniciar os agentes. Isso não comprova a execução em nenhum sistema.

## Para uma versão nova

1. Confira versão, plataforma, URL e caminho do executável no catálogo ACP.
   Baixe o arquivo em uma pasta temporária fora do repositório, sem executá-lo.
2. Calcule o SHA-256 local com `sha256sum` (ou `shasum -a 256`) e compare
   com o checksum publicado pelo fornecedor para a mesma versão e URL. Se não
   houver checksum, registre a ausência: só inclua a plataforma após verificação
   própria considerada confiável; se não puder fazer essa verificação, não a inclua.
3. Atualize versão, URL, checksum, fonte, data, caminho e tamanhos em
   `fixados.json`. Confira se o executável é regular e se o extrator aceita
   o pacote sem abrir exceções para links ou caminhos externos. Atualize no
   teste a lista de plataformas se ela mudar, rode os testes de
   `agent::registry` e descarte os downloads.

Para voltar a oferecer um agente suspenso, confira se o catálogo trouxe um
SHA-256 válido para cada build que se quer oferecer e compare-o com o arquivo
publicado. Em uma revisão, retire o ID do filtro em `registry.rs` e atualize os
testes. Sem hash válido, a instalação de um binário pelo catálogo segue recusada;
isso não impede o uso dos motores nativos instalados fora do catálogo.
