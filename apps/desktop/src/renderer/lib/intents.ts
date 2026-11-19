// D165 (revisão A, F1/F5): asynchronous work started by an intent writes only while that intent is
// still the latest. A choice, a removal or a new apply makes every older one stale.
export class Intents {
  #latest = 0

  /** A new intent: every earlier one is now stale. */
  begin(): number {
    return ++this.#latest
  }

  /** Whether `intent` is still the latest. */
  current(intent: number): boolean {
    return intent === this.#latest
  }
}
