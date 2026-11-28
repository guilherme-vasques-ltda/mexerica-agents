# Roteiro de verificação

O que provar antes de dizer que uma mudança funciona. Build e teste com mock, sozinhos, não
comprovam comportamento.

## 1. Ambiente isolado, sempre

- Nunca verifique contra o Hub instalado, o app de uso diário nem os dados reais de alguém.
- Hub de teste numa pasta temporária e noutra porta:

  ```sh
  export MEXERICA_HOME="$(mktemp -d)"
  cargo run --manifest-path host/Cargo.toml -- serve --port 19500
  ```

  Sem `--bind`, ele escuta só no próprio computador.
- Só dados e contas fictícios. Nada de chave real em arquivo, log ou captura.

## 2. O que provar

1. **O caminho pedido:** faça a ação como quem usa faria e veja o resultado (a resposta, a
   tela, a linha gravada).
2. **O caminho de falha:** sem permissão, sem rede ou com entrada ruim, o código nega ou
   avisa, nunca libera sozinho.
3. **Reinício:** se a mudança grava algo, pare e suba o Hub de novo e confira que o dado
   continua lá.
4. **Versão anterior:** se mudou o formato dos dados, confira a regra de
   [CONTRIBUTING.md → Mudar como os dados são gravados](../../CONTRIBUTING.md#mudar-como-os-dados-são-gravados).
5. **Tela:** se mudou a tela, prints de antes e depois.

## 3. Evidência

- Anote o comando e o que apareceu. Mostre só o pedaço que prova; nunca o JSON completo de
  informações do Hub nem o ambiente inteiro.
- Escreva no PR o que **não** foi provado e por quê (por exemplo: precisa de um Mac, de conta
  real ou de outra plataforma).

## 4. Ao terminar

Pare o Hub de teste e apague a pasta temporária.
