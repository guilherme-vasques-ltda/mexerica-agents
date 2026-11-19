// AuthContext vazio (achado do estudo de transportes, §2): o motor NUNCA lê
// chave do ambiente nem toca em arquivo de credencial de outro app. A única
// fonte de credencial é o cofre do Hub pelo canal do fd 3.
import type { AuthContext } from "@earendil-works/pi-ai";

export function emptyAuthContext(): AuthContext {
	return {
		env: async () => undefined,
		fileExists: async () => false,
	};
}
