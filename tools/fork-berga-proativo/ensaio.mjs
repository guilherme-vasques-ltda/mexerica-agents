// ENSAIO OFFLINE, NÃO É O BERGA. Não existe motor proativo no repositório (conferido
// em host/, apps/desktop/ e motor/). Este arquivo é um CONTRATO executável: regras
// escritas à mão que dizem o que um aviso proativo precisa distinguir (cenas de aceite
// fictícias do desenho do Berga proativo). Passar aqui NÃO prova que o
// Berga de produção acerta; só prova que o contrato é coerente e testável.
// Sem rede, sem modelo, sem contas: o único "leitor" é uma função injetada que devolve
// dados fictícios.

const FINALIDADE = "aviso_privado";
const hora = (iso, fuso) => Number(new Intl.DateTimeFormat("en-GB", { hour: "2-digit", hourCycle: "h23", timeZone: fuso }).format(new Date(iso)));
const calado = (h, ini, fim) => (ini <= fim ? h >= ini && h < fim : h >= ini || h < fim);
const horasAte = (alvo, agora) => (Date.parse(alvo) - Date.parse(agora)) / 3_600_000;
const distintos = (lista) => [...new Set(lista)].sort();

const regras = {
  multa: (d, agora) => ({
    pendente: true,
    interromper: horasAte(d.prazo, agora) < 24,
    origem: { afirmacao: "a mensagem diz que há possível multa", prazoInformado: d.prazo, confirmado: [] },
    incerteza: { nivel: "nao_confirmado", motivo: "remetente não verificado" },
    naoConfirmado: ["autenticidade", "divida"],
    vedadas: ["abrir_link", "pagar"],
    relevancia: [d.prazo],
  }),
  alunos: (d) => ({
    pendente: d.ultimaPerguntaDe === "aluno",
    silencioProprio: d.respostaPosterior ? "respondido" : null,
    interromper: false,
    origem: { afirmacao: "última pergunta do grupo " + d.grupo + " é de aluno", confirmado: [d.respostaPosterior ? "resposta posterior registrada" : "sem resposta registrada"] },
    incerteza: { nivel: "confirmado", motivo: null },
    naoConfirmado: [],
    vedadas: ["responder_pelo_dono", "expor_mensagens"],
    relevancia: [d.grupo],
  }),
  excedente: (d) => {
    const faturas = distintos(d.faturasCitadas);
    return {
      pendente: d.consumoAtual > d.limite,
      silencioProprio: d.liquidado ? "respondido" : null,
      interromper: d.bloqueioPrevisto === true,
      origem: { afirmacao: "o fornecedor afirma uso acima do limite (" + faturas.length + " fatura(s) distinta(s))", confirmado: ["consumo atual " + d.consumoAtual + " de " + d.limite] },
      incerteza: { nivel: "confirmado", motivo: "valor devido vem só do fornecedor" },
      naoConfirmado: ["valor_devido"],
      vedadas: ["pagar", "cancelar"],
      relevancia: [d.consumoAtual, ...faturas],
    };
  },
  renovacao: (d, agora) => {
    const passou = Date.parse(d.dataRenovacao + "T00:00:00-03:00") < Date.parse(agora);
    const prova = d.evidenciaAssinaturaAtiva === true;
    const nivel = prova ? "confirmado" : passou ? "incerto" : "nao_confirmado";
    return {
      pendente: true,
      interromper: false,
      origem: { afirmacao: "data prevista no " + d.origemDoDado, dataInformada: d.dataRenovacao, confirmado: prova ? ["assinatura ativa"] : [] },
      incerteza: { nivel, motivo: prova ? null : passou ? "a data já passou; pedido antigo não prova assinatura ativa" : "pedido não prova assinatura ativa" },
      naoConfirmado: prova ? ["autorizacao_de_pagamento"] : ["assinatura_ativa", "autorizacao_de_pagamento"],
      vedadas: ["pagar", "renovar"],
      relevancia: [d.dataRenovacao, prova],
    };
  },
  alertas: (d) => {
    const alertas = distintos(d.alertas);
    return {
      pendente: alertas.length > 0 && !d.recuperado && d.confirmacaoIndependente !== false,
      silencioProprio: d.recuperado ? "recuperado" : d.confirmacaoIndependente === false ? "falso_alarme" : null,
      interromper: d.severidade === "alta",
      origem: { afirmacao: alertas.length + " alerta(s) distinto(s) após a atualização", confirmado: ["sequência observada"] },
      incerteza: { nivel: "incerto", motivo: "sequência não prova causa nem impacto" },
      naoConfirmado: ["causa", "impacto"],
      vedadas: ["alterar_producao"],
      relevancia: [...alertas, d.severidade],
    };
  },
};

export function ensaiar(cena, { agora, preferencias, historico = [], leitor }) {
  const base = { cena: cena.id, oferta: null, acaoAutorizada: { permitidas: [], vedadas: [] }, silencio: null, origem: null, incerteza: null, assinatura: null };
  const concessao = cena.fonte.concessao;
  if (!concessao.ativa) return { ...base, silencio: { motivo: "acesso_revogado", ate: null } };
  if (concessao.finalidade !== FINALIDADE) return { ...base, silencio: { motivo: "sem_finalidade", ate: null } };

  const dados = leitor(cena);
  const r = regras[cena.id](dados, agora);
  const saida = {
    ...base,
    assinatura: JSON.stringify([...r.relevancia, r.interromper]),
    origem: { fonte: cena.fonte.tipo, ...r.origem, naoConfirmado: r.naoConfirmado },
    incerteza: r.incerteza,
    acaoAutorizada: { permitidas: [], vedadas: r.vedadas },
  };
  const calar = (motivo, ate = null) => ({ ...saida, silencio: { motivo, ate } });

  if (dados.retirado) return calar("cancelado");
  if (r.silencioProprio) return calar(r.silencioProprio);
  if (dados.resolvido) return calar("respondido");
  if (!r.pendente) return calar("nada_pendente");
  if (historico.some((h) => h.chave === dados.chave && h.assinatura === saida.assinatura)) return calar("repetido");

  const resumo = { tipo: "resumo", canal: "privado", quando: preferencias.resumoManha };
  if (!r.interromper) return { ...saida, oferta: resumo };

  const h = hora(agora, preferencias.fuso);
  if (calado(h, preferencias.silencioInicio, preferencias.silencioFim)) return { ...saida, oferta: resumo, silencio: { motivo: "fora_de_hora", ate: preferencias.resumoManha } };
  return { ...saida, oferta: { tipo: "interromper", canal: "privado", quando: "agora" } };
}
