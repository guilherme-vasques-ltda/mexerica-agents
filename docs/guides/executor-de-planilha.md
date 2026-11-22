# Planilha no computador escolhido

O assistente, no Hub, pode pedir uma planilha `.xlsx` **nova** numa pasta que o dono liberou no
próprio computador. Quem grava é um programa pequeno no computador (o executor); o Hub só
diz "criado" quando o computador devolve um recibo conferido. Usa o canal direto e cifrado
do Hub que os computadores do dono já usam, sem outro transporte.

## O que existe

- **Ferramenta `create_spreadsheet`** (`host/src/computers/tools.json`). O modelo informa
  `location` (hoje só `desktop`, a Área de Trabalho), uma `folder` opcional (um só nome),
  `filename`, `sheets` (até 5 abas, 500 linhas, 20 colunas; texto, número, booleano ou
  null; sem fórmulas; arquivo final de até 128 KiB) e `reason`. Nunca informa caminho.
- **Montagem do arquivo** (`host/src/computers/xlsx.rs`): determinística, sem compressão.
  O pedido leva o arquivo, o SHA-256, o SHA-256 do conteúdo tipado e o resumo das abas.
- **Capacidade**: o computador precisa anunciar `planilha` e `locais`. Sem isso o pedido
  nem entra na fila. Quem anuncia só `planilha` não recebe `run_command`, `read_file`, `list_dir` nem `write_file`.
- **Executor de referência** (`donnabot-host executor`, `host/src/computers/helper/`),
  processo separado do Hub, com identidade e estado próprios:
  - `enroll --hub URL --name NOME`: pede o código de cadastro na entrada padrão (não fica
    nos argumentos do processo), confere que o Hub conhece o código e fixa a chave dele.
    O endereço precisa ser `https://` (`http://` só em localhost), sem usuário, senha nem
    parâmetros. O estado (`--home`) precisa ser uma pasta nova ou vazia.
  - `authorize --location desktop`: o dono libera, no próprio computador, a pasta especial
    do sistema digitando `AUTORIZAR`. Áreas críticas (biblioteca do sistema, dados de
    aplicativos, pastas ocultas de configuração, fora da pasta pessoal) não podem ser liberadas.
  - `run`: atende os pedidos; só um `run` por pasta de estado. `unauthorize`, `status` e `revoke` retiram a liberação, mostram
    o vínculo e o diário, e revogam (apagando identidade, liberação, diário e pendências).

## O que o computador garante (no cliente, não no texto do pedido)

- Uma única operação: criar uma planilha nova. Não apaga, move, renomeia, sobrescreve, lê
  arquivos do dono nem roda comandos. Arquivo que já existe vira `file_exists`, sem abrir o
  que existe.
- Antes de gravar, regenera a planilha a partir do conteúdo que ele mesmo validou (limites,
  nomes, sem fórmulas nem partes extras) e só grava se os bytes forem idênticos aos recebidos.
- A pasta liberada é aberta componente a componente sem seguir links, e pasta, arquivo novo
  e reabertura são relativos a esses descritores: trocar uma pasta por um link depois da
  conferência não redireciona a gravação. Atalhos, links e arquivos com outros nomes são
  recusados, assim como nomes reservados do Windows.
- A liberação guarda a identidade da pasta (dispositivo e inode) que o dono confirmou: outra
  pasta comum no mesmo caminho não herda a liberação (`location_changed`).
- A retirada da liberação, a revogação local e a criação do arquivo usam o mesmo trinco:
  depois que `unauthorize` ou `revoke` confirma, nada novo é criado.
- O arquivo criado fica aberto até a conferência, que exige que a reabertura por nome aponte
  para o mesmo objeto (inode) que foi criado.
- Depois de gravar, reabre o arquivo e confere tamanho, SHA-256 e o conteúdo da planilha.
  O hash sozinho não basta: o conteúdo tem de abrir como planilha e bater com o resumo.

## Recibo e queda de conexão

`done` só vale com `receipt` coerente com o que o Hub enviou (caminho absoluto terminando em
`pasta/nome`, tamanho, SHA-256, conteúdo, abas) e `decidedBy` `policy` ou `click`. Recibo
incoerente vira `unknown_outcome` (`receipt_invalid`), e o recibo recusado não volta ao modelo.
Para o dono, "salvo" só vale com esse recibo: a planilha montada no servidor é prévia.

O executor guarda a tentativa antes de pedir o `start` (pedido, lease, fase). Fase `planned`
= nada tocado; `writing` = pode ter criado. A identidade do arquivo criado (dispositivo e
inode) é registrada antes de qualquer byte. Após a queda:

- o resultado guardado da mesma tentativa é reenviado até o Hub confirmar; falha de rede
  preserva o resultado, e só recusa definitiva do Hub (código estável) o descarta;
- fase `planned` (nada tocado): se o Hub chegou a confirmar o `start`, o resultado é `failed/not_written` e só então repetir é liberado; se o Hub nunca confirmou o `start`, ele recusa o resultado e encerra o pedido por conta própria (expirado), e nada foi gravado;
- fase `writing`: só o arquivo íntegro com a identidade registrada vira recibo `done`; o
  resultado reconstruído vai para a caixa de saída antes de ser enviado;
- qualquer outra coisa (arquivo sumiu, outro arquivo igual, pela metade, alvo que já
  existia) continua `unknown`, e o Hub bloqueia repetir o mesmo pedido no mesmo turno;
- falha depois de criar o arquivo (gravação interrompida ou conferência reprovada) também é
  `unknown`, nunca `failed`: o disco pode ter ficado com algo;
- passadas 24 horas o Hub apaga a especificação do pedido; um `done` tardio sem ela não é
  aceito como entrega.

## O que não está provado ou não existe

- Prova só **simulada**: Hub real e executores reais em processos próprios, pastas sintéticas,
  chamada da ferramenta roteirizada, plataforma declarada por variável que só existe em build
  de depuração. Não houve teste em computador físico.
- Só macOS: o Windows ainda não tem gravação confinada por handle, então o executor recusa lá.
- A presença enviada ao Hub é fixa (`locked:false`): o executor não detecta tela bloqueada
  nem ociosidade. A barreira real é a liberação local.
- Não há janela nativa nem Touch ID (previstos para o ajudante nativo); o consentimento é a
  liberação local com texto explícito. A chave do executor fica em arquivo `0600`, sem Chaveiro.
- O relay da conta não foi exercitado. O bloqueio de repetição vale para a mesma
  especificação no mesmo turno do chat (a chave inclui a mensagem de origem); um pedido novo
  do dono é outro pedido.
- A pasta de estado do executor fica na conta do próprio usuário. Quem já pode escrever nela
  (ou em pastas acima dela) é considerado fora do modelo de ameaça desta fatia; o estado não
  é aberto por descritor como a pasta liberada.
- Inteiros até ±(2^53 − 1), isto é, ±9007199254740991, são aceitos; de ±9007199254740992 (2^53) em diante são recusados, em qualquer grafia (inteiro, decimal ou exponencial), porque o número passa por double no caminho e 2^53 + 1 arredonda para 2^53: acima disso, mande como texto.
- Parar por revogação depende da mensagem do Hub no canal; a verificação não é tipada.
