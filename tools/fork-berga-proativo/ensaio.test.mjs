import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { ensaiar } from "./ensaio.mjs";

const arquivo = JSON.parse(readFileSync(new URL("./cenas.json", import.meta.url), "utf8"));
const { contexto, cenas, variantes, extras } = arquivo;
const { resumoManha } = contexto.preferencias;
const FORA_DE_HORA = "2026-11-18T23:30:00-03:00";

const mesclar = (a, b) => {
  if (b === null || typeof b !== "object" || Array.isArray(b)) return b;
  const out = { ...a };
  for (const k of Object.keys(b)) out[k] = mesclar(a?.[k], b[k]);
  return out;
};

function rodar(cena, { patch = {}, historico = false, historicoAgora, historicoPatch } = {}) {
  const { agora: agoraPatch, ...resto } = patch;
  const c = mesclar(cena, resto);
  const leitor = (x) => {
    leitor.chamadas++;
    return x.dados;
  };
  leitor.chamadas = 0;
  const ctx = { agora: agoraPatch ?? contexto.agora, preferencias: contexto.preferencias, leitor };
  const anterior = historicoPatch ? mesclar(cena, historicoPatch) : cena;
  const hist = historico ? [{ chave: cena.dados.chave, assinatura: ensaiar(anterior, { ...ctx, agora: historicoAgora ?? ctx.agora, leitor: (x) => x.dados }).assinatura }] : [];
  return { saida: ensaiar(c, { ...ctx, historico: hist }), leitor };
}
const cena = (id) => cenas.find((c) => c.id === id);

for (const c of cenas) {
  test(c.id + ": caso positivo separa oferta, ação, origem e incerteza", () => {
    const { saida } = rodar(c);
    assert.equal(saida.silencio, null);
    assert.equal(saida.oferta.tipo, c.esperado.oferta);
    assert.equal(saida.oferta.canal, "privado");
    assert.equal(saida.oferta.quando, c.esperado.oferta === "resumo" ? resumoManha : "agora");
    assert.deepEqual(saida.acaoAutorizada.permitidas, []);
    assert.deepEqual(saida.acaoAutorizada.vedadas, c.esperado.vedadas);
    assert.equal(saida.incerteza.nivel, c.esperado.incerteza);
    assert.deepEqual(saida.origem.naoConfirmado, c.esperado.naoConfirmado);
    assert.equal(saida.origem.fonte, c.fonte.tipo);
  });

  for (const [nome, v] of Object.entries(variantes)) {
    test(c.id + ": par negativo " + nome, () => {
      const opcoes = { ...v, patch: v.patchPorCena ? v.patchPorCena[c.id] : v.patch };
      const { saida, leitor } = rodar(c, opcoes);
      assert.deepEqual(saida.acaoAutorizada.permitidas, []);
      if (nome === "fora_de_hora" && c.esperado.oferta === "resumo") {
        assert.equal(saida.silencio, null);
        assert.deepEqual(saida.oferta, { tipo: "resumo", canal: "privado", quando: resumoManha });
        return;
      }
      assert.equal(saida.silencio?.motivo, v.silencio);
      if (nome === "respondido" && c.id === "alunos") assert.deepEqual(saida.origem.confirmado, ["resposta posterior registrada"]);
      if (nome === "acesso_revogado" || nome === "finalidade_outra") {
        assert.equal(leitor.chamadas, 0, "sem concessão válida não pode ler a fonte");
        assert.equal(saida.oferta, null);
        assert.equal(saida.origem, null);
      } else if (nome === "fora_de_hora") {
        assert.deepEqual(saida.oferta, { tipo: "resumo", canal: "privado", quando: resumoManha });
        assert.equal(saida.silencio.ate, resumoManha);
      } else {
        assert.equal(saida.oferta, null);
        assert.equal(leitor.chamadas, 1);
      }
    });
  }
}

