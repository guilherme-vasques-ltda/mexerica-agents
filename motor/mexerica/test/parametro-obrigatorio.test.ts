import { test } from "node:test";
import assert from "node:assert/strict";
import { requireParameters } from "../src/hub-tools.ts";

const schema = {
	type: "object",
	properties: { caminho: { type: "string", description: "Arquivo" }, modo: { type: "string" } },
	required: ["caminho"],
};

test("sem parâmetro obrigatório: erro com o que faltou, o recebido e a lista", () => {
	assert.throws(
		() => requireParameters("ler", schema, { modo: "x" }),
		(e: Error) =>
			e.message.includes("faltou caminho") &&
			e.message.includes('{"modo":"x"}') &&
			e.message.includes("caminho (string, obrigatório) — Arquivo") &&
			e.message.includes("modo (string, opcional)"),
	);
});

test("argumentos completos passam sem mudança", () => {
	const args = { caminho: "a" };
	assert.equal(requireParameters("ler", schema, args), args);
});

test("ferramenta sem obrigatórios aceita chamada vazia", () => {
	const args = {};
	assert.equal(requireParameters("lista", { type: "object", properties: {} }, args), args);
});
