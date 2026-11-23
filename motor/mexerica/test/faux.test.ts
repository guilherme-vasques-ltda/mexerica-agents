import assert from "node:assert/strict";
import { test } from "node:test";
import { createModels, fauxAssistantMessage, fauxProvider } from "@earendil-works/pi-ai";

test("provedor fictício fragmenta texto Unicode sem partir um caractere", async () => {
	const texto = "abc🎉éβfim";
	const faux = fauxProvider({ tokenSize: { min: 1, max: 1 } });
	const models = createModels();
	models.setProvider(faux.provider);
	faux.setResponses([fauxAssistantMessage(texto)]);
	const modelo = models.getModel("faux", "faux-1");
	assert.ok(modelo);

	const fragmentos: string[] = [];
	const stream = models.streamSimple(modelo, { messages: [] });
	for await (const evento of stream) {
		if (evento.type === "text_delta") fragmentos.push(evento.delta);
	}
	assert.deepEqual(fragmentos, ["abc", "🎉éβ", "fim"]);
	assert.ok(fragmentos.every((trecho) => trecho.length > 0 && trecho.length <= 4));
	assert.equal(fragmentos.join(""), texto);
});
