// HubCredentialStore: implementa o CredentialStore do pi-ai falando com o
// canal do cofre (fd 3). Uma credencial por provedor, presa à conta do
// processo; `modify`/`delete` devolvem o pedido ao Hub, que recusa escrita
// nesta fatia. `onSecret` registra cada chave lida para o saneamento da saída.
import type { AuthOperationOptions, Credential, CredentialInfo, CredentialStore } from "@earendil-works/pi-ai";
import type { VaultChannel } from "./vault-channel.ts";

export class HubCredentialStore implements CredentialStore {
	private readonly channel: VaultChannel;
	private readonly onSecret?: (key: string) => void;

	constructor(channel: VaultChannel, onSecret?: (key: string) => void) {
		this.channel = channel;
		this.onSecret = onSecret;
	}

	async read(providerId: string, _options?: AuthOperationOptions): Promise<Credential | undefined> {
		const credential = await this.channel.request<Credential | undefined>("credential.read", { providerId });
		const key = (credential as { key?: unknown } | undefined)?.key;
		if (typeof key === "string" && key) this.onSecret?.(key);
		return credential;
	}

	async list(): Promise<readonly CredentialInfo[]> {
		return []; // o Hub responde conta a conta; listagem não expõe outras
	}

	async delete(providerId: string): Promise<void> {
		await this.channel.request("credential.modify", { providerId, credential: undefined });
	}

	async modify(
		providerId: string,
		update: (current: Credential | undefined) => Promise<Credential | undefined>,
		options?: AuthOperationOptions,
	): Promise<Credential | undefined> {
		const next = await update(await this.read(providerId, options));
		await this.channel.request("credential.modify", { providerId, credential: next });
		return next;
	}
}