test("fora de hora: resumo da manhã não é afetado pelo horário de silêncio", () => {
  for (const id of ["alunos", "renovacao"]) {
    const { saida } = rodar(cena(id), { patch: { agora: FORA_DE_HORA } });
    assert.equal(saida.silencio, null);
    assert.deepEqual(saida.oferta, { tipo: "resumo", canal: "privado", quando: resumoManha });
  }
});

for (const e of extras) {
  test("extra " + e.id, () => {
    const { saida } = rodar(cena(e.cena), e);
    if (e.silencio) {
      assert.equal(saida.silencio.motivo, e.silencio);
      assert.equal(saida.oferta, null);
    }
    if (e.oferta) {
      assert.equal(saida.silencio, null);
      assert.equal(saida.oferta.tipo, e.oferta);
    }
    if (e.incerteza) assert.equal(saida.incerteza.nivel, e.incerteza);
  });
}

test("fuso: agora em UTC é avaliado no fuso das preferências", () => {
  const multa = cena("multa");
  assert.equal(rodar(multa, { patch: { agora: "2026-11-19T02:30:00Z" } }).saida.silencio?.motivo, "fora_de_hora");
  assert.equal(rodar(multa, { patch: { agora: "2026-11-18T18:00:00Z" } }).saida.oferta.tipo, "interromper");
});

test("janela de silêncio que não cruza a meia-noite só cala dentro dela", () => {
  const multa = cena("multa");
  const prefs = { ...contexto.preferencias, silencioInicio: 1, silencioFim: 5 };
  const proxima = mesclar(multa, { dados: { prazo: "2026-11-18T20:00:00-03:00" } });
  const roda = (agora) => ensaiar(proxima, { agora, preferencias: prefs, leitor: (x) => x.dados });
  assert.equal(roda("2026-11-18T15:00:00-03:00").oferta.tipo, "interromper");
  assert.equal(roda("2026-11-18T03:00:00-03:00").silencio?.motivo, "fora_de_hora");
});

test("origem informa prazo da multa e data da renovação", () => {
  assert.equal(rodar(cena("multa")).saida.origem.prazoInformado, cena("multa").dados.prazo);
  assert.equal(rodar(cena("renovacao")).saida.origem.dataInformada, cena("renovacao").dados.dataRenovacao);
});

test("renovação com data passada é incerta e nunca liga pagamento ao pedido", () => {
  const { saida } = rodar(cena("renovacao"));
  assert.equal(saida.incerteza.nivel, "incerto");
  assert.ok(saida.origem.naoConfirmado.includes("autorizacao_de_pagamento"));
});

test("faturas e alertas repetidos contam uma vez", () => {
  const conta = (id, campo, lista) => rodar(cena(id), { patch: { dados: { [campo]: lista } } }).saida.origem.afirmacao;
  assert.match(conta("excedente", "faturasCitadas", ["F1", "F1", "F2"]), /\(2 fatura\(s\) distinta/);
  assert.match(conta("excedente", "faturasCitadas", ["F1", "F2", "F3", "F3"]), /\(3 fatura\(s\) distinta/);
  assert.match(conta("alertas", "alertas", ["a1", "a1", "a2"]), /^2 alerta\(s\) distinto/);
  assert.match(conta("alertas", "alertas", ["a1", "a2", "a3", "a3"]), /^3 alerta\(s\) distinto/);
});

test("saída nunca carrega o conteúdo bruto da fonte", () => {
  for (const c of cenas) {
    const texto = JSON.stringify(rodar(c).saida);
    assert.ok(c.dados.conteudoPrivado.startsWith("TEXTO-BRUTO-FICTICIO-"));
    assert.ok(!texto.includes(c.dados.conteudoPrivado), c.id);
  }
});

test("verificação estática: o módulo não importa nem cita rede, processo, ambiente ou modelo", () => {
  const fonte = readFileSync(new URL("./ensaio.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(fonte, /\bimport\b|require\(|fetch\(|child_process|\bprocess\b|globalThis|XMLHttpRequest|WebSocket/);
});
