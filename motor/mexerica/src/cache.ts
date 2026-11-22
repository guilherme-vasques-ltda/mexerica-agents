// Identidade de cache da conversa no fornecedor. O Hub manda um valor aleatório, opaco e só para isso (nunca o id da
// sessão, o id do bot ou texto do usuário), que vale para uma conversa, uma conta e um fornecedor. O motor só o
// repassa como identidade de sessão do pedido: a chave de cache da OpenAI e os cabeçalhos de afinidade. Sem valor, o
// campo não existe no pedido; valor fora do formato é recusado com erro, em vez de ir para o corpo ou o cabeçalho.
const FORMATO = /^[A-Za-z0-9_-]{16,64}$/;

export function identidadeDeCache(valor: unknown): string | undefined {
	if (valor === undefined) return undefined;
	if (typeof valor !== "string" || !FORMATO.test(valor)) {
		throw new Error("cacheKey inválido: a identidade de cache é um valor opaco de 16 a 64 caracteres");
	}
	return valor;
}
