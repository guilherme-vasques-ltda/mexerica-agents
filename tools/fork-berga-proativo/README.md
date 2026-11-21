# Ensaio offline do Berga proativo

**Isto é um contrato, não o Berga.** O repositório não tem motor proativo (conferido em `host/`, `apps/desktop/` e `motor/`). `ensaio.mjs` são regras escritas à mão que dizem o que um aviso precisa distinguir. Passar aqui mostra que o contrato é coerente e testável; **não** prova que o Berga de produção avisa bem.

Origem: cenas de aceite fictícias do desenho do Berga proativo.

## O que roda

- `cenas.json`: as cinco cenas fictícias (multa, alunos, excedente, renovação, alertas) e as variantes negativas: respondido, cancelado, repetido, fora de hora, acesso revogado, mais finalidade errada. Extras: alerta recuperado, falso alarme, zero alertas, relevância que mudou ou não, renovação futura com e sem prova.
- `ensaio.mjs`: função `ensaiar` com resposta em cinco partes separadas: **oferta** (interromper, resumo da manhã ou nada), **ação autorizada** (sempre vazia: avisar não autoriza pagar, responder, cancelar nem alterar produção; só lista as ações vedadas), **silêncio** (motivo e até quando), **origem** (o que a fonte afirma e o que foi confirmado) e **incerteza**.
- `ensaio.test.mjs`: 51 testes. Garantem, entre outros: acesso revogado ou finalidade errada não lê a fonte; resumo da manhã não é adiado por horário; aviso repetido só volta se a relevância mudou (não basta a versão); a saída não carrega o conteúdo bruto (marcador fictício); o módulo não cita rede, processo, ambiente nem modelo (verificação estática por busca de padrões, não prova de execução). Foram feitas mutações de sanidade nas regras e os testes as derrubaram.

## Como rodar (manual)

```sh
node --test tools/fork-berga-proativo/ensaio.test.mjs
```

Não está ligado ao CI: ligar é um passo à parte, numa mudança própria do fluxo de testes.

## Limites

Sem dado privado, sem leitura de contas, sem serviços conectados, sem modelo externo, sem canal de notificação, sem mudança na Tower. Não mede utilidade nem omissões do Berga real; isso exige o replay com o motor de verdade, depois de aprovado.
