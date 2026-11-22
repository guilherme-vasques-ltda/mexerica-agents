#!/usr/bin/env python3
"""Prova serviço-parado: em nenhuma amostra o Hub roda com a transação em fase crítica,
e toda troca de seletor ocorre entre amostras com o Hub parado."""
import json, sys
CRITICAS = {"preparada", "slot_preparado", "desativacao_iniciada", "anterior_desativado", "seletor_trocado",
            "cancelamento_iniciado", "reversao_iniciada", "seletor_restaurado"}
amostras = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
violacoes, criticas, trocas = [], 0, 0
for a in amostras:
    if a["etapa"] in CRITICAS:
        criticas += 1
        if a["hub"]: violacoes.append(("fase crítica com Hub rodando", a))
for x, y in zip(amostras, amostras[1:]):
    if x["ativo"] != y["ativo"]:
        trocas += 1
        if x["hub"] or y["hub"]: violacoes.append(("seletor trocado com Hub rodando", x, y))
retomadas = sum(1 for x, y in zip(amostras, amostras[1:]) if not x["hub"] and y["hub"])
print(f"amostras={len(amostras)} em_fase_critica={criticas} trocas_de_seletor={trocas} reinicios_do_hub={retomadas} violacoes={len(violacoes)}")
for v in violacoes[:5]: print("  VIOLAÇÃO", v)
sys.exit(1 if violacoes or criticas == 0 or trocas == 0 else 0)
