struct HelperError: Error {
    let message: String
    init(_ message: String) { self.message = message }
}

var latch = ForkClickLatch<String>()
let first = latch.prepare("botão fictício", deadline: 200)
precondition(latch.consume(first, deadline: 200, now: 100) == nil)
latch.markReady(first)
precondition(latch.consume(UUID(), deadline: 200, now: 100) == nil)
precondition(latch.consume(first, deadline: 201, now: 100) == nil)
precondition(latch.consume(first, deadline: 200, now: 200) == nil)
precondition(latch.consume(first, deadline: 180, now: 179) == "botão fictício")
precondition(latch.consume(first, deadline: 180, now: 179) == nil)
let replaced = latch.prepare("janela fictícia", deadline: 200)
latch.markReady(replaced)
let current = latch.prepare("outra janela fictícia", deadline: 200)
precondition(latch.consume(replaced, deadline: 200, now: 100) == nil)
precondition(latch.consume(current, deadline: 200, now: 100) == nil)
latch.markReady(current)
latch.invalidate()
precondition(latch.consume(current, deadline: 200, now: 100) == nil)
